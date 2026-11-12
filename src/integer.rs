//! Integer PCM is decoded independently of Rodio's f32 mixer. No quantization via float.
#[cfg(test)]
use std::{fs::File, path::Path};
use symphonia::core::{
    audio::{AudioBufferRef, SampleBuffer},
    codecs::DecoderOptions,
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};

#[cfg(test)]
pub const MAX_PCM_BYTES: usize = 512 * 1024 * 1024;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Transport {
    #[default]
    Pcm,
    DoP,
}

pub struct DecodedPcm {
    pub transport: Transport,
    /// Normalized signed integer PCM, or f32 bit patterns when floating is true.
    pub samples: Vec<i32>,
    pub floating: bool,
    pub rate: u32,
    pub channels: usize,
    pub bits: u32,
}
impl DecodedPcm {
    #[cfg(test)]
    pub fn read(path: &Path) -> Result<Self, String> {
        Self::read_cancel(path, || false)
    }
    #[cfg(test)]
    pub fn read_cancel(path: &Path, cancelled: impl Fn() -> bool) -> Result<Self, String> {
        Self::read_mode(path, cancelled, true)
    }
    #[cfg(test)]
    pub fn read_mode(
        path: &Path,
        cancelled: impl Fn() -> bool,
        strict: bool,
    ) -> Result<Self, String> {
        let file = File::open(path).map_err(|_| "Cache PCM non leggibile")?;
        let mut reader = PacketDecoder::open(Box::new(file), strict)?;
        let mut pcm = reader.info();
        while let Some(block) = reader.next(&cancelled)? {
            if pcm.samples.len().saturating_add(block.samples.len()) > MAX_PCM_BYTES / 4 {
                return Err(
                    "Esclusiva: limite di 512 MiB PCM decodificato per brano raggiunto".into(),
                );
            }
            pcm.floating = block.floating;
            pcm.bits = block.bits;
            pcm.channels = block.channels;
            pcm.samples.extend(block.samples);
        }
        if pcm.samples.is_empty() {
            return Err("PCM vuoto".into());
        }
        Ok(pcm)
    }
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels
    }
}

