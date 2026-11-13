use super::*;
use unicode_normalization::UnicodeNormalization;

const CHAPTERS: [(&str, &[&str]); 12] = [
    (
        "Library",
        &[
            "Guide: add folders or mounted NAS volumes from File. Select a source to listen only to its tracks; All music includes every source. Double-click a track to play.",
        ],
    ),
    (
        "Playlists",
        &[
            "Guide: Click the stars beside a track to change its rating. Click the current rating again to clear it. Stars do not start playback or change the queue.",
            "Guide: In Playlists create, rename and reorder saved lists. Play copies a list to Playback queue. Playlist edits never change the active queue. Import/export supports M3U/M3U8.",
            "Guide: Dynamic playlists store tag rules. Choose All rules or Any rule. Years support inclusive bounds. Library changes refresh results; Play keeps a queue snapshot.",
            "Guide: Right-click a track to rate it. Dynamic rules support ratings and play counts. A play counts after 30 seconds or half a short track, at least one second; pauses and seeks do not count. Earlier history is not reconstructed.",
        ],
    ),
    (
        "Pair with MusicCraft Android",
        &[
            "Guide: On the phone, enable sharing and choose Pair computer.",
            "Guide: Four-digit code",
            "Guide: Confirm the pairing request on your phone.",
            "Guide: No phone found. Check that both devices use the same Wi-Fi.",
        ],
    ),
    (
        "Connect smartphone / FTP",
        &[
            "Guide: enter the phone FTP address, port and music folder, then your credentials. Enable automatic discovery when its IP changes. Keep the phone server running on the same network.",
        ],
    ),
    (
        "Sources and synchronization",
        &[
            "Guide: automatic library refresh updates the catalogue only. File copying and deletions require the separate Synchronization tab and its reviewed preview.",
        ],
    ),
    (
        "Synchronization",
        &[
            "Guide: choose origin and destination from existing Library sources. Select one-way or bidirectional synchronization, generate a preview and resolve every conflict before applying. Library filters and playback queues remain independent.",
            "Guide: one-way mode mirrors the origin, including deletions. Bidirectional mode uses the last completed baseline. Changed or unreachable sources require a new preview; missing sources are never empty libraries.",
            "Guide: on Android grant one music folder and enable computer access in Synchronization. Keep sharing enabled, review each preview on the computer and approve it on the phone. Replaced and deleted files stay in the hidden .musiccraft-sync/recovery directory; recovery is currently manual.",
        ],
    ),
    (
        "Playback",
        &[
            "Guide: Space plays or pauses; Left and Right seek five seconds. The order icon cycles sequence, random tracks and random albums. Repeat repeats one track. Double-click a filtered result to start that selection. Click a track beside the vinyl to play it.",
            "Guide: shuffle tracks plays the listening selection without repeats. Only started tracks count. Previous, manual choices and Repeat track can replay tracks. A new source or selection starts a new cycle; closing the app and changing order mode keep it.",
        ],
    ),
    (
        "Audio",
        &[
            "Guide: choose a device and shared or exclusive output in Settings. Exclusive bypasses software volume and ReplayGain. Strict integer stops on incompatible formats. ReplayGain and preamp apply to the next shared-mode track.",
        ],
    ),
    (
        "Artist and lyrics",
        &[
            "Guide: the artist biography follows the interface language, with English fallback. Lyrics keep their original language. Timed lyrics follow playback; click a line to seek. Retry repeats a failed lookup.",
        ],
    ),
    (
        "Cover",
        &[
            "Guide: artwork is read first from tags, then the album folder, then online. Optional saving adds an image only when no album artwork file exists. Existing images and audio tags are preserved.",
            "Guide: Double-click the cover to open album artwork. Use the arrows or Left and Right keys to browse cover, back and booklet images found in the album folder.",
        ],
    ),
    (
        "Multiroom",
        &[
            "Guide: select a source and start integrated multiroom in Settings. Choose MusicCraft or the displayed Mac IP in piCorePlayer. Select a room, play the source and group other rooms. Keep MusicCraft open and the Mac awake. New tracks appear automatically after source synchronization.",
        ],
    ),
    (
        "Interface",
        &[
            "Guide: the mini player keeps its size between tabs. Settings controls the global theme, language, 100–130% zoom and reduced animation. F1 opens this guide in both views.",
            "Guide: both player views remember their own sizes after restart. On a smaller display they fit the usable area with a 24-point margin, preserving the preferred size for larger displays.",
            "Guide: Choose Vinyl, CD, Cassette, Revox or VU meters in Settings. The central tab shows the selected player and remembers your choice after reopening MusicCraft.",
            "Guide: Enable Appearance by year in Settings to assign a player and last year to each period. Tracks without a year keep the manual appearance.",
            "Guide: Settings open in a separate window. Album artwork remembers its size and position across displays and app restarts.",
        ],
    ),
];

