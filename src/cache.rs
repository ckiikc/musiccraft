use crate::library::{is_remote, validate_url};
use std::{
    fs::{self, File},
    hash::{Hash, Hasher},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

static AUDIO_TRANSFERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub(crate) fn audio_transfer_pending() -> bool {
    AUDIO_TRANSFERS.load(Ordering::Relaxed) != 0
}
pub(crate) struct AudioTransfer;
impl AudioTransfer {
    pub(crate) fn begin() -> Self {
        AUDIO_TRANSFERS.fetch_add(1, Ordering::Relaxed);
        Self
    }
}
impl Drop for AudioTransfer {
    fn drop(&mut self) {
        AUDIO_TRANSFERS.fetch_sub(1, Ordering::Relaxed);
    }
}

pub const MAX_FILE: u64 = 1024 * 1024 * 1024;
const MAX_CACHE: u64 = 2 * MAX_FILE;

pub fn materialize(
    source: &str,
    root: &Path,
    generation: &AtomicU64,
    ticket: u64,
    auth: &crate::network::AuthStore,
    mut progress: impl FnMut(u64),
) -> Result<PathBuf, String> {
    materialize_limited(
        source,
        root,
        generation,
        ticket,
        auth,
        MAX_FILE,
        &mut progress,
    )
}

pub fn materialize_limited(
    source: &str,
    root: &Path,
    generation: &AtomicU64,
    ticket: u64,
    auth: &crate::network::AuthStore,
    limit: u64,
    mut progress: impl FnMut(u64),
) -> Result<PathBuf, String> {
    materialize_inner(
        source,
        root,
        (generation, ticket),
        auth,
        limit,
        || false,
        &mut progress,
    )
}

#[cfg(test)]
pub fn materialize_cancellable(
    source: &str,
    root: &Path,
    generation: &AtomicU64,
    ticket: u64,
    auth: &crate::network::AuthStore,
    cancelled: impl Fn() -> bool,
    progress: impl FnMut(u64),
) -> Result<PathBuf, String> {
    materialize_inner(
        source,
        root,
        (generation, ticket),
        auth,
        MAX_FILE,
        cancelled,
        progress,
    )
}

fn materialize_inner(
    source: &str,
    root: &Path,
    playback: (&AtomicU64, u64),
    auth: &crate::network::AuthStore,
    limit: u64,
    extra_cancelled: impl Fn() -> bool,
    mut progress: impl FnMut(u64),
) -> Result<PathBuf, String> {
    let (generation, ticket) = playback;
    let cancelled = || generation.load(Ordering::Relaxed) != ticket || extra_cancelled();
    if cancelled() {
        return Err("Caricamento annullato".into());
    }
    fs::create_dir_all(root).map_err(|_| "Cache non disponibile".to_string())?;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hash);
    if !is_remote(source) {
        // A fingerprint permits offline use without accidentally reusing changed NAS content.
        if let Ok(meta) = fs::metadata(source) {
            if meta.len() > limit {
                return Err("Brano oltre il limite cache di 1 GiB".into());
            }
            meta.len().hash(&mut hash);
            meta.modified().ok().hash(&mut hash);
        } else {
            return Err("File non raggiungibile. Verifica che il NAS sia montato".into());
        }
    } else {
        validate_url(source)?;
    }
    let key = format!("{:016x}.audio", hash.finish());
    let target = root.join(key);
    if target.is_file() && fs::metadata(&target).is_ok_and(|m| m.len() <= limit) {
        return Ok(target);
    }
    let part = target.with_extension(format!("{}.part", rand::random::<u64>()));
    let outcome = if is_remote(source) {
        let _priority = (limit > 8 * 1024 * 1024).then(AudioTransfer::begin);
        let mut command = Command::new("/usr/bin/curl");
        command
            .args([
                "--config",
                "-",
                "--silent",
                "--fail",
                "--location",
                "--proto",
                "=http,https,ftp,ftps",
                "--proto-redir",
                "=http,https,ftp,ftps",
                "--connect-timeout",
                if limit <= 8 * 1024 * 1024 { "4" } else { "10" },
                "--max-time",
                if limit <= 8 * 1024 * 1024 {
                    "20"
                } else {
                    "300"
                },
                "--max-filesize",
            ])
            .arg(limit.to_string())
            .arg("--output")
            .args([] as [&str; 0])
            .arg(&part)
            .arg("--url")
            .arg(source)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = crate::network::authenticated_spawn(&mut command, auth, source)?;
        loop {
            let size = fs::metadata(&part).map_or(0, |m| m.len());
            progress(size);
            if cancelled() || size > limit {
                let _ = child.kill();
                let _ = child.wait();
                break Err("Caricamento annullato".into());
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    break if status.success() {
                        Ok(())
                    } else {
                        Err("Download fallito: verifica indirizzo, accesso e rete".into())
                    };
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(80)),
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err("Trasferimento interrotto".into());
                }
            }
        }
    } else {
        (|| {
            let mut input =
                File::open(source).map_err(|_| "File NAS non disponibile".to_string())?;
            let mut output =
                File::create(&part).map_err(|_| "Impossibile creare la cache".to_string())?;
            let mut buf = vec![0; 256 * 1024];
            let mut total = 0u64;
            loop {
                if cancelled() {
                    return Err("Caricamento annullato".into());
                }
                let n = input
                    .read(&mut buf)
                    .map_err(|_| "Lettura NAS interrotta".to_string())?;
                if n == 0 {
                    break;
                }
                total = total.saturating_add(n as u64);
                if total > limit {
                    return Err("Brano oltre 1 GiB".into());
                }
                output
                    .write_all(&buf[..n])
                    .map_err(|_| "Cache piena o non scrivibile".to_string())?;
                progress(total);
            }
            output
                .sync_all()
                .map_err(|_| "Scrittura cache non riuscita".to_string())
        })()
    };
    if let Err(error) = outcome {
        let _ = fs::remove_file(&part);
        return Err(error);
    }
    fs::rename(&part, &target).map_err(|_| "Impossibile finalizzare la cache".to_string())?;
    trim(root, &target);
    Ok(target)
}

