use super::*;
use crate::listening::State;

pub(super) fn configure_language_font(ctx: &egui::Context) {
    configure_font_stack(ctx, false);
}
pub(super) fn configure_font_stack(ctx: &egui::Context, all_scripts: bool) {
    let language = crate::i18n::language();
    let paths = if all_scripts {
        vec![
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/System/Library/Fonts/SFArabic.ttf",
            "/System/Library/Fonts/KohinoorTelugu.ttc",
        ]
    } else {
        match language {
            "ja" | "zh-hans" | "zh-hant" => vec!["/System/Library/Fonts/Hiragino Sans GB.ttc"],
            "ar" => vec!["/System/Library/Fonts/SFArabic.ttf"],
            "te" => vec!["/System/Library/Fonts/KohinoorTelugu.ttc"],
            _ => vec![],
        }
    };
    let mut fonts = egui::FontDefinitions::default();
    // Rebuild one coherent stack; system fonts are used in place, never bundled.
    // epaint shapes one face run at a time, without full paragraph bidi.
    // Its default Latin face does not include Arabic: keeping it first in Arabic
    // separates spaces/numbers from Arabic words, which are reordered by i18n.
    // Arial includes Arabic and would merge them, reversing numeric strings too.
    for path in (language != "ar")
        .then_some("/System/Library/Fonts/Supplemental/Arial.ttf")
        .into_iter()
        .chain(paths)
    {
        if let Ok(bytes) = std::fs::read(path) {
            let name = path.to_string();
            fonts
                .font_data
                .insert(name.clone(), Arc::new(egui::FontData::from_owned(bytes)));
            fonts
                .families
                .get_mut(&egui::FontFamily::Proportional)
                .unwrap()
                .insert(if path.ends_with("Arial.ttf") { 0 } else { 1 }, name);
        }
    }
    let family = fonts.families[&egui::FontFamily::Proportional].clone();
    fonts
        .families
        .insert(egui::FontFamily::Name("biography-sans".into()), family);
    ctx.set_fonts(fonts);
}

