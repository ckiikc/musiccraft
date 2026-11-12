//! Independent DSF/DSDIFF reader from the public format and DoP 1.1 specifications.
//! No DSD-to-PCM conversion: blocks retain the original one-bit audio payload.
use crate::integer::{DecodedPcm, Transport};
use std::io::{Read, Seek, SeekFrom};
type Input = Box<dyn symphonia::core::io::MediaSource>;
enum Layout {
    Dsf { block: u64, lsb: bool },
    Dff,
}
pub struct Decoder {
    input: Input,
    layout: Layout,
    start: u64,
    length: u64,
    frames: u64,
    valid_bits: u64,
    position: u64,
    rate: u32,
    channels: usize,
    block_index: Option<u64>,
    block: Vec<u8>,
}
fn read<const N: usize>(input: &mut Input) -> Result<[u8; N], String> {
    let mut bytes = [0; N];
    input
        .read_exact(&mut bytes)
        .map_err(|_| "Truncated DSD container")?;
    Ok(bytes)
}
fn seek(input: &mut Input, offset: u64) -> Result<(), String> {
    input
        .seek(SeekFrom::Start(offset))
        .map(|_| ())
        .map_err(|_| "DSD seek failed".into())
}
fn add(a: u64, b: u64) -> Result<u64, String> {
    a.checked_add(b)
        .ok_or_else(|| "Invalid DSD chunk size".into())
}
fn be32(input: &mut Input) -> Result<u32, String> {
    Ok(u32::from_be_bytes(read(input)?))
}
fn be64(input: &mut Input) -> Result<u64, String> {
    Ok(u64::from_be_bytes(read(input)?))
}
fn le32(input: &mut Input) -> Result<u32, String> {
    Ok(u32::from_le_bytes(read(input)?))
}
fn le64(input: &mut Input) -> Result<u64, String> {
    Ok(u64::from_le_bytes(read(input)?))
}
impl Decoder {
    pub fn open(mut input: Input) -> Result<Self, String> {
        let magic = read::<4>(&mut input)?;
        let (layout, start, length, samples, rate, channels) = if &magic == b"DSD " {
            if le64(&mut input)? != 28 {
                return Err("Invalid DSF header".into());
            }
            let size = le64(&mut input)?;
            let _metadata = le64(&mut input)?;
            if input.byte_len().is_some_and(|n| size > n) {
                return Err("Truncated DSF file".into());
            }
            if read::<4>(&mut input)? != *b"fmt "
                || le64(&mut input)? != 52
                || le32(&mut input)? != 1
                || le32(&mut input)? != 0
            {
                return Err("Unsupported DSF version or encoding".into());
            }
            let _channel_type = le32(&mut input)?;
            let channels = le32(&mut input)? as usize;
            let rate = le32(&mut input)?;
            let bits = le32(&mut input)?;
            let samples = le64(&mut input)?;
            let block = le32(&mut input)? as u64;
            let _reserved = le32(&mut input)?;
            if !matches!(bits, 1 | 8) || block == 0 || block > 1_048_576 || !block.is_multiple_of(2)
            {
                return Err("Unsupported DSF block layout".into());
            }
            if read::<4>(&mut input)? != *b"data" {
                return Err("DSF audio chunk missing".into());
            }
            let length = le64(&mut input)?
                .checked_sub(12)
                .ok_or("Invalid DSF audio size")?;
            let start = input
                .stream_position()
                .map_err(|_| "DSF position unavailable")?;
            if add(start, length)? > size {
                return Err("DSF audio exceeds container bounds".into());
            }
            (
                Layout::Dsf {
                    block,
                    lsb: bits == 1,
                },
                start,
                length,
                samples,
                rate,
                channels,
            )
        } else if &magic == b"FRM8" {
            let end = add(be64(&mut input)?, 12)?;
            if read::<4>(&mut input)? != *b"DSD " || input.byte_len().is_some_and(|n| end > n) {
                return Err("Unsupported or truncated DSDIFF container".into());
            }
            let mut rate = 0;
            let mut channels = 0;
            let mut start = 0;
            let mut length = 0;
            let mut uncompressed = false;
            let mut pos = 16;
            for _ in 0..4096 {
                if add(pos, 12)? > end {
                    break;
                }
                seek(&mut input, pos)?;
                let id = read::<4>(&mut input)?;
                let size = be64(&mut input)?;
                let body = add(pos, 12)?;
                let next = add(body, size)?;
                if next > end {
                    return Err("DSDIFF chunk exceeds container bounds".into());
                }
                match &id {
                    b"PROP" => {
                        if size < 4 || read::<4>(&mut input)? != *b"SND " {
                            return Err("Invalid DSDIFF sound properties".into());
                        }
                        let mut sub = add(body, 4)?;
                        for _ in 0..1024 {
                            if add(sub, 12)? > next {
                                break;
                            }
                            seek(&mut input, sub)?;
                            let key = read::<4>(&mut input)?;
                            let n = be64(&mut input)?;
                            let value = add(sub, 12)?;
                            let limit = add(value, n)?;
                            if limit > next {
                                return Err("Invalid DSDIFF property size".into());
                            }
                            match &key {
                                b"FS  " if n == 4 => rate = be32(&mut input)?,
                                b"CHNL" if n >= 2 => {
                                    channels = u16::from_be_bytes(read(&mut input)?) as usize
                                }
                                b"CMPR" if n >= 4 => {
                                    uncompressed = read::<4>(&mut input)? == *b"DSD "
                                }
                                _ => {}
                            }
                            sub = add(limit, n % 2)?;
                        }
                    }
                    b"DSD " => {
                        start = body;
                        length = size;
                    }
                    b"DST " => {
                        return Err(
                            "Compressed DST is not supported for DoP; choose PCM conversion".into(),
                        );
                    }
                    _ => {}
                }
                pos = add(next, size % 2)?;
            }
            if !uncompressed || start == 0 || channels == 0 || length % (channels as u64) != 0 {
                return Err("Unsupported DSDIFF encoding or channel layout".into());
            }
            let samples = (length / (channels as u64))
                .checked_mul(8)
                .ok_or("Invalid DSDIFF sample count")?;
            (Layout::Dff, start, length, samples, rate, channels)
        } else {
            return Err("DoP requires a DSF or uncompressed DSDIFF file".into());
        };
        if !(1..=8).contains(&channels)
            || !(2_822_400..=24_576_000).contains(&rate)
            || !rate.is_multiple_of(16)
            || samples == 0
        {
            return Err("Unsupported DSD rate, channel count or empty payload".into());
        }
        let required = match layout {
            Layout::Dsf { block, .. } => samples
                .div_ceil(8)
                .div_ceil(block)
                .checked_mul(block)
                .and_then(|n| n.checked_mul(channels as u64)),
            Layout::Dff => Some(length),
        }
        .ok_or("Invalid DSD audio length")?;
        if required > length {
            return Err("DSD sample count exceeds audio payload".into());
        }
        Ok(Self {
            input,
            layout,
            start,
            length,
            frames: samples.div_ceil(16),
            valid_bits: samples,
            position: 0,
            rate: rate / 16,
            channels,
            block_index: None,
            block: vec![],
        })
    }
    pub fn info(&self) -> DecodedPcm {
        DecodedPcm {
            transport: Transport::DoP,
            samples: vec![],
            floating: false,
            rate: self.rate,
            channels: self.channels,
            bits: 24,
        }
    }
    pub fn duration(&self) -> f64 {
        self.frames as f64 / f64::from(self.rate)
    }
    pub fn seek(&mut self, seconds: f64) -> Result<(), String> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err("Invalid DSD seek position".into());
        }
        self.position = ((seconds * f64::from(self.rate)) as u64).min(self.frames);
        self.block_index = None;
        Ok(())
    }
    pub fn next(&mut self, cancel: &impl Fn() -> bool) -> Result<Option<DecodedPcm>, String> {
        if cancel() {
            return Err("DSD read cancelled".into());
        }
        if self.position >= self.frames {
            return Ok(None);
        }
        let count = (self.frames - self.position).min(4096) as usize;
        let mut samples = Vec::with_capacity(count * self.channels);
        match self.layout {
            Layout::Dsf { block, lsb } => {
                for _ in 0..count {
                    if cancel() {
                        return Err("DSD read cancelled".into());
                    }
                    let byte = self.position * 2;
                    let group = byte / block;
                    if self.block_index != Some(group) {
                        let offset = group
                            .checked_mul(block)
                            .and_then(|n| n.checked_mul(self.channels as u64))
                            .ok_or("DSF block offset overflow")?;
                        let size = block * self.channels as u64;
                        if add(offset, size)? > self.length {
                            return Err("Truncated DSF audio block".into());
                        }
                        seek(&mut self.input, add(self.start, offset)?)?;
                        self.block.resize(size as usize, 0);
                        self.input
                            .read_exact(&mut self.block)
                            .map_err(|_| "Truncated DSF audio block")?;
                        self.block_index = Some(group);
                    }
                    let offset = (byte % block) as usize;
                    for channel in 0..self.channels {
                        let a = self.block[channel * block as usize + offset];
                        let b = self.block[channel * block as usize + offset + 1];
                        let (a, b) = if lsb {
                            (a.reverse_bits(), b.reverse_bits())
                        } else {
                            (a, b)
                        };
                        samples.push(payload(
                            a,
                            b,
                            (self.valid_bits - self.position * 16).min(16) as u32,
                        ));
                    }
                    self.position += 1;
                }
            }
            Layout::Dff => {
                let offset = self.position * 2 * self.channels as u64;
                let available = ((self.length - offset) / (self.channels as u64))
                    .min((count * 2) as u64) as usize;
                self.block.resize(available * self.channels, 0);
                seek(&mut self.input, add(self.start, offset)?)?;
                self.input
                    .read_exact(&mut self.block)
                    .map_err(|_| "Truncated DSDIFF audio")?;
                for frame in 0..count {
                    for channel in 0..self.channels {
                        let a = self.block[(frame * 2) * self.channels + channel];
                        let b = self
                            .block
                            .get((frame * 2 + 1) * self.channels + channel)
                            .copied()
                            .unwrap_or(0x69);
                        samples.push(payload(
                            a,
                            b,
                            (self.valid_bits - (self.position + frame as u64) * 16).min(16) as u32,
                        ));
                    }
                }
                self.position += count as u64;
            }
        }
        Ok(Some(DecodedPcm {
            samples,
            ..self.info()
        }))
    }
}

