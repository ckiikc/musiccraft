use super::*;
use musiccraft_sync::{
    Direction, Plan, Side,
    filesystem::{self},
};

#[derive(Default, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub(super) struct Config {
    pub(super) a: String,
    pub(super) b: String,
    direction: Direction,
}
enum Update {
    Message(String),
    Preview(Result<Plan, String>),
    Progress(usize, usize),
    Finished(Result<(), String>),
}
pub(super) struct State {
    pub(super) config: Config,
    path: PathBuf,
    preview: Option<Plan>,
    rx: Option<mpsc::Receiver<Update>>,
    cancel: Arc<AtomicBool>,
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selectors_only_offer_library_sources_and_keep_queue_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx);
        let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
        app.saved.auto_sync = false;
        app.saved.folders = vec!["/synthetic/library/Music".into()];
        app.saved.sources = vec![FtpSource {
            name: "Read only FTP".into(),
            id: "ftp-fixture".into(),
            ..Default::default()
        }];
        app.playback_selection = Some(vec!["unchanged-track".into()]);
        let choices = app.file_sync_sources();
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0].label, "Music");
        assert!(choices[0].endpoint.is_some());
        assert!(choices[1].endpoint.is_none());
        app.saved.folders.clear();
        assert!(
            !app.file_sync_sources()
                .iter()
                .any(|c| c.key == choices[0].key)
        );
        assert_eq!(app.playback_selection, Some(vec!["unchanged-track".into()]));
    }
}
impl State {
    pub fn busy(&self) -> bool {
        self.rx.is_some()
    }
    pub fn new(path: PathBuf) -> Self {
        let config = std::fs::read(path.join("config.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Self {
            config,
            path,
            preview: None,
            rx: None,
            cancel: Arc::new(AtomicBool::new(false)),
            message: String::new(),
        }
    }
    pub fn poll(&mut self) -> bool {
        let mut finished = false;
        let messages = self
            .rx
            .as_ref()
            .map(|r| r.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        for message in messages {
            match message {
                Update::Message(message) => self.message = message,
                Update::Preview(result) => {
                    self.rx = None;
                    match result {
                        Ok(plan) => {
                            self.message = tr!("Review every change before synchronizing.").into();
                            self.preview = Some(plan);
                        }
                        Err(e) => self.message = e,
                    }
                }
                Update::Progress(n, total) => self.message = format!("{n} / {total}"),
                Update::Finished(result) => {
                    finished = true;
                    self.rx = None;
                    self.preview = None;
                    self.message = match result {
                        Ok(()) => tr!("Synchronization completed and verified.").into(),
                        Err(e) => e,
                    };
                }
            }
        }
        finished
    }
}
impl Aurea {
    pub(super) fn after_file_sync(&mut self) {
        let keys = [&self.file_sync.config.a, &self.file_sync.config.b];
        let choices = self.file_sync_sources();
        let scopes: Vec<_> = choices
            .into_iter()
            .filter(|c| keys.contains(&&c.key))
            .filter_map(|c| match c.endpoint {
                Some(Choice::Folder(p)) => Some(library::Scope::Local(p)),
                Some(Choice::Phone(root)) => Some(library::Scope::Remote(root)),
                None => None,
            })
            .collect();
        let mut reprepare = false;
        for track in &mut self.saved.tracks {
            if scopes.iter().any(|s| s.contains(&track.source)) {
                crate::cache::invalidate_remote(&track.source, &self.cache);
                track.rating_tags = None;
                if self.planned.as_ref() == Some(&track.source) {
                    reprepare = true;
                }
            }
        }
        self.library_revision = self.library_revision.wrapping_add(1);
        self.remote_tags.library_changed();
        if reprepare {
            self.prepare_track(self.planned.clone());
        }
        self.file_sync_refresh
            .extend(self.sync_targets().into_iter().filter(|t| {
                match t {
                    Target::Folder(p) => scopes
                        .iter()
                        .any(|s| matches!(s,library::Scope::Local(root) if root==p)),
                    Target::Ftp { root, .. } => scopes
                        .iter()
                        .any(|s| matches!(s,library::Scope::Remote(r) if r==root)),
                }
            }));
    }
    pub(super) fn file_sync_page(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("file-sync-page")
            .show(ui, |ui| self.file_sync_contents(ui));
    }
    fn file_sync_contents(&mut self, ui: &mut egui::Ui) {
        let busy = self.file_sync.rx.is_some();
        let before = self.file_sync.config.clone();
        ui.heading(tr!("Synchronize library sources"));
        ui.label(tr!("Preview copies, updates and deletions between a NAS folder and your phone. Playback queues are independent."));
        ui.add_space(12.0);
        let choices = self.file_sync_sources();
        if self.file_sync.config.a.is_empty() {
            self.file_sync.config.a = choices
                .iter()
                .find(|c| matches!(c.endpoint, Some(Choice::Folder(_))))
                .map(|c| c.key.clone())
                .unwrap_or_default();
        }
        if self.file_sync.config.b.is_empty() {
            self.file_sync.config.b = choices
                .iter()
                .find(|c| c.key != self.file_sync.config.a && c.endpoint.is_some())
                .map(|c| c.key.clone())
                .unwrap_or_default();
        }
        ui.add_enabled_ui(!busy,|ui| {
            egui::Grid::new("sync-source-selectors").num_columns(2).spacing([16.0,10.0]).show(ui,|ui|{
                for (label,key,id) in [(tr!("Origin · A"),&mut self.file_sync.config.a,"sync-origin"),(tr!("Destination · B"),&mut self.file_sync.config.b,"sync-destination")] {
                    ui.label(label);
                    let title=choices.iter().find(|c|&c.key==key).map(|c|c.label.as_str()).unwrap_or(tr!("Select a library source"));
                    egui::ComboBox::from_id_salt(id).selected_text(title).width(220.0).show_ui(ui,|ui|{
                        for choice in &choices {
                            if choice.endpoint.is_some(){ui.selectable_value(key,choice.key.clone(),&choice.label);}
                            else {ui.add_enabled(false,egui::Button::new(&choice.label)).on_hover_text(tr!("Use a mounted NAS folder or MusicCraft Android for file synchronization."));}
                        }
                    });
                    ui.end_row();
                }
                ui.label(tr!("Direction"));
                egui::ComboBox::from_id_salt("sync-direction").selected_text(direction_label(self.file_sync.config.direction)).show_ui(ui,|ui|{for d in [Direction::Forward,Direction::Reverse,Direction::Both]{ui.selectable_value(&mut self.file_sync.config.direction,d,direction_label(d));}});ui.end_row();
            });
            ui.add_space(6.0);
            if ui.small_button(tr!("Manage library sources…")).clicked(){self.sources_dialog=true;}
            ui.label(tr!("One-way mode mirrors the origin, including deletions. Bidirectional mode uses the last completed synchronization and asks about conflicting edits."));
        });
        if before != self.file_sync.config {
            self.file_sync.preview = None;
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !busy
                        && !self.scanning
                        && self.file_sync.config.a != self.file_sync.config.b
                        && [&self.file_sync.config.a, &self.file_sync.config.b]
                            .iter()
                            .all(|key| {
                                choices
                                    .iter()
                                    .any(|c| &c.key == *key && c.endpoint.is_some())
                            }),
                    egui::Button::new(tr!("Generate preview")),
                )
                .clicked()
            {
                self.start_file_sync(false);
            }
            if ui
                .add_enabled(
                    !busy
                        && self
                            .file_sync
                            .preview
                            .as_ref()
                            .is_some_and(|p| p.ready() && !p.operations.is_empty()),
                    egui::Button::new(tr!("Apply preview")),
                )
                .clicked()
            {
                self.start_file_sync(true);
            }
            if busy && ui.button(tr!("Cancel")).clicked() {
                self.file_sync.cancel.store(true, Ordering::Relaxed);
            }
        });
        if busy {
            ui.spinner();
            ui.ctx().request_repaint_after(Duration::from_millis(150));
        }
        if !self.file_sync.message.is_empty() {
            ui.label(&self.file_sync.message);
        }
        if let Some(plan) = self.file_sync.preview.as_mut() {
            let deletes = plan
                .operations
                .iter()
                .filter(|o| matches!(o.action, musiccraft_sync::Action::Delete(_)))
                .count();
            ui.label(format!(
                "{} · {}: {deletes}",
                plan.operations.len(),
                tr!("Deletions")
            ));
            ui.label(tr!("Replaced and deleted files are retained in .musiccraft-sync/recovery. Hidden files are excluded."));
            egui::ScrollArea::vertical()
                .id_salt("sync-preview")
                .max_height(320.0)
                .show_rows(ui, 42.0, plan.operations.len(), |ui, range| {
                    for index in range {
                        let op = &plan.operations[index];
                        let path = op.path.clone();
                        let action = op.action.clone();
                        ui.horizontal(|ui| {
                            ui.label(match action {
                                musiccraft_sync::Action::Copy(Side::A) => "B → A",
                                musiccraft_sync::Action::Copy(Side::B) => "A → B",
                                musiccraft_sync::Action::Delete(Side::A) => "− A",
                                musiccraft_sync::Action::Delete(Side::B) => "− B",
                                _ => "⚠",
                            });
                            ui.label(path);
                            if action == musiccraft_sync::Action::Conflict {
                                for (side, label) in
                                    [(Side::A, tr!("Keep A")), (Side::B, tr!("Keep B"))]
                                {
                                    if ui.button(label).clicked() {
                                        let _ = plan.resolve(index, side);
                                    }
                                }
                            }
                        });
                    }
                });
        }
    }
    fn file_sync_sources(&self) -> Vec<SourceChoice> {
        let mut result: Vec<_> = self
            .saved
            .folders
            .iter()
            .map(|path| SourceChoice {
                key: Target::Folder(path.clone()).key(),
                label: path
                    .file_name()
                    .unwrap_or(path.as_os_str())
                    .to_string_lossy()
                    .into_owned(),
                endpoint: Some(Choice::Folder(path.clone())),
            })
            .collect();
        result.extend(self.saved.sources.iter().map(|source| {
            SourceChoice {
                key: format!("source:{}", source.id),
                label: source.name.clone(),
                endpoint: source
                    .tls_pin
                    .as_ref()
                    .and_then(|_| source.url().ok())
                    .map(Choice::Phone),
            }
        }));
        result
    }
    fn start_file_sync(&mut self, apply: bool) {
        if self.file_sync.rx.is_some() || (apply && self.scanning) {
            return;
        }
        let config = self.file_sync.config.clone();
        let path = self.file_sync.path.clone();
        let auth = self.auth.clone();
        let choices = self.file_sync_sources();
        let endpoints = [&config.a, &config.b].map(|key| {
            choices
                .iter()
                .find(|c| &c.key == key)
                .and_then(|c| c.endpoint.clone())
        });
        let preview = self.file_sync.preview.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        self.file_sync.cancel = cancel.clone();
        let (tx, rx) = mpsc::channel();
        self.file_sync.rx = Some(rx);
        self.file_sync.message = tr!("Reading complete folder fingerprints…").into();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let run = (|| -> Result<Option<Plan>, String> {
                std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
                filesystem::save(&path.join("config.json"), &config)?;
                let [a, b] = endpoints;
                let a = a.ok_or("Origin source is no longer in the library")?;
                let b = b.ok_or("Destination source is no longer in the library")?;
                if config.a == config.b {
                    return Err("Choose different library sources".into());
                }
                let mut a = a.open(&b, &auth, &path)?;
                let mut b = b.open_choice(&auth, &path)?;
                use sha2::{Digest, Sha256};
                let key = format!(
                    "{:x}",
                    Sha256::digest({
                        let mut keys = [&config.a, &config.b];
                        keys.sort();
                        serde_json::to_vec(&keys).map_err(|e| e.to_string())?
                    })
                );
                let history = path.join(format!("{key}.json"));
                if apply {
                    let plan = preview.ok_or("Generate a preview first")?;
                    let message = if matches!(&a, crate::file_sync::Source::Phone(_))
                        || matches!(&b, crate::file_sync::Source::Phone(_))
                    {
                        tr!("Authorize this preview on your phone…")
                    } else {
                        tr!("Validating the reviewed preview…")
                    };
                    let _ = tx.send(Update::Message(message.into()));
                    ctx.request_repaint();
                    a.authorize(&plan, true, &cancel)?;
                    b.authorize(&plan, false, &cancel)?;
                    let after = filesystem::execute(
                        &plan,
                        a.endpoint(),
                        b.endpoint(),
                        &path.join("last-transaction.json"),
                        &cancel,
                        |n, total| {
                            let _ = tx.send(Update::Progress(n, total));
                            ctx.request_repaint();
                        },
                    )?;
                    filesystem::save(&history, &after)?;
                    a.finish(&cancel)?;
                    b.finish(&cancel)?;
                    Ok(None)
                } else {
                    let a = a.endpoint().snapshot(&cancel)?;
                    let b = b.endpoint().snapshot(&cancel)?;
                    let mut baseline = crate::file_sync::baseline(&history)?;
                    if let Some(old) = baseline.as_mut()
                        && old.a.root == b.root
                        && old.b.root == a.root
                    {
                        std::mem::swap(&mut old.a, &mut old.b);
                    }
                    Ok(Some(musiccraft_sync::plan(musiccraft_sync::Request {
                        a,
                        b,
                        direction: config.direction,
                        baseline,
                    })?))
                }
            })();
            let _ = tx.send(if apply {
                Update::Finished(run.map(|_| ()))
            } else {
                Update::Preview(run.and_then(|p| p.ok_or("Missing preview".into())))
            });
            ctx.request_repaint();
        });
    }
}
fn direction_label(direction: Direction) -> &'static str {
    match direction {
        Direction::Forward => tr!("A → B · One way"),
        Direction::Reverse => tr!("B → A · One way"),
        Direction::Both => tr!("A ↔ B · Bidirectional"),
    }
}

struct SourceChoice {
    key: String,
    label: String,
    endpoint: Option<Choice>,
}
#[derive(Clone)]
enum Choice {
    Folder(PathBuf),
    Phone(String),
}
impl Choice {
    fn open(
        &self,
        other: &Self,
        auth: &AuthStore,
        path: &std::path::Path,
    ) -> Result<crate::file_sync::Source, String> {
        if let (Self::Folder(a), Self::Folder(b)) = (self, other) {
            crate::file_sync::open_folder(a, Some(b))?;
        }
        if matches!((self, other), (Self::Phone(_), Self::Phone(_))) {
            return Err("Choose a NAS folder and one phone".into());
        }
        self.open_choice(auth, path)
    }
    fn open_choice(
        &self,
        auth: &AuthStore,
        path: &std::path::Path,
    ) -> Result<crate::file_sync::Source, String> {
        match self {
            Self::Folder(p) => Ok(crate::file_sync::Source::Folder(
                crate::file_sync::open_folder(p, None)?,
            )),
            Self::Phone(root) => Ok(crate::file_sync::Source::Phone(
                crate::file_sync::Phone::new(root.clone(), auth.clone(), path.join("transfers"))?,
            )),
        }
    }
}
