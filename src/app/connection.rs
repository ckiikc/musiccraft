//! Session-only FTP readiness. No audio transfer starts against an unverified saved endpoint.
use super::*;
#[derive(Default)]
pub(super) struct Gate {
    verified: HashSet<String>,
    manual_offered: HashSet<String>,
    retry: HashMap<String, (u32, Instant)>,
    request: Option<Request>,
    next: Option<Instant>,
}
struct Request {
    token: u64,
    source: FtpSource,
    cancel: Arc<AtomicBool>,
}
fn key(source: &FtpSource) -> String {
    format!("{}\n{}", source.url().unwrap_or_default(), source.username)
}
impl Gate {
    pub(super) fn request_pending(&self) -> bool {
        self.request.is_some()
    }
    pub(super) fn mark_verified(&mut self, source: &FtpSource) {
        self.verified(source);
    }
    pub(super) fn invalidate(&mut self, source: &FtpSource) {
        self.verified.remove(&key(source));
        self.retry.remove(&key(source));
    }
    fn ready(&self, source: &FtpSource) -> bool {
        self.verified.contains(&key(source))
    }
    fn verified(&mut self, source: &FtpSource) {
        let key = key(source);
        if self
            .request
            .as_ref()
            .is_some_and(|r| crate::app::connection::key(&r.source) == key)
        {
            self.request
                .take()
                .unwrap()
                .cancel
                .store(true, Ordering::Relaxed);
        }
        self.retry.remove(&key);
        self.verified.insert(key);
    }
    pub(super) fn failed(&mut self, source: &FtpSource) {
        let key = key(source);
        self.verified.remove(&key);
        let count = self.retry.get(&key).map_or(1, |(n, _)| (n + 1).min(6));
        // Android sharing is often enabled after the desktop has already opened.
        // Retry its saved identity with Bonjour; no subnet scan or new pairing.
        let seconds = if source.scheme == "https" {
            5
        } else {
            (15 * (1 << count)).min(600)
        };
        self.retry
            .insert(key, (count, Instant::now() + Duration::from_secs(seconds)));
    }
    pub(super) fn offer_manual_address(&mut self, id: &str) -> bool {
        self.manual_offered.insert(id.into())
    }
    pub(super) fn network_changed(&mut self) {
        self.clear();
        self.manual_offered.clear();
    }
    pub(super) fn clear(&mut self) {
        if let Some(r) = self.request.take() {
            r.cancel.store(true, Ordering::Relaxed);
        }
        self.verified.clear();
        self.manual_offered.clear();
        self.retry.clear();
    }
}
impl Drop for Gate {
    fn drop(&mut self) {
        self.clear();
    }
}
impl Aurea {
    pub(super) fn source_for_track(&self, id: &str) -> Option<&FtpSource> {
        self.saved
            .sources
            .iter()
            .find(|s| s.url().is_ok_and(|root| id.starts_with(&root)))
    }
    pub(super) fn track_connection_ready(&self, id: &str) -> bool {
        self.source_for_track(id)
            .is_none_or(|s| self.connections.ready(s))
    }
    fn selected_connection(&self) -> Option<&FtpSource> {
        self.saved
            .active_source
            .as_ref()
            .and_then(|id| self.saved.sources.iter().find(|s| &s.id == id))
    }
    pub(super) fn navigation_ready(&self) -> bool {
        !self.loading
            && self
                .selected_connection()
                .is_none_or(|s| self.connections.ready(s))
    }
    pub(super) fn can_toggle(&self) -> bool {
        !self.loading && (self.current.is_some() || self.navigation_ready())
    }
    pub(super) fn connection_tick(&mut self, now: Instant) {
        if self.phone_pair_dialog
            || self.network_probe_pending
            || self.connections.request.is_some()
            || self.discovery.is_some()
            || self.scanning
            || self.browsing
            || self.connections.next.is_some_and(|next| now < next)
        {
            return;
        }
        let selected = self.saved.active_source.as_deref();
        let source = self
            .saved
            .sources
            .iter()
            .filter(|s| {
                !self.connections.ready(s)
                    && !self.awaiting_credentials(&s.url().unwrap_or_default())
                    && (s.username.is_empty() || !s.password.is_empty())
                    && self
                        .connections
                        .retry
                        .get(&key(s))
                        .is_none_or(|(_, due)| now >= *due)
            })
            .min_by_key(|s| usize::from(Some(s.id.as_str()) != selected))
            .cloned();
        let Some(source) = source else {
            return;
        };
        let Ok(root) = source.url() else {
            return;
        };
        let token = rand::random();
        let cancel = Arc::new(AtomicBool::new(false));
        self.connections.request = Some(Request {
            token,
            source: source.clone(),
            cancel: cancel.clone(),
        });
        self.connections.next = Some(now + Duration::from_secs(1));
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let auth = AuthStore::default();
            let ok = auth.insert(&source).is_ok()
                && if source.scheme == "https" {
                    crate::phone::online(&root, &auth, &cancel, Duration::from_secs(2)).is_ok()
                } else {
                    network::ListingSession::with_timeout(Duration::from_secs(2))
                        .list(&root, &auth, &cancel)
                        .is_ok()
                };
            let _ = tx.send(Job::ConnectionChecked { token, ok });
            ctx.request_repaint();
        });
    }
    pub(super) fn connection_result(&mut self, token: u64, ok: bool) {
        if self
            .connections
            .request
            .as_ref()
            .is_none_or(|r| r.token != token)
        {
            return;
        }
        let request = self.connections.request.take().unwrap();
        if request.cancel.load(Ordering::Relaxed)
            || !self.saved.sources.iter().any(|s| {
                s.id == request.source.id
                    && s.url() == request.source.url()
                    && s.username == request.source.username
                    && s.password == request.source.password
            })
        {
            return;
        }
        if ok {
            self.connections.verified(&request.source);
        } else {
            self.connections.failed(&request.source);
            self.start_discovery(&request.source.id);
        }
    }
    pub(super) fn connection_indicator(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        small: bool,
    ) -> bool {
        let Some(source) = self
            .selected_connection()
            .filter(|s| !self.connections.ready(s))
            .cloned()
        else {
            return false;
        };
        let waiting = self.connections.request.is_some() || self.discovery.is_some();
        let needs_credentials = !source.username.is_empty() && source.password.is_empty();
        let label = format!(
            "{} · {}",
            source.name,
            if self.awaiting_credentials(&source.url().unwrap_or_default()) {
                tr!("Authorize Keychain or enter the password in Network")
            } else if needs_credentials {
                tr!("Password unavailable: open Network to enter it")
            } else if waiting || !self.connections.retry.contains_key(&key(&source)) {
                tr!("Checking…")
            } else {
                tr!("Connection unavailable or timed out")
            }
        );
        let retry_width = if small { 58.0 } else { 64.0 };
        let text_rect = egui::Rect::from_min_max(
            rect.min,
            pos2(
                (rect.right() - retry_width - 4.0).max(rect.left()),
                rect.bottom(),
            ),
        );
        ui.put(
            text_rect,
            egui::Label::new(
                RichText::new(&label)
                    .size(if small { 9.5 } else { 11.0 })
                    .color(palette::muted()),
            )
            .truncate(),
        )
        .on_hover_text(label);
        let response = ui
            .scope(|ui| {
                if waiting {
                    ui.disable();
                }
                ui.spacing_mut().button_padding = vec2(4.0, 1.0);
                ui.spacing_mut().interact_size.y = 14.0;
                ui.put(
                    egui::Rect::from_min_size(
                        pos2(rect.right() - retry_width, rect.top()),
                        vec2(retry_width, 14.0),
                    ),
                    egui::Button::new(
                        RichText::new(if needs_credentials {
                            tr!("Connection")
                        } else {
                            tr!("Retry")
                        })
                        .size(9.5),
                    ),
                )
            })
            .inner;
        if response.clicked() && needs_credentials {
            self.open_network();
        } else if response.clicked() {
            self.connections.retry.remove(&key(&source));
            self.connections.next = None;
            self.connection_tick(Instant::now());
        }
        true
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn paired_phone() -> FtpSource {
        crate::phone::pairing(&format!(
            "musiccraft://pair?v=1&host=127.0.0.1&port=8737&id=01234567-89ab-cdef-0123-456789abcdef&pin={}=&token={}",
            "A".repeat(43), "B".repeat(43)
        )).unwrap()
    }
    #[test]
    fn paired_phone_waits_without_pairing_and_recovers_in_the_same_session() {
        for auto_sync in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let ctx = egui::Context::default();
            let cc = eframe::CreationContext::_new_kittest(ctx.clone());
            let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
            let phone = paired_phone();
            let old = phone.url().unwrap();
            app.saved.auto_sync = auto_sync;
            app.saved.sources = vec![phone.clone()];
            app.saved.active_source = Some(phone.id.clone());
            app.saved.tracks = vec![Track::placeholder(format!("{old}42.flac"))];
            app.connections.mark_verified(&phone);
            let ticket = app.ticket;
            for token in 1..=8 {
                app.discovery = Some(RediscoveryRequest {
                    token,
                    source: phone.clone(),
                    networks: app.networks.clone(),
                    cancel: Arc::new(AtomicBool::new(false)),
                });
                app.tx
                    .send(Job::Rediscovered {
                        request: token,
                        outcome: Outcome::Missing(Reason::NotFound),
                    })
                    .unwrap();
                app.poll(&ctx);
                assert!(
                    !app.phone_pair_dialog,
                    "an offline paired phone must never open pairing"
                );
                assert!(!app.ftp_dialog);
                assert!(
                    !app.navigation_ready(),
                    "an offline endpoint must not remain verified"
                );
                let due = app.connections.retry[&key(&phone)].1;
                assert!(due > Instant::now());
                assert!(due <= Instant::now() + Duration::from_secs(5));
            }
            let mut found = phone.clone();
            found.host = "127.0.0.2".into();
            found.port = "8738".into();
            app.discovery = Some(RediscoveryRequest {
                token: 9,
                source: phone.clone(),
                networks: app.networks.clone(),
                cancel: Arc::new(AtomicBool::new(false)),
            });
            app.tx
                .send(Job::Rediscovered {
                    request: 9,
                    outcome: Outcome::Found(Box::new(found.clone())),
                })
                .unwrap();
            app.poll(&ctx);
            assert!(app.navigation_ready());
            assert!(!app.phone_pair_dialog && !app.ftp_dialog);
            assert_eq!(app.saved.sources[0].url(), found.url());
            assert_eq!(app.saved.sources[0].id, phone.id);
            assert_eq!(app.saved.sources[0].tls_pin, phone.tls_pin);
            assert_eq!(app.saved.sources[0].password, phone.password);
            assert_eq!(
                app.saved.tracks[0].source,
                format!("{}42.flac", found.url().unwrap())
            );
            assert_eq!(app.ticket, ticket);
            assert!(!app.loading);
        }
    }
    #[test]
    fn paired_phone_retries_stay_short_while_ftp_keeps_its_backoff() {
        let mut gate = Gate::default();
        let phone = paired_phone();
        let ftp = FtpSource::smartphone();
        for _ in 0..12 {
            gate.failed(&phone);
            gate.failed(&ftp);
            assert!(gate.retry[&key(&phone)].1 <= Instant::now() + Duration::from_secs(5));
        }
        assert!(gate.retry[&key(&ftp)].1 > Instant::now() + Duration::from_secs(500));
    }
    #[test]
    fn failed_manual_phone_sync_invalidates_the_cached_connection() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
        let phone = paired_phone();
        app.network_probe_pending = true; // Keep this state regression free of network workers.
        app.saved.sources = vec![phone.clone()];
        app.saved.active_source = Some(phone.id.clone());
        app.connections.mark_verified(&phone);
        assert!(app.navigation_ready());
        app.tx
            .send(Job::SyncDone(source_sync::Report {
                target: Target::Ftp {
                    id: phone.id.clone(),
                    root: phone.url().unwrap(),
                },
                tracks: vec![],
                absent: Default::default(),
                cancelled: false,
                stats: source_sync::Stats {
                    root_unreachable: true,
                    errors: 1,
                    ..Default::default()
                },
            }))
            .unwrap();
        app.poll(&ctx);
        assert!(!app.navigation_ready());
        assert!(!app.phone_pair_dialog);
        assert!(app.connections.retry[&key(&phone)].1 <= Instant::now() + Duration::from_secs(5));
    }
    #[test]
    #[ignore = "explicit synthetic HTTPS and Bonjour fixture on loopback"]
    fn live_paired_phone_returns_after_sharing_is_enabled_without_restart() {
        let fixture = PathBuf::from(std::env::var_os("MUSICCRAFT_QA_RECONNECT").unwrap());
        let phone =
            crate::phone::pairing(&std::fs::read_to_string(fixture.join("pairing.txt")).unwrap())
                .unwrap();
        assert_eq!(phone.host, "127.0.0.1", "synthetic loopback fixture only");
        assert_eq!(
            phone.port, "9",
            "the saved endpoint must initially be offline"
        );
        let profile = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = Aurea::new(&cc, profile.path().into(), vec![], None, None);
        app.saved.sources = vec![phone.clone()];
        app.saved.active_source = Some(phone.id.clone());
        app.saved.auto_sync = std::env::var_os("MUSICCRAFT_QA_NO_AUTO_SYNC").is_none();
        app.networks = discovery::lans();
        let started = Instant::now();
        let mut offline_seen = false;
        while started.elapsed() < Duration::from_secs(50) {
            app.poll(&ctx);
            assert!(!app.phone_pair_dialog && !app.ftp_dialog);
            if !offline_seen
                && app.discovery.is_none()
                && app.connections.retry.contains_key(&key(&phone))
            {
                offline_seen = true;
                std::fs::write(fixture.join("offline-observed"), "ready").unwrap();
            }
            if app.navigation_ready() {
                assert!(offline_seen, "first observe the phone with sharing off");
                let found = &app.saved.sources[0];
                assert_ne!(found.url(), phone.url());
                assert_eq!(found.id, phone.id);
                assert!(
                    found.host.parse::<std::net::Ipv4Addr>().is_ok(),
                    "Bonjour must not persist a name that audio needs to resolve again"
                );
                assert_eq!(found.tls_pin, phone.tls_pin);
                assert_eq!(found.password, phone.password);
                let auth = AuthStore::default();
                auth.insert(found).unwrap();
                let tracks = crate::phone::catalog(
                    &found.url().unwrap(),
                    &auth,
                    &AtomicBool::new(false),
                    Duration::from_secs(3),
                )
                .unwrap();
                assert_eq!(tracks.len(), 1);
                assert_eq!(tracks[0].title, "Synthetic reconnect");
                std::fs::write(
                    fixture.join("recovered.json"),
                    serde_json::json!({
                        "elapsed_seconds": started.elapsed().as_secs_f64(),
                        "pairing_dialog_opened": false,
                        "catalog_tracks": tracks.len(),
                    })
                    .to_string(),
                )
                .unwrap();
                return;
            }
            app.connection_tick(Instant::now());
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("paired phone did not reconnect in the same process");
    }
    #[test]
    fn desktop_and_mini_disable_start_and_navigation_until_the_selected_source_is_ready() {
        for mini in [false, true] {
            for ready in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                let ctx = egui::Context::default();
                let cc = eframe::CreationContext::_new_kittest(ctx.clone());
                let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
                let mut source = FtpSource::smartphone();
                source.host = "fixture.invalid".into();
                source.username = "fixture".into();
                source.password = "fixture-only".into();
                app.saved.active_source = Some(source.id.clone());
                app.saved.sources = vec![source.clone()];
                app.saved.tracks = vec![Track::placeholder(format!(
                    "{}Album/track.flac",
                    source.url().unwrap()
                ))];
                if !mini
                    && !ready
                    && let Some(path) = std::env::var_os("MUSICCRAFT_CONNECTION_UI_FIXTURE")
                {
                    app.saved.auto_sync = false;
                    app.saved.sources[0].host = "127.0.0.1".into();
                    app.saved.sources[0].port = "9".into();
                    app.saved.sources[0].username.clear();
                    app.saved.sources[0].password.clear();
                    app.saved.sources[0].rediscover = Some(false);
                    app.saved.tracks[0].source =
                        format!("{}Album/track.flac", app.saved.sources[0].url().unwrap());
                    let outer =
                        HashMap::from([("aurea", ron::ser::to_string(&app.saved).unwrap())]);
                    std::fs::create_dir_all(&path).unwrap();
                    std::fs::write(
                        PathBuf::from(path).join("state.ron"),
                        ron::ser::to_string(&outer).unwrap(),
                    )
                    .unwrap();
                    // The native fixture is independent of the assertions below.
                    app.saved.sources[0] = source.clone();
                }
                if ready {
                    app.connections.verified(&source);
                }
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            pos2(0., 0.),
                            vec2(1000., 700.),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        if mini {
                            app.mini_ui(ui);
                        } else {
                            app.desktop_ui(ui);
                        }
                    },
                );
                output.textures_delta.clear();
                let update = output.platform_output.accesskit_update.unwrap();
                for label in [tr!("Play"), tr!("Next"), tr!("Previous")] {
                    let (_, node) = update
                        .nodes
                        .iter()
                        .find(|(_, n)| n.label() == Some(label))
                        .unwrap_or_else(|| panic!("missing {label}, mini={mini}"));
                    assert_eq!(node.is_disabled(), !ready, "{label} mini={mini}");
                }
                assert!(!app.loading);
            }
        }
    }
    #[test]
    fn failed_checks_back_off_and_a_success_cancels_obsolete_checks() {
        let mut gate = Gate::default();
        let mut source = FtpSource::smartphone();
        source.host = "fixture.invalid".into();
        gate.failed(&source);
        assert!(!gate.ready(&source));
        assert!(gate.retry[&key(&source)].1 > Instant::now());
        let cancel = Arc::new(AtomicBool::new(false));
        gate.request = Some(Request {
            token: 1,
            source: source.clone(),
            cancel: cancel.clone(),
        });
        gate.verified(&source);
        assert!(cancel.load(Ordering::Relaxed));
        assert!(gate.request.is_none());
        assert!(gate.ready(&source));
        assert!(gate.retry.is_empty());
        source.username = "changed".into();
        assert!(!gate.ready(&source));
        gate.clear();
        assert!(gate.verified.is_empty());
    }
    #[test]
    fn stale_checks_do_not_unlock_playback_and_retry_does_not_transfer_audio() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
        app.saved.auto_sync = false;
        let mut source = FtpSource::smartphone();
        source.host = "old.local".into();
        source.username = "fixture".into();
        source.password = "fixture-only".into();
        let root = source.url().unwrap();
        let id = format!("{root}Album/track.flac");
        app.saved.sources = vec![source.clone()];
        app.saved.active_source = Some(source.id.clone());
        app.saved.tracks = vec![Track::placeholder(id.clone())];
        assert!(!app.navigation_ready());
        let ticket = app.ticket;
        app.play(id.clone());
        assert!(!app.loading);
        assert_eq!(app.ticket, ticket);
        app.connections.request = Some(Request {
            token: 7,
            source: source.clone(),
            cancel: Arc::new(AtomicBool::new(false)),
        });
        app.connection_result(8, true);
        assert!(!app.navigation_ready());
        app.saved.sources[0].host = "new.local".into();
        app.connection_result(7, true);
        assert!(!app.navigation_ready());
        let found = app.saved.sources[0].clone();
        app.apply_discovered(found.clone(), &root);
        let new_id = app.saved.tracks[0].source.clone();
        assert!(app.navigation_ready());
        assert!(app.track_connection_ready(&new_id));
        assert_eq!(app.canonical_source(&id), new_id);
        app.connections.clear();
        assert!(!app.track_connection_ready(&new_id));
        // Local audio and already loaded PCM can always be paused/resumed.
        assert!(app.track_connection_ready("/nas/track.flac"));
        app.current = Some(new_id);
        assert!(app.can_toggle());
    }
}
