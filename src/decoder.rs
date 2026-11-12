//! Native codecs, a bounded external-codec lane, and lossless DSD-over-PCM framing.
mod dsd;
mod extended;
use crate::{
    integer::{DecodedPcm, PacketDecoder},
    library::Track,
    network::AuthStore,
    streaming::Cancellation,
};
use std::path::Path;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Pcm,
    Strict,
    DoP,
}
impl From<bool> for Mode {
    fn from(strict: bool) -> Self {
        if strict { Self::Strict } else { Self::Pcm }
    }
}
pub fn is_dsd(source: &str) -> bool {
    matches!(extension(source).as_str(), "dsf" | "dff" | "dsd")
}
fn extension(source: &str) -> String {
    let decoded = url::Url::parse(source)
        .ok()
        .map(|u| u.path().to_owned())
        .unwrap_or_else(|| source.to_owned());
    Path::new(&decoded)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}
pub enum Decoder {
    Native(PacketDecoder),
    Extended(extended::Decoder),
    DoP(dsd::Decoder),
}
impl Decoder {
    pub fn open(
        track: &mut Track,
        root: &Path,
        auth: &AuthStore,
        cancel: Cancellation,
        mode: Mode,
    ) -> Result<Self, String> {
        if is_dsd(&track.source) && mode == Mode::DoP {
            let input = crate::streaming::remote_input(&track.source, root, auth, cancel)?;
            let decoder = dsd::Decoder::open(input)?;
            track.codec = "DSD / DoP".into();
            return Ok(Self::DoP(decoder));
        }
        let extension = extension(&track.source);
        if matches!(
            extension.as_str(),
            "dsf" | "dff" | "dsd" | "ape" | "wv" | "wma" | "asf" | "opus"
        ) {
            return extended::Decoder::open(track, root, auth, cancel, mode == Mode::Strict)
                .map(Self::Extended);
        }
        let input = crate::streaming::remote_input(&track.source, root, auth, cancel.clone())?;
        match PacketDecoder::open(input, mode == Mode::Strict) {
            Ok(decoder) => Ok(Self::Native(decoder)),
            Err(_) if matches!(extension.as_str(), "ogg" | "oga") => {
                extended::Decoder::open(track, root, auth, cancel, mode == Mode::Strict)
                    .map(Self::Extended)
            }
            Err(error) => Err(error),
        }
    }
    pub fn info(&self) -> DecodedPcm {
        match self {
            Self::Native(d) => d.info(),
            Self::Extended(d) => d.info(),
            Self::DoP(d) => d.info(),
        }
    }
    pub fn duration(&self) -> Option<f64> {
        match self {
            Self::Native(d) => d.duration,
            Self::Extended(d) => d.duration,
            Self::DoP(d) => Some(d.duration()),
        }
    }
    pub fn source_bits(&self) -> Option<u32> {
        match self {
            Self::Native(d) => d.source_bits,
            Self::Extended(d) => d.source_bits,
            Self::DoP(_) => Some(1),
        }
    }
    pub fn metadata(&mut self) -> Vec<symphonia::core::meta::MetadataRevision> {
        match self {
            Self::Native(d) => d
                .metadata
                .take()
                .into_iter()
                .chain(d.format.metadata().current().cloned())
                .collect(),
            _ => vec![],
        }
    }
    pub fn gain(&self) -> crate::dsp::Tags {
        match self {
            Self::Extended(d) => d.gain,
            _ => Default::default(),
        }
    }
    pub fn next(&mut self, cancel: &impl Fn() -> bool) -> Result<Option<DecodedPcm>, String> {
        match self {
            Self::Native(d) => d.next(cancel),
            Self::Extended(d) => d.next(cancel),
            Self::DoP(d) => d.next(cancel),
        }
    }
    pub fn seek(&mut self, seconds: f64) -> Result<(), String> {
        match self {
            Self::Native(d) => d.seek(seconds),
            Self::Extended(d) => d.seek(seconds),
            Self::DoP(d) => d.seek(seconds),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, atomic::AtomicU64};
    #[test]
    #[ignore = "requires explicit synthetic codec fixtures and the matching FFmpeg runtime"]
    fn live_extended_codecs_decode_seek_and_preserve_lossless_pcm() {
        let fixtures = std::path::PathBuf::from(
            std::env::var("MUSICCRAFT_CODEC_FIXTURES").expect("synthetic codec fixture directory"),
        );
        let data = tempfile::tempdir().unwrap();
        for extension in ["wv", "ape", "wma", "opus", "dsf", "dff"] {
            let path = fixtures.join(format!("fixture.{extension}"));
            if matches!(extension, "dsf" | "dff") {
                let (bytes, _) = dsd::tests::fixture(extension == "dsf", true, 96000);
                std::fs::write(&path, bytes).unwrap();
            }
            assert!(path.is_file(), "missing synthetic fixture: {extension}");
            let mut track = Track::placeholder(path.to_string_lossy().into_owned());
            let cancel = Cancellation::new(Arc::new(AtomicU64::new(1)), 1, None);
            let mut decoder = Decoder::open(
                &mut track,
                data.path(),
                &AuthStore::default(),
                cancel,
                Mode::Pcm,
            )
            .unwrap_or_else(|e| panic!("{extension}: {e}"));
            let info = decoder.info();
            assert_eq!(info.channels, 2);
            let mut samples = Vec::new();
            while let Some(block) = decoder.next(&|| false).unwrap() {
                samples.extend(block.samples);
            }
            assert!(samples.len() > 1000, "{extension}: empty output");
            assert!(
                samples.iter().any(|s| *s != 0),
                "{extension}: silent output"
            );
            if matches!(extension, "wv" | "ape") {
                let reference =
                    crate::integer::DecodedPcm::read(&fixtures.join(if extension == "wv" {
                        "reference32.wav"
                    } else {
                        "reference16.wav"
                    }))
                    .unwrap();
                assert_eq!(
                    samples, reference.samples,
                    "{extension}: lossless PCM changed"
                );
                let seek = 12000usize;
                decoder.seek(seek as f64 / f64::from(info.rate)).unwrap();
                let block = decoder.next(&|| false).unwrap().unwrap();
                assert_eq!(
                    block.samples,
                    &samples[seek * 2..seek * 2 + block.samples.len()]
                );
            } else {
                decoder.seek(0.1).unwrap();
                assert!(decoder.next(&|| false).unwrap().is_some());
            }
            assert!(decoder.next(&|| true).is_err());
            let mut track = Track::placeholder(path.to_string_lossy().into_owned());
            let strict = Decoder::open(
                &mut track,
                data.path(),
                &AuthStore::default(),
                Cancellation::new(Arc::new(AtomicU64::new(1)), 1, None),
                Mode::Strict,
            );
            assert_eq!(
                strict.is_ok(),
                matches!(extension, "wv" | "ape"),
                "{extension}: strict mode classification"
            );
        }
    }
}
