//! Debug-only visual fixtures. Requires an explicit isolated --data-dir.
use super::*;
impl Aurea {
    pub(crate) fn preview(&mut self, scene: &str) {
        let artwork_track = (scene.ends_with("-artwork") || scene.ends_with("-jacket"))
            .then(|| self.saved.tracks.first().cloned())
            .flatten();
        self.saved.auto_sync = false;
        self.saved.multiroom.auto_start = false;
        self.saved.multiroom.integrated = !scene.contains("multiroom");
        self.help_dialog = scene == "help" || scene == "mini-help";
        self.saved.sources.clear();
        self.credential_pending.clear();
        self.saved.mini = scene.starts_with("mini");
        if scene == "mini-vinyl-vu-side-by-side" {
            self.saved.player_appearance = player_visual::Appearance::VuMetersSideBySide;
        }
        self.saved.mini_view = if scene == "mini-compact" {
            0
        } else if scene.contains("lyrics") || scene == "mini-plain" {
            4
        } else if scene == "mini-artist" {
            3
        } else if scene.starts_with("mini-vinyl") {
            1
        } else {
            2
        };
        self.saved.theme = if scene.contains("light") {
            2
        } else if scene == "mini-orange" || scene.contains("graphite") {
            1
        } else {
            0
        };
        self.saved.shuffle = scene.ends_with("shuffle-tracks");
        self.saved.shuffle_albums = scene.ends_with("shuffle-albums");
        self.saved.pinned = false;
        self.saved.active_source = None;
        let mut phone = FtpSource::smartphone();
        phone.id = "preview-phone".into();
        phone.host = "phone.local".into();
        phone.username.clear();
        phone.rediscover = Some(false);
        let root = phone.url().unwrap();
        self.saved.sources.push(phone);
        self.saved.folders = vec!["/private/tmp/Aurea Preview NAS".into()];
        self.saved.tracks.clear();
        for album in 0..9 {
            for number in 1..=8 {
                let remote = album < 5;
                let source = if remote {
                    format!("{root}Sessions-{album:02}/{number:02}.flac")
                } else {
                    format!("/private/tmp/Aurea Preview NAS/Sessions-{album:02}/{number:02}.flac")
                };
                let mut t = Track::placeholder(source);
                t.title = [
                    "Opening Light",
                    "Beyond the Shore",
                    "Quiet Geometry",
                    "A Place to Return",
                    "Interlude No. 2",
                    "Evening Lines",
                    "The Last Train Home",
                    "After the Rain",
                ][number - 1]
                    .into();
                t.artist = ["Aurea Ensemble", "North Quartet", "Mira Studio"][album % 3].into();
                t.album = format!("Studio Sessions {:02}", album + 1);
                t.album_artist = t.artist.clone();
                t.genre = ["Jazz", "Classical", "Ambient"][album % 3].into();
                t.year = Some(2023 + (album % 3) as u32);
                t.number = number as u32;
                t.rate = if album % 2 == 0 { 96000 } else { 44100 };
                t.bits = 24;
                t.channels = 2;
                t.duration = 180.0 + number as f64 * 17.0;
                self.saved.tracks.push(t);
            }
        }
        if scene == "empty" {
            self.saved.tracks.clear();
        }
        if (scene.starts_with("mini") && scene != "mini-idle")
            || matches!(
                scene,
                "playing"
                    | "track"
                    | "loading"
                    | "mini"
                    | "mini-vinyl"
                    | "mini-compact"
                    | "long"
                    | "info"
                    | "info-artist"
            )
        {
            self.current = Some(self.saved.tracks[0].source.clone());
            self.position = 86.0;
            self.paused = scene == "track" || scene.ends_with("-paused");
            if scene.ends_with("-paused") {
                self.angle = 1.8;
            }
            self.loading = scene == "loading";
            self.output = "Fixture visiva · nessuna uscita audio aperta".into();
            self.meters = [0.65, 0.42];
            if scene == "long" {
                self.saved.tracks[0].title="Una composizione dal titolo molto lungo per verificare il troncamento dei metadati e la visibilità di tutti i controlli".into();
            }
        }
        if (scene.starts_with("mini") && scene != "mini-idle")
            || matches!(scene, "info" | "info-artist")
        {
            let pixels = (0..128 * 128)
                .map(|i| {
                    let x = (i % 128) as f32 / 128.0;
                    let y = (i / 128) as f32 / 128.0;
                    let light = ((x - 0.38).hypot(y - 0.4) * 3.0).clamp(0.0, 1.0);
                    Color32::from_rgb(
                        (198.0 - 156.0 * light) as u8,
                        (157.0 - 107.0 * light) as u8,
                        (103.0 - 51.0 * light) as u8,
                    )
                })
                .collect();
            self.art = Some(self.ctx.load_texture(
                "Synthetic preview artwork",
                egui::ColorImage::new([128, 128], pixels),
                egui::TextureOptions::LINEAR,
            ));
            let query = crate::listening::Query::from_track(&self.saved.tracks[0]).unwrap();
            let bio=crate::listening::Biography {title:"Aurea Ensemble · anteprima".into(),text:"Contenuto sintetico per il collaudo dell’interfaccia.\n\nUn piccolo ensemble immaginario esplora il dialogo tra pianoforte, contrabbasso e archi. Le sue Studio Sessions sono raccolte pensate per un ascolto continuo, con arrangiamenti essenziali e ampio spazio alla dinamica.\n\nQuesta scheda permette di verificare leggibilità, scorrimento e collegamenti senza attribuire informazioni inventate a un artista reale.".into(),url:"https://it.wikipedia.org".into()};
            let lyrics=crate::listening::Lyrics {lines: if scene=="mini-plain" { vec![] } else { crate::listening::parse_lrc("[00:00]\n[00:12]La luce attraversa la stanza\n[00:38]Un accordo rimane nell’aria\n[01:10]Ascolto il tempo che passa\n[01:45]Le note ritornano al mare\n[02:20]E il giorno comincia da qui\n[03:00]") },plain:"Testo inventato per la verifica visiva.\nLa luce attraversa la stanza\nUn accordo rimane nell’aria".into(),instrumental:false,url:"https://lrclib.net".into()};
            self.listening.fixture(query, bio, lyrics);
        }
        self.scanning = scene == "scanning";
        self.audio_dialog = scene == "audio";
        self.settings_dialog = matches!(
            scene,
            "settings"
                | "multiroom"
                | "mini-settings"
                | "mini-multiroom"
                | "mini-multiroom-controls"
                | "audio-dsp"
                | "integrated-server"
                | "mini-integrated-server"
                | "integrated-running"
        );
        self.settings_page = if scene.contains("multiroom") || scene.contains("integrated") {
            3
        } else if scene == "audio-dsp" {
            1
        } else {
            0
        };
        if scene.starts_with("settings-page-") {
            self.settings_dialog = true;
            self.settings_page = scene
                .trim_start_matches("settings-page-")
                .parse::<u8>()
                .unwrap_or(0)
                .min(3);
        }
        if scene == "integrated-running" {
            self.start_integrated();
        }
        if scene == "mini-multiroom-controls" {
            self.ctx
                .data_mut(|d| d.insert_temp(egui::Id::new("preview-room-controls"), true));
        }
        if scene == "audio-dsp" {
            self.saved.dsp.mode = crate::dsp::ReplayGain::Album;
            self.ctx
                .data_mut(|d| d.insert_temp(egui::Id::new("preview-audio-controls"), true));
        }
        if scene.contains("multiroom") {
            self.room_refresh_at = Instant::now() + Duration::from_secs(60);
            self.room_state = crate::multiroom::Snapshot {
                version: "9.1.1 · fixture".into(),
                players: vec![
                    crate::multiroom::Player {
                        id: "fixture-a".into(),
                        name: "Studio · Test".into(),
                        model: "Squeezelite".into(),
                        connected: true,
                    },
                    crate::multiroom::Player {
                        id: "fixture-b".into(),
                        name: "Living Room · Test".into(),
                        model: "piCorePlayer".into(),
                        connected: true,
                    },
                ],
                groups: vec!["Studio · Test, Living Room · Test".into()],
                status: Some(crate::multiroom::Status {
                    player: "fixture-a".into(),
                    mode: "play".into(),
                    title: "Synthetic playback fixture".into(),
                    volume: Some(35.0),
                    volume_control: true,
                    muted: false,
                }),
            };
            self.saved.multiroom.selected = "fixture-a".into();
            self.room_volume = 35.0;
        }

        self.sources_dialog = scene == "sources";
        self.track_dialog = scene == "track";
        self.info_dialog = matches!(scene, "info" | "info-artist");
        self.info_lyrics = scene != "info-artist";
        self.ftp_dialog = scene == "network";
        if self.ftp_dialog {
            self.ftp = self.saved.sources[0].clone();
            self.ftp_address = self.ftp.url().unwrap();
        }
        self.library_revision = self.library_revision.wrapping_add(1);
        self.refresh_browser();
        if scene == "filtered" {
            let key = self.browser.entries[0][0].key.clone();
            self.browser.select(0, &key, false, false);
            self.refresh_browser();
        }
        if let Some(track) = artwork_track {
            if !library::is_remote(&track.source) {
                let item = crate::jobs::gallery::Item {
                    source: std::path::Path::new(&track.source)
                        .with_file_name("front.png")
                        .to_string_lossy()
                        .into_owned(),
                    name: "front.png".into(),
                    front: true,
                };
                if let Some(image) = crate::jobs::gallery::load(
                    &item,
                    &self.cache,
                    &self.auth,
                    &std::sync::atomic::AtomicU64::new(1),
                    1,
                ) {
                    self.art = Some(self.ctx.load_texture(
                        "Fixture album jacket",
                        egui::ColorImage::from_rgba_unmultiplied(image.size, &image.rgba),
                        Default::default(),
                    ));
                }
            }
            self.current = Some(track.source.clone());
            self.saved.tracks = vec![track];
            if scene.ends_with("-artwork") {
                self.open_cover_viewer();
            }
        }
    }
}
