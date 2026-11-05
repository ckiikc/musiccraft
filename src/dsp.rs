//! Optional gain processing for shared output. The exclusive HAL path bypasses it.
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::path::Path;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplayGain {
    #[default]
    Off,
    Track,
    Album,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub mode: ReplayGain,
    pub preamp_db: f32,
    pub prevent_clipping: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            mode: ReplayGain::Off,
            preamp_db: 0.0,
            prevent_clipping: true,
        }
    }
}
#[derive(Clone, Copy, Default, Debug)]
pub struct Tags {
    pub track_gain: Option<f32>,
    pub album_gain: Option<f32>,
    pub track_peak: Option<f32>,
    pub album_peak: Option<f32>,
}
impl Tags {
    #[cfg(test)]
    pub fn read(path: &Path) -> Self {
        let Ok(mut file) = std::fs::File::open(path) else {
            return Self::default();
        };
        Self::read_from(&mut file)
    }
    pub fn read_from(reader: &mut (impl std::io::Read + std::io::Seek)) -> Self {
        use lofty::file::TaggedFileExt;
        use lofty::tag::ItemKey;
        let Some(file) = lofty::probe::Probe::new(reader)
            .guess_file_type()
            .ok()
            .and_then(|p| {
                p.options(
                    lofty::config::ParseOptions::new()
                        .read_properties(false)
                        .read_cover_art(false),
                )
                .read()
                .ok()
            })
        else {
            return Self::default();
        };
        let mut tags = Self::default();
        for tag in file.tags() {
            let number = |key| tag.get_string(key).and_then(parse_number);
            tags.track_gain = tags.track_gain.or(number(ItemKey::ReplayGainTrackGain));
            tags.album_gain = tags.album_gain.or(number(ItemKey::ReplayGainAlbumGain));
            tags.track_peak = tags.track_peak.or(number(ItemKey::ReplayGainTrackPeak));
            tags.album_peak = tags.album_peak.or(number(ItemKey::ReplayGainAlbumPeak));
        }
        tags
    }
}
pub(crate) fn parse_number(s: &str) -> Option<f32> {
    s.split_whitespace()
        .next()?
        .parse::<f32>()
        .ok()
        .filter(|n| n.is_finite())
}
impl Config {
    pub fn sanitized(mut self) -> Self {
        self.preamp_db = if self.preamp_db.is_finite() {
            self.preamp_db.clamp(-24.0, 12.0)
        } else {
            0.0
        };
        self
    }
    pub fn factor(self, tags: Tags) -> f32 {
        let gain = match self.mode {
            ReplayGain::Off => 0.0,
            ReplayGain::Track => tags.track_gain.unwrap_or(0.0),
            ReplayGain::Album => tags.album_gain.or(tags.track_gain).unwrap_or(0.0),
        };
        let gain = if gain.is_finite() { gain } else { 0.0 };
        let preamp = if self.preamp_db.is_finite() {
            self.preamp_db.clamp(-24.0, 12.0)
        } else {
            0.0
        };
        let mut factor = 10.0_f32.powf((gain.clamp(-60.0, 24.0) + preamp) / 20.0);
        if self.prevent_clipping && self.mode != ReplayGain::Off {
            let peak = match self.mode {
                ReplayGain::Album if tags.album_gain.is_some() => {
                    tags.album_peak.or(tags.track_peak)
                }
                _ => tags.track_peak,
            };
            if let Some(peak) = peak.filter(|p| p.is_finite() && *p > 0.0) {
                factor = factor.min(1.0 / peak);
            }
        }
        factor
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn off_is_unity_album_preserves_relative_dynamics_and_peak_limits_gain() {
        assert_eq!(Config::default().factor(Tags::default()), 1.0);
        let tags = Tags {
            track_gain: Some(-6.0),
            album_gain: Some(-3.0),
            track_peak: Some(0.9),
            album_peak: Some(1.4),
        };
        let mut c = Config {
            mode: ReplayGain::Track,
            ..Default::default()
        };
        assert!((c.factor(tags) - 0.5011872).abs() < 1e-6);
        c.mode = ReplayGain::Album;
        assert!((c.factor(tags) - 0.70794576).abs() < 1e-6);
        c.preamp_db = 12.0;
        assert!(c.factor(tags) * 1.4 <= 1.000001);
        assert_eq!(
            Config {
                preamp_db: f32::NAN,
                ..Default::default()
            }
            .factor(Tags::default()),
            1.0
        );
    }
    #[test]
    fn reads_real_tag_fields_without_modifying_media() {
        use lofty::{
            file::TaggedFileExt,
            tag::{ItemKey, TagExt},
        };
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("gain.wav");
        let mut writer = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 2,
                sample_rate: 44100,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..40 {
            writer.write_sample(0_i16).unwrap();
        }
        writer.finalize().unwrap();
        let mut file = lofty::read_from_path(&path).unwrap();
        let mut tag = lofty::tag::Tag::new(lofty::tag::TagType::Id3v2);
        tag.insert_text(ItemKey::ReplayGainAlbumGain, "-7.50 dB".into());
        tag.insert_text(ItemKey::ReplayGainAlbumPeak, "0.891".into());
        file.insert_tag(tag);
        file.primary_tag()
            .unwrap()
            .save_to_path(&path, Default::default())
            .unwrap();
        let before = std::fs::read(&path).unwrap();
        let tags = Tags::read(&path);
        assert_eq!(tags.album_gain, Some(-7.5));
        assert_eq!(tags.album_peak, Some(0.891));
        assert_eq!(before, std::fs::read(path).unwrap());
    }
}
