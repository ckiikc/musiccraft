//! Per-profile instance lock and authenticated, size-bounded loopback file-open requests.
//! This is local IPC, never a LAN control interface. A second process cannot overwrite state.
use super::*;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::Path,
    time::{Duration, Instant},
};
static LOCK: OnceLock<File> = OnceLock::new();
#[derive(Serialize, Deserialize)]
struct Endpoint {
    port: u16,
    token: String,
}
#[derive(Serialize, Deserialize)]
struct Request {
    token: String,
    paths: Vec<PathBuf>,
}
fn private_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}
pub fn forward_or_listen(data: &Path, paths: &[PathBuf]) -> Result<bool, String> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options
        .open(data.join(".instance-lock"))
        .map_err(|e| e.to_string())?;
    let endpoint_file = data.join(".instance-endpoint");
    match lock.try_lock_exclusive() {
        Ok(()) => {}
        Err(error) if lock_busy(&error) => {
            let deadline = Instant::now() + Duration::from_secs(3);
            while Instant::now() < deadline {
                if forward(&endpoint_file, paths).is_ok() {
                    return Ok(true);
                }
                std::thread::sleep(Duration::from_millis(80));
            }
            return Err("An existing MusicCraft instance is not responding; close it before reopening this profile.".into());
        }
        Err(error) => return Err(format!("Cannot lock MusicCraft profile: {error}")),
    }
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|e| e.to_string())?;
    let endpoint = Endpoint {
        port: listener.local_addr().map_err(|e| e.to_string())?.port(),
        token: format!(
            "{:032x}{:032x}",
            rand::random::<u128>(),
            rand::random::<u128>()
        ),
    };
    let bytes = serde_json::to_vec(&endpoint).map_err(|e| e.to_string())?;
    private_file(&endpoint_file)
        .and_then(|mut file| {
            file.write_all(&bytes)?;
            file.sync_all()
        })
        .map_err(|e| e.to_string())?;
    LOCK.set(lock)
        .map_err(|_| "Profile listener already installed")?;
    std::thread::Builder::new()
        .name("file-open-ipc".into())
        .spawn(move || {
            for mut client in listener.incoming().flatten() {
                let _ = client.set_read_timeout(Some(Duration::from_secs(1)));
                let _ = client.set_write_timeout(Some(Duration::from_secs(1)));
                let mut bytes = Vec::new();
                let request = std::io::Read::by_ref(&mut client)
                    .take(65537)
                    .read_to_end(&mut bytes)
                    .ok()
                    .filter(|_| bytes.len() <= 65536)
                    .and_then(|_| serde_json::from_slice::<Request>(&bytes).ok());
                if let Some(request) = request
                    && request.token.len() == endpoint.token.len()
                    && subtle::ConstantTimeEq::ct_eq(
                        request.token.as_bytes(),
                        endpoint.token.as_bytes(),
                    )
                    .unwrap_u8()
                        == 1
                    && request.paths.len() <= 1024
                {
                    enqueue(audio_files(request.paths));
                    if let Some(ctx) = CONTEXT.get() {
                        ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Focus);
                    }
                    let _ = client.write_all(b"OK");
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(false)
}
fn forward(path: &Path, paths: &[PathBuf]) -> Result<(), ()> {
    let bytes = std::fs::read(path).map_err(|_| ())?;
    if bytes.len() > 1024 {
        return Err(());
    }
    let endpoint: Endpoint = serde_json::from_slice(&bytes).map_err(|_| ())?;
    let mut stream = TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, endpoint.port)),
        Duration::from_millis(200),
    )
    .map_err(|_| ())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .map_err(|_| ())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(1)))
        .map_err(|_| ())?;
    // Resolve relative file-manager arguments before handing them to another process.
    let request = Request {
        token: endpoint.token,
        paths: audio_files(paths.to_vec()),
    };
    let bytes = serde_json::to_vec(&request).map_err(|_| ())?;
    if bytes.len() > 65536 {
        return Err(());
    }
    stream.write_all(&bytes).map_err(|_| ())?;
    stream.shutdown(std::net::Shutdown::Write).map_err(|_| ())?;
    let mut reply = [0; 2];
    stream.read_exact(&mut reply).map_err(|_| ())?;
    if &reply == b"OK" { Ok(()) } else { Err(()) }
}

fn lock_busy(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::WouldBlock
        || error.raw_os_error() == fs2::lock_contended_error().raw_os_error()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_contended_lock_is_routed_to_the_existing_instance() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.lock");
        let first = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let second = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        first.try_lock_exclusive().unwrap();
        let error = second.try_lock_exclusive().unwrap_err();
        assert!(lock_busy(&error), "{error}");
        assert!(!lock_busy(&std::io::Error::from(
            std::io::ErrorKind::PermissionDenied
        )));
    }
}
