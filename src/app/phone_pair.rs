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
}
impl Aurea {
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
            return;
        }
        if !self.phone_pair.searched {
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
            }
        }
        if !self.phone_pair.searching
            && !self.phone_pair.pairing
            && self.phone_pair.devices.is_empty()
            && self
                .phone_pair
                .next_search
                .is_some_and(|due| Instant::now() >= due)
        {
            self.search_phones();
        }
        let mut open = true;
        egui::Window::new(tr!("Pair with MusicCraft Android"))
            .id(egui::Id::new("phone-pairing"))
            .open(&mut open)
            .default_width(450.0)
            .show(ctx, |ui| {
                ui.label(tr!(
                    "On the phone, enable sharing and choose Pair computer."
                ));
                ui.add_space(8.0);
                if self.phone_pair.searching {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(tr!("Searching for phones…"));
                    });
                } else if self.phone_pair.devices.is_empty() {
                    ui.label(tr!(
                        "No phone found. Check that both devices use the same Wi-Fi."
                    ));
                } else {
                    for (i, device) in self.phone_pair.devices.iter().enumerate() {
                        ui.selectable_value(&mut self.phone_pair.selected, i, &device.name);
                    }
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(tr!("Four-digit code"));
                    ui.add_enabled(
                        !self.phone_pair.pairing,
                        egui::TextEdit::singleline(&mut self.phone_pair.code)
                            .char_limit(4)
                            .desired_width(72.0)
                            .font(egui::TextStyle::Monospace),
                    );
                });
                self.phone_pair.code.retain(|c| c.is_ascii_digit());
                if self.phone_pair.waiting {
                    ui.label(tr!("Confirm the pairing request on your phone."));
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let ready = !self.phone_pair.pairing
                        && !self.phone_pair.searching
                        && self.phone_pair.code.len() == 4
                        && self.phone_pair.selected < self.phone_pair.devices.len();
                    if ui
                        .add_enabled(ready, egui::Button::new(tr!("Pair")))
                        .clicked()
                    {
                        let peer = self.phone_pair.devices[self.phone_pair.selected].clone();
                        let code = std::mem::take(&mut self.phone_pair.code);
                        let cancel = self.phone_pair.cancel.clone();
                        let ctx = ctx.clone();
                        let (tx, rx) = mpsc::channel();
                        self.phone_pair.rx = Some(rx);
                        self.phone_pair.pairing = true;
                        self.phone_pair.error.clear();
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
                            !self.phone_pair.pairing && !self.phone_pair.searching,
                            egui::Button::new(tr!("Retry")),
                        )
                        .clicked()
                    {
                        self.search_phones();
                    }
                    if self.phone_pair.pairing {
                        ui.spinner();
                    }
                });
                ui.label(&self.phone_pair.error);
            });
        self.phone_pair_dialog &= open;
        if !self.phone_pair_dialog {
            self.phone_pair = State::default()
        } else if self.phone_pair.pairing || self.phone_pair.searching {
            ctx.request_repaint_after(Duration::from_millis(100));
        } else if self.phone_pair.devices.is_empty() {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
    }
}
