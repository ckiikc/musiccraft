#[macro_use]
mod i18n;
mod album_art;
mod app;
mod audio;
mod cache;
mod credentials;
mod discovery;
mod dsp;
mod embedded_server;
mod exclusive;
mod facets;
mod history;
mod integer;
mod jobs;
mod library;
mod listening;
mod media_server;
mod multiroom;
mod network;
mod open_documents;
mod phone;
mod remote_tags;
mod shuffle;
mod streaming;
mod sync;

fn main() -> eframe::Result {
    let all_args: Vec<_> = std::env::args().collect();
    if all_args.get(1).is_some_and(|s| s == "--configure-ftp") {
        if let Some(file) = all_args.get(2) {
            let result = (|| -> Result<(), String> {
                let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
                let mut sources: Vec<network::FtpSource> =
                    serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                let source = sources.first_mut().ok_or("Sorgente mancante")?;
                source.url()?;
                let mut password = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin().lock(), &mut password)
                    .map_err(|e| e.to_string())?;
                source.password = password.trim_end_matches(['\r', '\n']).to_string();
                credentials::save(source)?;
                println!("Credenziali salvate nel Portachiavi");
                Ok(())
            })();
            if let Err(e) = result {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    let mut args = std::env::args().skip(1);
    let mut paths = Vec::new();
    let mut screenshot = None;
    let mut pairing_file = None;
    let mut pair_phone_ui = false;
    let mut test_seconds = None;
    #[cfg(debug_assertions)]
    let mut drag_trace = None;
    let mut data = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Library/Application Support/MusicCraft");
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => {
                if let Some(path) = args.next() {
                    data = path.into();
                }
            }
            "--network" => {}
            "--pair-phone" => pair_phone_ui = true,
            "--pair-android" => pairing_file = args.next().map(std::path::PathBuf::from),
            #[cfg(debug_assertions)]
            "--window-drag-trace" => drag_trace = args.next().map(std::path::PathBuf::from),
            #[cfg(debug_assertions)]
            "--ui-preview" => {
                let _ = args.next();
            }
            "--screenshot" => screenshot = args.next().map(Into::into),
            "--test-seconds" => test_seconds = args.next().and_then(|s| s.parse().ok()),
            _ => paths.push(arg.into()),
        }
    }
    if let Err(e) = std::fs::create_dir_all(&data) {
        eprintln!("Impossibile creare la cartella dati: {e}");
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!(
                    "../assets/app-icon/musiccraft-dock.png"
                ))
                .expect("validated MusicCraft icon"),
            )
            .with_inner_size([520.0, 520.0])
            .with_decorations(false)
            .with_transparent(true)
            .with_clamp_size_to_monitor_size(true)
            .with_window_level(eframe::egui::WindowLevel::AlwaysOnTop)
            .with_min_inner_size([520.0, 262.0])
            .with_app_id("io.github.ckiikc.musiccraft"),
        persistence_path: Some(data.join("state.ron")),
        ..Default::default()
    };
    #[cfg(debug_assertions)]
    if all_args.iter().any(|arg| arg == "--data-dir")
        && let Some(path) = drag_trace
    {
        app::enable_window_drag_trace(path);
    }
    open_documents::install();
    eframe::run_native(
        "MusicCraft",
        options,
        Box::new(move |cc| {
            let mut app = app::Aurea::new(cc, data, paths, screenshot, test_seconds);
            #[cfg(debug_assertions)]
            if all_args.iter().any(|a| a == "--data-dir")
                && let Some(i) = all_args.iter().position(|a| a == "--ui-preview")
                && let Some(scene) = all_args.get(i + 1)
            {
                app.preview(scene);
            }
            if pair_phone_ui {
                app.open_phone_pair();
            }
            if let Some(file) = pairing_file
                && let Ok(link) = std::fs::read_to_string(file)
            {
                app.pair_phone(&link);
            }
            if all_args.iter().any(|a| a == "--network") {
                app.open_network();
            }
            Ok(Box::new(app))
        }),
    )
}
