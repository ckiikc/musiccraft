//! Folder adapter. Hidden control files retain originals; no recursive delete operation exists.
use crate::{Baseline, Snapshot, Stamp, valid_path};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub fn nonce() -> String {
    format!("{:032x}", rand::random::<u128>())
}
/// Refuse to clobber a concurrently created destination, including during recovery.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn move_exclusive(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let from = CString::new(from.as_os_str().as_bytes())?;
    let to = CString::new(to.as_os_str().as_bytes())?;
    // Both C strings live throughout the call. The flags prohibit replacement atomically.
    #[cfg(target_os = "macos")]
    let result = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
    #[cfg(target_os = "linux")]
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn move_exclusive(_from: &Path, _to: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Exclusive file commit is not available on this platform",
    ))
}
fn read_identity(path: &Path) -> std::io::Result<String> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() != 32 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Invalid folder identity",
        ));
    }
    let mut id = String::new();
    File::open(path)?.take(33).read_to_string(&mut id)?;
    Ok(id)
}
pub fn fingerprint(input: &mut dyn Read, cancel: &AtomicBool) -> Result<Stamp, String> {
    let mut hash = Sha256::new();
    let mut size = 0;
    let mut bytes = [0; 65536];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("Synchronization cancelled".into());
        }
        let n = input.read(&mut bytes).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(&bytes[..n]);
        size += n as u64;
    }
    Ok(Stamp {
        size,
        sha256: format!("{:x}", hash.finalize()),
    })
}
pub fn save(path: &Path, value: &impl serde::Serialize) -> Result<(), String> {
    let tmp = path.with_extension(format!("{}.tmp", nonce()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp)
            .map_err(|e| e.to_string())?;
        file.write_all(&serde_json::to_vec(value).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        fs::rename(&tmp, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}
pub trait Endpoint: Send {
    fn invalidate(&mut self, _paths: &[String]) -> Result<(), String> {
        Ok(())
    }
    fn snapshot(&mut self, cancel: &AtomicBool) -> Result<Snapshot, String>;
    fn read(&mut self, path: &str, cancel: &AtomicBool) -> Result<Box<dyn Read + Send>, String>;
    fn apply(
        &mut self,
        path: &str,
        expected: Option<&Stamp>,
        source: Option<(&Stamp, &mut dyn Read)>,
        run: &str,
        cancel: &AtomicBool,
    ) -> Result<(), String>;
}
pub struct Folder {
    root: PathBuf,
    id: String,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct CachedStamp {
    signature: String,
    checked: u64,
    stamp: Stamp,
}
fn signature(m: &fs::Metadata) -> Option<String> {
    let modified = m
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(format!(
            "{}:{modified}:{}:{}:{}",
            m.len(),
            m.ino(),
            m.ctime(),
            m.ctime_nsec()
        ))
    }
    #[cfg(not(unix))]
    {
        Some(format!("{}:{modified}", m.len()))
    }
}
impl Folder {
    pub fn open(path: &Path) -> Result<Self, String> {
        if !path.is_absolute()
            || !path.is_dir()
            || fs::symlink_metadata(path)
                .map_err(|e| e.to_string())?
                .file_type()
                .is_symlink()
        {
            return Err("Folder unavailable: mount the NAS and choose an existing folder".into());
        }
        let root = fs::canonicalize(path).map_err(|e| e.to_string())?;
        if root.parent().is_none() {
            return Err("Select a music folder, not the filesystem root".into());
        }
        #[cfg(target_os = "macos")]
        if root.starts_with("/Volumes") {
            use std::os::unix::fs::MetadataExt;
            let volume = Path::new("/Volumes").join(
                root.components()
                    .nth(2)
                    .ok_or("Select a mounted volume")?
                    .as_os_str(),
            );
            if fs::metadata(volume).map_err(|e| e.to_string())?.dev()
                == fs::metadata("/Volumes").map_err(|e| e.to_string())?.dev()
            {
                return Err("NAS volume is not mounted".into());
            }
        }
        let control = root.join(".musiccraft-sync");
        if control.exists()
            && fs::symlink_metadata(&control)
                .map_err(|e| e.to_string())?
                .file_type()
                .is_symlink()
        {
            return Err("Unsafe control folder".into());
        }
        fs::create_dir_all(&control).map_err(|e| e.to_string())?;
        let marker = control.join("root-id");
        if fs::symlink_metadata(&marker).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("Unsafe folder identity".into());
        }
        let id = match read_identity(&marker) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let id = nonce();
                let mut f = OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&marker)
                    .map_err(|e| e.to_string())?;
                f.write_all(id.as_bytes())
                    .and_then(|()| f.sync_all())
                    .map_err(|e| e.to_string())?;
                id
            }
            Err(e) => return Err(e.to_string()),
        };
        if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Invalid folder identity".into());
        }
        Ok(Self { root, id })
    }
    pub fn path(&self) -> &Path {
        &self.root
    }
    fn checked(&self, path: &str, create: bool) -> Result<PathBuf, String> {
        if !valid_path(path) {
            return Err("Unsafe relative path".into());
        }
        if read_identity(&self.root.join(".musiccraft-sync/root-id"))
            .map_err(|_| "Folder disconnected")?
            != self.id
        {
            return Err("Folder identity changed".into());
        }
        let mut p = self.root.clone();
        let parts: Vec<_> = path.split('/').collect();
        for (i, part) in parts.iter().enumerate() {
            p.push(part);
            match fs::symlink_metadata(&p) {
                Ok(m) if m.file_type().is_symlink() || (i + 1 < parts.len() && !m.is_dir()) => {
                    return Err("Unsafe file or folder".into());
                }
                Err(e)
                    if e.kind() == std::io::ErrorKind::NotFound
                        && create
                        && i + 1 < parts.len() =>
                {
                    fs::create_dir(&p).map_err(|e| e.to_string())?
                }
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.to_string()),
                _ => {}
            }
        }
        Ok(p)
    }
    fn current(&self, path: &str, cancel: &AtomicBool) -> Result<Option<Stamp>, String> {
        let p = self.checked(path, false)?;
        match File::open(p) {
            Ok(mut f) => Ok(Some(fingerprint(&mut f, cancel)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
}
impl Endpoint for Folder {
    fn invalidate(&mut self, paths: &[String]) -> Result<(), String> {
        let path = self.root.join(".musiccraft-sync/fingerprints.json");
        if fs::metadata(&path).is_ok_and(|m| m.len() < 32 * 1024 * 1024) {
            let mut cache: std::collections::BTreeMap<String, CachedStamp> = fs::read(&path)
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_default();
            for path in paths {
                cache.remove(path);
            }
            save(&path, &cache)?;
        }
        Ok(())
    }
    fn snapshot(&mut self, cancel: &AtomicBool) -> Result<Snapshot, String> {
        let mut files = std::collections::BTreeMap::new();
        let cache_path = self.root.join(".musiccraft-sync/fingerprints.json");
        let cached: std::collections::BTreeMap<String, CachedStamp> =
            if fs::symlink_metadata(&cache_path).is_ok_and(|m| {
                m.is_file() && !m.file_type().is_symlink() && m.len() < 32 * 1024 * 1024
            }) {
                fs::read(&cache_path)
                    .ok()
                    .and_then(|b| serde_json::from_slice(&b).ok())
                    .unwrap_or_default()
            } else {
                Default::default()
            };
        let mut next = std::collections::BTreeMap::new();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs();
        let mut count = 0;
        for entry in walkdir::WalkDir::new(&self.root)
            .follow_links(false)
            .max_depth(65)
            .into_iter()
            .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'))
        {
            if cancel.load(Ordering::Relaxed) {
                return Err("Synchronization cancelled".into());
            }
            let e = entry.map_err(|e| e.to_string())?;
            count += 1;
            if count > crate::MAX_FILES * 2 || e.depth() > 64 {
                return Err("Folder scan limit exceeded; no changes applied".into());
            }
            if e.file_type().is_symlink() {
                return Err("Symbolic links are not synchronized".into());
            }
            if e.depth() == 0 || e.file_type().is_dir() {
                continue;
            }
            if !e.file_type().is_file() {
                return Err("Special files are not synchronized".into());
            }
            let path = e
                .path()
                .strip_prefix(&self.root)
                .unwrap()
                .to_str()
                .ok_or("Unsupported filename")?
                .to_string();
            let before = fs::metadata(e.path()).map_err(|e| e.to_string())?;
            let sig = signature(&before);
            let hit = cached.get(&path).filter(|c| {
                Some(&c.signature) == sig.as_ref()
                    && now.checked_sub(c.checked).is_some_and(|age| age < 86400)
            });
            let stamp = if let Some(c) = hit {
                c.stamp.clone()
            } else {
                self.current(&path, cancel)?
                    .ok_or("File changed during preview")?
            };
            let after = fs::metadata(e.path()).map_err(|e| e.to_string())?;
            if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
                return Err("File changed during preview".into());
            }
            if let Some(signature) = sig {
                next.insert(
                    path.clone(),
                    CachedStamp {
                        signature,
                        checked: hit.map_or(now, |c| c.checked),
                        stamp: stamp.clone(),
                    },
                );
            }
            files.insert(path, stamp);
        }
        let s = Snapshot {
            root: format!("folder:{}", self.id),
            files,
        };
        crate::validate(&s)?;
        if read_identity(&self.root.join(".musiccraft-sync/root-id"))
            .map_err(|_| "Folder disconnected during preview")?
            != self.id
        {
            return Err("Folder identity changed during preview".into());
        }
        save(&cache_path, &next)?;
        Ok(s)
    }
    fn read(&mut self, path: &str, cancel: &AtomicBool) -> Result<Box<dyn Read + Send>, String> {
        if cancel.load(Ordering::Relaxed) {
            return Err("Synchronization cancelled".into());
        }
        Ok(Box::new(
            File::open(self.checked(path, false)?).map_err(|e| e.to_string())?,
        ))
    }
    fn apply(
        &mut self,
        path: &str,
        expected: Option<&Stamp>,
        source: Option<(&Stamp, &mut dyn Read)>,
        run: &str,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        if run.len() != 32 || !run.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Invalid transaction".into());
        }
        if self.current(path, cancel)?.as_ref() != expected {
            return Err(format!("File changed since preview: {path}"));
        }
        let dest = self.checked(path, true)?;
        let temp = dest.with_file_name(format!(".musiccraft-{}.tmp", nonce()));
        let mut created = false;
        let result = (|| {
            if let Some((stamp, input)) = source {
                let mut output = OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&temp)
                    .map_err(|e| e.to_string())?;
                created = true;
                let mut hash = Sha256::new();
                let mut size = 0;
                let mut buf = [0; 65536];
                loop {
                    if cancel.load(Ordering::Relaxed) {
                        return Err("Synchronization cancelled".into());
                    }
                    let n = input.read(&mut buf).map_err(|e| e.to_string())?;
                    if n == 0 {
                        break;
                    }
                    size += n as u64;
                    if size > stamp.size {
                        return Err("Transfer exceeds preview size".into());
                    }
                    hash.update(&buf[..n]);
                    output.write_all(&buf[..n]).map_err(|e| e.to_string())?;
                }
                output.sync_all().map_err(|e| e.to_string())?;
                if size != stamp.size || format!("{:x}", hash.finalize()) != stamp.sha256 {
                    return Err("Transfer fingerprint mismatch".into());
                }
            }
            // Revalidate immediately before moving the original to the recoverable archive.
            if self.current(path, cancel)?.as_ref() != expected {
                return Err("Destination changed during transfer".into());
            }
            let archive = self
                .root
                .join(".musiccraft-sync/recovery")
                .join(run)
                .join(path);
            if expected.is_some() {
                let mut parent = self.root.clone();
                for part in archive
                    .parent()
                    .unwrap()
                    .strip_prefix(&self.root)
                    .unwrap()
                    .components()
                {
                    parent.push(part);
                    match fs::symlink_metadata(&parent) {
                        Ok(m) if m.file_type().is_symlink() || !m.is_dir() => {
                            return Err("Unsafe recovery folder".into());
                        }
                        Ok(_) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                            fs::create_dir(&parent).map_err(|e| e.to_string())?
                        }
                        Err(e) => return Err(e.to_string()),
                    }
                }
                if archive.exists() {
                    return Err("Recovery file already exists".into());
                }
                move_exclusive(&dest, &archive).map_err(|e| e.to_string())?;
                let checked = (|| {
                    fingerprint(
                        &mut File::open(&archive).map_err(|e| e.to_string())?,
                        cancel,
                    )
                })();
                if checked.as_ref().ok() != expected {
                    let _ = move_exclusive(&archive, &dest);
                    return Err(
                        "Destination changed or transfer cancelled while moving to recovery".into(),
                    );
                }
            }
            if created && let Err(e) = move_exclusive(&temp, &dest) {
                if expected.is_some() {
                    let _ = move_exclusive(&archive, &dest);
                }
                return Err(e.to_string());
            }
            Ok(())
        })();
        if created {
            let _ = fs::remove_file(temp);
        }
        result
    }
}
pub fn execute(
    plan: &crate::Plan,
    a: &mut dyn Endpoint,
    b: &mut dyn Endpoint,
    journal: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, usize),
) -> Result<Baseline, String> {
    let after = plan.after()?;
    if a.snapshot(cancel)? != plan.before.a || b.snapshot(cancel)? != plan.before.b {
        return Err("Folders changed: generate a new preview".into());
    }
    let affected = |side| {
        plan.operations
            .iter()
            .filter(
                |o| matches!(o.action,crate::Action::Copy(s)|crate::Action::Delete(s) if s==side),
            )
            .map(|o| o.path.clone())
            .collect::<Vec<_>>()
    };
    a.invalidate(&affected(crate::Side::A))?;
    b.invalidate(&affected(crate::Side::B))?;
    let run = nonce();
    save(
        journal,
        &serde_json::json!({"transaction":run,"plan":plan,"completed":0}),
    )?;
    for (i, op) in plan.operations.iter().enumerate() {
        use crate::{Action, Side};
        match op.action {
            Action::Copy(Side::A) => {
                let mut input = b.read(&op.path, cancel)?;
                a.apply(
                    &op.path,
                    op.a.as_ref(),
                    Some((op.b.as_ref().ok_or("Missing source")?, input.as_mut())),
                    &run,
                    cancel,
                )?;
            }
            Action::Copy(Side::B) => {
                let mut input = a.read(&op.path, cancel)?;
                b.apply(
                    &op.path,
                    op.b.as_ref(),
                    Some((op.a.as_ref().ok_or("Missing source")?, input.as_mut())),
                    &run,
                    cancel,
                )?;
            }
            Action::Delete(Side::A) => a.apply(&op.path, op.a.as_ref(), None, &run, cancel)?,
            Action::Delete(Side::B) => b.apply(&op.path, op.b.as_ref(), None, &run, cancel)?,
            Action::Conflict => return Err("Unresolved conflict".into()),
        }
        save(
            journal,
            &serde_json::json!({"transaction":run,"plan":plan,"completed":i+1}),
        )?;
        progress(i + 1, plan.operations.len());
    }
    if a.snapshot(cancel)? != after.a || b.snapshot(cancel)? != after.b {
        return Err("Final verification failed; recovery files retained".into());
    }
    Ok(after)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn exclusive_commit_never_overwrites_an_unexpected_file() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("staged");
        let b = d.path().join("unexpected");
        fs::write(&a, b"incoming").unwrap();
        fs::write(&b, b"concurrent edit").unwrap();
        assert!(move_exclusive(&a, &b).is_err());
        assert_eq!(fs::read(&b).unwrap(), b"concurrent edit");
        assert_eq!(fs::read(&a).unwrap(), b"incoming");
    }
    #[test]
    fn copy_delete_recovery_and_stale_preview() {
        let x = tempfile::tempdir().unwrap();
        let y = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        fs::write(x.path().join("song.flac"), b"original").unwrap();
        fs::write(y.path().join("extra.flac"), b"keep for recovery").unwrap();
        let mut a = Folder::open(x.path()).unwrap();
        let mut b = Folder::open(y.path()).unwrap();
        let cancel = AtomicBool::new(false);
        let p = crate::plan(crate::Request {
            a: a.snapshot(&cancel).unwrap(),
            b: b.snapshot(&cancel).unwrap(),
            baseline: None,
            direction: crate::Direction::Forward,
        })
        .unwrap();
        let after = execute(
            &p,
            &mut a,
            &mut b,
            &state.path().join("journal.json"),
            &cancel,
            |_, _| {},
        )
        .unwrap();
        assert_eq!(fs::read(y.path().join("song.flac")).unwrap(), b"original");
        assert!(!y.path().join("extra.flac").exists());
        assert_eq!(
            walkdir::WalkDir::new(y.path().join(".musiccraft-sync/recovery"))
                .into_iter()
                .filter_map(Result::ok)
                .filter(|e| e.file_type().is_file())
                .count(),
            1
        );
        assert_eq!(after.a.files, after.b.files);
        fs::write(x.path().join("song.flac"), b"changed").unwrap();
        assert!(
            execute(
                &p,
                &mut a,
                &mut b,
                &state.path().join("second.json"),
                &cancel,
                |_, _| {}
            )
            .is_err()
        );
    }
    #[cfg(unix)]
    #[test]
    fn no_symlink_and_cancelled_copy_never_replaces_destination() {
        let d = tempfile::tempdir().unwrap();
        let mut folder = Folder::open(d.path()).unwrap();
        std::os::unix::fs::symlink("/tmp", d.path().join("escape")).unwrap();
        assert!(folder.snapshot(&AtomicBool::new(false)).is_err());
        assert!(
            folder
                .apply(
                    "escape/anything",
                    None,
                    None,
                    &nonce(),
                    &AtomicBool::new(false)
                )
                .is_err()
        );
    }
}
