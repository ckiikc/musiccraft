//! File synchronization is separate from catalogue refresh and playback queues.
use crate::network::AuthStore;
use musiccraft_sync::{
    Baseline, Plan, Snapshot, Stamp,
    filesystem::{self, Endpoint, Folder},
};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

pub struct Phone {
    root: String,
    auth: AuthStore,
    scratch: PathBuf,
    approved: Option<(String, Plan)>,
    known: Option<Snapshot>,
}
pub enum Source {
    Folder(Folder),
    Phone(Phone),
}
impl Source {
    pub fn endpoint(&mut self) -> &mut dyn Endpoint {
        match self {
            Self::Folder(f) => f,
            Self::Phone(p) => p,
        }
    }
    pub fn authorize(
        &mut self,
        plan: &Plan,
        is_a: bool,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        if let Self::Phone(phone) = self {
            let plan = if is_a { reversed(plan) } else { plan.clone() };
            phone.authorize(&plan, cancel)?;
        }
        Ok(())
    }
    pub fn finish(&mut self, cancel: &AtomicBool) -> Result<(), String> {
        if let Self::Phone(p) = self {
            p.finish(cancel)?;
        }
        Ok(())
    }
}
fn reversed(plan: &Plan) -> Plan {
    use musiccraft_sync::{Action, Side};
    let mut p = plan.clone();
    std::mem::swap(&mut p.before.a, &mut p.before.b);
    for op in &mut p.operations {
        std::mem::swap(&mut op.a, &mut op.b);
        op.action = match op.action {
            Action::Copy(Side::A) => Action::Copy(Side::B),
            Action::Copy(Side::B) => Action::Copy(Side::A),
            Action::Delete(Side::A) => Action::Delete(Side::B),
            Action::Delete(Side::B) => Action::Delete(Side::A),
            Action::Conflict => Action::Conflict,
        };
    }
    p
}
impl Phone {
    pub fn new(root: String, auth: AuthStore, scratch: PathBuf) -> Result<Self, String> {
        auth.peer(&root)
            .ok_or("Pair the phone before synchronizing")?;
        std::fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
        Ok(Self {
            root,
            auth,
            scratch,
            approved: None,
            known: None,
        })
    }
    fn request(
        &self,
        route: &str,
        body: Option<&[u8]>,
        cancel: &AtomicBool,
    ) -> Result<serde_json::Value, String> {
        let mut easy = self.easy(route)?;
        easy.timeout(Duration::from_secs(600))
            .map_err(|e| e.to_string())?;
        if let Some(body) = body {
            easy.post(true)
                .and_then(|()| easy.post_fields_copy(body))
                .map_err(|e| e.to_string())?;
        }
        let mut bytes = Vec::new();
        {
            let mut transfer = easy.transfer();
            transfer
                .write_function(|b| {
                    if bytes.len() + b.len() > 48 * 1024 * 1024 {
                        return Ok(0);
                    }
                    bytes.extend_from_slice(b);
                    Ok(b.len())
                })
                .map_err(|e| e.to_string())?;
            transfer
                .progress_function(|_, _, _, _| !cancel.load(Ordering::Relaxed))
                .map_err(|e| e.to_string())?;
            transfer.perform().map_err(|e| e.to_string())?;
        }
        if easy.response_code().unwrap_or(0) != 200 {
            return Err("Phone refused synchronization: open Synchronization on Android, select a folder and enable computer access".into());
        }
        serde_json::from_slice(&bytes).map_err(|_| "Invalid synchronization response".into())
    }
    fn easy(&self, route: &str) -> Result<curl::easy::Easy, String> {
        let mut url = url::Url::parse(&self.root).map_err(|e| e.to_string())?;
        // Route is generated locally; untrusted filenames are encoded in query pairs.
        let (path, query) = route.split_once('?').unwrap_or((route, ""));
        url.set_path(path);
        url.set_query((!query.is_empty()).then_some(query));
        let mut easy = curl::easy::Easy::new();
        easy.url(url.as_str())
            .and_then(|()| easy.connect_timeout(Duration::from_secs(5)))
            .and_then(|()| easy.signal(false))
            .and_then(|()| easy.progress(true))
            .and_then(|()| self.auth.configure(&mut easy, &self.root))
            .map_err(|e| e.to_string())?;
        Ok(easy)
    }
    pub fn authorize(&mut self, plan: &Plan, cancel: &AtomicBool) -> Result<(), String> {
        let id = filesystem::nonce();
        let body = serde_json::to_vec(&serde_json::json!({"id":id,"plan":plan}))
            .map_err(|e| e.to_string())?;
        self.request("/v1/sync/plan", Some(&body), cancel)?;
        let deadline = std::time::Instant::now() + Duration::from_secs(180);
        while std::time::Instant::now() < deadline {
            if cancel.load(Ordering::Relaxed) {
                return Err("Synchronization cancelled".into());
            }
            let status = self.request(&format!("/v1/sync/plan/{id}"), None, cancel)?;
            match status["state"].as_str() {
                Some("approved") => {
                    self.approved = Some((id, plan.clone()));
                    return Ok(());
                }
                Some("rejected") => return Err("Synchronization rejected on phone".into()),
                _ => {}
            }
            std::thread::sleep(Duration::from_millis(400));
        }
        Err("Phone approval expired; no synchronization started".into())
    }
    pub fn finish(&mut self, cancel: &AtomicBool) -> Result<(), String> {
        let (id, _) = self.approved.as_ref().ok_or("Missing approval")?;
        self.request(&format!("/v1/sync/finish/{id}"), Some(b"{}"), cancel)?;
        self.approved = None;
        Ok(())
    }
}
struct Download {
    file: std::fs::File,
    path: PathBuf,
}
impl Read for Download {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        self.file.read(b)
    }
}
impl Drop for Download {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
impl Endpoint for Phone {
    fn snapshot(&mut self, cancel: &AtomicBool) -> Result<Snapshot, String> {
        let snapshot: Snapshot =
            serde_json::from_value(self.request("/v1/sync/snapshot", None, cancel)?)
                .map_err(|e| e.to_string())?;
        musiccraft_sync::validate(&snapshot)?;
        self.known = Some(snapshot.clone());
        Ok(snapshot)
    }
    fn read(&mut self, path: &str, cancel: &AtomicBool) -> Result<Box<dyn Read + Send>, String> {
        if !musiccraft_sync::valid_path(path) {
            return Err("Unsafe path".into());
        }
        let expected = self
            .known
            .as_ref()
            .and_then(|s| s.files.get(path))
            .ok_or("File not in the verified phone snapshot")?
            .size;
        if expected > 8 * 1024 * 1024 * 1024 {
            return Err("Phone synchronization supports files up to 8 GiB".into());
        }
        let route = format!(
            "/v1/sync/file?{}",
            url::form_urlencoded::Serializer::new(String::new())
                .append_pair("path", path)
                .finish()
        );
        let mut easy = self.easy(&route)?;
        easy.timeout(Duration::from_secs(3600))
            .map_err(|e| e.to_string())?;
        let temp = self.scratch.join(filesystem::nonce());
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        let mut received = 0_u64;
        let result = (|| {
            {
                let mut t = easy.transfer();
                t.write_function(|b| {
                    received += b.len() as u64;
                    if received > expected {
                        return Ok(0);
                    }
                    file.write_all(b)
                        .map(|()| b.len())
                        .map_err(|_| curl::easy::WriteError::Pause)
                })
                .map_err(|e| e.to_string())?;
                t.progress_function(|_, _, _, _| !cancel.load(Ordering::Relaxed))
                    .map_err(|e| e.to_string())?;
                t.perform().map_err(|e| e.to_string())?;
            }
            if easy.response_code().unwrap_or(0) != 200 {
                return Err("Phone file unavailable".into());
            }
            if received != expected {
                return Err("Incomplete phone file transfer".into());
            }
            file.sync_all().map_err(|e| e.to_string())?;
            Ok(Box::new(Download {
                file: std::fs::File::open(&temp).map_err(|e| e.to_string())?,
                path: temp.clone(),
            }) as Box<dyn Read + Send>)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temp);
        }
        result
    }
    fn apply(
        &mut self,
        path: &str,
        expected: Option<&Stamp>,
        source: Option<(&Stamp, &mut dyn Read)>,
        _: &str,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        let (id, plan) = self
            .approved
            .as_ref()
            .ok_or("Phone preview not authorized")?;
        let (index, op) = plan
            .operations
            .iter()
            .enumerate()
            .find(|(_, o)| o.path == path)
            .ok_or("Operation absent from preview")?;
        if op.b.as_ref() != expected {
            return Err("Phone precondition mismatch".into());
        }
        let route = format!("/v1/sync/apply/{id}/{index}");
        let mut easy = self.easy(&route)?;
        easy.timeout(Duration::from_secs(3600))
            .map_err(|e| e.to_string())?;
        let mut reply = Vec::new();
        if let Some((stamp, input)) = source {
            easy.upload(true)
                .and_then(|()| easy.custom_request("POST"))
                .and_then(|()| easy.in_filesize(stamp.size))
                .map_err(|e| e.to_string())?;
            let mut headers = curl::easy::List::new();
            headers.append("Expect:").map_err(|e| e.to_string())?;
            easy.http_headers(headers).map_err(|e| e.to_string())?;
            let mut t = easy.transfer();
            t.read_function(|b| input.read(b).map_err(|_| curl::easy::ReadError::Abort))
                .map_err(|e| e.to_string())?;
            t.write_function(|b| {
                if reply.len() + b.len() > 4096 {
                    return Ok(0);
                }
                reply.extend_from_slice(b);
                Ok(b.len())
            })
            .map_err(|e| e.to_string())?;
            t.progress_function(|_, _, _, _| !cancel.load(Ordering::Relaxed))
                .map_err(|e| e.to_string())?;
            t.perform().map_err(|e| e.to_string())?;
        } else {
            easy.post(true)
                .and_then(|()| easy.post_fields_copy(&[]))
                .map_err(|e| e.to_string())?;
            let mut t = easy.transfer();
            t.write_function(|b| {
                reply.extend_from_slice(b);
                Ok(b.len())
            })
            .map_err(|e| e.to_string())?;
            t.perform().map_err(|e| e.to_string())?;
        }
        if easy.response_code().unwrap_or(0) != 200 {
            return Err(
                "Phone synchronization operation failed; originals retained for recovery".into(),
            );
        }
        Ok(())
    }
}
pub fn open_folder(path: &Path, other: Option<&Path>) -> Result<Folder, String> {
    let folder = Folder::open(path)?;
    if let Some(other) = other
        && let Ok(other) = std::fs::canonicalize(other)
        && (folder.path().starts_with(&other) || other.starts_with(folder.path()))
    {
        return Err("Synchronization folders must not contain each other".into());
    }
    Ok(folder)
}
pub fn baseline(path: &Path) -> Result<Option<Baseline>, String> {
    match std::fs::read(path) {
        Ok(b) => serde_json::from_slice(&b)
            .map(Some)
            .map_err(|_| "Invalid synchronization history; restore its backup".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Requires the dedicated emulator syncServer fixture and on-screen approvals"]
    fn actual_paired_android_copy_download_delete_and_finish() {
        use musiccraft_sync::{Direction, Request};
        let fixture = std::env::var("MUSICCRAFT_SYNC_PAIRING_FIXTURE")
            .expect("Emulator fixture link required");
        let mut source = crate::phone::pairing(&std::fs::read_to_string(fixture).unwrap()).unwrap();
        assert_eq!(source.host, "127.0.0.1", "Loopback emulator only");
        source.port = "18837".into();
        let auth = AuthStore::default();
        auth.insert(&source).unwrap();
        let data = tempfile::tempdir().unwrap();
        let root = data.path().join("origin");
        std::fs::create_dir(&root).unwrap();
        let content: Vec<u8> = (0..900000).map(|n| (n % 251) as u8).collect();
        let name = "A & B + prova.flac";
        std::fs::write(root.join(name), &content).unwrap();
        let cancel = AtomicBool::new(false);
        let mut a = Folder::open(&root).unwrap();
        let mut b = Phone::new(source.url().unwrap(), auth, data.path().join("transfers")).unwrap();
        let plan = musiccraft_sync::plan(Request {
            a: a.snapshot(&cancel).unwrap(),
            b: b.snapshot(&cancel).unwrap(),
            direction: Direction::Both,
            baseline: None,
        })
        .unwrap();
        assert!(plan.ready());
        eprintln!("Awaiting emulator approval for copy/download preview");
        b.authorize(&plan, &cancel).unwrap();
        let baseline = filesystem::execute(
            &plan,
            &mut a,
            &mut b,
            &data.path().join("journal.json"),
            &cancel,
            |_, _| {},
        )
        .unwrap();
        b.finish(&cancel).unwrap();
        let mut actual = Vec::new();
        b.read(name, &cancel)
            .unwrap()
            .read_to_end(&mut actual)
            .unwrap();
        assert_eq!(actual, content);
        std::fs::remove_file(root.join(name)).unwrap();
        let plan = musiccraft_sync::plan(Request {
            a: a.snapshot(&cancel).unwrap(),
            b: b.snapshot(&cancel).unwrap(),
            direction: Direction::Both,
            baseline: Some(baseline),
        })
        .unwrap();
        assert!(plan.ready());
        eprintln!("Awaiting emulator approval for recoverable deletion preview");
        b.authorize(&plan, &cancel).unwrap();
        filesystem::execute(
            &plan,
            &mut a,
            &mut b,
            &data.path().join("second-journal.json"),
            &cancel,
            |_, _| {},
        )
        .unwrap();
        b.finish(&cancel).unwrap();
        assert!(!b.snapshot(&cancel).unwrap().files.contains_key(name));
    }
    #[test]
    fn phone_as_origin_reverses_preview_without_changing_its_meaning() {
        use musiccraft_sync::{Action, Direction, Request, Side};
        let a = Snapshot {
            root: "a".into(),
            files: std::collections::BTreeMap::from([(
                "song.flac".into(),
                Stamp {
                    size: 1,
                    sha256: "a".repeat(64),
                },
            )]),
        };
        let b = Snapshot {
            root: "b".into(),
            files: Default::default(),
        };
        let plan = musiccraft_sync::plan(Request {
            a,
            b,
            baseline: None,
            direction: Direction::Forward,
        })
        .unwrap();
        let mirrored = reversed(&plan);
        assert_eq!(mirrored.operations[0].action, Action::Copy(Side::A));
        assert_eq!(reversed(&mirrored), plan);
        let after = plan.after().unwrap();
        let other = mirrored.after().unwrap();
        assert_eq!(after.a, other.b);
        assert_eq!(after.b, other.a);
    }
    #[test]
    fn nested_or_unmounted_folders_are_never_treated_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let child = dir.path().join("child");
        std::fs::create_dir(&child).unwrap();
        assert!(open_folder(dir.path(), Some(&child)).is_err());
        assert!(open_folder(&dir.path().join("missing"), None).is_err());
    }
}