fn search_key(text: &str) -> String {
    text.to_lowercase().nfc().collect()
}

fn matches_chapter(title: &'static str, paragraphs: &[&'static str], query: &str) -> bool {
    query.is_empty()
        || std::iter::once(&title)
            .chain(paragraphs)
            .any(|&key| search_key(crate::i18n::logical_text(key)).contains(query))
}

impl Aurea {
    pub(super) fn help_window(&mut self, ctx: &egui::Context) {
        if !self.help_dialog {
            self.auxiliary_geometry.close("musiccraft-help");
            return;
        }
        let open = self.detached_window(
            ctx,
            "musiccraft-help",
            tr!("User guide"),
            vec2(760.0, 700.0),
            |app, ui| {
                ui.label(
                    RichText::new(format!("MusicCraft {}", env!("CARGO_PKG_VERSION")))
                        .color(palette::accent()),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut app.help_query)
                        .hint_text(tr!("Search the guide…"))
                        .desired_width(f32::INFINITY),
                );
                ui.separator();
                let query = search_key(app.help_query.trim());
                egui::ScrollArea::vertical()
                    .id_salt("help-content")
                    .max_height(ui.available_height())
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (index, (title, paragraphs)) in CHAPTERS.into_iter().enumerate() {
                            if !matches_chapter(title, paragraphs, &query) {
                                continue;
                            }
                            let title = crate::i18n::text(title);
                            egui::CollapsingHeader::new(RichText::new(title).strong())
                                .id_salt(("guide", index))
                                .default_open(true)
                                .show(ui, |ui| {
                                    let body = paragraphs
                                        .iter()
                                        .map(|key| crate::i18n::text(key))
                                        .collect::<Vec<_>>()
                                        .join("\n\n");
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(body).color(palette::text()),
                                        )
                                        .wrap(),
                                    );
                                });
                            ui.add_space(8.0);
                        }
                    });
            },
        );
        self.help_dialog = open;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{LANGUAGES, logical_text, set};

    #[test]
    fn every_language_renders_and_searches_the_whole_guide() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
        app.help_dialog = true;

        for (code, _) in LANGUAGES {
            set(code);
            listening_ui::configure_language_font(&ctx);
            let mut queries = Vec::new();
            for (title, paragraphs) in CHAPTERS {
                assert!(!logical_text(title).is_empty(), "{code}: {title}");
                for &paragraph in paragraphs {
                    let english = paragraph.strip_prefix("Guide: ").unwrap();
                    let localized = logical_text(paragraph);
                    assert!(!localized.is_empty(), "{code}: {title}");
                    assert_eq!(localized == english, code == "en", "{code}: {title}");
                    queries.push((title, paragraphs, search_key(localized)));
                }
            }
            for (title, paragraphs, query) in queries {
                assert!(
                    matches_chapter(title, paragraphs, &query),
                    "{code}: {title}"
                );
            }

            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        pos2(0.0, 0.0),
                        vec2(1280.0, 1600.0),
                    )),
                    ..Default::default()
                },
                |_| {
                    app.help_window(&ctx);
                    #[cfg(target_os = "macos")]
                    ctx.fonts_mut(|fonts| {
                        for key in CHAPTERS.iter().flat_map(|(title, paragraphs)| {
                            std::iter::once(title).chain(paragraphs.iter())
                        }) {
                            for c in crate::i18n::text(key)
                                .chars()
                                .filter(|c| c.is_alphanumeric())
                            {
                                assert!(
                                    fonts.has_glyph(&egui::FontId::proportional(14.0), c),
                                    "{code}: missing glyph {c:?} in {key}"
                                );
                            }
                        }
                    });
                },
            );
            output.textures_delta.clear();
            assert!(!output.shapes.is_empty(), "{code}: manual did not render");
            let nodes = output.platform_output.accesskit_update.unwrap().nodes;
            for (title, paragraphs) in CHAPTERS {
                let title = crate::i18n::text(title);
                let has_text = |text: &str| {
                    nodes
                        .iter()
                        .any(|(_, node)| node.label() == Some(text) || node.value() == Some(text))
                };
                assert!(has_text(title), "{code}: chapter missing: {title}");
                let body = paragraphs
                    .iter()
                    .map(|key| crate::i18n::text(key))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                assert!(has_text(&body), "{code}: chapter text missing: {title}");
            }
            assert!(app.help_dialog);
        }
        set("vi");
        let (title, paragraphs) = CHAPTERS[0];
        let decomposed = "Thêm thư mục".nfd().collect::<String>();
        assert!(matches_chapter(title, paragraphs, &search_key(&decomposed)));
        assert!(!matches_chapter(title, paragraphs, "không có kết quả"));
        set("en");
    }
}