pub struct PacketDecoder {
    pub(crate) metadata: Option<symphonia::core::meta::MetadataRevision>,
    pub(crate) format: Box<dyn symphonia::core::formats::FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    id: u32,
    decoder_trims: bool,
    strict: bool,
    rate: u32,
    channels: usize,
    bits: u32,
    floating: Option<bool>,
    pub duration: Option<f64>,
    pub source_bits: Option<u32>,
    skip: usize,
}
impl PacketDecoder {
    pub fn open(
        source: Box<dyn symphonia::core::io::MediaSource>,
        strict: bool,
    ) -> Result<Self, String> {
        let mut probed = symphonia::default::get_probe()
            .format(
                &Hint::new(),
                MediaSourceStream::new(source, Default::default()),
                &FormatOptions {
                    enable_gapless: true,
                    ..Default::default()
                },
                &MetadataOptions {
                    limit_metadata_bytes: symphonia::core::meta::Limit::Maximum(1024 * 1024),
                    limit_visual_bytes: symphonia::core::meta::Limit::Maximum(8 * 1024 * 1024),
                },
            )
            .map_err(|error| format!("Errore di decodifica audio: {error}"))?;
        let metadata = probed.metadata.get().and_then(|m| m.current().cloned());
        let format = probed.format;
        let track = format.default_track().ok_or("Traccia audio non trovata")?;
        let id = track.id;
        use symphonia::core::codecs::{
            CODEC_TYPE_MP1, CODEC_TYPE_MP2, CODEC_TYPE_MP3, CODEC_TYPE_VORBIS,
        };
        let decoder_trims = [
            CODEC_TYPE_MP1,
            CODEC_TYPE_MP2,
            CODEC_TYPE_MP3,
            CODEC_TYPE_VORBIS,
        ]
        .contains(&track.codec_params.codec);
        let params = track.codec_params.clone();
        if strict {
            use symphonia::core::codecs::*;
            let lossless = [
                CODEC_TYPE_FLAC,
                CODEC_TYPE_ALAC,
                CODEC_TYPE_PCM_S32LE,
                CODEC_TYPE_PCM_S32LE_PLANAR,
                CODEC_TYPE_PCM_S32BE,
                CODEC_TYPE_PCM_S32BE_PLANAR,
                CODEC_TYPE_PCM_S24LE,
                CODEC_TYPE_PCM_S24LE_PLANAR,
                CODEC_TYPE_PCM_S24BE,
                CODEC_TYPE_PCM_S24BE_PLANAR,
                CODEC_TYPE_PCM_S16LE,
                CODEC_TYPE_PCM_S16LE_PLANAR,
                CODEC_TYPE_PCM_S16BE,
                CODEC_TYPE_PCM_S16BE_PLANAR,
                CODEC_TYPE_PCM_S8,
                CODEC_TYPE_PCM_S8_PLANAR,
                CODEC_TYPE_PCM_U32LE,
                CODEC_TYPE_PCM_U32LE_PLANAR,
                CODEC_TYPE_PCM_U32BE,
                CODEC_TYPE_PCM_U32BE_PLANAR,
                CODEC_TYPE_PCM_U24LE,
                CODEC_TYPE_PCM_U24LE_PLANAR,
                CODEC_TYPE_PCM_U24BE,
                CODEC_TYPE_PCM_U24BE_PLANAR,
                CODEC_TYPE_PCM_U16LE,
                CODEC_TYPE_PCM_U16LE_PLANAR,
                CODEC_TYPE_PCM_U16BE,
                CODEC_TYPE_PCM_U16BE_PLANAR,
                CODEC_TYPE_PCM_U8,
                CODEC_TYPE_PCM_U8_PLANAR,
            ];
            if !lossless.contains(&params.codec) {
                return Err("Integer rigoroso richiede PCM intero, FLAC o ALAC lossless".into());
            }
        }
        let decoder = symphonia::default::get_codecs()
            .make(&params, &DecoderOptions::default())
            .map_err(|error| format!("Errore di decodifica audio: {error}"))?;
        let effective = decoder.codec_params();
        let rate = effective
            .sample_rate
            .filter(|&v| v > 0)
            .ok_or("Frequenza non disponibile")?;
        let channels = effective.channels.map(|c| c.count()).unwrap_or(0);
        // ALAC's validated 24/48-byte magic cookie carries the source depth;
        // Symphonia 0.5.5 leaves bits_per_sample unset in MP4 codec parameters.
        let alac_bits = if params.codec == symphonia::core::codecs::CODEC_TYPE_ALAC {
            params
                .extra_data
                .as_ref()
                .filter(|d| matches!(d.len(), 24 | 48))
                .and_then(|d| d.get(5))
                .map(|v| u32::from(*v))
        } else {
            None
        };
        let source_bits = effective
            .bits_per_sample
            .or(alac_bits)
            .filter(|&n| (1..=64).contains(&n));
        let depth = effective
            .bits_per_sample
            .or(alac_bits)
            .filter(|&v| (1..=32).contains(&v));
        let bits = if strict {
            depth.ok_or("Esclusiva integer: serve PCM intero lossless con profondità nota (WAV/FLAC/ALAC/AIFF)")?
        } else {
            depth.unwrap_or(32)
        };

        let duration = params.n_frames.map(|n| n as f64 / f64::from(rate));
        Ok(Self {
            metadata,
            format,
            decoder,
            id,
            decoder_trims,
            strict,
            rate,
            channels,
            bits,
            floating: None,
            duration,
            source_bits,
            skip: 0,
        })
    }
    pub fn info(&self) -> DecodedPcm {
        DecodedPcm {
            transport: crate::integer::Transport::Pcm,
            samples: vec![],
            floating: self.floating.unwrap_or(false),
            rate: self.rate,
            channels: self.channels,
            bits: self.bits,
        }
    }
    pub fn seek(&mut self, seconds: f64) -> Result<(), String> {
        use symphonia::core::{
            formats::{SeekMode, SeekTo},
            units::Time,
        };
        let result = self
            .format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time: Time::from(seconds),
                    track_id: Some(self.id),
                },
            )
            .map_err(|e| format!("Seek non disponibile: {e}"))?;
        self.decoder.reset();
        let time_base = self
            .decoder
            .codec_params()
            .time_base
            .unwrap_or(symphonia::core::units::TimeBase::new(1, self.rate));
        let delta = time_base.calc_time(result.required_ts.saturating_sub(result.actual_ts));
        self.skip = ((delta.seconds as f64 + delta.frac) * f64::from(self.rate)).round() as usize;
        Ok(())
    }
    pub fn next(&mut self, cancelled: &impl Fn() -> bool) -> Result<Option<DecodedPcm>, String> {
        loop {
            if cancelled() {
                return Err("Decodifica annullata".into());
            }
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(symphonia::core::errors::Error::IoError(e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    return Ok(None);
                }
                Err(e) => return Err(format!("Lettura PCM interrotta: {e}")),
            };
            if packet.track_id() != self.id {
                continue;
            }
            let decoded = self
                .decoder
                .decode(&packet)
                .map_err(|error| format!("Errore di decodifica audio: {error}"))?;
            let is_float = matches!(&decoded, AudioBufferRef::F32(_) | AudioBufferRef::F64(_));
            if self.strict && is_float {
                return Err(
                    "Il decoder restituisce float: disattiva Integer rigoroso per questo formato"
                        .into(),
                );
            }
            if self.floating.is_some_and(|f| f != is_float) {
                return Err("Tipo PCM variabile".into());
            }
            self.floating = Some(is_float);
            if is_float {
                self.bits = 32;
            }
            if self.channels == 0 {
                self.channels = decoded.spec().channels.count();
            }
            if self.channels == 0 || self.channels > 8 {
                return Err("Esclusiva: da 1 a 8 canali supportati".into());
            }
            if decoded.spec().rate != self.rate
                || decoded.spec().channels.count() != self.channels
                || decoded.capacity() > 1_048_576
            {
                return Err("Formato PCM variabile o pacchetto troppo grande".into());
            }
            let decoded_samples: Vec<i32> = if is_float {
                let mut buffer =
                    SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
                buffer.copy_interleaved_ref(decoded);
                if buffer.samples().iter().any(|s| !s.is_finite()) {
                    return Err("Campione float non finito".into());
                }
                buffer
                    .samples()
                    .iter()
                    .map(|s| s.to_bits() as i32)
                    .collect()
            } else {
                let mut buffer =
                    SampleBuffer::<i32>::new(decoded.capacity() as u64, *decoded.spec());
                buffer.copy_interleaved_ref(decoded);
                buffer.samples().to_vec()
            };
            let start = (if self.decoder_trims {
                0
            } else {
                packet.trim_start() as usize
            })
            .checked_mul(self.channels)
            .ok_or("Trim PCM troppo grande")?;
            let trim_end = (if self.decoder_trims {
                0
            } else {
                packet.trim_end() as usize
            })
            .checked_mul(self.channels)
            .ok_or("Trim PCM troppo grande")?;
            let end = decoded_samples
                .len()
                .checked_sub(trim_end)
                .ok_or("Padding PCM non valido")?;

            let samples = decoded_samples
                .get(start..end)
                .ok_or("Ritardo encoder PCM non valido")?;
            if !is_float && self.bits < 32 {
                let mask = (1u32 << (32 - self.bits)) - 1;
                if samples.iter().any(|s| (*s as u32 & mask) != 0) {
                    return Err("Il decoder non rispetta la profondità dichiarata".into());
                }
            }
            let skipped = self.skip.min(samples.len() / self.channels);
            self.skip -= skipped;
            let samples = samples[skipped * self.channels..].to_vec();
            if samples.is_empty() {
                continue;
            }
            return Ok(Some(DecodedPcm {
                transport: Transport::Pcm,
                samples,
                floating: is_float,
                rate: self.rate,
                channels: self.channels,
                bits: self.bits,
            }));
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WireFormat {
    pub rate: u32,
    pub channels: usize,
    pub bits: u32,
    pub bytes: usize,
    pub big_endian: bool,
    pub high_aligned: bool,
    pub planar: bool,
    pub floating: bool,
}
impl WireFormat {
    pub fn supports(&self, pcm: &DecodedPcm) -> bool {
        (pcm.transport != Transport::DoP || (!self.floating && self.bits >= 24))
            && self.rate == pcm.rate
            && self.channels == pcm.channels
            && (self.floating || (!pcm.floating && self.bits >= pcm.bits))
    }
    #[cfg(test)]
    pub fn sample_bytes(&self, sample: i32, target: &mut [u8]) -> bool {
        self.encode(sample, false, target)
    }
    pub fn encode(&self, sample: i32, source_float: bool, target: &mut [u8]) -> bool {
        if self.floating {
            if self.bytes != 4 || target.len() != 4 {
                return false;
            }
            let value = if source_float {
                f32::from_bits(sample as u32)
            } else {
                (sample as f64 / 2147483648.0) as f32
            };
            target.copy_from_slice(&if self.big_endian {
                value.to_be_bytes()
            } else {
                value.to_le_bytes()
            });
            return true;
        }
        if source_float {
            return false;
        }

        if target.len() != self.bytes
            || !(1..=4).contains(&self.bytes)
            || self.bits == 0
            || self.bits > self.bytes as u32 * 8
        {
            return false;
        }
        let shift = if self.high_aligned {
            32 - self.bytes as u32 * 8
        } else {
            32 - self.bits
        };
        let value = sample >> shift;
        let bytes = if self.big_endian {
            value.to_be_bytes()
        } else {
            value.to_le_bytes()
        };
        let range = if self.big_endian {
            4 - self.bytes..4
        } else {
            0..self.bytes
        };
        if let Some(bytes) = bytes.get(range) {
            target.copy_from_slice(bytes);
            true
        } else {
            false
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn damaged_packet_reports_the_decoder_cause_in_shared_playback() {
        let fixture = std::io::Cursor::new(
            include_str!("../tests/fixtures/invalid-subframe.hex")
                .split_whitespace()
                .flat_map(|line| {
                    (0..line.len())
                        .step_by(2)
                        .map(|i| u8::from_str_radix(&line[i..i + 2], 16).unwrap())
                })
                .collect::<Vec<_>>(),
        );
        let mut decoder = PacketDecoder::open(Box::new(fixture), false).unwrap();
        let reason = decoder
            .next(&|| false)
            .err()
            .expect("damaged packet must fail");
        assert!(reason.contains("unexpected end of bitstream"), "{reason}");
        assert!(!reason.contains("bit-perfect"), "{reason}");
    }

    #[test]
    fn integer_conversion_retains_every_bit_including_low_bits_of_32_bit_pcm() {
        for bits in [16, 24, 32] {
            let shift = 32 - bits;
            for value in [i32::MIN, 0x12345678, 0x7ffffffe, -0x1234567] {
                let sample = (value >> shift) << shift;
                for be in [false, true] {
                    let bytes = (bits / 8) as usize;
                    let fmt = WireFormat {
                        rate: 44100,
                        channels: 2,
                        bits,
                        bytes,
                        big_endian: be,
                        high_aligned: false,
                        planar: false,
                        floating: false,
                    };
                    let mut encoded = vec![0; bytes];
                    assert!(fmt.sample_bytes(sample, &mut encoded));
                    let mut restored = [if sample < 0 { 255 } else { 0 }; 4];
                    if be {
                        restored[4 - bytes..].copy_from_slice(&encoded);
                    } else {
                        restored[..bytes].copy_from_slice(&encoded);
                    }
                    let raw = if be {
                        i32::from_be_bytes(restored)
                    } else {
                        i32::from_le_bytes(restored)
                    };
                    assert_eq!(raw << shift, sample);
                }
            }
        }
    }
    #[test]
    fn native_float_path_preserves_f32_bits_without_clipping_or_integer_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("float.wav");
        let values = [-1.25f32, -0.0, 0.12345679, 1.25];
        let mut wav = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 2,
                sample_rate: 44100,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .unwrap();
        for value in values {
            wav.write_sample(value).unwrap();
        }
        wav.finalize().unwrap();
        let pcm = DecodedPcm::read_mode(&path, || false, false).unwrap();
        assert!(pcm.floating);
        let fmt = WireFormat {
            rate: 44100,
            channels: 2,
            bits: 32,
            bytes: 4,
            big_endian: false,
            high_aligned: false,
            planar: false,
            floating: true,
        };
        for (raw, expected) in pcm.samples.iter().zip(values) {
            let mut bytes = [0; 4];
            assert!(fmt.encode(*raw, true, &mut bytes));
            assert_eq!(u32::from_le_bytes(bytes), expected.to_bits());
        }
        assert!(DecodedPcm::read_mode(&path, || false, true).is_err());
    }
    #[test]
    fn wav_decode_is_exact_and_float_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        for bits in [16, 24, 32] {
            let path = dir.path().join(format!("{bits}.wav"));
            let mut writer = hound::WavWriter::create(
                &path,
                hound::WavSpec {
                    channels: 2,
                    sample_rate: 44100,
                    bits_per_sample: bits,
                    sample_format: hound::SampleFormat::Int,
                },
            )
            .unwrap();
            let data = [-127i32, -1, 0, 1, 126, 127];
            for sample in data {
                writer.write_sample(sample).unwrap();
            }
            writer.finalize().unwrap();
            let pcm = DecodedPcm::read(&path).unwrap();
            assert_eq!(pcm.bits, u32::from(bits));
            let expected: Vec<_> = data.into_iter().map(|v| v << (32 - bits)).collect();
            assert_eq!(pcm.samples, expected);
        }
        let path = dir.path().join("float.wav");
        let mut writer = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 2,
                sample_rate: 44100,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .unwrap();
        writer.write_sample(0.125f32).unwrap();
        writer.write_sample(-0.125f32).unwrap();
        writer.finalize().unwrap();
        assert!(DecodedPcm::read(&path).is_err());
    }
}
