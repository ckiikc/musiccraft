use crate::{
    audio::{self, Audio, Command, Event},
    discovery::{self, Lan, Outcome, Reason},
    history::History,
    jobs::{self, Job},
    library::{self, Track},
    network::{self, AuthStore, FtpSource},
    sync::{self as source_sync, Target},
};
use eframe::egui::{self, Color32, RichText, pos2, vec2};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

mod browser;
mod connection;
mod cover_viewer;
mod desktop;
mod help;
mod listening_ui;
mod miniplayer;
mod palette;
#[cfg(test)]
mod performance;
mod phone_pair;
mod player_visual;
#[cfg(any(debug_assertions, test))]
mod preview;
mod settings;
mod tag_progress;
mod widgets;
mod window_drag;
mod window_geometry;
mod window_placement;
use widgets::{Icons, caption, clipped, section};
#[cfg(debug_assertions)]
pub(crate) use window_drag::enable_trace as enable_window_drag_trace;

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    tracks: Vec<Track>,
    sources: Vec<FtpSource>,
    folders: Vec<PathBuf>,
    active_source: Option<String>,
    auto_sync: bool,
    ftp_scan_pace: discovery::ScanPace,
    device: String,
    exclusive: bool,
    strict_integer: bool,
    mini: bool,
    mini_view: u8,
    mini_last_view: u8,
    mini_size: Option<[f32; 2]>,
    full_size: Option<[f32; 2]>,
    shuffle_session: Option<ShuffleSession>,
    theme: u8,
    pinned: bool,
    shuffle: bool,
    shuffle_albums: bool,
    repeat: bool,
    facets_visible: bool,
    facet_fields: [crate::facets::Field; 3],
    library_sort: crate::facets::Sort,
    volume_db: f32,
    online_covers: bool,
    save_covers: bool,
    dsp: crate::dsp::Config,
    native_rate: bool,
    language: String,
    ui_scale: f32,
    reduce_motion: bool,
    player_appearance: player_visual::Appearance,
    appearance_by_year: bool,
    appearance_periods: player_visual::YearAppearance,
    artwork_window: Option<window_placement::Placement>,
    settings_window: Option<window_placement::Placement>,
    multiroom: crate::multiroom::Config,
}