pub(crate) fn trim(root: &Path, keep: &Path) {
    let mut entries: Vec<_> = fs::read_dir(root)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|s| s == "audio"))
        .filter_map(|e| Some((e.path(), e.metadata().ok()?)))
        .collect();
    let mut total: u64 = entries.iter().map(|(_, m)| m.len()).sum();
    entries.sort_by_key(|(_, m)| m.modified().ok());
    for (path, meta) in entries {
        if total <= MAX_CACHE {
            break;
        }
        if path != keep && fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(meta.len());
        }
    }
}

#[cfg(test)]
mod plan_cancellation_tests {
    use super::*;
    #[test]
    fn obsolete_plan_does_not_touch_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("unused");
        let generation = AtomicU64::new(7);
        assert!(
            materialize_cancellable(
                "http://127.0.0.1:9/not-requested.wav",
                &cache,
                &generation,
                7,
                &Default::default(),
                || true,
                |_| {}
            )
            .is_err()
        );
        assert!(!cache.exists());
        assert_eq!(generation.load(Ordering::Relaxed), 7);
    }
    #[test]
    #[ignore = "explicit slow loopback transfer cancellation"]
    fn new_plan_cancels_an_inflight_transfer_without_changing_playback_generation() {
        use std::{
            net::TcpListener,
            sync::{Arc, atomic::AtomicBool, mpsc},
            time::Instant,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (started, ready) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut headers = Vec::new();
            let mut byte = [0];
            while !headers.ends_with(b"\r\n\r\n") {
                socket.read_exact(&mut byte).unwrap();
                headers.push(byte[0]);
            }
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: 16777216\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            started.send(()).unwrap();
            for _ in 0..512 {
                if socket.write_all(&[5; 32768]).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let generation = Arc::new(AtomicU64::new(7));
        let cancelled = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let cache = dir.path().to_path_buf();
        let cancel = cancelled.clone();
        let epoch = generation.clone();
        let worker = std::thread::spawn(move || {
            let result = materialize_cancellable(
                &format!("http://{address}/track.wav"),
                &cache,
                &epoch,
                7,
                &Default::default(),
                || cancel.load(Ordering::Relaxed),
                |_| {},
            );
            tx.send(result).unwrap();
        });
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        let at = Instant::now();
        cancelled.store(true, Ordering::Relaxed);
        assert!(rx.recv_timeout(Duration::from_secs(2)).unwrap().is_err());
        assert!(at.elapsed() < Duration::from_secs(2));
        worker.join().unwrap();
        server.join().unwrap();
        assert_eq!(generation.load(Ordering::Relaxed), 7);
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            0,
            "partial files removed"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Synthetic FTP server provided by scripts/test_ftp.py; isolated audio priority state"]
    fn live_ftp_tag_priority_yields_to_audio_transfers() {
        let root = std::env::var("AUREA_TEST_FTP_URL").unwrap();
        let mut source = crate::network::FtpSource::smartphone();
        source.set_url(&root).unwrap();
        source.username = "fixture".into();
        source.password = "fixture-only".into();
        let auth = crate::network::AuthStore::default();
        auth.insert(&source).unwrap();
        let folder = crate::network::child_url(&root, "Album Città", true).unwrap();
        let url = crate::network::child_url(&folder, "02 Interlude.flac", false).unwrap();
        let generation = std::sync::Arc::new(AtomicU64::new(0));
        let guard = AudioTransfer::begin();
        assert!(audio_transfer_pending());
        assert!(
            crate::remote_tags::read(
                crate::library::Track::placeholder(url.clone()),
                &auth,
                generation.clone(),
                0
            )
            .is_err()
        );
        drop(guard);
        assert!(!audio_transfer_pending());
        assert_eq!(
            crate::remote_tags::read(
                crate::library::Track::placeholder(url),
                &auth,
                generation,
                0
            )
            .unwrap()
            .title,
            "Interlude"
        );
    }
    #[test]
    fn cache_is_byte_exact_invalidates_and_cancels_without_partial_files() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("nas.flac");
        fs::write(&source, b"exact bytes").unwrap();
        let gen_id = AtomicU64::new(1);
        let root = dir.path().join("cache");
        let p = materialize(
            source.to_str().unwrap(),
            &root,
            &gen_id,
            1,
            &Default::default(),
            |_| {},
        )
        .unwrap();
        assert_eq!(fs::read(p).unwrap(), b"exact bytes");
        fs::write(&source, b"different content").unwrap();
        assert!(
            materialize(
                source.to_str().unwrap(),
                &root,
                &gen_id,
                2,
                &Default::default(),
                |_| {}
            )
            .is_err()
        );
        assert!(
            fs::read_dir(&root)
                .unwrap()
                .all(|e| e.unwrap().path().extension().unwrap() != "part")
        );
        let p = materialize(
            source.to_str().unwrap(),
            &root,
            &gen_id,
            1,
            &Default::default(),
            |_| {},
        )
        .unwrap();
        assert_eq!(fs::read(p).unwrap(), b"different content");
    }
}

/// A confirmed remote tag edit changes the cached header under the same URL.
/// Existing decoders retain their open file; future opens must fetch the new bytes.
pub(crate) fn invalidate_remote(source: &str, root: &Path) {
    if is_remote(source) {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        source.hash(&mut hash);
        let _ = fs::remove_file(root.join(format!("{:016x}.audio", hash.finish())));
    }
}
