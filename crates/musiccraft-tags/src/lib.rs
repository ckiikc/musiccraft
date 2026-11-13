//! Read-only rating inspection and byte-preserving FLAC/ID3 rating patches.
//! No audio decoder or tag-library rewrite: unrelated metadata and audio stay byte-identical.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};
const LIMIT: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scale {
    #[default]
    Explicit,
    Five,
    Hundred,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub values: Vec<String>,
    pub issue: Option<String>,
}
impl Summary {
    pub fn rating(&self, scale: Scale) -> Result<Option<u8>, String> {
        if let Some(issue) = &self.issue {
            return Err(issue.clone());
        }
        let mut result = None;
        for value in &self.values {
            let n =
                decode(value, scale).ok_or("Rating tag has an ambiguous or unsupported scale")?;
            if result.is_some_and(|previous| previous != n) {
                return Err("VOTE/RATING tags disagree".into());
            }
            result = Some(n);
        }
        Ok(result)
    }
}
fn decode(value: &str, scale: Scale) -> Option<u8> {
    let value = value.trim();
    let (n, max) = if let Some((n, max)) = value.split_once('/') {
        (
            n.trim().parse::<u16>().ok()?,
            max.trim().parse::<u16>().ok()?,
        )
    } else {
        let max = match scale {
            Scale::Five => 5,
            Scale::Hundred => 100,
            Scale::Explicit => return None,
        };
        (value.parse::<u16>().ok()?, max)
    };
    if !matches!(max, 5 | 100) || n > max || n * 5 % max != 0 {
        return None;
    }
    Some((n * 5 / max) as u8)
}
fn err() -> String {
    "Unsupported or invalid rating metadata; original file was not changed".into()
}
fn bytes<R: Read>(r: &mut R, len: usize) -> Result<Vec<u8>, String> {
    if len > LIMIT {
        return Err(err());
    }
    let mut b = vec![0; len];
    r.read_exact(&mut b).map_err(|_| err())?;
    Ok(b)
}
fn syn(b: &[u8]) -> Result<usize, String> {
    if b.len() != 4 || b.iter().any(|x| x & 128 != 0) {
        return Err(err());
    }
    Ok(b.iter().fold(0, |n, v| (n << 7) | usize::from(*v)))
}
fn ss(n: usize) -> [u8; 4] {
    [
        ((n >> 21) & 127) as u8,
        ((n >> 14) & 127) as u8,
        ((n >> 7) & 127) as u8,
        (n & 127) as u8,
    ]
}
fn vote(key: &str) -> bool {
    key.eq_ignore_ascii_case("VOTE") || key.eq_ignore_ascii_case("RATING")
}
fn text(b: &[u8]) -> Result<(String, String), String> {
    let (&enc, content) = b.split_first().ok_or_else(err)?;
    let s = match enc {
        0 => content.iter().map(|&b| char::from(b)).collect(),
        3 => String::from_utf8(content.to_vec()).map_err(|_| err())?,
        1 | 2 => {
            let (little, content) = if enc == 1 {
                if content.starts_with(&[255, 254]) {
                    (true, &content[2..])
                } else if content.starts_with(&[254, 255]) {
                    (false, &content[2..])
                } else {
                    return Err(err());
                }
            } else {
                (false, content)
            };
            if content.len() % 2 != 0 {
                return Err(err());
            }
            let words: Vec<_> = content
                .chunks_exact(2)
                .map(|b| {
                    if little {
                        u16::from_le_bytes([b[0], b[1]])
                    } else {
                        u16::from_be_bytes([b[0], b[1]])
                    }
                })
                .collect();
            String::from_utf16(&words).map_err(|_| err())?
        }
        _ => return Err(err()),
    };
    let (key, value) = s.split_once('\0').ok_or_else(err)?;
    Ok((key.into(), value.trim_end_matches('\0').into()))
}
#[derive(Clone)]
struct Field {
    start: usize,
    end: usize,
    key: String,
    value: String,
}
struct Container {
    prefix: Vec<u8>,
    fields: Vec<Field>,
    kind: Kind,
}
enum Kind {
    Flac { comment: Option<(usize, usize)> },
    Id3 { version: u8, used: usize },
}
fn container<R: Read + Seek>(r: &mut R) -> Result<Container, String> {
    r.seek(SeekFrom::Start(0)).map_err(|_| err())?;
    let h = bytes(r, 4)?;
    if h == b"fLaC" {
        let mut prefix = h;
        let mut comment = None;
        let mut fields = vec![];
        loop {
            let start = prefix.len();
            let h = bytes(r, 4)?;
            let n = (usize::from(h[1]) << 16) | (usize::from(h[2]) << 8) | usize::from(h[3]);
            if start + 4 + n > LIMIT {
                return Err(err());
            }
            let body = bytes(r, n)?;
            if h[0] & 127 == 4 {
                if comment.is_some() {
                    return Err(err());
                }
                comment = Some((start, start + 4 + n));
                let mut p = 0;
                let take = |p: &mut usize| -> Result<usize, String> {
                    let b = body.get(*p..*p + 4).ok_or_else(err)?;
                    *p += 4;
                    Ok(u32::from_le_bytes(b.try_into().unwrap()) as usize)
                };
                let vendor = take(&mut p)?;
                p = p
                    .checked_add(vendor)
                    .filter(|&p| p <= body.len())
                    .ok_or_else(err)?;
                let count = take(&mut p)?;
                if count > 100_000 {
                    return Err(err());
                }
                for _ in 0..count {
                    let at = p;
                    let n = take(&mut p)?;
                    let b = body.get(p..p + n).ok_or_else(err)?;
                    if let Ok(s) = std::str::from_utf8(b)
                        && let Some((k, v)) = s.split_once('=')
                        && vote(k)
                    {
                        if fields.len() >= 16 || v.len() > 128 {
                            return Err(err());
                        }
                        fields.push(Field {
                            start: start + 4 + at,
                            end: start + 4 + p + n,
                            key: k.into(),
                            value: v.into(),
                        });
                    }
                    p += n;
                }
                if p != body.len() {
                    return Err(err());
                }
            }
            prefix.extend(&h);
            prefix.extend(body);
            if h[0] & 128 != 0 {
                break;
            }
        }
        return Ok(Container {
            prefix,
            fields,
            kind: Kind::Flac { comment },
        });
    }
    if &h[..3] != b"ID3" {
        return Err(err());
    }
    let mut prefix = h;
    prefix.extend(bytes(r, 6)?);
    let version = prefix[3];
    // Refuse unsynchronisation, extended headers, footer or experimental layouts.
    if !matches!(version, 3 | 4) || prefix[4] != 0 || prefix[5] != 0 {
        return Err(err());
    }
    let n = syn(&prefix[6..10])?;
    prefix.extend(bytes(r, n)?);
    let mut p = 10;
    let mut fields = vec![];
    while p < prefix.len() && prefix[p] != 0 {
        let header = prefix.get(p..p + 10).ok_or_else(err)?;
        if !header[..4]
            .iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        {
            return Err(err());
        }
        let n = if version == 4 {
            syn(&header[4..8])?
        } else {
            u32::from_be_bytes(header[4..8].try_into().unwrap()) as usize
        };
        let end = p
            .checked_add(10 + n)
            .filter(|&end| end <= prefix.len())
            .ok_or_else(err)?;
        if &header[..4] == b"TXXX" {
            if header[8..10] != [0, 0] {
                return Err(err());
            }
            let (k, v) = text(&prefix[p + 10..end])?;
            if vote(&k) {
                if fields.len() >= 16 || v.len() > 128 {
                    return Err(err());
                }
                fields.push(Field {
                    start: p,
                    end,
                    key: k,
                    value: v,
                });
            }
        }
        p = end;
    }
    if prefix[p..].iter().any(|&b| b != 0) {
        return Err(err());
    }
    Ok(Container {
        prefix,
        fields,
        kind: Kind::Id3 { version, used: p },
    })
}
pub fn inspect<R: Read + Seek>(reader: &mut R) -> Summary {
    match read_values(reader) {
        Ok(values) => Summary {
            values,
            issue: None,
        },
        Err(e) => Summary {
            values: vec![],
            issue: Some(e),
        },
    }
}
fn read_values<R: Read + Seek>(r: &mut R) -> Result<Vec<String>, String> {
    r.seek(SeekFrom::Start(0)).map_err(|_| err())?;
    let h = bytes(r, 4)?;
    let mut values = vec![];
    if h == b"fLaC" {
        let mut total = 4;
        loop {
            let h = bytes(r, 4)?;
            let n = (usize::from(h[1]) << 16) | (usize::from(h[2]) << 8) | usize::from(h[3]);
            total += 4 + n;
            if total > LIMIT {
                return Err(err());
            }
            if h[0] & 127 == 4 {
                if n > 256 * 1024 {
                    return Err(err());
                }
                let b = bytes(r, n)?;
                let mut at = 0;
                let take = |at: &mut usize| -> Result<usize, String> {
                    let v = b.get(*at..*at + 4).ok_or_else(err)?;
                    *at += 4;
                    Ok(u32::from_le_bytes(v.try_into().unwrap()) as usize)
                };
                let vendor = take(&mut at)?;
                at = at
                    .checked_add(vendor)
                    .filter(|&v| v <= b.len())
                    .ok_or_else(err)?;
                let count = take(&mut at)?;
                if count > 100_000 {
                    return Err(err());
                }
                for _ in 0..count {
                    let n = take(&mut at)?;
                    let item = b.get(at..at + n).ok_or_else(err)?;
                    at += n;
                    if let Ok(item) = std::str::from_utf8(item)
                        && let Some((k, v)) = item.split_once('=')
                        && vote(k)
                    {
                        if values.len() >= 16 || v.len() > 128 {
                            return Err(err());
                        }
                        values.push(v.into());
                    }
                }
                if at != b.len() {
                    return Err(err());
                }
            } else {
                r.seek(SeekFrom::Current(n as i64)).map_err(|_| err())?;
            }
            if h[0] & 128 != 0 {
                break;
            }
        }
        return Ok(values);
    }
    if &h[..3] != b"ID3" {
        return Err(err());
    }
    let mut h = h;
    h.extend(bytes(r, 6)?);
    let version = h[3];
    if !matches!(version, 3 | 4) || h[4] != 0 || h[5] != 0 {
        return Err(err());
    }
    let size = syn(&h[6..10])?;
    if size > LIMIT {
        return Err(err());
    }
    let mut at = 0;
    while at < size {
        if size - at < 10 {
            break;
        }
        let h = bytes(r, 10)?;
        if h.iter().all(|&b| b == 0) {
            break;
        }
        let n = if version == 4 {
            syn(&h[4..8])?
        } else {
            u32::from_be_bytes(h[4..8].try_into().unwrap()) as usize
        };
        at += 10 + n;
        if at > size {
            return Err(err());
        }
        if &h[..4] == b"TXXX" {
            if n > 4096 || h[8..10] != [0, 0] {
                return Err(err());
            }
            let (k, v) = text(&bytes(r, n)?)?;
            if vote(&k) {
                if values.len() >= 16 || v.len() > 128 {
                    return Err(err());
                }
                values.push(v);
            }
        } else {
            r.seek(SeekFrom::Current(n as i64)).map_err(|_| err())?;
        }
    }
    Ok(values)
}
#[derive(Serialize)]
pub struct Patch {
    pub before: Vec<u8>,
    pub after: Vec<u8>,
    pub before_sha256: String,
    pub after_sha256: String,
}
pub fn patch<R: Read + Seek>(reader: &mut R, rating: u8, scale: Scale) -> Result<Patch, String> {
    if rating > 5 || scale == Scale::Explicit {
        return Err("Choose the numeric tag scale before writing ratings".into());
    }
    let c = container(reader)?;
    Summary {
        values: c.fields.iter().map(|f| f.value.clone()).collect(),
        issue: None,
    }
    .rating(scale)?;
    let value = match scale {
        Scale::Hundred => (rating * 20).to_string(),
        _ => (rating).to_string(),
    };
    let mut replacement = vec![];
    let keys: Vec<_> = if c.fields.is_empty() {
        vec!["RATING".to_owned()]
    } else {
        c.fields.iter().map(|f| f.key.clone()).collect()
    };
    for (i, key) in keys.iter().enumerate() {
        let v = if let Some(field) = c.fields.get(i)
            && let Some((_, max)) = field.value.split_once('/')
        {
            let max = max.trim().parse::<u16>().map_err(|_| err())?;
            format!("{}/{}", u16::from(rating) * max / 5, max)
        } else {
            value.clone()
        };
        match c.kind {
            Kind::Flac { .. } => {
                let s = format!("{key}={v}");
                replacement.extend((s.len() as u32).to_le_bytes());
                replacement.extend(s.as_bytes());
            }
            Kind::Id3 { version, .. } => {
                let s = format!("\0{key}\0{v}");
                replacement.extend(b"TXXX");
                replacement.extend(if version == 4 {
                    ss(s.len())
                } else {
                    (s.len() as u32).to_be_bytes()
                });
                replacement.extend([0, 0]);
                replacement.extend(s.as_bytes());
            }
        }
    }
    let mut after = c.prefix.clone();
    for f in c.fields.iter().rev() {
        after.drain(f.start..f.end);
    }
    match c.kind {
        Kind::Id3 { used, .. } => {
            let removed: usize = c.fields.iter().map(|f| f.end - f.start).sum();
            let used = used - removed;
            let content_end = used + replacement.len();
            after.splice(used..used, replacement);
            // Fixed-length writes only: never move or rewrite the audio payload.
            if content_end > c.prefix.len() {
                return Err("Not enough metadata padding; audio file was not changed".into());
            }
            after.resize(c.prefix.len(), 0);
            after.truncate(c.prefix.len());
        }
        Kind::Flac { comment } => {
            let (start, end) =
                comment.ok_or("FLAC has no comment block; audio file was not changed")?;
            let removed: usize = c.fields.iter().map(|f| f.end - f.start).sum();
            let mut body = after[start + 4..end - removed].to_vec();
            let vendor = u32::from_le_bytes(body[..4].try_into().unwrap()) as usize;
            let count_at = 4 + vendor;
            let count = u32::from_le_bytes(body[count_at..count_at + 4].try_into().unwrap());
            let count = if c.fields.is_empty() {
                count + 1
            } else {
                count
            };
            body[count_at..count_at + 4].copy_from_slice(&count.to_le_bytes());
            body.extend(replacement);
            let growth = body.len() as isize - (end - start - 4) as isize;
            let mut block = vec![
                after[start],
                (body.len() >> 16) as u8,
                (body.len() >> 8) as u8,
                body.len() as u8,
            ];
            block.extend(body);
            if growth == 0 {
                after.splice(start..end - removed, block);
            } else {
                let next = end - removed;
                let h = after
                    .get(next..next + 4)
                    .ok_or("FLAC rating needs a following padding block")?;
                if after[start] & 128 != 0 || h[0] & 127 != 1 {
                    return Err("FLAC rating needs a following padding block".into());
                }
                let pad = (usize::from(h[1]) << 16) | (usize::from(h[2]) << 8) | usize::from(h[3]);
                let new_pad = pad as isize - growth;
                if !(0..=0xffffff).contains(&new_pad) {
                    return Err("Not enough metadata padding; audio file was not changed".into());
                }
                block.extend([
                    h[0],
                    (new_pad >> 16) as u8,
                    (new_pad >> 8) as u8,
                    new_pad as u8,
                ]);
                block.resize(block.len() + new_pad as usize, 0);
                after.splice(start..next + 4 + pad, block);
            }
        }
    }
    if after.len() != c.prefix.len() {
        return Err(err());
    }
    let verify = inspect(&mut std::io::Cursor::new(&after));
    if verify.rating(scale)? != Some(rating) {
        return Err("Rating verification failed".into());
    }
    Ok(Patch {
        before_sha256: format!("{:x}", Sha256::digest(&c.prefix)),
        after_sha256: format!("{:x}", Sha256::digest(&after)),
        before: c.prefix,
        after,
    })
}
#[cfg(target_os = "android")]
mod android;