impl Default for Saved {
    fn default() -> Self {
        Self {
            tracks: Vec::new(),
            sources: Vec::new(),
            folders: Vec::new(),
            active_source: None,
            auto_sync: true,
            ftp_scan_pace: Default::default(),
            device: String::new(),
            exclusive: false,
            strict_integer: false,
            mini: true,
            mini_view: 1,
            mini_last_view: 1,
            mini_size: None,
            full_size: None,
            shuffle_session: None,
            theme: 0,
            pinned: true,
            shuffle: false,
            shuffle_albums: false,
            repeat: false,
            facets_visible: true,
            facet_fields: [
                crate::facets::Field::Artist,
                crate::facets::Field::Album,
                crate::facets::Field::Folder,
            ],
            library_sort: Default::default(),
            volume_db: -12.0,
            online_covers: true,
            save_covers: true,
            dsp: Default::default(),
            native_rate: true,
            language: String::new(),
            ui_scale: 1.0,
            reduce_motion: false,
            player_appearance: Default::default(),
            appearance_by_year: false,
            appearance_periods: Default::default(),
            artwork_window: None,
            settings_window: None,
            multiroom: Default::default(),
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct ShuffleSession {
    cycle: crate::shuffle::Cycle,
    selection: Option<Vec<String>>,
}

struct RediscoveryRequest {
    token: u64,
    source: FtpSource,
    networks: Vec<Lan>,
    cancel: Arc<AtomicBool>,
}

pub struct Aurea {
    ctx: egui::Context,
    saved: Saved,
    audio: Audio,
    auth: AuthStore,
    credential_pending: HashSet<String>,
    icons: Icons,
    browser: crate::facets::Browser,
    mini_index: miniplayer::MiniIndex,
    vinyl_index: miniplayer::VinylIndex,
    remote_tags: crate::remote_tags::Library,
    tag_progress: tag_progress::Cache,
    library_revision: u64,
    playback_selection: Option<Vec<String>>,
    restored_selection: bool,
    cache: PathBuf,
    tx: mpsc::Sender<Job>,
    rx: mpsc::Receiver<Job>,
    cancel: Arc<AtomicBool>,
    scanning: bool,
    scan_automatic: bool,
    sync_scheduler: source_sync::Scheduler,
    sync_status: String,
    connections: connection::Gate,
    discovery: Option<RediscoveryRequest>,
    networks: Vec<Lan>,
    network_probe_due: Instant,
    network_probe_pending: bool,
    network_changed: bool,
    root_aliases: Vec<(String, String)>,
    scan_added: usize,
    history: History,
    shuffle_cycle: crate::shuffle::Cycle,
    current: Option<String>,
    requested: Option<String>,
    ticket: u64,
    position: f64,
    paused: bool,
    loading: bool,
    loaded_bytes: u64,
    output: String,
    output_exclusive: bool,
    message: String,
    query: String,
    devices: Vec<String>,
    volume_db: f32,
    mute: bool,
    native: bool,
    remember_password: bool,
    ftp_address: String,
    pending_ftp: Option<(FtpSource, Option<String>, Option<usize>)>,
    phone_pair_dialog: bool,
    phone_pair: phone_pair::State,
    ftp_dialog: bool,
    ftp_focus_host: bool,
    audio_dialog: bool,
    sources_dialog: bool,
    track_dialog: bool,
    info_dialog: bool,
    info_lyrics: bool,
    settings_dialog: bool,
    settings_page: u8,
    settings_geometry: window_placement::Tracker,
    settings_focused_on_open: bool,
    help_dialog: bool,
    help_query: String,
    language_fonts_open: bool,
    media_server: Option<crate::media_server::Server>,
    shared_source: Option<String>,
    shared_revision: u64,
    embedded_server: crate::embedded_server::Manager,
    embedded_connection: Option<crate::embedded_server::Connection>,
    embedded_starting: bool,
    embedded_autostart_attempted: bool,
    embedded_message: String,
    room_state: crate::multiroom::Snapshot,
    room_password: String,
    room_rx: Option<mpsc::Receiver<settings::RoomUpdate>>,
    room_refresh_at: Instant,
    room_volume: f32,
    room_volume_dragging: bool,
    room_message: String,
    listening: crate::listening::Info,
    lyric_follow: bool,
    last_lyric_line: Option<usize>,
    ftp: FtpSource,
    ftp_edit: Option<usize>,
    ftp_old: Option<String>,
    directory: String,
    entries: Vec<network::Entry>,
    browsing: bool,
    art: Option<egui::TextureHandle>,
    cover_viewer: cover_viewer::Viewer,
    art_origin: Option<(String, String)>,
    online_covers: Arc<AtomicBool>,
    save_covers: Arc<AtomicBool>,
    covers: HashMap<String, Option<library::Artwork>>,
    planned: Option<String>,
    meters: [f32; 2],
    angle: f32,
    reel_motion: player_visual::ReelMotion,
    needle_motion: player_visual::NeedleMotion,
    last_frame: Instant,
    background_due: Instant,
    screenshot: Option<PathBuf>,
    screenshot_sent: bool,
    viewport_mini: Option<bool>,
    window_geometry: window_geometry::State,
    viewport_zoom: f32,
    viewport_pinned: Option<bool>,
    test_exit: Option<Instant>,
}

impl Aurea {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        data: PathBuf,
        paths: Vec<PathBuf>,
        screenshot: Option<PathBuf>,
        test_seconds: Option<u64>,
    ) -> Self {
        cc.egui_ctx.enable_accesskit();
        let mut saved: Saved = cc
            .storage
            .and_then(|s| eframe::get_value(s, "aurea"))
            .unwrap_or_default();
        if saved.sources.is_empty()
            && let Ok(bytes) = std::fs::read(data.join("ftp-sources.json"))
        {
            saved.sources = serde_json::from_slice(&bytes).unwrap_or_default();
        }
        if saved.tracks.is_empty()
            && let Ok(bytes) = std::fs::read(data.join("tracks-seed.json"))
        {
            saved.tracks = serde_json::from_slice(&bytes).unwrap_or_default();
            saved.tracks.truncate(library::MAX_TRACKS);
        }
        for track in &mut saved.tracks {
            track.repair_placeholder();
        }
        #[cfg(test)]
        crate::i18n::set("it");
        #[cfg(not(test))]
        {
            let language = if saved.language.is_empty() {
                crate::i18n::system_language()
            } else {
                saved.language.clone()
            };
            crate::i18n::set(&language);
        }
        listening_ui::configure_language_font(&cc.egui_ctx);
        for source in &mut saved.sources {
            source.rediscover = Some(source.discovery_enabled());
            if source.id.is_empty() {
                source.id = format!("{:016x}", rand::random::<u64>());
            }
        }
        saved.dsp = saved.dsp.sanitized();
        saved.mini_last_view = if (1..=4).contains(&saved.mini_view) {
            saved.mini_view
        } else {
            saved.mini_last_view.clamp(1, 4)
        };
        saved.theme %= 3;
        saved.appearance_periods.sanitize();
        palette::configure(&cc.egui_ctx, saved.theme);
        let auth = AuthStore::default();
        for source in &saved.sources {
            let _ = auth.insert(source);
        }
        let cache = data.join("cache");
        let embedded_server = crate::embedded_server::Manager::new(
            data.join("multiroom/server"),
            crate::embedded_server::runtime(),
            cc.egui_ctx.clone(),
        );
        let audio = Audio::new(cache.clone(), auth.clone());
        let _ = audio.tx.send(Command::Dsp(saved.dsp));
        let native_rate = saved.native_rate;
        let (tx, rx) = mpsc::channel();
        let credentials: Vec<_> = saved
            .sources
            .iter()
            .filter(|s| !s.username.is_empty() && s.password.is_empty())
            .cloned()
            .collect();
        let credential_pending = credentials.iter().map(|s| s.id.clone()).collect();
        let credential_tx = tx.clone();
        let credential_ctx = cc.egui_ctx.clone();
        std::thread::spawn(move || {
            for source in credentials {
                let password = crate::credentials::load(&source);
                if credential_tx
                    .send(Job::Credential {
                        id: source.id,
                        username: source.username,
                        password,
                    })
                    .is_err()
                {
                    break;
                }
                credential_ctx.request_repaint();
            }
        });
        saved.ui_scale = if saved.ui_scale.is_finite() {
            saved.ui_scale.clamp(1.0, 1.3)
        } else {
            1.0
        };
        cc.egui_ctx.set_zoom_factor(saved.ui_scale);
        let ui_scale = saved.ui_scale;
        let volume_db = valid_volume(saved.volume_db);
        let online_covers = Arc::new(AtomicBool::new(saved.online_covers));
        let save_covers = Arc::new(AtomicBool::new(saved.save_covers));
        let remote_tags =
            crate::remote_tags::Library::new(data.join("metadata-index"), &mut saved.tracks);
        let aliases = library::deduplicate_tracks(&mut saved.tracks);
        if let Some(session) = &mut saved.shuffle_session {
            session.cycle.remap_sources(&aliases);
            if let Some(selection) = &mut session.selection {
                let mut seen = HashSet::new();
                for id in selection.iter_mut() {
                    if let Some(kept) = aliases.get(id) {
                        *id = kept.clone();
                    }
                }
                selection.retain(|id| seen.insert(id.clone()));
            }
        }
        let restored_selection = saved.shuffle_session.is_some();
        let session = saved.shuffle_session.take().unwrap_or_default();
        let mut app = Self {
            ctx: cc.egui_ctx.clone(),
            saved,
            audio,
            auth,
            credential_pending,
            icons: Icons::new(&cc.egui_ctx),
            browser: Default::default(),
            mini_index: Default::default(),
            vinyl_index: Default::default(),
            remote_tags,
            tag_progress: Default::default(),
            library_revision: 0,
            playback_selection: session.selection,
            restored_selection,
            cache,
            tx,
            rx,
            cancel: Arc::new(AtomicBool::new(false)),
            scanning: false,
            scan_automatic: false,
            sync_scheduler: source_sync::Scheduler::default(),
            sync_status: String::new(),
            connections: Default::default(),
            discovery: None,
            networks: Vec::new(),
            network_probe_due: Instant::now(),
            network_probe_pending: false,
            network_changed: false,
            root_aliases: Vec::new(),
            scan_added: 0,
            history: History::default(),
            shuffle_cycle: session.cycle,
            current: None,
            requested: None,
            ticket: 0,
            position: 0.0,
            paused: true,
            loading: false,
            loaded_bytes: 0,
            output: tr!("Select a track to open audio output").into(),
            output_exclusive: false,
            message: String::new(),
            query: String::new(),
            devices: audio::devices(),
            volume_db,
            mute: false,
            native: native_rate,
            remember_password: true,
            ftp_address: String::new(),
            pending_ftp: None,
            phone_pair_dialog: false,
            phone_pair: Default::default(),
            ftp_dialog: false,
            ftp_focus_host: false,
            audio_dialog: false,
            sources_dialog: false,
            track_dialog: false,
            info_dialog: false,
            info_lyrics: true,
            settings_dialog: false,
            settings_page: 0,
            settings_geometry: Default::default(),
            settings_focused_on_open: false,
            help_dialog: false,
            help_query: String::new(),
            language_fonts_open: false,
            media_server: None,
            shared_source: None,
            shared_revision: 0,
            embedded_server,
            embedded_connection: None,
            embedded_starting: false,
            embedded_autostart_attempted: false,
            embedded_message: String::new(),
            room_state: Default::default(),
            room_password: String::new(),
            room_rx: None,
            room_refresh_at: Instant::now(),
            room_volume: 50.0,
            room_volume_dragging: false,
            room_message: String::new(),
            listening: Default::default(),
            lyric_follow: true,
            last_lyric_line: None,
            ftp: FtpSource::smartphone(),
            ftp_edit: None,
            ftp_old: None,
            directory: String::new(),
            entries: Vec::new(),
            browsing: false,
            art: None,
            cover_viewer: Default::default(),
            art_origin: None,
            online_covers,
            save_covers,
            covers: HashMap::new(),
            planned: None,
            meters: [0.0; 2],
            angle: 0.0,
            reel_motion: Default::default(),
            needle_motion: Default::default(),
            last_frame: Instant::now(),
            background_due: Instant::now(),
            screenshot,
            screenshot_sent: false,
            viewport_mini: None,
            window_geometry: Default::default(),
            viewport_zoom: ui_scale,
            viewport_pinned: None,
            test_exit: test_seconds.map(|s| Instant::now() + Duration::from_secs(s)),
        };
        app.cover_viewer.load_placement(app.saved.artwork_window);
        window_drag::install();
        crate::about::install();
        crate::open_documents::attach(&app.ctx);
        app.settings_geometry.load(app.saved.settings_window);
        app.browser.sort = app.saved.library_sort;
        app.gain();
        if !paths.is_empty() {
            app.open_external_files(paths);
        }
        app
    }
    fn online_cover_control(&mut self, ui: &mut egui::Ui) {
        if ui
            .checkbox(
                &mut self.saved.online_covers,
                tr!("Search online for missing covers"),
            )
            .changed()
        {
            self.online_covers
                .store(self.saved.online_covers, Ordering::Relaxed);
        }
    }
    fn cover_save_control(&mut self, ui: &mut egui::Ui) {
        if ui
            .checkbox(
                &mut self.saved.save_covers,
                tr!("Save found covers in the album folder"),
            )
            .changed()
        {
            self.save_covers
                .store(self.saved.save_covers, Ordering::Relaxed);
        }
    }
    fn player_animation_visible(&self) -> bool {
        self.saved.mini && self.saved.mini_view == 1 && !self.saved.reduce_motion
    }

    fn repaint_delay(&self, visual_active: bool, minimized: bool) -> Duration {
        if self.screenshot.is_some() && !self.screenshot_sent {
            return Duration::from_millis(16);
        }
        let busy = self.loading
            || self.scanning
            || self.browsing
            || matches!(self.listening.artist, crate::listening::State::Loading)
            || matches!(self.listening.lyrics, crate::listening::State::Loading);
        let playing = !self.paused && !self.loading;
        if playing && !minimized && self.player_animation_visible() {
            Duration::from_millis(33)
        } else if !visual_active && (playing || busy) {
            Duration::from_millis(200)
        } else if self.saved.mini && (playing || self.meters.iter().any(|&v| v > 0.001)) {
            Duration::from_millis(67)
        } else if playing || busy {
            Duration::from_millis(100)
        } else {
            Duration::from_millis(500)
        }
    }
    fn source_choices(&self) -> Vec<(Option<String>, String)> {
        let mut choices = vec![(None, tr!("All music").into())];
        choices.extend(self.saved.sources.iter().map(|s| {
            (
                Some(s.id.clone()),
                if s.name.is_empty() {
                    s.host.clone()
                } else {
                    s.name.clone()
                },
            )
        }));
        choices.extend(self.saved.folders.iter().map(|p| {
            (
                Some(format!("folder:{}", p.display())),
                p.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            )
        }));
        choices
    }
    fn selected_scope(&self) -> library::Scope {
        self.scope_for_source(&self.saved.active_source)
    }
    fn scope_for_source(&self, source: &Option<String>) -> library::Scope {
        let Some(id) = source else {
            return library::Scope::All;
        };
        if let Some(source) = self.saved.sources.iter().find(|s| &s.id == id) {
            return source
                .url()
                .map(library::Scope::Remote)
                .unwrap_or(library::Scope::Missing);
        }
        if let Some(path) = self
            .saved
            .folders
            .iter()
            .find(|p| format!("folder:{}", p.display()) == *id)
        {
            return library::Scope::Local(path.clone());
        }
        library::Scope::Missing
    }
    fn scoped_tracks(&self) -> impl Iterator<Item = &Track> {
        let scope = self.selected_scope();
        self.saved
            .tracks
            .iter()
            .filter(move |t| scope.contains(&t.source))
    }
    fn source_title(&self) -> String {
        self.source_choices()
            .into_iter()
            .find(|(id, _)| id == &self.saved.active_source)
            .map(|(_, name)| name)
            .unwrap_or_else(|| tr!("Source unavailable").into())
    }
    fn select_source(&mut self, id: Option<String>) {
        if self.saved.active_source == id {
            return;
        }
        self.stop();
        self.history = History::default();
        self.shuffle_cycle.clear();
        self.restored_selection = false;
        self.audio.plan_next();
        self.saved.active_source = id;
        self.browser.clear();
        self.playback_selection = None;
        self.query.clear();
        self.message = format!(
            "{} · {}",
            self.source_title(),
            crate::i18n::tracks(self.scoped_tracks().count())
        );
    }
    fn exclusive_active(&self) -> bool {
        self.output_exclusive && self.current.is_some()
    }
    fn gain(&self) {
        let gain = if self.mute {
            0.0
        } else {
            10.0f32.powf(self.volume_db / 20.0)
        };
        self.audio.gain.store(gain.to_bits(), Ordering::Relaxed);
    }
    fn open_external_files(&mut self, paths: Vec<PathBuf>) {
        let files = crate::open_documents::audio_files(paths.clone());
        if files.is_empty() {
            // Preserve the existing directory/playlist import command path.
            self.import(paths);
            return;
        }
        let ids: Vec<_> = files
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        self.select_source(None);
        library::merge_tracks(
            &mut self.saved.tracks,
            ids.iter().cloned().map(Track::placeholder).collect(),
        );
        self.library_revision = self.library_revision.wrapping_add(1);
        self.playback_selection = Some(ids.clone());
        // Opening multiple documents is an explicitly ordered listening action.
        // Do not inherit shuffle or repeat-one from the previous session.
        if ids.len() > 1 {
            self.saved.shuffle = false;
            self.saved.shuffle_albums = false;
            self.saved.repeat = false;
        }
        self.restored_selection = false;
        self.history = History::default();
        self.shuffle_cycle.clear();
        self.query.clear();
        self.browser.clear();
        self.play(ids[0].clone());
    }
    fn import(&mut self, paths: Vec<PathBuf>) {
        if self.scanning {
            self.message = tr!("Wait or cancel the current scan").into();
            return;
        }
        self.cancel_discovery();
        for path in &paths {
            if path.is_dir() && !self.saved.folders.contains(path) {
                self.saved.folders.push(path.clone());
            }
        }
        self.cancel = Arc::new(AtomicBool::new(false));
        self.scanning = true;
        self.scan_automatic = false;
        self.scan_added = 0;
        self.message = tr!("Synchronizing folders…").into();
        jobs::import(paths, self.tx.clone(), self.cancel.clone());
    }
    fn awaiting_credentials(&self, target: &str) -> bool {
        let Ok(target) = url::Url::parse(target) else {
            return false;
        };
        self.saved.sources.iter().any(|s| {
            self.credential_pending.contains(&s.id)
                && s.password.is_empty()
                && !s.username.is_empty()
                && s.url()
                    .ok()
                    .and_then(|u| url::Url::parse(&u).ok())
                    .is_some_and(|u| {
                        u.scheme() == target.scheme()
                            && u.host_str() == target.host_str()
                            && u.port_or_known_default() == target.port_or_known_default()
                    })
        })
    }
    fn sync_targets(&self) -> Vec<Target> {
        let mut targets: Vec<_> = self
            .saved
            .sources
            .iter()
            .filter_map(|s| {
                s.url().ok().map(|root| Target::Ftp {
                    id: s.id.clone(),
                    root,
                })
            })
            .collect();
        targets.extend(self.saved.folders.iter().cloned().map(Target::Folder));
        targets
    }
    fn start_sync(&mut self, target: Target, automatic: bool) {
        if self.scanning {
            return;
        }
        if !automatic {
            self.cancel_discovery();
        }
        self.cancel = Arc::new(AtomicBool::new(false));
        self.scanning = true;
        self.scan_automatic = automatic;
        self.scan_added = 0;
        if !automatic {
            self.message = tr!("Synchronizing source…").into();
        }
        source_sync::start(
            target,
            self.auth.clone(),
            self.saved.tracks.iter().map(|t| t.source.clone()).collect(),
            self.cache.with_file_name("sync-index"),
            !automatic,
            self.cancel.clone(),
            self.tx.clone(),
        );
    }
    fn auto_sync_tick(&mut self, now: Instant) {
        if !self.saved.auto_sync
            || self.scanning
            || self.loading
            || self.browsing
            || self.discovery.is_some()
            || self.network_probe_pending
            || self.connections.request_pending()
        {
            return;
        }
        // An empty password for a named account needs a user action, never an anonymous retry.
        let targets: Vec<_> = self
            .sync_targets()
            .into_iter()
            .filter(|t| match t {
                Target::Ftp { id, .. } => self
                    .saved
                    .sources
                    .iter()
                    .find(|s| &s.id == id)
                    .is_some_and(|s| s.username.is_empty() || !s.password.is_empty()),
                Target::Folder(_) => true,
            })
            .collect();
        if let Some(target) = self.sync_scheduler.due(&targets, now) {
            self.start_sync(target, true);
        }
    }
    fn cancel_discovery(&mut self) {
        if let Some(request) = self.discovery.take() {
            request.cancel.store(true, Ordering::Relaxed);
        }
    }
    fn network_tick(&mut self, now: Instant) {
        if self.network_probe_pending
            || now < self.network_probe_due
            || self.saved.sources.is_empty()
        {
            return;
        }
        self.network_probe_due = now + Duration::from_secs(15);
        self.network_probe_pending = true;
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Job::NetworkSnapshot(discovery::lans()));
            ctx.request_repaint();
        });
    }
    fn start_discovery(&mut self, id: &str) {
        if self.phone_pair_dialog
            || self.discovery.is_some()
            || self.network_probe_pending
            || !self.saved.auto_sync
        {
            return;
        }
        let Some(source) = self
            .saved
            .sources
            .iter()
            .find(|s| {
                s.id == id
                    && s.discovery_enabled()
                    && (s.username.is_empty() || !s.password.is_empty())
            })
            .cloned()
        else {
            return;
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let token = rand::random();
        self.sync_status = crate::i18n::format(
            "Searching for {source} on the local network…",
            &[("source", source.name.clone())],
        );
        self.discovery = Some(RediscoveryRequest {
            token,
            source: source.clone(),
            networks: self.networks.clone(),
            cancel: cancel.clone(),
        });
        discovery::start(
            source,
            self.saved.tracks.iter().map(|t| t.source.clone()).collect(),
            discovery::SearchNetwork {
                lans: self.networks.clone(),
                pace: self.saved.ftp_scan_pace,
            },
            self.cache.with_file_name("sync-index"),
            cancel,
            token,
            (self.tx.clone(), self.ctx.clone()),
        );
    }
    fn canonical_source(&self, id: &str) -> String {
        self.root_aliases
            .iter()
            .fold(id.to_owned(), |id, (old, new)| {
                network::rebase(&id, old, new).unwrap_or(id)
            })
    }
    fn apply_discovered(&mut self, source: FtpSource, old: &str) {
        let Ok(new) = source.url() else {
            return;
        };
        let Some(stored) = self.saved.sources.iter_mut().find(|s| s.id == source.id) else {
            return;
        };
        stored.host = source.host;
        stored.port = source.port;
        stored.recent_hosts = source.recent_hosts;
        stored.hardware_ids = source.hardware_ids;
        let updated = stored.clone();
        let _ = self.auth.insert(&updated);
        for track in &mut self.saved.tracks {
            if let Some(id) = network::rebase(&track.source, old, &new) {
                track.source = id;
            }
        }
        for id in [&mut self.current, &mut self.requested, &mut self.planned] {
            if let Some(value) = id
                && let Some(rebased) = network::rebase(value, old, &new)
            {
                *value = rebased;
            }
        }
        self.history.rebase(old, &new);
        self.shuffle_cycle.rebase(old, &new);
        if let Some(id) = &mut self.browser.selected_track
            && let Some(rebased) = network::rebase(id, old, &new)
        {
            *id = rebased;
        }
        for col in 0..3 {
            if self.saved.facet_fields[col] == crate::facets::Field::Folder {
                self.browser.selected[col] = self.browser.selected[col]
                    .iter()
                    .map(|key| network::rebase(key, old, &new).unwrap_or_else(|| key.clone()))
                    .collect();
            }
        }

        self.library_revision = self.library_revision.wrapping_add(1);
        if let Some(ids) = &mut self.playback_selection {
            for id in ids {
                if let Some(new_id) = network::rebase(id, old, &new) {
                    *id = new_id;
                }
            }
        }
        self.covers = std::mem::take(&mut self.covers)
            .into_iter()
            .map(|(id, art)| (network::rebase(&id, old, &new).unwrap_or(id), art))
            .collect();
        self.root_aliases.push((old.into(), new.clone()));
        if self.root_aliases.len() > 32 {
            self.root_aliases.remove(0);
        }
        if self.ftp.id == updated.id && !self.ftp_dialog {
            self.ftp = updated.clone();
            self.ftp_old = Some(new.clone());
            self.ftp_address = new.clone();
        }
        self.connections.mark_verified(&updated);
        self.sync_scheduler.reset();
        self.message = crate::i18n::format(
            "{source} found · connection updated to {address}",
            &[
                ("source", updated.name.clone()),
                ("address", updated.host.clone()),
            ],
        );
        self.sync_status = self.message.clone();
        if self.current.is_some() && !self.loading {
            let next = self.planned.clone().or_else(|| self.choose_next());
            self.prepare_track(next);
        }
        // Leave current PCM playback, volume, history cursor and source selection intact.
    }
    fn auto_sync_control(&mut self, ui: &mut egui::Ui) {
        if ui.checkbox(&mut self.saved.auto_sync, tr!("Synchronize automatically")).on_hover_text(tr!("Checks every 2 minutes while MusicCraft is open, backs off offline sources, and audits every 30 minutes.")).changed() && !self.saved.auto_sync && self.scan_automatic {
            self.cancel.store(true, Ordering::Relaxed);
        }
        if !self.saved.auto_sync {
            self.cancel_discovery();
        }
    }
    fn sync_ftp(&mut self, root: String) {
        if self.awaiting_credentials(&root) {
            self.message = tr!("Authorize Keychain or enter the password in Network").into();
            return;
        }
        if self.scanning {
            self.message = tr!("Wait or cancel the current scan").into();
            return;
        }
        let id = self
            .saved
            .sources
            .iter()
            .find(|s| s.url().is_ok_and(|url| root.starts_with(&url)))
            .map_or_else(String::new, |s| s.id.clone());
        self.start_sync(Target::Ftp { id, root }, false);
    }
    fn play(&mut self, id: String) {
        let id = self.canonical_source(&id);
        if !self.track_connection_ready(&id) {
            self.message = tr!("Checking…").into();
            return;
        }
        if self.awaiting_credentials(&id) {
            self.message = tr!("Authorize Keychain or enter the password in Network").into();
            return;
        }
        let track = self.scoped_tracks().find(|t| t.source == id).cloned();
        if let Some(track) = track {
            self.ticket = self.audio.load(
                track,
                self.saved.device.clone(),
                self.native,
                self.saved.exclusive,
                self.saved.strict_integer,
            );
            self.requested = Some(id.clone());
            self.loading = true;
            self.loaded_bytes = 0;
            self.message.clear();
            self.planned = None;
            let art = self.covers.get(&id).cloned().flatten();
            self.set_texture(&self.ctx.clone(), art);
        }
    }
    fn order_mode(&self) -> usize {
        if self.saved.shuffle_albums {
            2
        } else if self.saved.shuffle {
            1
        } else {
            0
        }
    }
    fn order_label(&self) -> &'static str {
        [
            tr!("Sequence"),
            tr!("Shuffle tracks"),
            tr!("Shuffle albums"),
        ][self.order_mode()]
    }
    fn order_selector(&mut self, ui: &mut egui::Ui, accent: Color32) {
        let mode = self.order_mode();
        let next = (mode + 1) % 3;
        let label = self.order_label();
        let response = ui
            .scope(|ui| {
                ui.spacing_mut().button_padding = vec2(6.0, 6.0);
                ui.add(
                    egui::Button::image(self.icons.image(
                        ["list-ordered", "shuffle", "disc-3"][mode],
                        18.0,
                        if mode == 0 {
                            ui.visuals().text_color()
                        } else {
                            accent
                        },
                    ))
                    .min_size(vec2(30.0, 30.0))
                    .frame_when_inactive(false)
                    .corner_radius(6.0),
                )
            })
            .inner;
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                ui.is_enabled(),
                crate::i18n::format("Playback order: {mode}", &[("mode", label.to_string())]),
            )
        });
        if response
            .on_hover_text(format!(
                "{label}\n{}",
                crate::i18n::format(
                    "Click to switch to {mode}",
                    &[(
                        "mode",
                        [
                            tr!("Sequence"),
                            tr!("Shuffle tracks"),
                            tr!("Shuffle albums")
                        ][next]
                            .to_string()
                    )]
                )
            ))
            .clicked()
        {
            self.set_order_mode(next);
        }
    }
    fn set_order_mode(&mut self, mode: usize) {
        let mode = mode % 3;
        if self.order_mode() == mode {
            return;
        }
        self.saved.shuffle = mode == 1;
        self.saved.shuffle_albums = mode == 2;
        self.history.discard_forward();
        self.prepare_next();
    }
    fn listening_tracks(&self) -> Vec<&Track> {
        let scoped: Vec<_> = self.scoped_tracks().collect();
        if let Some(ids) = &self.playback_selection {
            let map: HashMap<_, _> = scoped.into_iter().map(|t| (t.source.as_str(), t)).collect();
            ids.iter()
                .filter_map(|id| map.get(id.as_str()).copied())
                .collect()
        } else {
            scoped
        }
    }
    fn choose_next(&mut self) -> Option<String> {
        if self.saved.repeat {
            return self
                .current
                .as_ref()
                .filter(|id| {
                    self.listening_tracks()
                        .into_iter()
                        .any(|t| &t.source == *id)
                })
                .cloned();
        }
        if self.saved.shuffle_albums {
            return library::random_album_next(
                self.listening_tracks().into_iter(),
                self.current.as_deref(),
            );
        }
        if self.saved.shuffle {
            // Move only the set header so candidates can borrow the library
            // without cloning every source path for each transition.
            let mut cycle = std::mem::take(&mut self.shuffle_cycle);
            let next = cycle.choose(
                self.listening_tracks()
                    .into_iter()
                    .map(|t| t.source.as_str()),
                self.current.as_deref(),
            );
            self.shuffle_cycle = cycle;
            next
        } else {
            let tracks = self.listening_tracks();
            let index = tracks
                .iter()
                .position(|t| Some(&t.source) == self.current.as_ref());
            tracks
                .get(index.map_or(0, |i| i + 1))
                .map(|t| t.source.clone())
        }
    }
    fn prepare_next(&mut self) {
        let next = if self.current.is_some() && !self.loading {
            self.history.peek_next().or_else(|| self.choose_next())
        } else {
            None
        };
        self.prepare_track(next);
    }
    fn prepare_track(&mut self, next: Option<String>) {
        self.audio.plan_next();
        self.planned =
            next.filter(|id| !self.awaiting_credentials(id) && self.track_connection_ready(id));
        if let Some(track) = self
            .planned
            .as_ref()
            .and_then(|id| self.saved.tracks.iter().find(|t| &t.source == id))
            .cloned()
        {
            jobs::prefetch(
                track,
                self.cache.clone(),
                self.auth.clone(),
                self.audio.generation.clone(),
                self.ticket,
                self.tx.clone(),
                jobs::PrepareOptions {
                    artwork_only: false,
                    next_plan: Some((
                        self.audio.plan.clone(),
                        self.audio.plan.load(Ordering::Relaxed),
                    )),
                    exclusive: self.saved.exclusive.then_some(self.saved.strict_integer),
                    online_art: Some(self.online_covers.clone()),
                    save_art: Some(self.save_covers.clone()),
                },
            );
        }
    }
    fn next(&mut self) {
        if !self.navigation_ready() {
            return;
        }
        if self.current.is_none() && !self.loading {
            self.capture_start_selection();
        }
        if let Some(id) = self
            .history
            .next()
            .or_else(|| self.planned.clone())
            .or_else(|| self.choose_next())
        {
            self.play(id);
        }
    }
    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.audio.rx.try_recv() {
            match event {
                Event::Loading(ticket, bytes) if ticket == self.ticket => self.loaded_bytes = bytes,
                Event::Started {
                    ticket,
                    mut track,
                    output,
                    cover,
                    exclusive,
                } if ticket == self.ticket => {
                    track.source = self.canonical_source(&track.source);
                    self.current = Some(track.source.clone());
                    self.requested = None;
                    self.history.record(track.source.clone());
                    self.shuffle_cycle.record(&track.source);
                    self.loading = false;
                    self.paused = false;
                    self.position = 0.0;
                    self.output = output;
                    self.output_exclusive = exclusive;
                    if let Some(stored) = self
                        .saved
                        .tracks
                        .iter_mut()
                        .find(|t| t.source == track.source)
                    {
                        *stored = track.clone();
                        self.library_revision = self.library_revision.wrapping_add(1);
                    }
                    let bytes = cover.or_else(|| self.covers.get(&track.source).cloned().flatten());
                    self.set_texture(ctx, bytes);
                    // Missing current artwork and the next track are fetched independently of audio.
                    if self.art.is_none() {
                        jobs::prefetch(
                            track,
                            self.cache.clone(),
                            self.auth.clone(),
                            self.audio.generation.clone(),
                            self.ticket,
                            self.tx.clone(),
                            jobs::PrepareOptions {
                                artwork_only: true,
                                next_plan: None,
                                exclusive: None,
                                online_art: Some(self.online_covers.clone()),
                                save_art: Some(self.save_covers.clone()),
                            },
                        );
                    }
                    self.prepare_next();
                }
                Event::Notice(text) => self.message = text,
                Event::Position(pos, paused) if !self.loading => {
                    self.position = pos;
                    self.paused = paused;
                }
                Event::Ended if !self.loading => {
                    self.paused = true;
                    if let Some(id) = self
                        .history
                        .next()
                        .or_else(|| self.planned.clone())
                        .or_else(|| self.choose_next())
                    {
                        self.play(id);
                    } else {
                        self.stop();
                    }
                }
                Event::Error(ticket, e) if ticket == 0 || ticket == self.ticket => {
                    if let Some(id) = self.requested.as_ref().or(self.current.as_ref())
                        && let Some(source) = self.source_for_track(id).cloned()
                    {
                        self.connections.invalidate(&source);
                    }
                    self.stop();
                    self.ticket = self.audio.generation.load(Ordering::Relaxed);
                    self.output = tr!("Output closed after an error").into();
                    self.output_exclusive = false;
                    self.message = e;
                    self.history.cancel();
                }
                _ => {}
            }
        }
        while let Ok(job) = self.rx.try_recv() {
            match job {
                Job::ConnectionChecked { token, ok } => self.connection_result(token, ok),
                Job::Credential {
                    id,
                    username,
                    password,
                } => {
                    self.credential_pending.remove(&id);
                    if let Some(source) = self
                        .saved
                        .sources
                        .iter_mut()
                        .find(|s| s.id == id && s.username == username && s.password.is_empty())
                    {
                        if let Some(password) = password {
                            source.password = password;
                            let _ = self.auth.insert(source);
                            if self.ftp.id == id
                                && self.ftp.username == username
                                && self.ftp.password.is_empty()
                            {
                                self.ftp.password = source.password.clone();
                            }
                        } else {
                            self.message =
                                tr!("Password unavailable: open Network to enter it").into();
                        }
                    }
                }
                Job::NetworkSnapshot(networks) => {
                    self.network_probe_pending = false;
                    if networks != self.networks {
                        self.connections.network_changed();
                        self.networks = networks;
                        self.network_changed = true;
                        self.cancel_discovery();
                        if self.scan_automatic {
                            self.cancel.store(true, Ordering::Relaxed);
                        }
                    }
                }
                Job::Rediscovered { request, outcome } => {
                    if !self.discovery.as_ref().is_some_and(|r| r.token == request) {
                        continue;
                    }
                    let request = self.discovery.take().unwrap();
                    let valid = self.saved.auto_sync
                        && !request.cancel.load(Ordering::Relaxed)
                        && request.networks == self.networks
                        && self.saved.sources.iter().any(|s| {
                            s.id == request.source.id
                                && s.discovery_enabled()
                                && s.url() == request.source.url()
                                && s.username == request.source.username
                                && s.password == request.source.password
                        });
                    if !valid {
                        continue;
                    }
                    match outcome {
                        Outcome::Found(source) => {
                            if let Ok(old) = request.source.url() {
                                self.apply_discovered(*source, &old);
                            }
                        }
                        Outcome::Unchanged => {
                            self.connections.mark_verified(&request.source);
                            self.sync_scheduler.reset();
                        }
                        Outcome::Missing(reason) => {
                            self.sync_status = match reason {
                                Reason::Ambiguous => tr!(
                                    "Multiple matching libraries found: choose a connection in Network"
                                ),
                                Reason::NoReference => {
                                    tr!("Connect and index the phone once so it can be recognized")
                                }
                                Reason::NoLan => {
                                    tr!("Waiting for a compatible Wi-Fi or Ethernet network")
                                }
                                Reason::Cancelled => tr!("Search cancelled"),
                                Reason::NotFound => {
                                    tr!("Phone not found: another automatic attempt is scheduled")
                                }
                            }
                            .into();
                            if reason == Reason::Cancelled {
                                self.network_probe_due = Instant::now();
                            } else if !self.phone_pair_dialog
                                && self
                                    .saved
                                    .active_source
                                    .as_deref()
                                    .is_none_or(|id| id == request.source.id)
                                && !self.ftp_dialog
                                && !self.browsing
                                && self.connections.offer_manual_address(&request.source.id)
                            {
                                let index = self
                                    .saved
                                    .sources
                                    .iter()
                                    .position(|s| s.id == request.source.id);
                                self.open_ftp_editor(index);
                                self.ftp_focus_host = true;
                                self.message = if reason == Reason::NotFound {
                                    tr!("No phone connected").into()
                                } else {
                                    self.sync_status.clone()
                                };
                            }
                        }
                    }
                }
                Job::SyncDone(report) => {
                    self.scanning = false;
                    let automatic = std::mem::take(&mut self.scan_automatic);
                    self.sync_scheduler.finished(
                        &report.target,
                        Instant::now(),
                        report.stats.errors == 0 || report.cancelled,
                    );
                    // A network source may have been reconfigured while its worker was running.
                    let valid = match &report.target {
                        Target::Ftp { id, root } => {
                            id.is_empty()
                                || self.saved.sources.iter().any(|s| {
                                    &s.id == id && s.url().is_ok_and(|url| root.starts_with(&url))
                                })
                        }
                        Target::Folder(p) => self.saved.folders.contains(p),
                    };
                    if !valid {
                        continue;
                    }
                    if !report.cancelled
                        && !report.stats.root_unreachable
                        && let Target::Ftp { id, .. } = &report.target
                        && let Some(source) =
                            self.saved.sources.iter().find(|s| &s.id == id).cloned()
                    {
                        self.connections.mark_verified(&source);
                    }
                    let rediscover_id =
                        if automatic && !report.cancelled && report.stats.root_unreachable {
                            match &report.target {
                                Target::Ftp { id, .. } => Some(id.clone()),
                                _ => None,
                            }
                        } else {
                            None
                        };
                    let before = self.saved.tracks.len();
                    if !report.cancelled
                        && !self.cancel.load(Ordering::Relaxed)
                        && !report.absent.is_empty()
                    {
                        self.saved
                            .tracks
                            .retain(|track| !report.absent.contains(&track.source));
                    }
                    let removed = before - self.saved.tracks.len();
                    if removed > 0 {
                        if let Some(selection) = &mut self.playback_selection {
                            selection.retain(|id| !report.absent.contains(id));
                        }
                        self.history
                            .remove_missing(&report.absent, self.current.as_deref());
                    }
                    let added = library::merge_tracks(&mut self.saved.tracks, report.tracks);
                    if added > 0 || removed > 0 {
                        self.library_revision = self.library_revision.wrapping_add(1);
                    }
                    self.sync_status = crate::i18n::format(
                        "{status} · Added: {added} · Read: {read} · Unchanged: {skipped} · Errors: {errors}",
                        &[
                            (
                                "status",
                                if report.cancelled {
                                    tr!("Check cancelled")
                                } else if report.stats.suspended {
                                    tr!("Sharing paused: phone is playing")
                                } else if report.stats.errors > 0 {
                                    tr!("Source not fully reachable")
                                } else {
                                    tr!("Check complete")
                                }
                                .to_string(),
                            ),
                            ("added", added.to_string()),
                            ("read", report.stats.listed.to_string()),
                            ("skipped", report.stats.skipped.to_string()),
                            ("errors", report.stats.errors.to_string()),
                        ],
                    );
                    if !automatic || added > 0 || removed > 0 {
                        self.message = self.sync_status.clone();
                    }
                    if let Some(id) = rediscover_id {
                        self.start_discovery(&id);
                    }
                    let plan_removed = removed > 0
                        && self
                            .planned
                            .as_ref()
                            .is_some_and(|id| report.absent.contains(id));
                    if plan_removed
                        || ((added > 0 || removed > 0)
                            && self.current.is_some()
                            && !self.loading
                            && self.planned.is_none())
                    {
                        self.prepare_next();
                    }
                }
                Job::TagAlias {
                    source,
                    target,
                    ticket,
                } => {
                    if !self.remote_tags.accepts(ticket) {
                        continue;
                    }
                    let source = self.canonical_source(&source);
                    let target = self.canonical_source(&target);
                    if library::merge_verified_alias(&mut self.saved.tracks, &source, &target) {
                        self.remote_tags.library_changed();
                        self.history.rebase(&source, &target);
                        self.shuffle_cycle.rebase(&source, &target);
                        for id in [
                            &mut self.current,
                            &mut self.requested,
                            &mut self.planned,
                            &mut self.browser.selected_track,
                        ] {
                            if id.as_ref() == Some(&source) {
                                *id = Some(target.clone());
                            }
                        }
                        if let Some(ids) = &mut self.playback_selection {
                            for id in ids.iter_mut() {
                                if *id == source {
                                    *id = target.clone();
                                }
                            }
                            let mut seen = HashSet::new();
                            ids.retain(|id| seen.insert(id.clone()));
                        }
                        self.root_aliases.push((source, target));
                        if self.root_aliases.len() > 32 {
                            self.root_aliases.remove(0);
                        }
                        self.library_revision = self.library_revision.wrapping_add(1);
                    }
                }
                Job::Tags(mut track, epoch) => {
                    if !self.remote_tags.accepts(epoch) {
                        continue;
                    }
                    track.source = self.canonical_source(&track.source);
                    if let Some(stored) = self
                        .saved
                        .tracks
                        .iter_mut()
                        .find(|t| t.source == track.source)
                        && stored.rate == 0
                        && !stored.metadata_loaded
                    {
                        stored.adopt_tags(track);
                        self.library_revision = self.library_revision.wrapping_add(1);
                    }
                }
                Job::TagsDone(epoch, success) => self.remote_tags.finished(epoch, success),
                Job::Tracks(tracks) => {
                    let added = library::merge_tracks(&mut self.saved.tracks, tracks);
                    self.scan_added += added;
                    if added > 0 {
                        self.library_revision = self.library_revision.wrapping_add(1);
                    }
                }
                Job::Done(errors) => {
                    self.scanning = false;
                    self.message = crate::i18n::format(
                        "{status} · Added: {added} · Unreachable folders: {errors}{limit}",
                        &[
                            (
                                "status",
                                if self.cancel.load(Ordering::Relaxed) {
                                    tr!("Synchronization cancelled")
                                } else {
                                    tr!("Synchronization complete")
                                }
                                .to_string(),
                            ),
                            ("added", self.scan_added.to_string()),
                            ("errors", errors.to_string()),
                            (
                                "limit",
                                if self.saved.tracks.len() >= library::MAX_TRACKS {
                                    tr!(" · Library limit reached")
                                } else {
                                    ""
                                }
                                .to_string(),
                            ),
                        ],
                    );
                    if self.current.is_some() && !self.loading && self.planned.is_none() {
                        self.prepare_next();
                    }
                }
                Job::Problem(e) => self.message = e,
                Job::Directory(url, result) => {
                    self.browsing = false;
                    match result {
                        Ok(entries) => {
                            self.accept_ftp(&url);
                            self.directory = url;
                            self.entries = entries;
                        }
                        Err(e) => {
                            self.pending_ftp = None;
                            self.message = format!(
                                "{e}. {}",
                                tr!("The saved connection has not been changed.")
                            );
                        }
                    }
                }
                Job::Ready(mut track, pcm, ticket, plan) => {
                    track.source = self.canonical_source(&track.source);
                    if ticket == self.ticket
                        && plan
                            .is_some_and(|value| value == self.audio.plan.load(Ordering::Relaxed))
                        && self.planned.as_ref() == Some(&track.source)
                        && !self.loading
                    {
                        self.audio.queue(
                            track,
                            self.ticket,
                            plan.unwrap(),
                            pcm,
                            self.saved.exclusive,
                            self.saved.strict_integer,
                        );
                    }
                }
                Job::Prepared(mut track, bytes, ticket) => {
                    track.source = self.canonical_source(&track.source);
                    if ticket != self.ticket {
                        continue;
                    }
                    let id = track.source.clone();
                    if let Some(stored) = self.saved.tracks.iter_mut().find(|t| t.source == id) {
                        *stored = track;
                        self.library_revision = self.library_revision.wrapping_add(1);
                    }
                    if !self.loading && self.current.as_ref() == Some(&id) && self.art.is_none() {
                        self.set_texture(ctx, bytes.clone());
                    }
                    if self.covers.len() > 3 {
                        self.covers.clear();
                    }
                    self.covers.insert(id, bytes);
                }
            }
        }
        if self.network_changed && !self.scanning && self.discovery.is_none() {
            self.sync_scheduler.reset();
            self.network_changed = false;
        }
    }

    fn background_tick(&mut self, now: Instant) {
        if now < self.background_due {
            return;
        }
        self.background_due = now + Duration::from_millis(250);
        let roots = if self.saved.auto_sync {
            self.saved
                .sources
                .iter()
                .filter(|source| {
                    !self.credential_pending.contains(&source.id)
                        && (source.username.is_empty() || !source.password.is_empty())
                })
                .filter_map(|source| source.url().ok())
                .collect()
        } else {
            Vec::new()
        };
        self.remote_tags.tick(
            &self.saved.tracks,
            &self.auth,
            &self.tx,
            roots,
            self.loading || self.scanning || self.browsing || self.discovery.is_some(),
        );
        let priority = if !self.query.trim().is_empty() {
            self.query.as_str()
        } else {
            self.saved
                .facet_fields
                .iter()
                .zip(&self.browser.searches)
                .find(|(field, search)| {
                    **field == crate::facets::Field::Artist && !search.trim().is_empty()
                })
                .map_or("", |(_, search)| search.as_str())
        };
        self.remote_tags.prioritize(&self.saved.tracks, priority);
        self.network_tick(now);
        self.connection_tick(now);
        self.auto_sync_tick(now);
    }

    fn set_texture(&mut self, ctx: &egui::Context, artwork: Option<library::Artwork>) {
        self.art_origin = artwork.as_ref().and_then(|art| art.origin.clone());
        self.art = artwork.map(|art| {
            ctx.load_texture(
                "album",
                egui::ColorImage::from_rgba_unmultiplied(art.size, &art.rgba),
                Default::default(),
            )
        });
    }
    fn ftp_browse(&mut self, url: String) {
        if self.awaiting_credentials(&url) {
            self.message = tr!("Authorize Keychain or enter the password in Network").into();
            return;
        }
        if self.browsing {
            return;
        }
        self.browsing = true;
        let (tx, auth) = (self.tx.clone(), self.auth.clone());
        std::thread::spawn(move || {
            let entries = network::list(&url, &auth);
            let _ = tx.send(Job::Directory(url, entries));
        });
    }
    fn connect(&mut self) {
        self.cancel_discovery();
        if self.browsing {
            self.message = tr!("Connection already in progress").into();
            return;
        }
        let candidate_auth = AuthStore::default();
        match self.ftp.url().and_then(|url| {
            candidate_auth.insert(&self.ftp)?;
            Ok(url)
        }) {
            Ok(url) => {
                self.pending_ftp = Some((self.ftp.clone(), self.ftp_old.clone(), self.ftp_edit));
                self.message = tr!("Checking new path…").into();
                self.browsing = true;
                let tx = self.tx.clone();
                std::thread::spawn(move || {
                    let result = if url.starts_with("https://") {
                        crate::phone::online(
                            &url,
                            &candidate_auth,
                            &AtomicBool::new(false),
                            Duration::from_secs(5),
                        )
                        .map(|_| Vec::new())
                    } else {
                        network::list(&url, &candidate_auth)
                    };
                    let _ = tx.send(Job::Directory(url, result));
                });
            }
            Err(e) => self.message = e,
        }
    }
    fn accept_ftp(&mut self, url: &str) {
        let Some((source, old, index)) = self.pending_ftp.take() else {
            return;
        };
        if source.url().as_deref() != Ok(url) {
            return;
        }
        let _ = self.auth.insert(&source);
        if let Some(old) = old.as_ref().filter(|old| old.as_str() != url) {
            self.stop();
            for track in &mut self.saved.tracks {
                if let Some(new) = network::rebase(&track.source, old, url) {
                    track.source = new;
                }
            }
            self.history = History::default();
            self.shuffle_cycle.clear();
            self.restored_selection = false;
            self.library_revision = self.library_revision.wrapping_add(1);
            self.browser.clear();
            self.playback_selection = None;
        }
        let mut keychain_error = None;
        if !self.remember_password {
            crate::credentials::forget(&source);
        } else if !source.username.is_empty()
            && !source.password.is_empty()
            && let Err(e) = crate::credentials::save(&source)
        {
            keychain_error = Some(e);
        }
        if let Some(i) = index {
            if let Some(stored) = self.saved.sources.get_mut(i) {
                *stored = source.clone();
            }
        } else {
            self.saved.sources.push(source.clone());
            self.ftp_edit = Some(self.saved.sources.len() - 1);
        }
        self.connections.clear();
        self.connections.mark_verified(&source);
        self.ftp = source;
        self.ftp_old = Some(url.into());
        self.ftp_address = url.into();
        self.message = keychain_error.unwrap_or_else(|| {
            if self.ftp.scheme == "https" {
                format!("MusicCraft Android · {}", tr!("Check complete"))
            } else {
                tr!("Connection verified and saved. Update IP, port and folder here.").into()
            }
        });
        if self.ftp.scheme == "https" {
            if self.current.is_none() {
                self.select_source(Some(self.ftp.id.clone()));
            }
            self.start_sync(
                Target::Ftp {
                    id: self.ftp.id.clone(),
                    root: url.into(),
                },
                true,
            );
        }
    }
    pub fn open_network(&mut self) {
        let selected = self
            .saved
            .active_source
            .as_deref()
            .and_then(|id| self.saved.sources.iter().position(|source| source.id == id));
        self.open_ftp_editor(selected.or_else(|| (!self.saved.sources.is_empty()).then_some(0)));
    }
    fn open_ftp_editor(&mut self, index: Option<usize>) {
        if index.is_some_and(|i| self.saved.sources[i].scheme == "https") {
            self.open_phone_pair();
            return;
        }
        if self.saved.mini {
            self.viewport_mini = Some(true);
        }
        self.saved.mini = false;
        if let Some(index) = index {
            self.ftp = self.saved.sources[index].clone();
            self.ftp_edit = Some(index);
            self.ftp_old = self.ftp.url().ok();
        } else {
            self.ftp = FtpSource::smartphone();
            self.ftp_edit = None;
            self.ftp_old = None;
        }
        self.ftp_address = self.ftp.url().unwrap_or_default();
        self.directory.clear();
        self.entries.clear();
        self.ftp_dialog = true;
    }
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        caption(ui, tr!("YOUR MUSIC"));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add(self.icons.image("library", 17.0, palette::accent()));
            if ui
                .selectable_label(self.saved.active_source.is_none(), tr!("All music"))
                .clicked()
            {
                self.select_source(None);
            }
        });
        ui.add_space(16.0);
        caption(ui, tr!("SMARTPHONE AND NETWORK"));
        let sources = self.saved.sources.clone();
        for (index, source) in sources.iter().enumerate() {
            let active = self.saved.active_source.as_ref() == Some(&source.id);
            let count = source.url().ok().map_or(0, |root| {
                self.saved
                    .tracks
                    .iter()
                    .filter(|t| t.source.starts_with(&root))
                    .count()
            });
            egui::Frame::new()
                .fill(if active {
                    palette::selected()
                } else {
                    palette::panel()
                })
                .corner_radius(7.0)
                .inner_margin(8.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let response = ui.add_sized(
                            [(ui.available_width() - 76.0).max(60.0), 28.0],
                            egui::Button::new("")
                                .left_text(
                                    RichText::new(if source.name.is_empty() {
                                        &source.host
                                    } else {
                                        &source.name
                                    })
                                    .color(if active {
                                        palette::accent()
                                    } else {
                                        palette::text()
                                    }),
                                )
                                .frame(false)
                                .wrap_mode(egui::TextWrapMode::Truncate),
                        );
                        if response.clicked() {
                            self.select_source(Some(source.id.clone()));
                        }
                        if self
                            .icons
                            .button(ui, "settings", tr!("Configure"), false)
                            .clicked()
                        {
                            self.open_ftp_editor(Some(index));
                        }
                        if ui
                            .add_enabled_ui(!self.scanning, |ui| {
                                self.icons
                                    .button(ui, "refresh-cw", tr!("Synchronize"), false)
                            })
                            .inner
                            .clicked()
                        {
                            match source.url() {
                                Ok(url) => self.sync_ftp(url),
                                Err(e) => self.message = e,
                            }
                        }
                    });
                    ui.label(
                        RichText::new(format!(
                            "{} · {}",
                            crate::i18n::tracks(count),
                            if source.scheme == "https" {
                                "Android"
                            } else {
                                "FTP"
                            }
                        ))
                        .small()
                        .color(palette::muted()),
                    );
                });
        }
        if sources.is_empty() {
            ui.label(
                RichText::new(tr!("No phone connected"))
                    .small()
                    .color(palette::muted()),
            );
        }
        ui.add_space(12.0);
        caption(ui, tr!("FOLDERS AND NAS"));
        for folder in self.saved.folders.clone() {
            let id = format!("folder:{}", folder.display());
            let active = self.saved.active_source.as_ref() == Some(&id);
            egui::Frame::new()
                .fill(if active {
                    palette::selected()
                } else {
                    palette::panel()
                })
                .corner_radius(7.0)
                .inner_margin(8.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let response = ui.add_sized(
                            [(ui.available_width() - 38.0).max(60.0), 28.0],
                            egui::Button::new("")
                                .left_text(
                                    RichText::new(
                                        folder.file_name().unwrap_or_default().to_string_lossy(),
                                    )
                                    .color(if active {
                                        palette::accent()
                                    } else {
                                        palette::text()
                                    }),
                                )
                                .frame(false)
                                .wrap_mode(egui::TextWrapMode::Truncate),
                        );
                        if response
                            .on_hover_text(folder.display().to_string())
                            .clicked()
                        {
                            self.select_source(Some(id));
                        }
                        if ui
                            .add_enabled_ui(!self.scanning, |ui| {
                                self.icons
                                    .button(ui, "refresh-cw", tr!("Synchronize"), false)
                            })
                            .inner
                            .clicked()
                        {
                            self.start_sync(Target::Folder(folder.clone()), false);
                        }
                    });
                    let count = self
                        .saved
                        .tracks
                        .iter()
                        .filter(|t| {
                            !library::is_remote(&t.source)
                                && std::path::Path::new(&t.source).starts_with(&folder)
                        })
                        .count();
                    ui.label(
                        RichText::new(format!(
                            "{} · {}",
                            crate::i18n::tracks(count),
                            tr!("Folder")
                        ))
                        .small()
                        .color(palette::muted()),
                    );
                });
        }
        if self.saved.folders.is_empty() {
            ui.label(
                RichText::new(tr!("No folders added"))
                    .small()
                    .color(palette::muted()),
            );
        }
        ui.add_space(12.0);
        ui.menu_button(tr!("Add source…"), |ui| {
            if ui.button("MusicCraft Android…").clicked() {
                self.open_phone_pair();
                ui.close();
            }

            if ui.button(tr!("Smartphone / FTP")).clicked() {
                self.ftp = FtpSource::smartphone();
                self.ftp_edit = None;
                self.ftp_old = None;
                self.ftp_address.clear();
                self.directory.clear();
                self.entries.clear();
                self.ftp_dialog = true;
                ui.close();
            }
            if ui.button(tr!("Local / NAS folder")).clicked() {
                ui.close();
                self.pick_music(true);
            }
            if ui.button(tr!("Files or playlists")).clicked() {
                ui.close();
                self.pick_music(false);
            }
        });
        ui.add_space(14.0);
        ui.separator();
        self.auto_sync_control(ui);
        if self.scanning {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(14.0));
                ui.label(
                    RichText::new(tr!("Synchronizing…"))
                        .small()
                        .color(palette::muted()),
                );
                if self
                    .icons
                    .button(ui, "x", tr!("Cancel synchronization"), false)
                    .clicked()
                {
                    self.cancel.store(true, Ordering::Relaxed);
                }
            });
        }
        if !self.sync_status.is_empty() {
            ui.add(
                egui::Label::new(
                    RichText::new(&self.sync_status)
                        .size(10.5)
                        .color(palette::muted()),
                )
                .truncate(),
            )
            .on_hover_text(&self.sync_status);
        }
    }
    fn now_playing(&mut self, ui: &mut egui::Ui, _compact: bool) {
        let track = self
            .current
            .as_ref()
            .or(self.requested.as_ref())
            .and_then(|id| self.saved.tracks.iter().find(|t| &t.source == id))
            .cloned();
        section(
            ui,
            tr!("Your listening session"),
            if self.loading {
                tr!("Preparing track…")
            } else if self.paused {
                tr!("Playback paused")
            } else {
                tr!("Playing")
            },
        );
        ui.allocate_ui(vec2(ui.available_width(), 142.0), |ui| {
            self.desktop_transport(ui)
        });
        ui.separator();
        if let Some(t) = track {
            let quality = t.quality();
            egui::Grid::new("track-info")
                .num_columns(2)
                .spacing(vec2(18.0, 10.0))
                .show(ui, |ui| {
                    for (label, value) in [
                        (tr!("Title"), t.title),
                        (tr!("Artist"), t.artist),
                        (tr!("Album"), t.album),
                        (tr!("Genre"), t.genre),
                        (tr!("Format"), quality),
                    ] {
                        caption(ui, label);
                        ui.add(
                            egui::Label::new(if value.is_empty() {
                                tr!("Unavailable")
                            } else {
                                &value
                            })
                            .wrap(),
                        );
                        ui.end_row();
                    }
                });
        }
        ui.add_space(8.0);
        caption(ui, tr!("SIGNAL LEVELS"));
        for (index, name) in ["L", "R"].iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(*name)
                        .monospace()
                        .small()
                        .color(palette::muted()),
                );
                let db = 20.0 * self.meters[index].max(0.000001).log10();
                ui.add(
                    egui::ProgressBar::new(((db + 60.0) / 60.0).clamp(0.0, 1.0))
                        .desired_height(5.0)
                        .fill(if db > -3.0 {
                            palette::accent()
                        } else {
                            Color32::from_rgb(108, 172, 149)
                        }),
                );
            });
        }
        ui.add_space(8.0);
        ui.label(
            RichText::new(crate::i18n::signal_message(&self.output))
                .small()
                .color(palette::muted()),
        );
    }
    fn toggle(&mut self) {
        if !self.can_toggle() {
            self.message = tr!("Checking…").into();
            return;
        }
        if self.current.is_none() {
            self.capture_start_selection();
            let first = self.choose_next();
            if let Some(id) = first {
                self.play(id);
            }
        } else {
            self.paused = !self.paused;
            let _ = self.audio.tx.send(if self.paused {
                Command::Pause
            } else {
                Command::Resume
            });
        }
    }
    fn stop(&mut self) {
        self.output_exclusive = false;
        self.output = tr!("Select a track to open audio output").into();
        self.audio.stop();
        self.ticket = self.audio.generation.load(Ordering::Relaxed);
        self.loading = false;
        self.paused = true;
        self.position = 0.0;
        self.current = None;
        self.requested = None;
        self.planned = None;
        self.art = None;
    }
    fn signal(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        section(
            ui,
            tr!("Output device"),
            tr!("Choose where to listen to your music."),
        );
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("output")
                .width((ui.available_width() - 50.0).max(140.0))
                .truncate()
                .selected_text(if self.saved.device.is_empty() {
                    tr!("System output")
                } else {
                    &self.saved.device
                })
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(
                            &mut self.saved.device,
                            String::new(),
                            tr!("System output"),
                        )
                        .changed();
                    for device in &self.devices {
                        changed |= ui
                            .selectable_value(&mut self.saved.device, device.clone(), device)
                            .changed();
                    }
                });
            if self
                .icons
                .button(ui, "refresh-cw", tr!("Refresh devices"), false)
                .clicked()
            {
                self.devices = audio::devices();
            }
        });
        ui.add_space(14.0);
        section(
            ui,
            tr!("Listening mode"),
            tr!("Applies when the next track starts."),
        );
        ui.horizontal(|ui| {
            changed |= ui
                .selectable_value(&mut self.saved.exclusive, false, tr!("Shared"))
                .changed();
            changed |= ui
                .selectable_value(&mut self.saved.exclusive, true, tr!("Exclusive"))
                .changed();
        });
        ui.label(RichText::new(if self.saved.exclusive{tr!("MusicCraft requests the device and source sample rate. Adjust volume on your audio device.")}else{tr!("Shared listening with other apps; MusicCraft controls the volume.")}).small().color(palette::muted()));
        ui.add_space(8.0);
        changed |= ui
            .add_enabled(
                !self.saved.exclusive,
                egui::Checkbox::new(&mut self.native, tr!("Follow source sample rate")),
            )
            .changed();
        changed |= ui
            .add_enabled(
                self.saved.exclusive,
                egui::Checkbox::new(&mut self.saved.strict_integer, tr!("Strict integer")),
            )
            .changed();
        ui.label(RichText::new(tr!("Strict integer requires lossless PCM and a compatible integer output at 0 dB. Playback stops if unavailable.")).size(11.0).color(palette::muted()));
        ui.add_space(14.0);
        ui.separator();
        egui::CollapsingHeader::new(RichText::new(tr!("Level and dynamics")).strong()).id_salt("audio-dynamics").default_open(self.saved.dsp != crate::dsp::Config::default()).show(ui, |ui| {
        ui.label(RichText::new(tr!("Optional processing, applied to the next track in shared mode.")).small().color(palette::muted()));
        let previous = self.saved.dsp;
        ui.add_enabled_ui(!self.saved.exclusive, |ui| {
            egui::ComboBox::from_id_salt("replay-gain")
                .selected_text(match self.saved.dsp.mode {
                    crate::dsp::ReplayGain::Off => tr!("Off"),
                    crate::dsp::ReplayGain::Track => tr!("ReplayGain track"),
                    crate::dsp::ReplayGain::Album => tr!("ReplayGain album"),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.saved.dsp.mode,
                        crate::dsp::ReplayGain::Off,
                        tr!("Off"),
                    );
                    ui.selectable_value(
                        &mut self.saved.dsp.mode,
                        crate::dsp::ReplayGain::Track,
                        tr!("ReplayGain track"),
                    );
                    ui.selectable_value(
                        &mut self.saved.dsp.mode,
                        crate::dsp::ReplayGain::Album,
                        tr!("ReplayGain album"),
                    );
                });
            ui.add(
                egui::Slider::new(&mut self.saved.dsp.preamp_db, -24.0..=12.0)
                    .text(tr!("Preamp"))
                    .suffix(" dB"),
            );
            ui.add_enabled(self.saved.dsp.mode != crate::dsp::ReplayGain::Off,egui::Checkbox::new(&mut self.saved.dsp.prevent_clipping,tr!("Prevent clipping using ReplayGain peaks")));
        });
        ui.label(RichText::new(if self.saved.exclusive { tr!("Exclusive output bypasses ReplayGain and preamp.") } else { tr!("Album mode preserves track dynamics. Missing album tags use track gain; missing tags use 0 dB.") }).small().color(palette::muted()));
        if !self.saved.exclusive {
            ui.label(RichText::new(tr!("Peak protection needs peak tags; without them, positive gain can clip.")).small().color(palette::muted()));
        }
        if previous != self.saved.dsp {
            let _ = self.audio.tx.send(Command::Dsp(self.saved.dsp));
            self.prepare_next();
        }
        ui.add_space(14.0);
        });
        ui.add_space(14.0);
        section(ui, tr!("Current signal path"), "");
        egui::Frame::new()
            .fill(palette::bg())
            .corner_radius(7.0)
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.label(
                    RichText::new(crate::i18n::signal_message(&self.output))
                        .small()
                        .color(if self.exclusive_active() {
                            palette::accent()
                        } else {
                            palette::text()
                        }),
                );
                if self.current.is_some() {
                    ui.label(
                        RichText::new(if self.exclusive_active() {
                            tr!("Original PCM → exclusive HAL → output · no DSP")
                        } else {
                            tr!("Float PCM → software gain → shared CoreAudio")
                        })
                        .small()
                        .color(palette::muted()),
                    );
                }
            });
        if ui.button(tr!("Check device formats")).clicked() {
            let _ = self
                .audio
                .tx
                .send(Command::Probe(self.saved.device.clone()));
        }
        if changed {
            self.prepare_next();
            self.message = tr!("Output updated: applies when the next track starts").into();
        }
        #[cfg(debug_assertions)]
        if self.ctx.data(|d| {
            d.get_temp::<bool>(egui::Id::new("preview-audio-controls"))
                .unwrap_or_default()
        }) {
            ui.scroll_to_cursor(Some(egui::Align::Max));
        }
    }
    fn ftp_window(&mut self, ctx: &egui::Context) {
        if !self.ftp_dialog {
            return;
        }
        let mut open = true;
        let available = ctx.content_rect();
        egui::Window::new(tr!("Connect smartphone / FTP")).id(egui::Id::new("connection-v06")).open(&mut open).default_width(500.0).max_width((available.width()-40.0).max(300.0)).max_height((available.height()-32.0).max(200.0)).default_pos(available.center()-vec2(250.0,230.0)).show(ctx,|ui| {
            let scroll_height=(available.height()-160.0).clamp(180.0,510.0);
            egui::ScrollArea::vertical().id_salt("connection-fields").max_height(scroll_height).show(ui,|ui|{
                section(ui,tr!("Connection"),tr!("Enter the server address and music folder."));
                ui.horizontal(|ui|{
                    ui.add(egui::TextEdit::singleline(&mut self.ftp_address).hint_text("ftp://telefono:porta/Music/").desired_width((ui.available_width()-92.0).max(160.0)).margin(vec2(8.0,7.0)));
                    if ui.button(tr!("Read URL")).clicked() && let Err(e)=self.ftp.set_url(&self.ftp_address){self.message=e;}
                });
                ui.add_space(8.0);
                let width=(ui.available_width()-116.0).max(160.0);
                egui::Grid::new("ftp-fields").num_columns(2).spacing(vec2(16.0,8.0)).show(ui,|ui|{
                    for (row, (label, value)) in [(tr!("Source name"),&mut self.ftp.name),(tr!("Host / IP"),&mut self.ftp.host),(tr!("Port"),&mut self.ftp.port),(tr!("Folder"),&mut self.ftp.root)].into_iter().enumerate() {
                        ui.label(RichText::new(label).color(palette::muted()));
                        let response = ui.add(egui::TextEdit::singleline(value).desired_width(width).margin(vec2(7.0,6.0)));
                        if row == 1 && std::mem::take(&mut self.ftp_focus_host) {
                            response.request_focus();
                        }
                        ui.end_row();
                    }
                    ui.label(RichText::new(tr!("Protocol")).color(palette::muted()));
                    egui::ComboBox::from_id_salt("ftp-scheme").selected_text(if self.ftp.scheme=="ftps"{"FTPS"}else{"FTP"}).show_ui(ui,|ui|{ui.selectable_value(&mut self.ftp.scheme,"ftp".into(),"FTP");ui.selectable_value(&mut self.ftp.scheme,"ftps".into(),tr!("Implicit FTPS"));});ui.end_row();
                });
                ui.add_space(12.0);section(ui,tr!("Access"),"");
                egui::Grid::new("ftp-auth").num_columns(2).spacing(vec2(16.0,8.0)).show(ui,|ui|{
                    ui.label(RichText::new(tr!("User")).color(palette::muted()));ui.add(egui::TextEdit::singleline(&mut self.ftp.username).desired_width(width).margin(vec2(7.0,6.0)));ui.end_row();
                    ui.label(RichText::new(tr!("Password")).color(palette::muted()));ui.add(egui::TextEdit::singleline(&mut self.ftp.password).password(true).desired_width(width).margin(vec2(7.0,6.0)));ui.end_row();
                });
                ui.checkbox(&mut self.remember_password,tr!("Remember in macOS Keychain"));
                ui.label(RichText::new(tr!("FTP sends unencrypted data: use a trusted network.")).small().color(palette::muted()));
                ui.add_space(12.0);section(ui,tr!("Automatic reconnection"),"");
                let mut rediscover=self.ftp.discovery_enabled();
                if ui.checkbox(&mut rediscover,tr!("Find the phone when its IP changes")).changed(){self.ftp.rediscover=Some(rediscover);if let Some(stored)=self.saved.sources.iter_mut().find(|s|s.id==self.ftp.id){stored.rediscover=Some(rediscover);}
                    if !rediscover{self.cancel_discovery();}}
                ui.add_enabled_ui(rediscover, |ui| {
                    use discovery::ScanPace;
                    let pace = &mut self.saved.ftp_scan_pace;
                    egui::ComboBox::from_label(tr!("LAN scan interval"))
                        .selected_text(pace.label())
                        .show_ui(ui, |ui| {
                            for option in [ScanPace::Ms5, ScanPace::Ms50, ScanPace::Fast, ScanPace::Conservative] {
                                ui.selectable_value(pace, option, option.label());
                            }
                        });
                });
                ui.label(RichText::new(tr!("MusicCraft checks the configured port and verifies the library before changing the connection.")).small().color(palette::muted()));
                if !self.directory.is_empty(){
                    ui.add_space(12.0);section(ui,tr!("Server folders"),"");
                    ui.add(egui::Label::new(RichText::new(&self.directory).small().color(palette::accent())).truncate()).on_hover_text(&self.directory);
                    ui.horizontal_wrapped(|ui|{
                        if ui.button(tr!("Parent folder")).clicked() && let Ok(u)=url::Url::parse(&self.directory) && let Ok(parent)=u.join("../"){self.ftp_browse(parent.into());}
                        if ui.button(tr!("Use as root")).clicked(){if let Err(e)=self.ftp.set_url(&self.directory){self.message=e;}else{self.connect();}}
                        if ui.add_enabled(!self.scanning,egui::Button::new(tr!("Synchronize folder"))).clicked(){self.sync_ftp(self.directory.clone());}
                    });
                    let mut navigate=None;let mut add=None;
                    for entry in &self.entries {
                        if entry.directory {if ui.button(crate::i18n::format("Folder /{name}",&[("name",entry.name.clone())])).clicked(){navigate=network::child_url(&self.directory,&entry.name,true);}}
                        else if library::audio_path(std::path::Path::new(&entry.name))&&ui.button(crate::i18n::format("Add {name}",&[("name",entry.name.clone())])).clicked(){add=network::child_url(&self.directory,&entry.name,false);}
                    }
                    if let Some(url)=navigate{self.ftp_browse(url);}
                    if let Some(url)=add && !self.saved.tracks.iter().any(|t|t.source==url){self.saved.tracks.push(Track::placeholder(url));self.library_revision=self.library_revision.wrapping_add(1);}
                }
            });
            ui.separator();
            if !self.message.is_empty(){ui.label(RichText::new(crate::i18n::engine_message(&self.message)).small().color(palette::accent()));}
            ui.horizontal(|ui|{
                if self.browsing {ui.add(egui::Spinner::new().size(14.0));ui.label(tr!("Checking…"));}
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{
                    if ui.add_enabled(!self.browsing,egui::Button::new(RichText::new(tr!("Check and save")).color(palette::bg())).fill(palette::accent())).clicked(){self.connect();}
                    if ui.button(tr!("Close")).clicked(){self.ftp_dialog=false;}
                });
            });
        });
        self.ftp_dialog &= open;
    }
}

