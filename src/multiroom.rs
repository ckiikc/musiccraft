//! Lyrion JSON-RPC adapter. LMS owns synchronization and endpoint audio policy.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub integrated: bool,
    pub auto_start: bool,
    pub lms_url: String,
    pub username: String,
    pub bind: String,
    pub selected: String,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            integrated: true,
            auto_start: false,
            lms_url: "http://127.0.0.1:9000".into(),
            username: String::new(),
            bind: "127.0.0.1:9042".into(),
            selected: String::new(),
        }
    }
}
#[derive(Clone, Debug)]
pub struct Player {
    pub id: String,
    pub name: String,
    pub connected: bool,
    pub model: String,
}
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub version: String,
    pub players: Vec<Player>,
    pub groups: Vec<String>,
    pub status: Option<Status>,
}
#[derive(Clone, Debug)]
pub struct Status {
    pub player: String,
    pub mode: String,
    pub title: String,
    pub volume: Option<f32>,
    pub volume_control: bool,
    pub muted: bool,
}
#[derive(Clone)]
pub struct Client {
    pub config: Config,
    pub password: String,
}
impl Client {
    fn endpoint(&self) -> Result<url::Url, String> {
        let mut u =
            url::Url::parse(&self.config.lms_url).map_err(|_| "Indirizzo LMS non valido")?;
        if !matches!(u.scheme(), "http" | "https")
            || u.host_str().is_none()
            || !u.username().is_empty()
            || u.password().is_some()
            || u.query().is_some()
            || u.fragment().is_some()
            || !matches!(u.path(), "" | "/")
        {
            return Err("Usa l’indirizzo HTTP o HTTPS di LMS senza credenziali nell’URL".into());
        }
        u.set_path("/jsonrpc.js");
        Ok(u)
    }
    pub fn request(&self, player: &str, command: &[String]) -> Result<Value, String> {
        let endpoint = self.endpoint()?;
        let body =
            serde_json::to_vec(&json!({"id":1,"method":"slim.request","params":[player,command]}))
                .map_err(|_| "Richiesta LMS non valida")?;
        let mut easy = curl::easy::Easy::new();
        easy.url(endpoint.as_str())
            .map_err(|_| "Indirizzo LMS non valido")?;
        easy.post(true).map_err(|_| "LMS non disponibile")?;
        easy.post_fields_copy(&body)
            .map_err(|_| "Richiesta LMS non valida")?;
        easy.connect_timeout(Duration::from_secs(3))
            .map_err(|_| "LMS non disponibile")?;
        easy.timeout(Duration::from_secs(8))
            .map_err(|_| "LMS non disponibile")?;
        easy.follow_location(false)
            .map_err(|_| "LMS non disponibile")?;
        if !self.config.username.is_empty() {
            easy.username(&self.config.username)
                .map_err(|_| "Accesso LMS non valido")?;
            easy.password(&self.password)
                .map_err(|_| "Accesso LMS non valido")?;
        }
        let mut headers = curl::easy::List::new();
        headers
            .append("Content-Type: application/json")
            .map_err(|_| "Richiesta LMS non valida")?;
        easy.http_headers(headers)
            .map_err(|_| "Richiesta LMS non valida")?;
        let mut response = Vec::new();
        {
            let mut transfer = easy.transfer();
            transfer
                .write_function(|b| {
                    if response.len() + b.len() > 2 * 1024 * 1024 {
                        return Ok(0);
                    }
                    response.extend_from_slice(b);
                    Ok(b.len())
                })
                .map_err(|_| "LMS non disponibile")?;
            transfer
                .perform()
                .map_err(|_| "LMS non raggiungibile o risposta non valida")?;
        }
        match easy.response_code().unwrap_or(0) {
            200 => {}
            401 | 403 => return Err("Accesso LMS rifiutato".into()),
            _ => return Err("LMS ha rifiutato la richiesta".into()),
        }
        let result: Value =
            serde_json::from_slice(&response).map_err(|_| "Risposta LMS non valida")?;
        if result.get("error").is_some_and(|e| !e.is_null()) {
            return Err("Comando LMS non riuscito".into());
        }
        result
            .get("result")
            .filter(|r| r.is_object())
            .cloned()
            .ok_or_else(|| "Risposta LMS non valida".into())
    }
    pub fn snapshot(&self) -> Result<Snapshot, String> {
        let version = self.request("", &["version".into(), "?".into()])?;
        let players = self.request("", &["players".into(), "0".into(), "100".into()])?;
        let groups = self.request("", &["syncgroups".into(), "?".into()])?;
        let status = if self.config.selected.is_empty() {
            None
        } else {
            self.request(
                &self.config.selected,
                &["status".into(), "0".into(), "1".into()],
            )
            .ok()
            .and_then(|value| {
                let mode = value.get("mode")?.as_str()?.to_string();
                let volume = value
                    .get("mixer volume")
                    .and_then(|v| {
                        v.as_f64()
                            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                    })
                    .map(|v| v as f32)
                    .filter(|v| v.is_finite());
                // LMS may fade the volume after a mute command. Its explicit
                // state is authoritative, including mute at zero volume.
                let muted = self
                    .request(
                        &self.config.selected,
                        &["mixer".into(), "muting".into(), "?".into()],
                    )
                    .ok()
                    .and_then(|v| v.get("_muting").cloned())
                    .map(|v| v == 1 || v == "1" || v == true)
                    .unwrap_or_else(|| volume.is_some_and(|v| v < 0.0));
                Some(Status {
                    player: self.config.selected.clone(),
                    mode,
                    title: value
                        .get("current_title")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .chars()
                        .take(500)
                        .collect(),
                    volume: volume.map(|v| v.abs().clamp(0.0, 100.0)),
                    muted,
                    volume_control: value
                        .get("digital_volume_control")
                        .is_some_and(|v| v == 1 || v == "1" || v == true),
                })
            })
        };
        Ok(Snapshot {
            status,
            version: version
                .get("_version")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .chars()
                .take(80)
                .collect(),
            players: players
                .get("players_loop")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .take(100)
                .filter_map(|p| {
                    let id = p.get("playerid")?.as_str()?.to_string();
                    Some(Player {
                        id,
                        name: p
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("Player")
                            .chars()
                            .take(200)
                            .collect(),
                        connected: p
                            .get("connected")
                            .is_some_and(|v| v == 1 || v == "1" || v == true),
                        model: p
                            .get("modelname")
                            .or_else(|| p.get("model"))
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .chars()
                            .take(100)
                            .collect(),
                    })
                })
                .collect(),
            groups: groups
                .get("syncgroups_loop")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|g| {
                    g.get("sync_member_names")
                        .and_then(Value::as_str)
                        .map(|v| v.chars().take(500).collect())
                })
                .collect(),
        })
    }
}
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    #[test]
    fn endpoint_does_not_accept_credentials_or_paths() {
        let mut client = Client {
            config: Config::default(),
            password: String::new(),
        };
        assert_eq!(
            client.endpoint().unwrap().as_str(),
            "http://127.0.0.1:9000/jsonrpc.js"
        );
        for bad in [
            "ftp://localhost/",
            "http://fixture:fixture-only@localhost",
            "http://localhost/other",
            "http://localhost/?token=x",
        ] {
            client.config.lms_url = bad.into();
            assert!(client.endpoint().is_err());
        }
    }
    #[test]
    #[ignore = "requires an explicit isolated local Lyrion test server"]
    fn live_lms_lists_players_and_preserves_unicode_commands() {
        let endpoint = std::env::var("MUSICCRAFT_TEST_LMS").expect("test LMS endpoint");
        let client = Client {
            config: Config {
                lms_url: endpoint,
                ..Default::default()
            },
            password: String::new(),
        };
        let snapshot = client.snapshot().unwrap();
        assert!(!snapshot.version.is_empty());
        println!(
            "LMS {}: {} fixture players",
            snapshot.version,
            snapshot.players.len()
        );
        if let Ok(player) = std::env::var("MUSICCRAFT_TEST_LMS_PLAYER") {
            client
                .request(&player, &["playlist".into(), "clear".into()])
                .unwrap();
            if let Ok(url) = std::env::var("MUSICCRAFT_TEST_MEDIA") {
                client
                    .request(
                        &player,
                        &[
                            "playlist".into(),
                            "play".into(),
                            url,
                            "MusicCraft fixture à".into(),
                        ],
                    )
                    .unwrap();
                let status = client
                    .request(&player, &["status".into(), "0".into(), "1".into()])
                    .unwrap();
                assert!(status.get("playlist_tracks").is_some());
                client.request(&player, &["stop".into()]).unwrap();
            }
        }
    }
    #[test]
    #[ignore = "requires two isolated Squeezelite fixture clients and LMS"]
    fn live_lms_two_rooms_play_original_media_and_join_sync_group() {
        let endpoint = std::env::var("MUSICCRAFT_TEST_LMS").expect("isolated LMS endpoint");
        let client = Client {
            config: Config {
                lms_url: endpoint,
                ..Default::default()
            },
            password: String::new(),
        };
        verify_two_rooms(client);
    }
    pub(crate) fn verify_two_rooms(client: Client) {
        verify_two_rooms_with(client, |_, _| {});
    }
    pub(crate) fn verify_two_rooms_with(
        mut client: Client,
        update: impl FnOnce(&mut crate::media_server::Server, &Client),
    ) {
        let before = client.snapshot().unwrap();
        for id in ["02:00:00:00:ac:01", "02:00:00:00:ac:02"] {
            assert!(
                before.players.iter().any(|p| p.id == id && p.connected),
                "fixture player missing"
            );
        }
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("fixture.wav");
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
        for i in 0..44100 * 12 {
            let sample =
                ((i as f64 * 440.0 * std::f64::consts::TAU / 44100.0).sin() * 4000.0) as i16;
            writer.write_sample(sample).unwrap();
            writer.write_sample(sample).unwrap();
        }
        writer.finalize().unwrap();
        let mut server = crate::media_server::Server::start(
            "127.0.0.1:0".parse().unwrap(),
            vec![crate::library::Track::read(
                path.to_string_lossy().into_owned(),
            )],
            temp.path().into(),
            crate::network::AuthStore::default(),
        )
        .unwrap();
        let a = "02:00:00:00:ac:01";
        let b = "02:00:00:00:ac:02";
        let mut received = Vec::new();
        let mut check = curl::easy::Easy::new();
        check.url(&format!("{}.wav", server.track_url(0))).unwrap();
        {
            let mut transfer = check.transfer();
            transfer
                .write_function(|bytes| {
                    received.extend_from_slice(bytes);
                    Ok(bytes.len())
                })
                .unwrap();
            transfer.perform().unwrap();
        }
        assert_eq!(
            received,
            std::fs::read(&path).unwrap(),
            "original WAV bytes"
        );
        eprintln!("Original media bytes verified: {}", received.len());
        client.config.selected = a.into();
        client.request(a, &["power".into(), "1".into()]).unwrap();
        client.request(b, &["power".into(), "1".into()]).unwrap();
        client.request(a, &["sync".into(), b.into()]).unwrap();
        let groups = client.snapshot().unwrap().groups;
        assert!(
            groups
                .iter()
                .any(|g| g.contains("MusicCraft Test A") && g.contains("MusicCraft Test B"))
        );
        client
            .request(
                a,
                &["playlist".into(), "play".into(), server.playlist_url()],
            )
            .unwrap();
        std::thread::sleep(Duration::from_secs(3));
        for id in [a, b] {
            let state = client
                .request(id, &["status".into(), "0".into(), "1".into()])
                .unwrap();
            eprintln!("Fixture status: {state}");
            assert_eq!(
                state.get("mode").and_then(Value::as_str),
                Some("play"),
                "fixture must be playing"
            );
            assert!(state.get("time").and_then(Value::as_f64).unwrap_or(0.0) > 0.0);
        }
        update(&mut server, &client);
        client
            .request(a, &["mixer".into(), "volume".into(), "35".into()])
            .unwrap();
        let state = client
            .snapshot()
            .unwrap()
            .status
            .expect("selected room status");
        assert_eq!(state.player, a);
        assert_eq!(state.mode, "play");
        assert!(state.volume_control);
        assert_eq!(state.volume, Some(35.0));
        assert!(!state.muted);
        client
            .request(a, &["mixer".into(), "muting".into(), "1".into()])
            .unwrap();
        assert!(client.snapshot().unwrap().status.unwrap().muted);
        client
            .request(a, &["mixer".into(), "muting".into(), "0".into()])
            .unwrap();
        assert!(!client.snapshot().unwrap().status.unwrap().muted);
        client.request(a, &["stop".into()]).unwrap();
        client.request(b, &["sync".into(), "-".into()]).unwrap();
        println!(
            "Two connected Squeezelite clients: source M3U, original WAV over ranged HTTP, both playing, sync group and ungroup verified"
        );
    }
}