/// A same-directory temporary copy plus a durable private backup. Never edits the source in place.
pub fn write_local(
    path: &std::path::Path,
    rating: u8,
    scale: Scale,
    backups: &std::path::Path,
) -> Result<Summary, String> {
    use std::{fs, io::Write};
    let meta = fs::symlink_metadata(path).map_err(|_| "Audio file unavailable")?;
    if !meta.is_file() || meta.permissions().readonly() {
        return Err("Audio file is not writable".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.nlink() != 1 {
            return Err("Hard-linked audio file: rating not written".into());
        }
    }
    let mut input = fs::File::open(path).map_err(|_| err())?;
    let p = patch(&mut input, rating, scale)?;
    fs::create_dir_all(backups).map_err(|_| "Could not create rating backup")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(backups, fs::Permissions::from_mode(0o700)).map_err(|_| err())?;
    }
    let hash_file = |path: &std::path::Path| -> Result<String, String> {
        let mut f = fs::File::open(path).map_err(|_| err())?;
        let mut hash = Sha256::new();
        let mut b = [0; 65536];
        loop {
            let n = f.read(&mut b).map_err(|_| err())?;
            if n == 0 {
                break;
            }
            hash.update(&b[..n]);
        }
        Ok(format!("{:x}", hash.finalize()))
    };
    let before_hash = hash_file(path)?;
    let backup = backups.join(format!("{before_hash}.original"));
    if !backup.exists() {
        let mut out = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&backup)
            .map_err(|_| "Could not create rating backup")?;
        input.seek(SeekFrom::Start(0)).map_err(|_| err())?;
        std::io::copy(&mut input, &mut out).map_err(|_| err())?;
        out.sync_all().map_err(|_| err())?;
    }
    if hash_file(&backup)? != before_hash {
        return Err("Rating backup verification failed".into());
    }
    let parent = path.parent().ok_or_else(err)?;
    let temp = parent.join(format!(
        ".musiccraft-rating-{}-{}.tmp",
        std::process::id(),
        p.after_sha256
    ));
    let mut created = false;
    let result = (|| {
        let mut out = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|_| "Could not create safe rating copy")?;
        created = true;
        fs::set_permissions(&temp, meta.permissions()).map_err(|_| err())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let target = out.metadata().map_err(|_| err())?;
            if target.uid() != meta.uid() || target.gid() != meta.gid() {
                return Err("Could not preserve file ownership; original retained".into());
            }
        }
        out.write_all(&p.after).map_err(|_| err())?;
        input
            .seek(SeekFrom::Start(p.before.len() as u64))
            .map_err(|_| err())?;
        std::io::copy(&mut input, &mut out).map_err(|_| err())?;
        #[cfg(target_os = "macos")]
        {
            use std::os::fd::AsRawFd;
            unsafe extern "C" {
                fn fcopyfile(from: i32, to: i32, state: *mut std::ffi::c_void, flags: u32) -> i32;
            }
            // macOS COPYFILE_ACL | COPYFILE_XATTR. Preserve Finder tags/resource forks/ACLs.
            if unsafe { fcopyfile(input.as_raw_fd(), out.as_raw_fd(), std::ptr::null_mut(), 5) }
                != 0
            {
                return Err("Could not preserve file attributes; original retained".into());
            }
        }
        out.sync_all().map_err(|_| err())?;
        drop(out);
        // Byte-exact copy of the complete payload, including trailing tags. No transcoding.
        let mut check = fs::File::open(&temp).map_err(|_| err())?;
        check
            .seek(SeekFrom::Start(p.after.len() as u64))
            .map_err(|_| err())?;
        input
            .seek(SeekFrom::Start(p.before.len() as u64))
            .map_err(|_| err())?;
        let mut a = [0; 65536];
        let mut b = [0; 65536];
        loop {
            let n = input.read(&mut a).map_err(|_| err())?;
            check.read_exact(&mut b[..n]).map_err(|_| err())?;
            if a[..n] != b[..n] {
                return Err(err());
            }
            if n == 0 {
                break;
            }
        }
        let mut f = fs::File::open(&temp).map_err(|_| err())?;
        let summary = inspect(&mut f);
        if summary.rating(scale)? != Some(rating) {
            return Err(err());
        }
        if hash_file(path)? != before_hash {
            return Err("Audio file changed during rating edit; original retained".into());
        }
        fs::rename(&temp, path)
            .map_err(|_| "Atomic rating replacement failed; original retained")?;
        if let Ok(dir) = fs::File::open(parent) {
            let _ = dir.sync_all();
        }
        Ok(summary)
    })();
    if result.is_err() && created {
        let _ = fs::remove_file(temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    fn flac(items: &[&str], padding: usize) -> Vec<u8> {
        let mut body = vec![];
        body.extend(4u32.to_le_bytes());
        body.extend(b"test");
        body.extend((items.len() as u32).to_le_bytes());
        for item in items {
            body.extend((item.len() as u32).to_le_bytes());
            body.extend(item.as_bytes());
        }
        let mut b = b"fLaC".to_vec();
        b.extend([0, 0, 0, 34]);
        b.extend([0; 34]);
        b.extend([
            4,
            (body.len() >> 16) as u8,
            (body.len() >> 8) as u8,
            body.len() as u8,
        ]);
        b.extend(body);
        b.extend([
            129,
            (padding >> 16) as u8,
            (padding >> 8) as u8,
            padding as u8,
        ]);
        b.resize(b.len() + padding, 0);
        b.extend(b"UNCHANGED AUDIO PAYLOAD AND TRAILING TAGS");
        b
    }
    fn id3(version: u8, padding: usize) -> Vec<u8> {
        let mut body = vec![];
        for (key, value) in [
            ("TIT2", b"\0Synthetic title".as_slice()),
            ("TXXX", b"\0VOTE\0".as_slice()),
            ("TXXX", b"\0RATING\0".as_slice()),
        ] {
            let mut value = value.to_vec();
            if key == "TXXX" {
                value.extend(b"4");
            }
            body.extend(key.as_bytes());
            body.extend(if version == 4 {
                ss(value.len())
            } else {
                (value.len() as u32).to_be_bytes()
            });
            body.extend([0, 0]);
            body.extend(value);
        }
        body.resize(body.len() + padding, 0);
        let mut b = vec![b'I', b'D', b'3', version, 0, 0];
        b.extend(ss(body.len()));
        b.extend(body);
        b.extend(b"UNCHANGED AUDIO PAYLOAD");
        b
    }
    #[test]
    fn scales_are_explicit_and_never_guessed() {
        for (v, s, n) in [
            ("4", Scale::Five, Some(4)),
            ("80", Scale::Hundred, Some(4)),
            ("4", Scale::Explicit, None),
            ("4", Scale::Hundred, None),
            ("4/5", Scale::Explicit, Some(4)),
            ("80/100", Scale::Five, Some(4)),
            ("4.5", Scale::Five, None),
            ("255", Scale::Hundred, None),
            ("-1", Scale::Five, None),
        ] {
            assert_eq!(decode(v, s), n, "{v}");
        }
        assert!(
            Summary {
                values: vec!["3".into(), "4".into()],
                issue: None
            }
            .rating(Scale::Five)
            .is_err()
        );
    }
    #[test]
    fn patches_preserve_audio_and_unrelated_metadata() {
        for b in [
            flac(
                &[
                    "RATING=4",
                    "VOTE=4",
                    "TITLE=Preserve me",
                    "CUSTOM=Opaque tag",
                ],
                64,
            ),
            id3(3, 64),
            id3(4, 64),
        ] {
            let original = b.clone();
            let p = patch(&mut Cursor::new(&b), 2, Scale::Five).unwrap();
            assert_eq!(b, original);
            assert_eq!(p.before.len(), p.after.len());
            let mut changed = p.after.clone();
            changed.extend(&b[p.before.len()..]);
            assert_eq!(&changed[p.after.len()..], &b[p.before.len()..]);
            let summary = inspect(&mut Cursor::new(&changed));
            assert_eq!(summary.rating(Scale::Five).unwrap(), Some(2));
            assert_eq!(summary.values.len(), 2);
            assert!(
                changed.windows(11).any(|s| s == b"Preserve me")
                    || changed.windows(15).any(|s| s == b"Synthetic title")
            );
        }
    }
    #[test]
    fn invalid_unsupported_and_no_padding_never_produce_a_write() {
        for b in [
            b"RIFFnot supported".to_vec(),
            flac(&["RATING=90"], 32),
            flac(&["RATING=4", "VOTE=2"], 32),
            flac(&[], 0),
            vec![0; 2],
        ] {
            assert!(patch(&mut Cursor::new(b), 5, Scale::Five).is_err());
        }
        let mut b = id3(4, 64);
        b[5] = 128;
        assert!(patch(&mut Cursor::new(b), 5, Scale::Five).is_err());
    }
    #[test]
    fn atomic_local_edit_has_verified_backup_and_identical_payload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.flac");
        let b = flac(&["VOTE=4", "TITLE=Preserve me"], 128);
        std::fs::write(&path, &b).unwrap();
        let backups = dir.path().join("backups");
        let summary = write_local(&path, 5, Scale::Five, &backups).unwrap();
        assert_eq!(summary.rating(Scale::Five).unwrap(), Some(5));
        let backup = std::fs::read_dir(backups)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(std::fs::read(backup).unwrap(), b);
        let offset = container(&mut Cursor::new(&b)).unwrap().prefix.len();
        assert_eq!(&std::fs::read(path).unwrap()[offset..], &b[offset..]);
    }
}