impl eframe::App for Aurea {
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        if self.saved.mini {
            [0.0, 0.0, 0.0, 0.0]
        } else {
            palette::bg().to_normalized_gamma_f32()
        }
    }

    fn logic(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if self.viewport_pinned != Some(self.saved.pinned) {
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(if self.saved.pinned {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            }));
            self.viewport_pinned = Some(self.saved.pinned);
        }
        #[cfg(debug_assertions)]
        crate::about::preview(ctx);
        if palette::current() != self.saved.theme {
            palette::configure(ctx, self.saved.theme);
        }
        let observed = ctx.zoom_factor();
        let zoom = observed.clamp(1.0, 1.3);
        if observed != zoom {
            ctx.set_zoom_factor(zoom);
        }
        if zoom != self.viewport_zoom {
            self.viewport_zoom = zoom;
            self.saved.ui_scale = zoom;
            self.viewport_mini = None;
        }
        for paths in crate::open_documents::take() {
            self.open_external_files(paths);
        }
        self.sync_window_geometry(ctx);
        self.poll(ctx);
        let info_open = self.info_dialog || (self.saved.mini && self.saved.mini_view >= 3);
        let query = self
            .current
            .as_ref()
            .filter(|_| info_open && !self.loading)
            .and_then(|id| self.saved.tracks.iter().find(|t| &t.source == id))
            .and_then(crate::listening::Query::from_track);
        if self.listening.update(info_open, query) {
            self.last_lyric_line = None;
            self.lyric_follow = true;
        }
        let now = Instant::now();
        self.background_tick(now);
        let (visual_active, minimized) = ctx.input(|input| {
            let viewport = input.viewport();
            let minimized = viewport.minimized == Some(true);
            let active = viewport.focused.unwrap_or(true) || self.saved.mini && self.saved.pinned;
            (!minimized && active, minimized)
        });
        let elapsed = now.duration_since(self.last_frame).as_secs_f32().min(0.25);
        self.last_frame = now;
        let playing = !self.paused && !self.loading;
        if !minimized && playing && self.player_animation_visible() {
            let speed = if self.effective_player_appearance() == player_visual::Appearance::Cassette
            {
                1.4
            } else {
                0.85
            };
            self.reel_motion.advance(elapsed, speed);
            self.angle = (self.angle + elapsed * std::f32::consts::TAU * (33.333 / 60.0))
                .rem_euclid(std::f32::consts::TAU);
        }
        for i in 0..2 {
            let peak = self.audio.meters[i].swap(0, Ordering::Relaxed);
            self.meters[i] =
                if !minimized && self.saved.mini && (visual_active || self.saved.mini_view == 1) {
                    let new = if playing { f32::from_bits(peak) } else { 0.0 };
                    new.max(self.meters[i] * (-elapsed * 8.0).exp())
                } else {
                    0.0
                };
        }
        if !minimized {
            self.needle_motion.advance(elapsed, self.meters);
        }
        ctx.request_repaint_after(self.repaint_delay(visual_active, minimized));
        if let Some(deadline) = self.test_exit
            && Instant::now() >= deadline
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
    fn ui(&mut self, root: &mut egui::Ui, _: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        if ctx.input(|i| i.key_pressed(egui::Key::F1)) {
            self.help_dialog = true;
        }
        if !(ctx.embed_viewports() && self.cover_viewer.is_open())
            && !ctx.egui_wants_keyboard_input()
        {
            if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
                self.toggle();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
                let _ = self.audio.tx.send(Command::Seek(self.position + 5.0));
            }
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
                let _ = self
                    .audio
                    .tx
                    .send(Command::Seek((self.position - 5.0).max(0.0)));
            }
        }
        if self.saved.mini {
            self.mini_ui(root);
        } else {
            self.desktop_ui(root);
        }
        self.auxiliary_windows(&ctx);
        self.settings_window(&ctx);
        self.help_window(&ctx);
        self.cover_window(&ctx);

        let dropped: Vec<_> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if !dropped.is_empty() {
            self.import(dropped);
        }
        self.ftp_window(&ctx);
        self.phone_pair_window(&ctx);
        if self.screenshot.is_some()
            && !self.screenshot_sent
            && self.last_frame.elapsed() < Duration::from_secs(1)
        {
            // Capture after the initial layout settles.
            if ctx.cumulative_frame_nr() > 12 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.screenshot_sent = true;
            }
        }
        ctx.input(|i| {
            for event in &i.events {
                if let egui::Event::Screenshot { image, .. } = event
                    && let Some(path) = &self.screenshot
                {
                    let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                    let _ = image::save_buffer(
                        path,
                        &rgba,
                        image.size[0] as u32,
                        image.size[1] as u32,
                        image::ColorType::Rgba8,
                    );
                }
            }
        });
    }
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if let Some(size) = self.ctx.input_for(egui::ViewportId::ROOT, |i| {
            i.viewport().inner_rect.map(|r| r.size())
        }) {
            if self.viewport_mini.unwrap_or(self.saved.mini) {
                self.remember_mini_size(size);
            } else {
                self.remember_full_size(size);
            }
        }
        self.saved.artwork_window = self.cover_viewer.placement();
        self.saved.settings_window = self.settings_geometry.saved();
        self.saved.library_sort = self.browser.sort;
        self.saved.volume_db = valid_volume(self.volume_db);
        self.saved.native_rate = self.native;
        // Serialize without retaining a second copy of long source paths in RAM.
        self.saved.shuffle_session = Some(ShuffleSession {
            cycle: std::mem::take(&mut self.shuffle_cycle),
            selection: self.playback_selection.take(),
        });
        eframe::set_value(storage, "aurea", &self.saved);
        let session = self.saved.shuffle_session.take().unwrap();
        self.shuffle_cycle = session.cycle;
        self.playback_selection = session.selection;
    }
    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        window_drag::shutdown();
        self.embedded_server.stop();
        self.media_server = None;
        self.cancel_discovery();
        self.connections.clear();
        self.cancel.store(true, Ordering::Relaxed);
        self.audio.stop();
    }
}

