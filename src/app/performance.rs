//! Repeatable CPU-only UI profiles with synthetic metadata and no audio output.
use super::*;

#[derive(Default)]
struct Storage(HashMap<String, String>);
impl eframe::Storage for Storage {
    fn get_string(&self, k: &str) -> Option<String> {
        self.0.get(k).cloned()
    }
    fn set_string(&mut self, k: &str, v: String) {
        self.0.insert(k.into(), v);
    }
    fn flush(&mut self) {}
    fn remove_string(&mut self, k: &str) {
        self.0.remove(k);
    }
}
#[test]
fn duplicate_ftp_migration_preserves_tags_source_isolation_and_shuffle_cycle() {
    let dir = tempfile::tempdir().unwrap();
    let legacy = "ftp://phone/Album%20%28FLAC%20%2B%20CUE%29/01.flac";
    let current = "ftp://phone/Album%20(FLAC%20+%20CUE)/01.flac";
    let other = "/nas/01.flac";
    let mut rich = Track::placeholder(current.into());
    rich.artist = "Fixture artist".into();
    rich.metadata_loaded = true;
    let mut cycle = crate::shuffle::Cycle::default();
    cycle.record(current);
    let saved = Saved {
        tracks: vec![
            Track::placeholder(legacy.into()),
            rich,
            Track::placeholder(other.into()),
        ],
        auto_sync: false,
        online_covers: false,
        save_covers: false,
        shuffle_session: Some(ShuffleSession {
            cycle,
            selection: Some(vec![current.into(), legacy.into(), other.into()]),
        }),
        ..Default::default()
    };
    let mut storage = Storage::default();
    eframe::set_value(&mut storage, "aurea", &saved);
    let ctx = egui::Context::default();
    let mut cc = eframe::CreationContext::_new_kittest(ctx);
    cc.storage = Some(&storage);
    if let Some(profile) = std::env::var_os("MUSICCRAFT_DEDUP_FIXTURE") {
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(
            PathBuf::from(profile).join("state.ron"),
            serde_json::to_vec(&storage.0).unwrap(),
        )
        .unwrap();
    }
    let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
    assert_eq!(app.saved.tracks.len(), 2);
    assert_eq!(app.saved.tracks[0].source, legacy);
    assert_eq!(app.saved.tracks[0].artist, "Fixture artist");
    assert_eq!(
        app.playback_selection.as_ref().unwrap(),
        &vec![legacy.to_string(), other.to_string()]
    );
    for _ in 0..10 {
        assert_eq!(
            app.shuffle_cycle
                .choose(app.saved.tracks.iter().map(|t| t.source.as_str()), None)
                .as_deref(),
            Some(other)
        );
    }
}
#[test]
fn shuffle_cycle_and_filtered_selection_survive_save_reopen_and_first_play() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = egui::Context::default();
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
    app.saved.auto_sync = false;
    app.online_covers.store(false, Ordering::Relaxed);
    app.saved.tracks = ["/synthetic/A.wav", "/synthetic/B.wav", "/synthetic/C.wav"]
        .into_iter()
        .map(|s| Track::placeholder(s.into()))
        .collect();
    app.saved.shuffle = true;
    app.playback_selection = Some(vec!["/synthetic/A.wav".into(), "/synthetic/C.wav".into()]);
    app.shuffle_cycle.record("/synthetic/A.wav");
    app.remember_full_size(vec2(1380.0, 840.0));
    app.remember_mini_size(vec2(812.0, 640.0));
    let mut storage = Storage::default();
    for _ in 0..3 {
        eframe::App::save(&mut app, &mut storage);
        assert_eq!(app.choose_next().as_deref(), Some("/synthetic/C.wav"));
        if let Some(profile) = std::env::var_os("MUSICCRAFT_GEOMETRY_FIXTURE") {
            let path = PathBuf::from(profile).join("state.ron");
            std::fs::write(path, serde_json::to_vec(&storage.0).unwrap()).unwrap();
        }
        let file = dir.path().join("restart-store.json");
        serde_json::to_writer(std::fs::File::create(&file).unwrap(), &storage.0).unwrap();
        storage = Storage(serde_json::from_reader(std::fs::File::open(file).unwrap()).unwrap());
        let mut cc = eframe::CreationContext::_new_kittest(ctx.clone());
        cc.storage = Some(&storage);
        app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
        assert!(app.current.is_none() && !app.loading);
        assert_eq!(app.listening_tracks().len(), 2);
        assert!(app.restored_selection);
        assert_eq!(app.full_size(), vec2(1380.0, 840.0));
        assert_eq!(app.mini_size(), vec2(812.0, 640.0));
        assert_eq!(app.choose_next().as_deref(), Some("/synthetic/C.wav"));
    }
    let (tx, commands) = mpsc::channel();
    app.audio.tx = tx;
    app.toggle();
    assert_eq!(app.requested.as_deref(), Some("/synthetic/C.wav"));
    assert!(
        matches!(commands.try_recv(), Ok(Command::Load { track, .. }) if track.source == "/synthetic/C.wav")
    );
    let old: Saved = serde_json::from_str("{}").unwrap();
    assert!(old.shuffle_session.is_none() && old.full_size.is_none());
}
#[test]
fn volume_round_trips_and_old_profiles_keep_the_previous_default() {
    let old: Saved = serde_json::from_str("{}").unwrap();
    assert_eq!(old.volume_db, -12.0);
    let dir = tempfile::tempdir().unwrap();
    let ctx = egui::Context::default();
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
    app.volume_db = -27.5;
    let mut storage = Storage::default();
    eframe::App::save(&mut app, &mut storage);
    let mut cc = eframe::CreationContext::_new_kittest(ctx);
    cc.storage = Some(&storage);
    let reopened = Aurea::new(&cc, dir.path().into(), vec![], None, None);
    assert_eq!(reopened.volume_db, -27.5);
    assert_eq!(
        f32::from_bits(reopened.audio.gain.load(Ordering::Relaxed)),
        10.0f32.powf(-27.5 / 20.0)
    );
    assert_eq!(valid_volume(f32::NAN), -12.0);
    assert_eq!(valid_volume(9.0), 0.0);
    assert_eq!(valid_volume(-90.0), -60.0);
}
#[test]
fn repaint_cadence_follows_visible_work() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = egui::Context::default();
    let cc = eframe::CreationContext::_new_kittest(ctx);
    let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
    assert_eq!(app.repaint_delay(true, false), Duration::from_millis(500));
    app.paused = false;
    assert_eq!(app.repaint_delay(true, false), Duration::from_millis(33));
    assert_eq!(app.repaint_delay(false, false), Duration::from_millis(33));
    assert_eq!(app.repaint_delay(false, true), Duration::from_millis(200));
    app.saved.mini_view = 2;
    assert_eq!(app.repaint_delay(true, false), Duration::from_millis(67));
    app.saved.mini = false;
    assert_eq!(app.repaint_delay(true, false), Duration::from_millis(100));
    app.saved.mini = true;
    app.paused = true;
    app.meters[0] = 0.1;
    assert_eq!(app.repaint_delay(true, false), Duration::from_millis(67));
    app.meters = [0.0; 2];
    app.scanning = true;
    assert_eq!(app.repaint_delay(true, false), Duration::from_millis(100));
}

