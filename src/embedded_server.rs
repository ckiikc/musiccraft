//! Owned LMS subprocess. No launch agent, global installation or shared LMS profile.
use crate::multiroom::{Client, Config};
use std::{
    fs::{self, File},
    io::Write,
    net::{Ipv4Addr, TcpListener, UdpSocket},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

const WATCHDOG: &str = include_str!("../scripts/lms-watchdog.pl");
pub fn interfaces() -> Vec<Ipv4Addr> {
    let mut addresses: Vec<_> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| {
            i.is_oper_up()
                && !i.is_p2p()
                && (!cfg!(target_os = "macos") || i.name.starts_with("en"))
        })
        .filter_map(|i| match i.addr {
            if_addrs::IfAddr::V4(v) if v.ip.is_private() => Some(v.ip),
            _ => None,
        })
        .collect();
    addresses.sort();
    addresses.dedup();
    addresses
}
#[derive(Clone)]
pub struct Connection {
    pub client: Client,
    pub host: Ipv4Addr,
}
pub enum Event {
    Ready(Connection),
    Stopped,
    Failed(&'static str),
    LibraryUpdated,
    LibraryUpdateFailed(String),
}
enum Control {
    Start(Ipv4Addr, u64, String),
    Stop(u64),
    Publish(u64, Arc<str>),
    Quit,
}
pub struct Manager {
    tx: mpsc::Sender<Control>,
    rx: mpsc::Receiver<(u64, Event)>,
    generation: Arc<AtomicU64>,
}
struct Running {
    child: Child,
}
impl Drop for Running {
    fn drop(&mut self) {
        // Signal only the PID returned by our own spawn, and reap it before reuse.
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = Command::new("/bin/kill")
                .args(["-TERM", &self.child.id().to_string()])
                .status();
            let deadline = Instant::now() + Duration::from_secs(7);
            while Instant::now() < deadline {
                if self.child.try_wait().ok().flatten().is_some() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
pub fn runtime() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let bundled = executable
        .parent()?
        .parent()?
        .join("Resources/LyrionRuntime.app");
    if bundled.join("Contents/MacOS/perl").is_file() {
        return Some(bundled);
    }
    #[cfg(debug_assertions)]
    {
        let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor/LyrionRuntime.app");
        if dev.join("Contents/MacOS/perl").is_file() {
            return Some(dev);
        }
    }
    None
}
impl Manager {
    pub fn new(profile: PathBuf, runtime: Option<PathBuf>, ctx: eframe::egui::Context) -> Self {
        let (tx, control) = mpsc::channel();
        let (events, rx) = mpsc::channel();
        let generation = Arc::new(AtomicU64::new(0));
        std::thread::spawn(move || {
            let mut running: Option<Running> = None;
            let mut running_token = 0;
            let mut connection: Option<Connection> = None;
            let mut pending_playlist = None;
            let mut publish_after = Instant::now();
            loop {
                match control.recv_timeout(Duration::from_millis(200)) {
                    Ok(Control::Quit) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Ok(Control::Stop(token)) => {
                        running = None;
                        connection = None;
                        pending_playlist = None;
                        let _ = events.send((token, Event::Stopped));
                        ctx.request_repaint();
                    }
                    Ok(Control::Start(host, token, playlist)) if running.is_none() => {
                        running_token = token;
                        let result = runtime
                            .as_ref()
                            .ok_or("Bundled server component unavailable")
                            .and_then(|runtime| start(runtime, &profile, host, &playlist));
                        match result {
                            Ok((process, connection_value)) => {
                                running = Some(process);
                                publish_after = Instant::now() + Duration::from_millis(1200);
                                connection = Some(connection_value.clone());
                                let _ = events.send((token, Event::Ready(connection_value)));
                            }
                            Err(message) => {
                                let _ = events.send((token, Event::Failed(message)));
                            }
                        }
                        ctx.request_repaint();
                    }
                    Ok(Control::Publish(token, playlist))
                        if token == running_token && running.is_some() =>
                    {
                        // Keep only the latest catalog while a previous scan finishes.
                        pending_playlist = Some(playlist);
                        publish_after =
                            publish_after.max(Instant::now() + Duration::from_millis(200));
                    }
                    Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                if Instant::now() >= publish_after
                    && let (Some(playlist), Some(connection)) = (&pending_playlist, &connection)
                {
                    match publish_playlist(&profile, playlist, &connection.client) {
                        Ok(()) => {
                            pending_playlist = None;
                            let _ = events.send((running_token, Event::LibraryUpdated));
                            publish_after = Instant::now() + Duration::from_millis(1200);
                        }
                        Err(message) => {
                            let _ =
                                events.send((running_token, Event::LibraryUpdateFailed(message)));
                            publish_after = Instant::now() + Duration::from_secs(5);
                        }
                    }
                    ctx.request_repaint();
                }
                if let Some(process) = &mut running
                    && process.child.try_wait().ok().flatten().is_some()
                {
                    running = None;
                    connection = None;
                    pending_playlist = None;
                    let _ = events.send((
                        running_token,
                        Event::Failed("Integrated server stopped unexpectedly"),
                    ));
                    ctx.request_repaint();
                }
            }
        });
        Self { tx, rx, generation }
    }
    pub fn start(&self, host: Ipv4Addr, playlist: String) {
        let token = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = self.tx.send(Control::Start(host, token, playlist));
    }
    pub fn update_playlist(&self, playlist: Arc<str>) {
        let token = self.generation.load(Ordering::Relaxed);
        let _ = self.tx.send(Control::Publish(token, playlist));
    }
    pub fn stop(&self) {
        let token = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = self.tx.send(Control::Stop(token));
    }
    pub fn poll(&self) -> Option<Event> {
        while let Ok((token, event)) = self.rx.try_recv() {
            if token == self.generation.load(Ordering::Relaxed) {
                return Some(event);
            }
        }
        None
    }
}
impl Drop for Manager {
    fn drop(&mut self) {
        let _ = self.tx.send(Control::Quit);
    }
}

fn publish_playlist(profile: &Path, playlist: &str, client: &Client) -> Result<(), String> {
    let directory = profile.join("playlists");
    let temporary = directory.join("MusicCraft.pending");
    fs::write(&temporary, playlist).map_err(|_| "Server profile is not writable")?;
    fs::rename(&temporary, directory.join("MusicCraft.m3u8"))
        .map_err(|_| "Server profile is not writable")?;
    // This only refreshes our private playlist directory. It neither reloads
    // player queues nor restarts LMS or its connected/synchronized players.
    client.request("", &["rescan".into(), "playlists".into()])?;
    Ok(())
}

fn start(
    runtime: &Path,
    profile: &Path,
    host: Ipv4Addr,
    playlist: &str,
) -> Result<(Running, Connection), &'static str> {
    if !host.is_private() && !host.is_loopback() {
        return Err("Choose a private local address");
    }
    // Never attach to, stop, or replace an existing user's LMS on this interface.
    let tcp = TcpListener::bind((host, 3483)).map_err(|_| "Player port 3483 is already in use")?;
    let udp = UdpSocket::bind((host, 3483)).map_err(|_| "Player port 3483 is already in use")?;
    let http = TcpListener::bind((host, 0)).map_err(|_| "Cannot reserve a server port")?;
    let port = http
        .local_addr()
        .map_err(|_| "Cannot reserve a server port")?
        .port();
    for dir in ["prefs/plugin", "cache", "tmp", "logs", "playlists"] {
        fs::create_dir_all(profile.join(dir)).map_err(|_| "Server profile is not writable")?;
    }
    fs::write(profile.join("playlists/MusicCraft.m3u8"), playlist)
        .map_err(|_| "Server profile is not writable")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(profile, fs::Permissions::from_mode(0o700))
            .map_err(|_| "Server profile is not writable")?;
    }
    let perl = runtime.join("Contents/MacOS/perl");
    let server = runtime.join("Contents/Resources/server/slimserver.pl");
    if !perl.is_file() || !server.is_file() {
        return Err("Bundled server component unavailable");
    }
    let secret = format!("{:032x}", rand::random::<u128>());
    let mut hash = Command::new(&perl)
        .args([
            "-MDigest::SHA=sha1_base64",
            "-e",
            "local $/; print sha1_base64(<STDIN>)",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "Bundled server component unavailable")?;
    hash.stdin
        .take()
        .ok_or("Cannot configure the integrated server")?
        .write_all(secret.as_bytes())
        .map_err(|_| "Cannot configure the integrated server")?;
    let hash = hash
        .wait_with_output()
        .map_err(|_| "Cannot configure the integrated server")?;
    let hash =
        String::from_utf8(hash.stdout).map_err(|_| "Cannot configure the integrated server")?;
    if hash.len() != 27
        || !hash
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"+/".contains(&c))
    {
        return Err("Cannot configure the integrated server");
    }
    // Regenerate only our generated settings. LMS's other namespaced preference
    // files (including player volumes/groups) are left intact across restarts.
    fs::write(profile.join("prefs/server.prefs"), format!(
        "---\n_version: 1\nauthorize: 1\nusername: musiccraft\npassword: '{hash}'\ncheckVersion: 0\nautoDownloadUpdate: 0\ndisableStatistics: 1\nprotectSettings: 1\nmediadirs: []\nplaylistdir: '{}'\nlibraryname: MusicCraft\nlanguage: EN\n", profile.join("playlists").display().to_string().replace('\'', "''")
    )).map_err(|_| "Cannot configure the integrated server")?;
    // The CLI plugin migrates an unset preference to 9090 after command-line
    // parsing. Seed its current schema so --cliport 0 remains disabled.
    fs::write(
        profile.join("prefs/plugin/cli.prefs"),
        "---\n_version: 1\ncliport: '0'\n",
    )
    .map_err(|_| "Cannot configure the integrated server")?;
    let wrapper = profile.join("lms-watchdog.pl");
    fs::write(&wrapper, WATCHDOG).map_err(|_| "Cannot configure the integrated server")?;
    let log = profile.join("logs/process.log");
    if fs::metadata(&log).is_ok_and(|m| m.len() > 1024 * 1024) {
        let _ = fs::rename(&log, profile.join("logs/process.previous.log"));
    }
    let output = File::create(log).map_err(|_| "Server profile is not writable")?;
    let mut command = Command::new(&perl);
    command
        .arg(&wrapper)
        .arg(std::process::id().to_string())
        .arg(&server)
        .arg("--prefsdir")
        .arg(profile.join("prefs"))
        .arg("--cachedir")
        .arg(profile.join("cache"))
        .arg("--tmpdir")
        .arg(profile.join("tmp"))
        .arg("--logdir")
        .arg(profile.join("logs"))
        .args([
            "--httpport",
            &port.to_string(),
            "--httpaddr",
            &host.to_string(),
            "--playeraddr",
            &host.to_string(),
            "--advertiseaddr",
            &host.to_string(),
            "--cliport",
            "0",
            "--cliaddr",
            "127.0.0.1",
            "--nosetup",
            "--norestart",
            "--nostatistics",
        ])
        .env_remove("PERL5LIB")
        .env_remove("PERL5OPT")
        .stdin(Stdio::null())
        .stdout(
            output
                .try_clone()
                .map_err(|_| "Server profile is not writable")?,
        )
        .stderr(output);
    drop((tcp, udp, http));
    let mut process = Running {
        child: command
            .spawn()
            .map_err(|_| "Cannot start the integrated server")?,
    };
    let client = Client {
        config: Config {
            lms_url: format!("http://{host}:{port}"),
            username: "musiccraft".into(),
            ..Default::default()
        },
        password: secret,
    };
    let deadline = Instant::now() + Duration::from_secs(25);
    while Instant::now() < deadline {
        if process.child.try_wait().ok().flatten().is_some() {
            return Err("Cannot start the integrated server");
        }
        if client.request("", &["version".into(), "?".into()]).is_ok() {
            let _ = client.request("", &["rescan".into(), "playlists".into()]);
            return Ok((process, Connection { client, host }));
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    Err("Integrated server startup timed out")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_public_addresses_without_touching_profile() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            start(
                Path::new("/nonexistent"),
                &dir.path().join("unused"),
                "8.8.8.8".parse().unwrap(),
                ""
            )
            .err(),
            Some("Choose a private local address")
        );
        assert!(!dir.path().join("unused").exists());
    }
    #[test]
    #[ignore = "explicit bundled LMS lifecycle integration on loopback port 3483"]
    fn bundled_server_starts_authenticated_restarts_and_stops() {
        let dir = tempfile::tempdir().unwrap();
        let rt = runtime().expect("bundled or development runtime");
        let cli_was_open = std::net::TcpStream::connect((Ipv4Addr::LOCALHOST, 9090)).is_ok();
        for _ in 0..2 {
            let playlist =
                "#EXTM3U\n#EXTINF:12,MusicCraft fixture\nhttp://127.0.0.1:9/fixture.wav\n";
            let (process, connection) =
                start(&rt, dir.path(), Ipv4Addr::LOCALHOST, playlist).unwrap();
            if !cli_was_open {
                assert!(
                    std::net::TcpStream::connect((Ipv4Addr::LOCALHOST, 9090)).is_err(),
                    "CLI remains disabled"
                );
            }
            assert!(
                connection
                    .client
                    .snapshot()
                    .unwrap()
                    .version
                    .starts_with("9.1.1")
            );
            let unauthenticated = Client {
                password: String::new(),
                ..connection.client.clone()
            };
            assert!(unauthenticated.snapshot().is_err());
            let discovery = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            discovery
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            discovery
                .send_to(b"eNAME\0JSON\0VERS\0", (Ipv4Addr::LOCALHOST, 3483))
                .unwrap();
            let mut packet = [0; 1024];
            let (n, _) = discovery.recv_from(&mut packet).unwrap();
            assert_eq!(packet[0], b'E');
            assert!(String::from_utf8_lossy(&packet[..n]).contains("MusicCraft"));
            let deadline = Instant::now() + Duration::from_secs(8);
            loop {
                let lists = connection
                    .client
                    .request("", &["playlists".into(), "0".into(), "100".into()])
                    .unwrap();
                if lists
                    .get("playlists_loop")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|lists| {
                        lists.iter().any(|p| {
                            p["playlist"]
                                .as_str()
                                .is_some_and(|s| s.starts_with("MusicCraft"))
                        })
                    })
                {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "shared playlist visible to remote controllers"
                );
                std::thread::sleep(Duration::from_millis(100));
            }
            let conflict = start(&rt, &dir.path().join("other"), Ipv4Addr::LOCALHOST, "");
            assert_eq!(conflict.err(), Some("Player port 3483 is already in use"));
            drop(process);
            TcpListener::bind((Ipv4Addr::LOCALHOST, 3483)).expect("owned process stopped");
        }
    }
    #[test]
    #[ignore = "explicit integrated lifecycle race test; uses loopback port 3483"]
    fn rapid_start_stop_start_keeps_only_the_current_server() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Manager::new(
            dir.path().into(),
            runtime(),
            eframe::egui::Context::default(),
        );
        manager.start(Ipv4Addr::LOCALHOST, String::new());
        manager.stop();
        manager.start(Ipv4Addr::LOCALHOST, String::new());
        let deadline = Instant::now() + Duration::from_secs(35);
        loop {
            assert!(Instant::now() < deadline, "current server ready");
            if let Some(event) = manager.poll() {
                match event {
                    Event::Ready(c) => {
                        assert!(c.client.snapshot().unwrap().version.starts_with("9.1.1"));
                        break;
                    }
                    Event::Failed(e) => panic!("{e}"),
                    Event::Stopped => panic!("stale stop event"),
                    Event::LibraryUpdated | Event::LibraryUpdateFailed(_) => {
                        panic!("unexpected publication")
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        manager.stop();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if matches!(manager.poll(), Some(Event::Stopped)) {
                break;
            }
            assert!(Instant::now() < deadline, "owned server stopped");
            std::thread::sleep(Duration::from_millis(100));
        }
        TcpListener::bind((Ipv4Addr::LOCALHOST, 3483)).expect("no orphan after restart");
    }
    #[test]
    #[ignore = "explicit integrated LMS with two owned Squeezelite clients; uses port 3483"]
    fn integrated_server_plays_two_owned_rooms_without_external_lms() {
        use std::io::Read;
        struct Player {
            child: Child,
            reader: Option<std::thread::JoinHandle<usize>>,
        }
        impl Drop for Player {
            fn drop(&mut self) {
                let _ = self.child.kill();
                let _ = self.child.wait();
                if let Some(reader) = self.reader.take() {
                    let decoded = reader.join().unwrap_or_default();
                    if !std::thread::panicking() {
                        assert!(decoded > 0, "decoded fixture audio");
                    }
                }
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let manager = Manager::new(
            dir.path().into(),
            runtime(),
            eframe::egui::Context::default(),
        );
        manager.start(Ipv4Addr::LOCALHOST, String::new());
        let deadline = Instant::now() + Duration::from_secs(35);
        let connection = loop {
            match manager.poll() {
                Some(Event::Ready(connection)) => break connection,
                Some(Event::Failed(e)) => panic!("{e}"),
                _ => {}
            }
            assert!(Instant::now() < deadline, "server ready");
            std::thread::sleep(Duration::from_millis(100));
        };
        let binary =
            std::env::var("MUSICCRAFT_SQUEEZELITE").expect("explicit isolated fixture binary");
        let players: Vec<_> = [1, 2]
            .into_iter()
            .map(|i| {
                let mut child = Command::new(&binary)
                    .args([
                        "-s",
                        "127.0.0.1",
                        "-o",
                        "-",
                        "-a",
                        "16",
                        "-r",
                        "44100",
                        "-c",
                        "pcm,flac",
                        "-m",
                        &format!("02:00:00:00:ac:0{i}"),
                        "-n",
                        &format!("MusicCraft Test {}", if i == 1 { "A" } else { "B" }),
                    ])
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap();
                let mut output = child.stdout.take().unwrap();
                let reader = std::thread::spawn(move || {
                    let mut bytes = [0; 65536];
                    let mut nonzero = 0;
                    while let Ok(n) = output.read(&mut bytes) {
                        if n == 0 {
                            break;
                        }
                        nonzero += usize::from(bytes[..n].iter().any(|b| *b != 0));
                        std::thread::sleep(Duration::from_secs_f64(n as f64 / 176400.0));
                    }
                    nonzero
                });
                Player {
                    child,
                    reader: Some(reader),
                }
            })
            .collect();
        let deadline = Instant::now() + Duration::from_secs(10);
        while connection
            .client
            .snapshot()
            .unwrap()
            .players
            .iter()
            .filter(|p| p.connected)
            .count()
            < 2
        {
            assert!(Instant::now() < deadline, "owned players connected");
            std::thread::sleep(Duration::from_millis(100));
        }
        crate::multiroom::tests::verify_two_rooms_with(connection.client, |origin, client| {
            let a = "02:00:00:00:ac:01";
            let before = client
                .request(a, &["status".into(), "0".into(), "1".into()])
                .unwrap();
            let old_playlist = origin.playlist_url();
            let original_url = origin.track_url(0);
            let original = origin.shared_track_for_test(0);
            let mut added = original.clone();
            added.source = std::path::Path::new(&original.source)
                .with_file_name("added.wav")
                .to_string_lossy()
                .into_owned();
            added.title = "Added during playback".into();
            std::fs::copy(&original.source, &added.source).unwrap();
            assert!(origin.update(vec![added, original], &[]));
            manager.update_playlist(origin.playlist());
            let deadline = Instant::now() + Duration::from_secs(8);
            loop {
                match manager.poll() {
                    Some(Event::LibraryUpdated) => break,
                    Some(Event::Failed(e)) => panic!("{e}"),
                    Some(Event::LibraryUpdateFailed(error)) => {
                        panic!("publication failed: {error}")
                    }
                    _ => {}
                }
                assert!(Instant::now() < deadline, "catalog published");
                std::thread::sleep(Duration::from_millis(100));
            }
            loop {
                let lists = client
                    .request("", &["playlists".into(), "0".into(), "100".into()])
                    .unwrap();
                let count = lists["playlists_loop"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|p| {
                        p["playlist"]
                            .as_str()
                            .is_some_and(|s| s.starts_with("MusicCraft"))
                    })
                    .and_then(|p| p["id"].as_i64())
                    .map(|id| {
                        client
                            .request(
                                "",
                                &[
                                    "playlists".into(),
                                    "tracks".into(),
                                    "0".into(),
                                    "10".into(),
                                    format!("playlist_id:{id}"),
                                    "tags:u".into(),
                                ],
                            )
                            .unwrap()
                    });
                if count.as_ref().is_some_and(|v| v["count"] == 2) {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "LMS controllers see the new track"
                );
                std::thread::sleep(Duration::from_millis(100));
            }
            assert_eq!(origin.playlist_url(), old_playlist);
            assert_eq!(origin.track_url(0), original_url);
            for id in [a, "02:00:00:00:ac:02"] {
                let after = client
                    .request(id, &["status".into(), "0".into(), "1".into()])
                    .unwrap();
                assert_eq!(after["mode"], "play");
                assert!(after["time"].as_f64().unwrap() > before["time"].as_f64().unwrap());
                assert_eq!(after["playlist_tracks"], 1, "current queue is not replaced");
                assert_eq!(
                    after["sync_master"], before["sync_master"],
                    "sync group stays intact"
                );
            }
            println!(
                "Live catalog: LMS sees 2 tracks, both rooms continue with increasing position, same queue, group and URLs"
            );
        });
        drop(players);
        manager.stop();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !matches!(manager.poll(), Some(Event::Stopped)) {
            assert!(Instant::now() < deadline, "owned server stopped");
            std::thread::sleep(Duration::from_millis(100));
        }
        TcpListener::bind((Ipv4Addr::LOCALHOST, 3483)).expect("integrated server released port");
    }
}