fn valid_volume(db: f32) -> f32 {
    if db.is_finite() {
        db.clamp(-60.0, 0.0)
    } else {
        -12.0
    }
}

fn vinyl(p: &egui::Painter, rect: egui::Rect, angle: f32, art: Option<&egui::TextureHandle>) {
    let c = rect.center();
    let radius = rect.width() / 2.0 - 2.0;
    p.circle_filled(c, radius, Color32::from_rgb(8, 10, 12));
    for n in 0..18 {
        p.circle_stroke(
            c,
            radius * (0.46 + n as f32 * 0.029),
            egui::Stroke::new(0.6, Color32::from_rgb(38, 42, 44)),
        );
    }
    p.add(egui::Shape::mesh(vinyl_reflection(c, radius, angle)));
    if let Some(art) = art {
        let mut mesh = egui::Mesh::with_texture(art.id());
        mesh.vertices.push(egui::epaint::Vertex {
            pos: c,
            uv: pos2(0.5, 0.5),
            color: Color32::WHITE,
        });
        for i in 0..=64 {
            let a = i as f32 * std::f32::consts::TAU / 64.0;
            mesh.vertices.push(egui::epaint::Vertex {
                pos: c + vec2((a + angle).cos(), (a + angle).sin()) * radius * 0.4,
                uv: pos2(0.5 + 0.5 * a.cos(), 0.5 + 0.5 * a.sin()),
                color: Color32::WHITE,
            });
            if i > 0 {
                mesh.indices.extend([0, i, i + 1]);
            }
        }
        p.add(egui::Shape::mesh(mesh));
    } else {
        p.circle_filled(c, radius * 0.4, palette::accent());
        p.text(
            c,
            egui::Align2::CENTER_CENTER,
            "A",
            egui::FontId::proportional(radius * 0.42),
            palette::bg(),
        );
    }
    p.circle_filled(c, 2.0, Color32::from_rgb(10, 12, 14));
}
fn vinyl_reflection(center: egui::Pos2, radius: f32, angle: f32) -> egui::Mesh {
    let mut mesh = egui::Mesh::default();
    // Opposing soft glints over the grooves; both edges fade to the vinyl surface.
    const ANGULAR: usize = 24;
    const RADIAL: usize = 8;
    for (band, peak) in [(0.0, 38.0), (std::f32::consts::PI, 25.0)] {
        let start = mesh.vertices.len() as u32;
        for a in 0..=ANGULAR {
            let x = 2.0 * a as f32 / ANGULAR as f32 - 1.0;
            let theta = angle + band + 0.4 + x * 0.34;
            let angular = (x * std::f32::consts::FRAC_PI_2).cos().max(0.0).powi(3);
            for r in 0..=RADIAL {
                let t = r as f32 / RADIAL as f32;
                let radial = (std::f32::consts::PI * t).sin().max(0.0);
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: center + vec2(theta.cos(), theta.sin()) * radius * (0.43 + 0.55 * t),
                    uv: egui::epaint::WHITE_UV,
                    color: Color32::from_white_alpha((peak * angular * radial) as u8),
                });
                if a > 0 && r > 0 {
                    let n = start + (a * (RADIAL + 1) + r) as u32;
                    let before = n - (RADIAL + 1) as u32;
                    mesh.indices
                        .extend([before - 1, before, n - 1, before, n, n - 1]);
                }
            }
        }
    }
    mesh
}
fn time(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod source_tests {
    use super::*;
    #[test]
    fn remote_tag_results_refresh_the_library_without_replacing_playback_properties() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
        app.saved.auto_sync = false;
        app.saved.tracks = vec![Track::placeholder(
            "ftp://phone/Album/02%20Song.flac".into(),
        )];
        let mut result = app.saved.tracks[0].clone();
        result.title = "Actual title".into();
        result.artist = "Actual artist".into();
        result.album = "Actual album".into();
        result.number = 2;
        app.tx.send(Job::Tags(result.clone(), 0)).unwrap();
        app.poll(&ctx);
        assert_eq!(app.saved.tracks[0].title, "Actual title");
        assert!(app.saved.tracks[0].metadata_loaded);
        assert_eq!(app.saved.tracks[0].number, 2);
        let revision = app.library_revision;
        app.saved.tracks[0].rate = 96000;
        result.title = "Late title".into();
        app.tx.send(Job::Tags(result.clone(), 0)).unwrap();
        app.tx.send(Job::Tags(result, u64::MAX)).unwrap();
        app.poll(&ctx);
        assert_eq!(app.saved.tracks[0].title, "Actual title");
        assert_eq!(app.saved.tracks[0].rate, 96000);
        assert_eq!(app.library_revision, revision);
    }

    #[test]
    fn track_shuffle_exhausts_successfully_started_tracks_before_repeating() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
        let (commands_tx, _commands) = mpsc::channel();
        let (events_tx, events) = mpsc::channel();
        app.audio.tx = commands_tx;
        app.audio.rx = events;
        app.saved.auto_sync = false;
        app.online_covers.store(false, Ordering::Relaxed);
        app.saved.tracks = ["/synthetic/A.wav", "/synthetic/B.wav", "/synthetic/C.wav"]
            .into_iter()
            .map(|s| Track::placeholder(s.into()))
            .collect();
        app.saved.repeat = false;
        for i in [0, 1] {
            events_tx
                .send(Event::Started {
                    ticket: app.ticket,
                    track: app.saved.tracks[i].clone(),
                    output: "synthetic".into(),
                    cover: None,
                    exclusive: false,
                })
                .unwrap();
            app.poll(&ctx);
        }
        app.set_order_mode(1);
        for _ in 0..64 {
            assert_eq!(app.choose_next().as_deref(), Some("/synthetic/C.wav"));
        }
        let previous = app.history.previous().unwrap();
        assert_eq!(previous, "/synthetic/A.wav");
        app.play(previous);
        events_tx
            .send(Event::Started {
                ticket: app.ticket,
                track: app.saved.tracks[0].clone(),
                output: "synthetic".into(),
                cover: None,
                exclusive: false,
            })
            .unwrap();
        app.poll(&ctx);
        assert_eq!(app.planned.as_deref(), Some("/synthetic/B.wav"));
        app.next();
        assert_eq!(app.requested.as_deref(), Some("/synthetic/B.wav"));
        events_tx
            .send(Event::Started {
                ticket: app.ticket,
                track: app.saved.tracks[1].clone(),
                output: "synthetic".into(),
                cover: None,
                exclusive: false,
            })
            .unwrap();
        app.poll(&ctx);
        assert_eq!(app.planned.as_deref(), Some("/synthetic/C.wav"));
        // Cycling modes and stopping must not erase the listening cycle.
        for mode in [2, 0, 1] {
            app.set_order_mode(mode);
        }
        assert_eq!(app.planned.as_deref(), Some("/synthetic/C.wav"));
        app.stop();
        assert_eq!(app.choose_next().as_deref(), Some("/synthetic/C.wav"));
        // Starting the same results in a different order preserves membership.
        app.capture_selection();
        assert_eq!(app.choose_next().as_deref(), Some("/synthetic/C.wav"));
        app.play("/synthetic/C.wav".into());
        events_tx
            .send(Event::Error(app.ticket, "synthetic failed load".into()))
            .unwrap();
        app.poll(&ctx);
        assert_eq!(app.choose_next().as_deref(), Some("/synthetic/C.wav"));
    }
    #[test]
    fn shuffle_cycles_follow_real_start_notifications_and_both_advance_paths() {
        for filtered in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let ctx = egui::Context::default();
            let cc = eframe::CreationContext::_new_kittest(ctx.clone());
            let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
            let (commands_tx, commands) = mpsc::channel();
            let (events_tx, events) = mpsc::channel();
            app.audio.tx = commands_tx;
            app.audio.rx = events;
            app.saved.auto_sync = false;
            app.online_covers.store(false, Ordering::Relaxed);
            app.saved.folders = vec!["/synthetic/phone".into(), "/synthetic/NAS".into()];
            app.saved.active_source = Some("folder:/synthetic/phone".into());
            app.saved.tracks = (0..8)
                .map(|i| Track::placeholder(format!("/synthetic/phone/album{}/{}.wav", i / 2, i)))
                .chain(std::iter::once(Track::placeholder(
                    "/synthetic/NAS/foreign.wav".into(),
                )))
                .collect();
            if filtered {
                app.playback_selection = Some(
                    app.saved.tracks[..5]
                        .iter()
                        .map(|t| t.source.clone())
                        .collect(),
                );
            }
            app.saved.shuffle = true;
            let count = if filtered { 5 } else { 8 };
            let first = app.choose_next().unwrap();
            app.play(first);
            let mut last = None;
            for cycle in 0..3 {
                let mut heard = HashSet::new();
                for index in 0..count {
                    let track = match commands.try_recv().unwrap() {
                        Command::Load { track, .. } => track,
                        _ => panic!("expected the existing audio load path"),
                    };
                    assert!(track.source.starts_with("/synthetic/phone/"));
                    assert_ne!(last.as_ref(), Some(&track.source));
                    assert!(
                        heard.insert(track.source.clone()),
                        "repeat in cycle {cycle}"
                    );
                    last = Some(track.source.clone());
                    events_tx
                        .send(Event::Started {
                            ticket: app.ticket,
                            track,
                            output: "synthetic".into(),
                            cover: None,
                            exclusive: false,
                        })
                        .unwrap();
                    app.poll(&ctx);
                    let planned = app.planned.clone().unwrap();
                    if index % 2 == 0 {
                        app.next();
                    } else {
                        events_tx.send(Event::Ended).unwrap();
                        app.poll(&ctx);
                    }
                    assert_eq!(app.requested.as_ref(), Some(&planned));
                }
                assert_eq!(heard.len(), count);
            }
        }
    }
    #[test]
    fn stale_prefetch_for_the_same_track_cannot_adopt_a_new_order_plan() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
        let (tx, commands) = mpsc::channel();
        app.audio.tx = tx;
        let track = Track::placeholder("/synthetic/next.wav".into());
        app.saved.tracks = vec![track.clone()];
        app.planned = Some(track.source.clone());
        app.ticket = 7;
        app.audio.generation.store(7, Ordering::Relaxed);
        app.audio.plan.store(4, Ordering::Relaxed);
        for plan in [Some(1), None, Some(3)] {
            app.tx
                .send(Job::Ready(track.clone(), None, 7, plan))
                .unwrap();
            app.poll(&ctx);
            assert!(
                commands.try_recv().is_err(),
                "stale or artwork-only work must not enter the audio queue"
            );
        }
        app.tx.send(Job::Ready(track, None, 7, Some(4))).unwrap();
        app.poll(&ctx);
        assert!(matches!(
            commands.try_recv(),
            Ok(Command::Queue {
                ticket: 7,
                plan: 4,
                ..
            })
        ));
    }
    #[test]
    #[ignore = "explicit live shared library scheduler fixture on loopback"]
    fn completed_sync_updates_only_the_shared_source_with_settings_closed() {
        let profile = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = Aurea::new(&cc, profile.path().into(), vec![], None, None);
        let root = profile.path().join("phone");
        let nas = profile.path().join("NAS");
        app.saved.folders = vec![root.clone(), nas.clone()];
        app.saved.active_source = Some(format!("folder:{}", root.display()));
        let first = Track::placeholder(root.join("first.wav").to_string_lossy().into_owned());
        app.saved.tracks = vec![first.clone()];
        app.saved.multiroom.integrated = false;
        app.media_server = Some(
            crate::media_server::Server::start(
                "127.0.0.1:0".parse().unwrap(),
                vec![first],
                app.cache.clone(),
                app.auth.clone(),
            )
            .unwrap(),
        );
        app.shared_source = app.saved.active_source.clone();
        let url = app.media_server.as_ref().unwrap().playlist_url();
        app.saved.active_source = Some(format!("folder:{}", nas.display()));
        assert!(!app.settings_dialog);
        app.scanning = true;
        app.tx
            .send(Job::SyncDone(source_sync::Report {
                target: Target::Folder(root.clone()),
                tracks: vec![
                    Track::placeholder(root.join("second.wav").to_string_lossy().into_owned()),
                    Track::placeholder(nas.join("foreign.wav").to_string_lossy().into_owned()),
                ],
                absent: Default::default(),
                stats: Default::default(),
                cancelled: false,
            }))
            .unwrap();
        app.poll(&ctx);
        app.settings_window(&ctx);
        let server = app.media_server.as_ref().unwrap();
        assert_eq!(server.playlist_url(), url);
        assert_eq!(server.count, 2);
        assert!(server.playlist().contains("second"));
        assert!(!server.playlist().contains("foreign"));
        let revision = app.library_revision;
        app.tx
            .send(Job::SyncDone(source_sync::Report {
                target: Target::Folder(root),
                tracks: vec![],
                absent: Default::default(),
                stats: Default::default(),
                cancelled: false,
            }))
            .unwrap();
        app.poll(&ctx);
        assert_eq!(
            app.library_revision, revision,
            "unchanged scans skip publication"
        );
        app.refresh_shared_library(true);
        assert_eq!(
            app.media_server.as_ref().unwrap().count,
            1,
            "explicit refresh selects the current source without restart"
        );
        assert_eq!(app.media_server.as_ref().unwrap().playlist_url(), url);
    }
    #[test]
    fn failed_phone_search_opens_its_ip_editor_once_and_preserves_other_edits() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
        let first = FtpSource {
            id: "first".into(),
            ..FtpSource::smartphone()
        };
        let phone = FtpSource {
            id: "phone".into(),
            host: "192.168.42.8".into(),
            port: "8736".into(),
            ..FtpSource::smartphone()
        };
        app.saved.auto_sync = true;
        app.saved.sources = vec![first, phone.clone()];
        let complete_search = |app: &mut Aurea, reason| {
            app.discovery = Some(RediscoveryRequest {
                token: 5,
                source: phone.clone(),
                networks: app.networks.clone(),
                cancel: Arc::new(AtomicBool::new(false)),
            });
            app.tx
                .send(Job::Rediscovered {
                    request: 5,
                    outcome: Outcome::Missing(reason),
                })
                .unwrap();
            app.poll(&ctx);
        };

        app.saved.active_source = Some("first".into());
        complete_search(&mut app, Reason::NotFound);
        assert!(
            !app.ftp_dialog,
            "a missing inactive source must not interrupt the selected library"
        );
        app.saved.active_source = None;
        app.phone_pair_dialog = true;
        complete_search(&mut app, Reason::NotFound);
        assert!(
            !app.ftp_dialog,
            "FTP fallback must not interrupt Android pairing"
        );
        app.phone_pair_dialog = false;
        complete_search(&mut app, Reason::Cancelled);
        assert!(!app.ftp_dialog);
        app.open_ftp_editor(Some(0));
        app.ftp.host = "unsaved-edit".into();
        complete_search(&mut app, Reason::NotFound);
        assert_eq!(app.ftp.host, "unsaved-edit");

        app.ftp_dialog = false;
        complete_search(&mut app, Reason::NotFound);
        assert!(app.ftp_dialog && app.ftp_focus_host);
        assert_eq!(app.ftp_edit, Some(1));
        assert_eq!(app.ftp.host, phone.host);
        assert_eq!(app.ftp.port, phone.port);
        assert_eq!(app.saved.sources[1].url(), phone.url());

        app.ftp_dialog = false;
        complete_search(&mut app, Reason::NotFound);
        assert!(!app.ftp_dialog);
        app.tx
            .send(Job::NetworkSnapshot(vec![
                Lan::new(
                    "192.168.43.4".parse().unwrap(),
                    "255.255.255.0".parse().unwrap(),
                )
                .unwrap(),
            ]))
            .unwrap();
        app.poll(&ctx);
        complete_search(&mut app, Reason::NotFound);
        assert!(app.ftp_dialog);
    }

    #[test]
    fn rediscovery_rebases_only_one_source_without_stopping_pcm_or_losing_history() {
        let profile = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = Aurea::new(&cc, profile.path().into(), vec![], None, None);
        let mut phone = FtpSource::smartphone();
        phone.host = "old.local".into();
        phone.id = "phone".into();
        phone.username = "fixture".into();
        phone.password = "synthetic".into();
        let old = phone.url().unwrap();
        let p1 = format!("{old}Album/01.flac");
        let p2 = format!("{old}Album/02.flac");
        let foreign = "ftp://other.local/Music/foreign.flac".to_owned();
        app.saved.sources.push(phone.clone());
        app.saved.active_source = Some(phone.id.clone());
        app.saved.tracks = [p1.clone(), p2.clone(), foreign.clone()]
            .into_iter()
            .map(Track::placeholder)
            .collect();
        app.saved.tracks[0].artist = "Preserved artist".into();
        app.current = Some(p2.clone());
        app.position = 71.5;
        app.paused = false;
        app.history.record(p1.clone());
        app.history.record(p2.clone());
        app.covers.insert(p1.clone(), None);
        app.playback_selection = Some(vec![p1.clone(), p2.clone()]);
        app.saved.facet_fields[0] = crate::facets::Field::Folder;
        app.browser.selected[0].insert(format!("{old}Album"));
        app.browser.selected_track = Some(p2.clone());
        let ticket = app.ticket;
        let mut found = phone.clone();
        found.host = "new.local".into();
        let new = found.url().unwrap();
        let n1 = network::rebase(&p1, &old, &new).unwrap();
        let n2 = network::rebase(&p2, &old, &new).unwrap();
        app.apply_discovered(found, &old);
        assert_eq!(
            app.ticket, ticket,
            "the current audio generation must not be stopped"
        );
        assert_eq!(app.current, Some(n2.clone()));
        assert_eq!(app.playback_selection, Some(vec![n1.clone(), n2.clone()]));
        assert_eq!(app.browser.selected_track, Some(n2.clone()));
        assert!(app.browser.selected[0].contains(&format!("{new}Album")));
        assert_eq!(app.position, 71.5);
        assert!(!app.paused);
        assert_eq!(app.saved.active_source.as_deref(), Some("phone"));
        assert_eq!(
            app.saved
                .tracks
                .iter()
                .map(|t| t.source.clone())
                .collect::<Vec<_>>(),
            [n1.clone(), n2.clone(), foreign]
        );
        assert_eq!(app.saved.tracks[0].artist, "Preserved artist");
        assert!(app.covers.contains_key(&n1));
        assert_eq!(app.history.previous(), Some(n1.clone()));
        app.history.record(n1);
        assert_eq!(app.history.next(), Some(n2.clone()));
        assert_eq!(app.canonical_source(&p2), n2);
        let mut prepared = Track::placeholder(p1);
        prepared.artist = "Updated metadata".into();
        app.tx.send(Job::Prepared(prepared, None, ticket)).unwrap();
        app.poll(&ctx);
        assert_eq!(
            app.saved.tracks[0].artist, "Updated metadata",
            "late workers from the previous IP are normalized"
        );
        // The user may change the address while a discovery worker is returning.
        let token = 42;
        let stale_source = app.saved.sources[0].clone();
        app.discovery = Some(RediscoveryRequest {
            token,
            source: stale_source.clone(),
            networks: vec![],
            cancel: Arc::new(AtomicBool::new(false)),
        });
        app.saved.sources[0].host = "manual.local".into();
        app.tx
            .send(Job::Rediscovered {
                request: token,
                outcome: Outcome::Found(Box::new(stale_source)),
            })
            .unwrap();
        app.poll(&ctx);
        assert_eq!(app.saved.sources[0].host, "manual.local");
        // A changed LAN resets a stale offline retry delay.
        let target = app.sync_targets()[0].clone();
        let now = Instant::now();
        app.sync_scheduler.finished(&target, now, false);
        app.tx
            .send(Job::NetworkSnapshot(vec![
                Lan::new(
                    "192.168.1.5".parse().unwrap(),
                    "255.255.255.0".parse().unwrap(),
                )
                .unwrap(),
            ]))
            .unwrap();
        app.poll(&ctx);
        assert!(
            app.sync_scheduler
                .due(std::slice::from_ref(&target), now)
                .is_none()
        );
        assert_eq!(
            app.sync_scheduler
                .due(std::slice::from_ref(&target), now + Duration::from_secs(5)),
            Some(target)
        );
    }
    #[test]
    fn automatic_sync_discovers_new_songs_without_playing_and_respects_pause_and_overlap() {
        let profile = tempfile::tempdir().unwrap();
        let music = tempfile::tempdir().unwrap();
        let album = music.path().join("Album");
        std::fs::create_dir(&album).unwrap();
        std::fs::write(album.join("01.flac"), b"synthetic").unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = Aurea::new(&cc, profile.path().into(), vec![], None, None);
        app.saved.folders.push(music.path().into());
        let now = Instant::now();
        app.auto_sync_tick(now);
        assert!(!app.scanning);
        app.saved.auto_sync = false;
        app.auto_sync_tick(now + Duration::from_secs(6));
        assert!(!app.scanning);
        app.saved.auto_sync = true;
        app.loading = true;
        app.auto_sync_tick(now + Duration::from_secs(6));
        assert!(!app.scanning);
        app.loading = false;
        app.auto_sync_tick(now + Duration::from_secs(6));
        assert!(app.scanning);
        let tx = app.tx.clone();
        app.auto_sync_tick(now + Duration::from_secs(999));
        assert!(app.scanning);
        let wait = |app: &mut Aurea| {
            let deadline = Instant::now() + Duration::from_secs(5);
            while app.scanning && Instant::now() < deadline {
                app.poll(&ctx);
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(!app.scanning);
        };
        wait(&mut app);
        assert_eq!(app.saved.tracks.len(), 1);
        assert!(app.current.is_none());
        assert!(app.requested.is_none());
        let stamp = std::fs::metadata(&album).unwrap().modified().unwrap();
        std::fs::write(album.join("02.flac"), b"synthetic").unwrap();
        std::fs::File::open(&album)
            .unwrap()
            .set_modified(stamp + Duration::from_secs(2))
            .unwrap();
        app.auto_sync_tick(now + Duration::from_secs(121));
        wait(&mut app);
        assert_eq!(app.saved.tracks.len(), 2);
        assert!(app.paused);
        assert!(app.current.is_none());
        // A response from the old FTP root after editing its address is ignored.
        let mut phone = FtpSource::smartphone();
        phone.id = "old-synthetic".into();
        phone.host = "new.local".into();
        app.saved.sources.push(phone);
        tx.send(Job::SyncDone(source_sync::Report {
            target: Target::Ftp {
                id: "old-synthetic".into(),
                root: "ftp://old.local/".into(),
            },
            tracks: vec![Track::placeholder("ftp://old.local/foreign.flac".into())],
            absent: Default::default(),
            stats: Default::default(),
            cancelled: false,
        }))
        .unwrap();
        app.poll(&ctx);
        assert_eq!(app.saved.tracks.len(), 2);
        // Named accounts without a recovered password wait for user input.
        app.saved.folders.clear();
        app.saved.sources[0].username = "fixture".into();
        app.auto_sync_tick(now + Duration::from_secs(999));
        assert!(!app.scanning);
        let saved = serde_json::to_value(&app.saved).unwrap();
        assert_eq!(saved["auto_sync"], true);
    }
    #[test]
    fn credential_completion_is_guarded_and_never_overwrites_manual_password() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
        let mut source = FtpSource::smartphone();
        source.id = "synthetic".into();
        source.host = "synthetic.local".into();
        source.username = "test".into();
        source.password.clear();
        let root = source.url().unwrap();
        app.saved.sources.push(source);
        app.credential_pending.insert("synthetic".into());
        assert!(app.awaiting_credentials(&root));
        app.tx
            .send(Job::Credential {
                id: "synthetic".into(),
                username: "test".into(),
                password: Some("synthetic-secret".into()),
            })
            .unwrap();
        app.poll(&ctx);
        assert!(!app.awaiting_credentials(&root));
        assert_eq!(app.saved.sources[0].password, "synthetic-secret");
        app.saved.sources[0].password = "manual-synthetic".into();
        app.tx
            .send(Job::Credential {
                id: "synthetic".into(),
                username: "test".into(),
                password: Some("obsolete-synthetic".into()),
            })
            .unwrap();
        app.poll(&ctx);
        assert_eq!(app.saved.sources[0].password, "manual-synthetic");
    }
    #[test]
    fn source_isolation_controls_sequential_shuffle_repeat_and_cancels_old_history() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx);
        let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
        let mut phone = FtpSource::smartphone();
        phone.id = "phone".into();
        phone.host = "phone.local".into();
        phone.port = "8736".into();
        let root = phone.url().unwrap();
        let p1 = format!("{root}1.flac");
        let p2 = format!("{root}2.flac");
        let n1 = "/Volumes/NAS/Music/1.flac".to_string();
        let n2 = "/Volumes/NAS/Music/2.flac".to_string();
        app.saved.sources = vec![phone];
        app.saved.folders = vec!["/Volumes/NAS/Music".into()];
        app.saved.tracks = [
            p1.clone(),
            n1.clone(),
            p2.clone(),
            n2.clone(),
            "/Volumes/NAS/MusicOther/foreign.flac".into(),
        ]
        .into_iter()
        .map(Track::placeholder)
        .collect();
        app.select_source(Some("phone".into()));
        assert_eq!(app.scoped_tracks().count(), 2);
        assert_eq!(app.choose_next(), Some(p1.clone()));
        app.current = Some(p1.clone());
        app.history.record(p1.clone());
        app.history.record(p2.clone());
        assert_eq!(app.choose_next(), Some(p2.clone()));
        app.saved.shuffle = true;
        for _ in 0..50 {
            assert_eq!(app.choose_next(), Some(p2.clone()));
        }
        app.saved.repeat = true;
        assert_eq!(app.choose_next(), Some(p1));
        let old_ticket = app.ticket;
        app.select_source(Some("folder:/Volumes/NAS/Music".into()));
        assert!(app.ticket > old_ticket);
        assert!(app.current.is_none());
        assert!(app.history.previous().is_none());
        assert!(app.planned.is_none());
        assert_eq!(
            app.scoped_tracks()
                .map(|t| t.source.clone())
                .collect::<Vec<_>>(),
            [n1.clone(), n2.clone()]
        );
        app.saved.repeat = false;
        app.saved.shuffle = false;
        assert_eq!(app.choose_next(), Some(n1));
        app.current = Some(n2);
        assert_eq!(app.choose_next(), None);
        let ticket = app.ticket;
        app.play(p2);
        assert_eq!(app.ticket, ticket);
        assert!(
            app.requested.is_none(),
            "an old source cannot be played through a stale action"
        );
        app.select_source(None);
        assert_eq!(app.scoped_tracks().count(), 5);
    }
}