#[test]
#[cfg(target_os = "macos")]
fn arabic_rendering_keeps_latin_numbers_and_word_shaping_in_order() {
    let ctx = egui::Context::default();
    crate::i18n::set("ar");
    super::listening_ui::configure_language_font(&ctx);
    let mut output = ctx.run_ui(Default::default(), |ui| {
        let galley = ui.painter().layout_no_wrap(
            crate::i18n::tracks(72),
            egui::FontId::proportional(14.0),
            Color32::WHITE,
        );
        let digits: String = galley
            .rows
            .iter()
            .flat_map(|r| &r.glyphs)
            .map(|g| g.chr)
            .filter(char::is_ascii_digit)
            .collect();
        assert_eq!(digits, "72", "Arabic shaping must not reverse Latin digits");
    });
    output.textures_delta.clear();
    crate::i18n::set("en");
}

#[test]
#[ignore = "Explicit optimized UI benchmark; synthetic library, no network or audio"]
fn profile_large_library_ui() {
    for (scene, query) in [
        ("mini-idle", ""),
        ("mini", ""),
        ("mini", "Opening"),
        ("mini-vinyl", ""),
        ("mini-artist", ""),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
        app.preview(scene);
        let root = PathBuf::from("/Volumes/Benchmark NAS/Music");
        app.saved.folders = vec![root.clone()];
        app.saved.active_source = Some(format!("folder:{}", root.display()));
        let template = app.saved.tracks[0].clone();
        app.saved.tracks = (0..20_202)
            .map(|i| {
                let mut t = template.clone();
                t.source = format!("{}/{:04}/{i:05}.flac", root.display(), i / 10);
                t.number = (i % 10 + 1) as u32;
                t
            })
            .collect();
        if app.current.is_some() {
            app.current = Some(app.saved.tracks[0].source.clone());
        }
        app.query = query.into();
        app.library_revision += 1;
        let mut frame = |angle: f32| {
            app.angle = angle;
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        pos2(0.0, 0.0),
                        vec2(812.0, 640.0),
                    )),
                    ..Default::default()
                },
                |ui| app.mini_ui(ui),
            );
            std::hint::black_box(ctx.tessellate(output.shapes, output.pixels_per_point));
        };
        for _ in 0..5 {
            frame(0.0);
        }
        let start = Instant::now();
        for i in 0..240 {
            frame(i as f32 * 0.1);
        }
        println!(
            "PROFILE scene={scene} query={query:?} tracks=20202 frames=240 us_per_frame={:.1}",
            start.elapsed().as_micros() as f64 / 240.0
        );
    }
}

