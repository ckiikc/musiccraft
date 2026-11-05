//! A bounded metadata-only projection. Artwork and padding are skipped with Seek, never fetched.
//! Lofty still decodes text encodings and tag versions; no audio or original file is rewritten.
use lofty::{file::TaggedFileExt, tag::Tag};
use std::io::{self, Cursor, Read, Seek, SeekFrom};
const LIMIT: usize = 512 * 1024;
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid or oversized metadata structure",
    )
}
fn syncsafe(b: &[u8]) -> io::Result<usize> {
    if b.len() != 4 || b.iter().any(|x| x & 128 != 0) {
        return Err(invalid());
    }
    Ok(b.iter().fold(0, |n, b| (n << 7) | usize::from(*b)))
}
fn size_bytes(n: usize) -> [u8; 4] {
    [
        (n >> 21) as u8 & 127,
        (n >> 14) as u8 & 127,
        (n >> 7) as u8 & 127,
        n as u8 & 127,
    ]
}
fn parse(bytes: Vec<u8>, ty: lofty::file::FileType) -> io::Result<Option<Tag>> {
    let parsed = lofty::probe::Probe::with_file_type(Cursor::new(bytes), ty)
        .options(
            lofty::config::ParseOptions::new()
                .read_properties(false)
                .read_cover_art(false),
        )
        .read()
        .map_err(super::parse_error)?;
    Ok(parsed.primary_tag().or_else(|| parsed.first_tag()).cloned())
}
/// Returns None only when the prefix is not one of the projected formats.
pub(super) fn read<R: Read + Seek>(r: &mut R, length: u64) -> io::Result<Option<Option<Tag>>> {
    let mut h = [0; 10];
    let n = length.min(h.len() as u64) as usize;
    r.read_exact(&mut h[..n])?;
    r.seek(SeekFrom::Start(0))?;
    if n >= 10 && &h[..3] == b"ID3" {
        return id3(r, h, length).map(Some);
    }
    if n >= 4 && &h[..4] == b"fLaC" {
        return flac(r, length).map(Some);
    }
    Ok(None)
}
fn id3<R: Read + Seek>(r: &mut R, h: [u8; 10], length: u64) -> io::Result<Option<Tag>> {
    let version = h[3];
    if !(2..=4).contains(&version) {
        return Err(invalid());
    }
    let end = 10 + syncsafe(&h[6..10])? as u64;
    if end > length {
        return Err(invalid());
    }
    // Pre-v2.4 global unsynchronization changes physical frame offsets. Keep its byte stream
    // intact rather than guessing boundaries; still cap the allocation and network budget.
    if version < 4 && h[5] & 0x80 != 0 {
        if end as usize > LIMIT {
            return Err(invalid());
        }
        let mut bytes = vec![0; end as usize];
        r.read_exact(&mut bytes)?;
        bytes.extend_from_slice(&[0; 256]);
        return parse(bytes, lofty::file::FileType::Mpeg);
    }
    r.seek(SeekFrom::Start(10))?;
    if h[5] & 0x40 != 0 {
        if version == 2 {
            return Err(invalid());
        }
        let mut b = [0; 4];
        r.read_exact(&mut b)?;
        let size = if version == 4 {
            syncsafe(&b)? as u64
        } else {
            u32::from_be_bytes(b) as u64 + 4
        };
        if size < 4 || 10 + size > end {
            return Err(invalid());
        }
        r.seek(SeekFrom::Start(10 + size))?;
    }
    let mut body = Vec::new();
    let header = if version == 2 { 6 } else { 10 };
    let mut frames = 0;
    while r.stream_position()?.saturating_add(header as u64) <= end {
        frames += 1;
        if frames > 2048 {
            return Err(invalid());
        }
        let mut f = [0; 10];
        r.read_exact(&mut f[..header])?;
        if f[0] == 0 {
            break;
        }
        let idlen = if version == 2 { 3 } else { 4 };
        if !f[..idlen]
            .iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        {
            return Err(invalid());
        }
        let size = match version {
            2 => u32::from_be_bytes([0, f[3], f[4], f[5]]) as usize,
            3 => u32::from_be_bytes(f[4..8].try_into().map_err(|_| invalid())?) as usize,
            _ => syncsafe(&f[4..8])?,
        };
        let next = r
            .stream_position()?
            .checked_add(size as u64)
            .filter(|p| *p <= end)
            .ok_or_else(invalid)?;
        // Only textual frames are needed for the library. APIC/PIC, private data and padding
        // can be arbitrarily large and must never consume the metadata transfer budget.
        if f[0] == b'T' && size <= 64 * 1024 {
            if body.len().saturating_add(header + size) > LIMIT {
                return Err(invalid());
            }
            body.extend_from_slice(&f[..header]);
            let start = body.len();
            body.resize(start + size, 0);
            r.read_exact(&mut body[start..])?;
        }
        r.seek(SeekFrom::Start(next))?;
    }
    let mut bytes = h.to_vec();
    bytes[5] = h[5] & 0x80;
    bytes[6..10].copy_from_slice(&size_bytes(body.len()));
    bytes.extend(body);
    bytes.extend_from_slice(&[0; 256]);
    parse(bytes, lofty::file::FileType::Mpeg)
}
fn u32le<R: Read>(r: &mut R) -> io::Result<u32> {
    let mut b = [0; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}
fn comments<R: Read + Seek>(r: &mut R, end: u64) -> io::Result<Vec<u8>> {
    let vendor = u32le(r)? as u64;
    let next = r
        .stream_position()?
        .checked_add(vendor)
        .filter(|n| n.saturating_add(4) <= end)
        .ok_or_else(invalid)?;
    r.seek(SeekFrom::Start(next))?;
    let count = u32le(r)?;
    if count > 16384 {
        return Err(invalid());
    }
    let mut fields = Vec::new();
    let mut body = Vec::new();
    for _ in 0..count {
        if r.stream_position()?.saturating_add(4) > end {
            return Err(invalid());
        }
        let len = u32le(r)? as usize;
        let next = r
            .stream_position()?
            .checked_add(len as u64)
            .filter(|n| *n <= end)
            .ok_or_else(invalid)?;
        let mut prefix = vec![0; len.min(64)];
        r.read_exact(&mut prefix)?;
        let key = prefix.split(|b| *b == b'=').next().unwrap_or_default();
        let keep = [
            b"TITLE".as_slice(),
            b"ARTIST",
            b"ALBUM",
            b"ALBUMARTIST",
            b"ALBUM ARTIST",
            b"GENRE",
            b"DATE",
            b"YEAR",
            b"TRACKNUMBER",
            b"TRACKTOTAL",
            b"DISCNUMBER",
            b"DISCTOTAL",
            b"TOTALTRACKS",
            b"TOTALDISCS",
        ]
        .iter()
        .any(|k| key.eq_ignore_ascii_case(k));
        if keep && len <= 64 * 1024 {
            if body.len().saturating_add(len + 4) > LIMIT {
                return Err(invalid());
            }
            let start = body.len();
            body.extend_from_slice(&(len as u32).to_le_bytes());
            body.extend_from_slice(&prefix);
            body.resize(start + 4 + len, 0);
            r.read_exact(&mut body[start + 4 + prefix.len()..])?;
            fields.push(());
        }
        r.seek(SeekFrom::Start(next))?;
    }
    let mut bytes = 0u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&(fields.len() as u32).to_le_bytes());
    bytes.extend(body);
    Ok(bytes)
}
fn flac<R: Read + Seek>(r: &mut R, length: u64) -> io::Result<Option<Tag>> {
    r.seek(SeekFrom::Start(4))?;
    let mut bytes = b"fLaC".to_vec();
    let mut found = false;
    let mut ended = false;
    for index in 0..128 {
        let mut h = [0; 4];
        r.read_exact(&mut h)?;
        let ty = h[0] & 127;
        let size = u32::from_be_bytes([0, h[1], h[2], h[3]]) as u64;
        let end = r
            .stream_position()?
            .checked_add(size)
            .filter(|n| *n <= length)
            .ok_or_else(invalid)?;
        if index == 0 {
            if ty != 0 || size != 34 {
                return Err(invalid());
            }
            bytes.extend_from_slice(&[0, 0, 0, 34]);
            let start = bytes.len();
            bytes.resize(start + 34, 0);
            r.read_exact(&mut bytes[start..])?;
        } else if ty == 4 {
            let c = comments(r, end)?;
            bytes.push(0x84);
            bytes.extend_from_slice(&(c.len() as u32).to_be_bytes()[1..]);
            bytes.extend(c);
            found = true;
            break;
        }
        if h[0] & 128 != 0 {
            ended = true;
            break;
        }
        r.seek(SeekFrom::Start(end))?;
    }
    if !found && !ended {
        return Err(invalid());
    }
    if !found && let Some(b) = bytes.get_mut(4) {
        *b |= 128;
    }
    parse(bytes, lofty::file::FileType::Flac)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lofty::tag::Accessor;
    struct Counted {
        inner: Cursor<Vec<u8>>,
        bytes: usize,
    }
    impl Read for Counted {
        fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
            let n = self.inner.read(b)?;
            self.bytes += n;
            Ok(n)
        }
    }
    impl Seek for Counted {
        fn seek(&mut self, p: SeekFrom) -> io::Result<u64> {
            self.inner.seek(p)
        }
    }
    fn frame(v: u8, id: &[u8], data: &[u8]) -> Vec<u8> {
        let mut f = id.to_vec();
        let n = data.len();
        if v == 2 {
            f.extend_from_slice(&(n as u32).to_be_bytes()[1..]);
        } else {
            f.extend_from_slice(&if v == 4 {
                size_bytes(n)
            } else {
                (n as u32).to_be_bytes()
            });
            f.extend_from_slice(&[0; 2]);
        }
        f.extend_from_slice(data);
        f
    }
    #[test]
    fn id3_versions_skip_large_picture_and_padding_without_audio_detection() {
        for v in [2, 3, 4] {
            let mut body = frame(
                v,
                if v == 2 { b"PIC" } else { b"APIC" },
                &vec![0; 2 * 1024 * 1024],
            );
            body.extend(frame(
                v,
                if v == 2 { b"TT2" } else { b"TIT2" },
                b"\0A real title",
            ));
            body.extend(frame(
                v,
                if v == 2 { b"TP1" } else { b"TPE1" },
                b"\0Test Artist",
            ));
            body.resize(body.len() + 1024 * 1024, 0);
            let mut bytes = vec![b'I', b'D', b'3', v, 0, 0];
            bytes.extend(size_bytes(body.len()));
            bytes.extend(body);
            bytes.extend_from_slice(&[0; 256]);
            let len = bytes.len() as u64;
            let mut r = Counted {
                inner: Cursor::new(bytes),
                bytes: 0,
            };
            let tag = read(&mut r, len).unwrap().unwrap().unwrap();
            assert_eq!(tag.title().as_deref(), Some("A real title"));
            assert_eq!(tag.artist().as_deref(), Some("Test Artist"));
            assert!(
                r.bytes < 1024,
                "large artwork and padding must be sought past: {}",
                r.bytes
            );
        }
    }
    #[test]
    fn flac_skips_embedded_base64_artwork_and_retains_text_after_it() {
        let mut fields = vec![b"METADATA_BLOCK_PICTURE=".to_vec()];
        fields[0].resize(2 * 1024 * 1024, b'A');
        fields.push(b"TITLE=After picture".to_vec());
        fields.push(b"ARTIST=Test Artist".to_vec());
        let mut comment = 0u32.to_le_bytes().to_vec();
        comment.extend_from_slice(&(fields.len() as u32).to_le_bytes());
        for f in fields {
            comment.extend_from_slice(&(f.len() as u32).to_le_bytes());
            comment.extend(f);
        }
        let mut bytes = b"fLaC\0\0\0\x22".to_vec();
        bytes.extend_from_slice(&[0; 34]);
        bytes.push(0x84);
        bytes.extend_from_slice(&(comment.len() as u32).to_be_bytes()[1..]);
        bytes.extend(comment);
        let len = bytes.len() as u64;
        let mut r = Counted {
            inner: Cursor::new(bytes),
            bytes: 0,
        };
        let tag = read(&mut r, len).unwrap().unwrap().unwrap();
        assert_eq!(tag.title().as_deref(), Some("After picture"));
        assert!(r.bytes < 1024);
    }
    #[test]
    fn malformed_lengths_do_not_allocate_or_publish_partial_tags() {
        let mut bytes = b"ID3\x04\0\0".to_vec();
        bytes.extend(size_bytes(1024));
        assert!(read(&mut Cursor::new(bytes), 10).is_err());
        let mut bytes = b"fLaC\0\0\0\x22".to_vec();
        bytes.extend_from_slice(&[0; 34]);
        bytes.extend_from_slice(&[0x84, 0xff, 0xff, 0xff]);
        let len = bytes.len() as u64;
        assert!(read(&mut Cursor::new(bytes), len).is_err());
    }
}