fn payload(first: u8, second: u8, valid: u32) -> i32 {
    let raw = (u32::from(first) << 8) | u32::from(second);
    let mask = (0xffffu32 << (16 - valid)) & 0xffff;
    (((raw & mask) | (0x6969 & !mask)) << 8) as i32
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) fn fixture(dsf: bool, lsb: bool, count: usize) -> (Vec<u8>, Vec<Vec<u8>>) {
        let audio: Vec<Vec<u8>> = (0..2)
            .map(|channel| {
                (0..count)
                    .map(|i| (i * 37 + channel * 53 + 11) as u8)
                    .collect()
            })
            .collect();
        let rate = 2_822_400u32;
        let bytes = if dsf {
            let block = 64usize;
            let groups = count.div_ceil(block);
            let length = groups * block * 2;
            let mut bytes = b"DSD ".to_vec();
            bytes.extend(28u64.to_le_bytes());
            bytes.extend((92 + length as u64).to_le_bytes());
            bytes.extend(0u64.to_le_bytes());
            bytes.extend(b"fmt ");
            bytes.extend(52u64.to_le_bytes());
            for value in [1u32, 0, 2, 2, rate, if lsb { 1 } else { 8 }] {
                bytes.extend(value.to_le_bytes());
            }
            bytes.extend((count as u64 * 8).to_le_bytes());
            bytes.extend((block as u32).to_le_bytes());
            bytes.extend(0u32.to_le_bytes());
            bytes.extend(b"data");
            bytes.extend((length as u64 + 12).to_le_bytes());
            for group in 0..groups {
                for channel in &audio {
                    for i in 0..block {
                        let byte = channel.get(group * block + i).copied().unwrap_or(0x69);
                        bytes.push(if lsb { byte.reverse_bits() } else { byte });
                    }
                }
            }
            bytes
        } else {
            fn chunk(id: &[u8; 4], data: &[u8]) -> Vec<u8> {
                let mut bytes = id.to_vec();
                bytes.extend((data.len() as u64).to_be_bytes());
                bytes.extend(data);
                if !data.len().is_multiple_of(2) {
                    bytes.push(0);
                }
                bytes
            }
            let mut properties = b"SND ".to_vec();
            properties.extend(chunk(b"FS  ", &rate.to_be_bytes()));
            let mut channels = 2u16.to_be_bytes().to_vec();
            channels.extend(b"SLFTSRGT");
            properties.extend(chunk(b"CHNL", &channels));
            properties.extend(chunk(b"CMPR", b"DSD \0"));
            let mut body = b"DSD ".to_vec();
            body.extend(chunk(b"FVER", &0x01050000u32.to_be_bytes()));
            body.extend(chunk(b"PROP", &properties));
            let interleaved: Vec<_> = (0..count)
                .flat_map(|i| [audio[0][i], audio[1][i]])
                .collect();
            body.extend(chunk(b"DSD ", &interleaved));
            let mut bytes = b"FRM8".to_vec();
            bytes.extend((body.len() as u64).to_be_bytes());
            bytes.extend(body);
            bytes
        };
        (bytes, audio)
    }
    #[test]
    fn dsf_dff_dop_payloads_preserve_bit_order_channel_order_seek_and_tail() {
        for (dsf, lsb) in [(true, true), (true, false), (false, false)] {
            let (bytes, audio) = fixture(dsf, lsb, 10010);
            let mut decoder = Decoder::open(Box::new(std::io::Cursor::new(bytes))).unwrap();
            assert_eq!(decoder.info().transport, Transport::DoP);
            assert_eq!(decoder.info().rate, 176400);
            let mut frames = Vec::new();
            while let Some(block) = decoder.next(&|| false).unwrap() {
                frames.extend(block.samples);
            }
            let expected: Vec<_> = (0..5005)
                .flat_map(|i| (0..2).map(move |channel| (i, channel)))
                .map(|(i, channel)| {
                    (i32::from(audio[channel][2 * i]) << 16)
                        | (i32::from(audio[channel][2 * i + 1]) << 8)
                })
                .collect();
            assert_eq!(frames, expected);
            decoder.seek(4999.0 / 176400.0).unwrap();
            let block = decoder.next(&|| false).unwrap().unwrap();
            assert_eq!(block.samples, &expected[4999 * 2..]);
            assert!(decoder.next(&|| true).is_err());
        }
    }
    #[test]
    fn odd_dsd_tail_preserves_valid_bits_and_pads_only_with_dsd_silence() {
        for dsf in [true, false] {
            let (mut bytes, audio) = fixture(dsf, true, 129);
            let valid = if dsf { 5 } else { 8 };
            if dsf {
                bytes[64..72].copy_from_slice(&(128u64 * 8 + 5).to_le_bytes());
            }
            let mut decoder = Decoder::open(Box::new(std::io::Cursor::new(bytes))).unwrap();
            let block = decoder.next(&|| false).unwrap().unwrap();
            assert_eq!(block.samples.len(), 65 * 2);
            for (channel, samples) in audio.iter().enumerate() {
                let mask = (0xffffu32 << (16 - valid)) & 0xffff;
                let expected = (((u32::from(samples[128]) << 8) & mask) | (0x6969 & !mask)) << 8;
                assert_eq!(block.samples[128 + channel], expected as i32);
            }
            assert!(decoder.next(&|| false).unwrap().is_none());
        }
    }
    #[test]
    fn malformed_dsd_bounds_are_rejected_before_output() {
        let (mut bytes, _) = fixture(true, true, 128);
        bytes[64..72].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(Decoder::open(Box::new(std::io::Cursor::new(bytes))).is_err());
        let (mut bytes, _) = fixture(false, false, 128);
        bytes.truncate(40);
        assert!(Decoder::open(Box::new(std::io::Cursor::new(bytes))).is_err());
    }
}