impl Aurea {
    pub(super) fn listening_split_content(
        &mut self,
        ui: &mut egui::Ui,
        text: Color32,
        muted: Color32,
        accent: Color32,
    ) {
        if self.loading || self.current.is_none() {
            self.listening_content(ui, false, text, muted, accent);
            return;
        }
        let area = ui.available_rect_before_wrap();
        let middle = area.center().x;
        ui.painter().line_segment(
            [pos2(middle, area.top()), pos2(middle, area.bottom())],
            egui::Stroke::new(1.0, muted.gamma_multiply(0.18)),
        );
        for (lyrics, rect) in [
            (
                false,
                egui::Rect::from_min_max(area.min, pos2(middle - 10.0, area.bottom())),
            ),
            (
                true,
                egui::Rect::from_min_max(pos2(middle + 10.0, area.top()), area.max),
            ),
        ] {
            ui.scope_builder(
                egui::UiBuilder::new().max_rect(rect).id_salt(if lyrics {
                    "split-lyrics"
                } else {
                    "split-biography"
                }),
                |ui| {
                    ui.set_clip_rect(rect.intersect(ui.clip_rect()));
                    self.listening_content(ui, lyrics, text, muted, accent);
                },
            );
        }
    }
    pub(super) fn listening_content(
        &mut self,
        ui: &mut egui::Ui,
        lyrics: bool,
        text: Color32,
        muted: Color32,
        accent: Color32,
    ) {
        ui.visuals_mut().hyperlink_color = accent;
        if self.loading {
            ui.add_space(24.0);
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(14.0));
                ui.label(RichText::new(tr!("Waiting for the new track…")).color(muted));
            });
            return;
        }
        if self.current.is_none() {
            listening_status::<()>(ui, &State::Empty, "", muted);
            return;
        }
        if lyrics {
            match &self.listening.lyrics {
                State::Ready(song) => {
                    let timed = !song.lines.is_empty();
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(if song.instrumental {
                                    tr!("Instrumental track")
                                } else if timed {
                                    tr!("Karaoke · line synchronization")
                                } else {
                                    tr!("Unsynchronized lyrics")
                                })
                                .size(11.5)
                                .color(muted),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.hyperlink_to(RichText::new("LRCLIB ↗").size(11.5), &song.url);
                            if timed
                                && ui
                                    .selectable_label(self.lyric_follow, tr!("Follow"))
                                    .on_hover_text(tr!("Follow the playing line"))
                                    .clicked()
                            {
                                self.lyric_follow = !self.lyric_follow;
                                self.last_lyric_line = None;
                            }
                        });
                    });
                    ui.add_space(10.0);
                    let active = song.active(self.position);
                    let scroll = self.lyric_follow && self.last_lyric_line != active;
                    if ui.rect_contains_pointer(ui.max_rect())
                        && ui.input(|i| i.smooth_scroll_delta.y != 0.0)
                    {
                        self.lyric_follow = false;
                    }
                    let mut seek = None;
                    egui::ScrollArea::vertical()
                        .id_salt("listening-lyrics")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if song.instrumental {
                                ui.label(
                                    RichText::new(tr!("Just music, no sung lyrics.")).color(text),
                                );
                            } else if timed {
                                ui.add_space(12.0);
                                for (i, line) in song.lines.iter().enumerate() {
                                    let current = active == Some(i);
                                    let label = if line.text.is_empty() {
                                        "♪"
                                    } else {
                                        &line.text
                                    };
                                    let r = ui.add(
                                        egui::Label::new(
                                            RichText::new(label)
                                                .font(egui::FontId::new(
                                                    if current { 19.0 } else { 14.0 },
                                                    egui::FontFamily::Name("biography-sans".into()),
                                                ))
                                                .color(if current { accent } else { muted }),
                                        )
                                        .wrap()
                                        .sense(egui::Sense::click()),
                                    );
                                    r.widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::WidgetType::Button,
                                            true,
                                            label,
                                        )
                                    });
                                    if current && scroll {
                                        r.scroll_to_me(Some(egui::Align::Center));
                                    }
                                    if r.on_hover_text(tr!("Click to play from this row"))
                                        .clicked()
                                    {
                                        seek = Some(line.time);
                                    }
                                    ui.add_space(10.0);
                                }
                                ui.add_space(36.0);
                            } else {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&song.plain)
                                            .font(egui::FontId::new(
                                                14.0,
                                                egui::FontFamily::Name("biography-sans".into()),
                                            ))
                                            .color(text),
                                    )
                                    .wrap(),
                                );
                            }
                        });
                    self.last_lyric_line = active;
                    if let Some(position) = seek
                        && !self.loading
                        && self.current.is_some()
                    {
                        self.position = position;
                        let _ = self.audio.tx.send(Command::Seek(position));
                    }
                }
                state => {
                    if listening_status(
                        ui,
                        state,
                        tr!("Lyrics unavailable for this track version."),
                        muted,
                    ) {
                        self.retry_listening();
                    }
                }
            }
        } else {
            match &self.listening.artist {
                State::Ready(bio) => {
                    ui.vertical(|ui| {
                        ui.add_sized(
                            vec2(ui.available_width(), 24.0),
                            egui::Label::new(
                                RichText::new(&bio.title).size(16.0).strong().color(text),
                            )
                            .truncate(),
                        )
                        .on_hover_text(&bio.title);
                        ui.horizontal(|ui| {
                            let language = url::Url::parse(&bio.url)
                                .ok()
                                .and_then(|u| {
                                    u.host_str()
                                        .map(|h| h.split('.').next().unwrap_or("").to_uppercase())
                                })
                                .unwrap_or_default();
                            ui.hyperlink_to(
                                RichText::new(format!("Wikipedia {language} ↗")).size(11.5),
                                &bio.url,
                            );
                        });
                    });
                    ui.add_space(12.0);
                    egui::ScrollArea::vertical()
                        .id_salt("listening-artist")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for paragraph in bio.text.split('\n').filter(|s| !s.trim().is_empty()) {
                                let mut job = egui::text::LayoutJob::simple(
                                    paragraph.into(),
                                    egui::FontId::new(
                                        14.0,
                                        egui::FontFamily::Name("biography-sans".into()),
                                    ),
                                    muted,
                                    ui.available_width(),
                                );
                                job.justify = true;
                                job.sections[0].format.line_height = Some(19.0);
                                ui.add(egui::Label::new(job).wrap());
                                ui.add_space(10.0);
                            }
                            ui.horizontal_wrapped(|ui| {
                                ui.hyperlink_to(
                                    RichText::new(tr!("Authors and history")).size(11.0),
                                    format!("{}?action=history", bio.url),
                                );
                                ui.hyperlink_to(
                                    RichText::new("CC BY-SA 4.0").size(11.0),
                                    "https://creativecommons.org/licenses/by-sa/4.0/",
                                );
                            });
                        });
                }
                state => {
                    if listening_status(
                        ui,
                        state,
                        tr!("No matching artist biography found on Wikipedia."),
                        muted,
                    ) {
                        self.retry_listening();
                    }
                }
            }
        }
    }
    fn retry_listening(&mut self) {
        self.listening.retry();
        self.last_lyric_line = None;
        self.lyric_follow = true;
    }
}
fn listening_status<T>(ui: &mut egui::Ui, state: &State<T>, missing: &str, muted: Color32) -> bool {
    ui.add_space(24.0);
    let mut retry = false;
    match state {
        State::Loading => {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(14.0));
                ui.label(RichText::new(tr!("Searching…")).color(muted));
            });
        }
        State::Empty => {
            ui.label(
                RichText::new(tr!("The story behind the music"))
                    .size(17.0)
                    .color(muted),
            );
            ui.add_space(8.0);
            ui.label(
                RichText::new(tr!(
                    "Play a track with artist and title tags to see its biography and lyrics."
                ))
                .color(muted),
            );
        }
        State::Missing => {
            ui.label(RichText::new(missing).color(muted));
            retry = ui.button(tr!("Retry")).clicked();
        }
        State::Error(e) => {
            ui.label(RichText::new(crate::i18n::engine_message(e)).color(muted));
            retry = ui.button(tr!("Retry")).clicked();
        }
        State::Ready(_) => {}
    }
    retry
}