#[test]
fn settings_round_trip_without_rewriting_tracks_or_passwords_and_output_mode_is_structured() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = egui::Context::default();
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
    app.preview("mini-vinyl");
    app.saved.language = "ja".into();
    app.saved.ui_scale = 1.15;
    app.saved.reduce_motion = true;
    app.saved.dsp = crate::dsp::Config {
        mode: crate::dsp::ReplayGain::Album,
        preamp_db: -3.0,
        prevent_clipping: true,
    };
    app.native = false;
    app.volume_db = -23.0;
    app.room_password = "fixture-session-only".into();
    app.output = "translated output text".into();
    app.output_exclusive = true;
    assert!(app.exclusive_active());
    app.output = "Esclusiva integer".into();
    app.output_exclusive = false;
    assert!(!app.exclusive_active());
    let expected = serde_json::to_value(&app.saved.tracks).unwrap();
    let mut storage = Storage::default();
    eframe::App::save(&mut app, &mut storage);
    assert!(
        !storage
            .0
            .values()
            .any(|s| s.contains("fixture-session-only"))
    );
    let mut cc = eframe::CreationContext::_new_kittest(ctx);
    cc.storage = Some(&storage);
    let reopened = Aurea::new(&cc, dir.path().into(), vec![], None, None);
    assert_eq!(reopened.saved.language, "ja");
    assert_eq!(reopened.saved.ui_scale, 1.15);
    assert!(reopened.saved.reduce_motion);
    assert_eq!(reopened.saved.dsp, app.saved.dsp);
    assert!(!reopened.native);
    assert_eq!(reopened.volume_db, -23.0);
    assert!(reopened.room_password.is_empty());
    assert_eq!(
        serde_json::to_value(&reopened.saved.tracks).unwrap(),
        expected
    );
}
