use super::*;
use std::sync::mpsc;
#[derive(Default)]
pub(super) struct State {
    devices: Vec<crate::phone::Nearby>,
    selected: usize,
    code: String,
    error: String,
    searched: bool,
    next_search: Option<Instant>,
    searching: bool,
    pairing: bool,
    waiting: bool,
    rx: Option<mpsc::Receiver<Event>>,
    cancel: Arc<AtomicBool>,
    address: Option<AddressEdit>,
}
struct AddressEdit {
    source_id: String,
    value: String,
}
impl Drop for State {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed)
    }
}
enum Event {
    Found(Vec<crate::phone::Nearby>),
    Waiting,
    Paired(Box<Result<FtpSource, String>>),
    AddressChecked {
        original: Box<FtpSource>,
        candidate: Box<FtpSource>,
        verified: bool,
    },
}
impl Aurea {
    pub(super) fn open_phone_address(&mut self, index: usize) {
        let source = &self.saved.sources[index];
        let address = AddressEdit {
            source_id: source.id.clone(),
            value: format!("{}:{}", source.host, source.port),
        };
        self.open_phone_pair();
        self.phone_pair.address = Some(address);
    }

    fn check_phone_address(&mut self) {
        let Some(edit) = &self.phone_pair.address else {
            return;
        };
        let Some(original) = self.saved.sources.iter().find(|s| s.id == edit.source_id) else {
            self.phone_pair.error = tr!("The paired source changed. Reopen its settings.").into();
            return;
        };
        if original.password.is_empty() {
            self.phone_pair.error = tr!("Authorize Keychain to access the saved pairing.").into();
            return;
        }
        let Some(candidate) = crate::phone::address_candidate(original, &edit.value) else {
            self.phone_pair.error =
                tr!("Enter a private IPv4 address, optionally followed by a port.").into();
            return;
        };
        let original = original.clone();
        let (tx, rx) = mpsc::channel();
        self.phone_pair.rx = Some(rx);
        self.phone_pair.pairing = true;
        self.phone_pair.error.clear();
        let cancel = self.phone_pair.cancel.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let auth = AuthStore::default();
            let verified = candidate.url().is_ok_and(|root| {
                auth.insert(&candidate).is_ok()
                    && crate::phone::online(&root, &auth, &cancel, Duration::from_secs(5)).is_ok()
            });
            let _ = tx.send(Event::AddressChecked {
                original: Box::new(original),
                candidate: Box::new(candidate),
                verified,
            });
            ctx.request_repaint();
        });
    }

    fn apply_phone_address(&mut self, original: &FtpSource, candidate: FtpSource) -> bool {
        let Ok(old) = original.url() else {
            return false;
        };
        if !self.saved.sources.iter().any(|source| {
            source.id == original.id
                && source.url().as_deref() == Ok(old.as_str())
                && source.tls_pin == original.tls_pin
                && source.username == original.username
                && source.password == original.password
        }) {
            return false;
        }
        self.cancel_discovery();
        let root = candidate.url().ok();
        if root.as_deref() == Some(old.as_str()) {
            self.connections.mark_verified(&candidate);
        } else {
            self.apply_discovered(candidate, &old);
        }
        if let Some(root) = root
            && !self.scanning
        {
            self.sync_ftp(root);
        }
        true
    }

    pub(crate) fn open_phone_pair(&mut self) {
        self.cancel_discovery();
        self.ftp_dialog = false;
        self.phone_pair_dialog = true;
        self.phone_pair = State::default();
        self.message.clear();
    }
    fn accept_phone_pair(&mut self, source: FtpSource) {
        self.ftp_edit = self.saved.sources.iter().position(|s| s.id == source.id);
        self.ftp_old = self.ftp_edit.and_then(|i| self.saved.sources[i].url().ok());
        self.ftp = source;
        self.remember_password = true;
        self.phone_pair_dialog = false;
        self.phone_pair = State::default();
        self.connect();
    }
    // A private file argument remains available for repeatable QA; ordinary users use discovery and a PIN.
    pub(crate) fn pair_phone(&mut self, link: &str) {
        match crate::phone::pairing(link) {
            Ok(source) => self.accept_phone_pair(source),
            Err(error) => self.phone_pair.error = error,
        }
    }
    fn search_phones(&mut self) {
        let (tx, rx) = mpsc::channel();
        self.phone_pair.rx = Some(rx);
        self.phone_pair.searching = true;
        self.phone_pair.searched = true;
        let ctx = self.ctx.clone();
        let cancel = self.phone_pair.cancel.clone();
        std::thread::spawn(move || {
            let devices = crate::phone::nearby(&cancel);
            let _ = tx.send(Event::Found(devices));
            ctx.request_repaint();
        });
    }
    pub(super) fn phone_pair_window(&mut self, ctx: &egui::Context) {
        if !self.phone_pair_dialog {
            self.auxiliary_geometry.close("phone-pairing");
            self.auxiliary_geometry.close("phone-address");
            return;
        }
        if self.phone_pair.address.is_none() && !self.phone_pair.searched {
            self.search_phones()
        }
        let events: Vec<_> = self
            .phone_pair
            .rx
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        for event in events {
            match event {
                Event::Found(devices) => {
                    self.phone_pair.devices = devices;
                    self.phone_pair.selected = 0;
                    self.phone_pair.searching = false;
                    self.phone_pair.next_search = Some(Instant::now() + Duration::from_secs(3));
                }
                Event::Waiting => self.phone_pair.waiting = true,
                Event::Paired(result) => {
                    self.phone_pair.pairing = false;
                    self.phone_pair.waiting = false;
                    match *result {
                        Ok(source) => {
                            self.accept_phone_pair(source);
                            return;
                        }
                        Err(error) => self.phone_pair.error = error,
                    }
                }
                Event::AddressChecked {
                    original,
                    candidate,
                    verified,
                } => {
                    self.phone_pair.pairing = false;
                    if !verified {
                        self.phone_pair.error = tr!("Could not verify the paired phone at this address. Check its IP, port and sharing status.").into();
                    } else if self.apply_phone_address(&original, *candidate) {
                        self.phone_pair_dialog = false;
                        self.phone_pair = State::default();
                        return;
                    } else {
                        self.phone_pair.error =
                            tr!("The paired source changed. Reopen its settings.").into();
                    }
                }
            }
        }
        if self.phone_pair.address.is_none()
            && !self.phone_pair.searching
            && !self.phone_pair.pairing
            && self.phone_pair.devices.is_empty()
            && self
                .phone_pair
                .next_search
                .is_some_and(|due| Instant::now() >= due)
        {
            self.search_phones();
        }
        if self.phone_pair.address.is_some() {
            self.phone_address_window(ctx);
            return;
        }
        let open = self.detached_window(
            ctx,
            "phone-pairing",
            tr!("Pair with MusicCraft Android"),
            vec2(520.0, 420.0),
            |app, ui| {
                ui.label(tr!(
                    "On the phone, enable sharing and choose Pair computer."
                ));
                ui.add_space(8.0);
                if app.phone_pair.searching {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(tr!("Searching for phones…"));
                    });
                } else if app.phone_pair.devices.is_empty() {
                    ui.label(tr!(
                        "No phone found. Check that both devices use the same Wi-Fi."
                    ));
                } else {
                    for (i, device) in app.phone_pair.devices.iter().enumerate() {
                        ui.selectable_value(&mut app.phone_pair.selected, i, &device.name);
                    }
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(tr!("Four-digit code"));
                    ui.add_enabled(
                        !app.phone_pair.pairing,
                        egui::TextEdit::singleline(&mut app.phone_pair.code)
                            .char_limit(4)
                            .desired_width(72.0)
                            .font(egui::TextStyle::Monospace),
                    );
                });
                app.phone_pair.code.retain(|c| c.is_ascii_digit());
                if app.phone_pair.waiting {
                    ui.label(tr!("Confirm the pairing request on your phone."));
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let ready = !app.phone_pair.pairing
                        && !app.phone_pair.searching
                        && app.phone_pair.code.len() == 4
                        && app.phone_pair.selected < app.phone_pair.devices.len();
                    if ui
                        .add_enabled(ready, egui::Button::new(tr!("Pair")))
                        .clicked()
                    {
                        let peer = app.phone_pair.devices[app.phone_pair.selected].clone();
                        let code = std::mem::take(&mut app.phone_pair.code);
                        let cancel = app.phone_pair.cancel.clone();
                        let ctx = ctx.clone();
                        let (tx, rx) = mpsc::channel();
                        app.phone_pair.rx = Some(rx);
                        app.phone_pair.pairing = true;
                        app.phone_pair.error.clear();
                        std::thread::spawn(move || {
                            let result = crate::phone::pair(&peer, &code, &cancel, || {
                                let _ = tx.send(Event::Waiting);
                                ctx.request_repaint();
                            });
                            let _ = tx.send(Event::Paired(Box::new(result)));
                            ctx.request_repaint();
                        });
                    }
                    if ui
                        .add_enabled(
                            !app.phone_pair.pairing && !app.phone_pair.searching,
                            egui::Button::new(tr!("Retry")),
                        )
                        .clicked()
                    {
                        app.search_phones();
                    }
                    if app.phone_pair.pairing {
                        ui.spinner();
                    }
                });
                ui.label(&app.phone_pair.error);
            },
        );
        self.phone_pair_dialog &= open;
        if !self.phone_pair_dialog {
            self.phone_pair = State::default()
        } else if self.phone_pair.pairing || self.phone_pair.searching {
            ctx.request_repaint_after(Duration::from_millis(100));
        } else if self.phone_pair.devices.is_empty() {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
    }
    fn phone_address_window(&mut self, ctx: &egui::Context) {
        let open = self.detached_window(
            ctx,
            "phone-address",
            tr!("Update paired phone address"),
            vec2(520.0, 220.0),
            |app, ui| {
                let Some(edit) = &mut app.phone_pair.address else {
                    return;
                };
                ui.label(tr!(
                    "Enter the phone's current Wi-Fi address to reuse its existing pairing."
                ));
                ui.add_space(8.0);
                ui.add_enabled(
                    !app.phone_pair.pairing,
                    egui::TextEdit::singleline(&mut edit.value)
                        .hint_text(tr!("Host / IP"))
                        .char_limit(32),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !app.phone_pair.pairing,
                            egui::Button::new(tr!("Check and save")),
                        )
                        .clicked()
                    {
                        app.check_phone_address();
                    }
                    if ui
                        .add_enabled(
                            !app.phone_pair.pairing,
                            egui::Button::new(tr!("Pair again")),
                        )
                        .clicked()
                    {
                        app.open_phone_pair();
                    }
                    if app.phone_pair.pairing {
                        ui.spinner();
                    }
                });
                ui.label(&app.phone_pair.error);
            },
        );
        self.phone_pair_dialog &= open;
        if !self.phone_pair_dialog {
            self.phone_pair = State::default();
        } else if self.phone_pair.pairing {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_editor_preserves_the_paired_source_and_queue() {
        let profile = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx);
        let mut app = Aurea::new(&cc, profile.path().into(), vec![], None, None);
        let original = FtpSource {
            scheme: "https".into(),
            id: "phone:00000000-0000-4000-8000-000000000001".into(),
            name: "Fixture Android".into(),
            host: "192.168.42.12".into(),
            port: "8737".into(),
            root: "/v1/audio/".into(),
            username: "musiccraft".into(),
            password: "synthetic-credential".into(),
            tls_pin: Some(format!("{}=", "A".repeat(43))),
            ..Default::default()
        };
        let new = crate::phone::address_candidate(&original, " 192.168.42.13:8738 ").unwrap();
        for invalid in [
            "192.168.42.13:0",
            "127.0.0.1",
            "8.8.8.8",
            "https://192.168.42.13",
            "192.168.42.13/path",
        ] {
            assert!(
                crate::phone::address_candidate(&original, invalid).is_none(),
                "{invalid}"
            );
        }
        assert_eq!(
            crate::phone::address_candidate(&original, "192.168.42.13")
                .unwrap()
                .port,
            original.port
        );
        app.saved.sources = vec![original.clone()];
        let track = original.url().unwrap() + "1.flac";
        let new_track = new.url().unwrap() + "1.flac";
        app.saved.tracks = vec![Track::placeholder(track.clone())];
        app.playback_selection = Some(vec![track]);
        app.open_ftp_editor(Some(0));
        assert_eq!(
            app.phone_pair.address.as_ref().unwrap().value,
            "192.168.42.12:8737"
        );
        assert!(!app.ftp_dialog);
        let mut replaced = original.clone();
        replaced.tls_pin = Some(format!("{}=", "B".repeat(43)));
        app.saved.sources[0] = replaced;
        assert!(!app.apply_phone_address(&original, new.clone()));
        assert_eq!(app.saved.sources[0].host, original.host);
        app.saved.sources[0] = original.clone();
        app.scanning = true;
        assert!(app.apply_phone_address(&original, new));
        let saved = &app.saved.sources[0];
        assert_eq!(
            (&saved.id, &saved.tls_pin, &saved.password),
            (&original.id, &original.tls_pin, &original.password)
        );
        assert_eq!(app.saved.tracks[0].source, new_track);
        assert_eq!(app.playback_selection, Some(vec![new_track]));
    }
}
