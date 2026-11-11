use super::*;
use crate::app::App;
use eframe::egui::{Align2, RichText};

/// Seitenverhältnisse in den Filmeinstellungen (Breite, Höhe, Name).
const RATIOS: [(f64, f64, &str); 10] = [
    (16.0, 9.0, "16:9 Querformat (YouTube)"),
    (9.0, 16.0, "9:16 Hochformat (Shorts, Reels)"),
    (1.0, 1.0, "1:1 Quadrat"),
    (4.0, 5.0, "4:5 Hochformat (Instagram)"),
    (4.0, 3.0, "4:3 Querformat"),
    (3.0, 4.0, "3:4 Hochformat"),
    (1.85, 1.0, "1,85:1 Kino (Flat)"),
    (2.0, 1.0, "2:1 Univisium"),
    (21.0, 9.0, "21:9 Kino"),
    (2.39, 1.0, "2,39:1 Cinemascope"),
];

/// Entwurf der Filmeinstellungen: Seitenverhältnis (Breite : Höhe), kurze Seite, Bildrate; erst „OK“ übernimmt ihn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SettingsDraft {
    pub(crate) ratio: (f64, f64),
    pub(crate) short: i64,
    pub(crate) fps: i64,
}

/// Gleiches Seitenverhältnis (auf 0,5 % genau; gerade Pixelzahlen weichen etwas ab).
pub(crate) fn same_ratio(a: (f64, f64), b: (f64, f64)) -> bool {
    let (x, y) = (a.0 / a.1.max(1e-9), b.0 / b.1.max(1e-9));
    (x / y - 1.0).abs() < 0.005
}

/// Kürzt ein Pixelmaß zu einem lesbaren Verhältnis: 2560 × 1080 → 64 : 27, krumme Werte → x : 1.
pub(crate) fn reduce(r: (f64, f64)) -> (f64, f64) {
    let (w, h) = (r.0.round() as i64, r.1.round() as i64);
    if (r.0 - w as f64).abs() > 1e-6 || (r.1 - h as f64).abs() > 1e-6 || w <= 0 || h <= 0 {
        return r;
    }
    let (mut a, mut b) = (w, h);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    let (w, h) = (w / a, h / a);
    if w.max(h) > 100 { ((w as f64 / h as f64 * 100.0).round() / 100.0, 1.0) } else { (w as f64, h as f64) }
}

fn ratio_text(r: (f64, f64)) -> String {
    let (w, h) = reduce(r);
    let f = |v: f64| if v.fract().abs() < 1e-9 { format!("{v:.0}") } else { format!("{v:.2}").replace('.', ",") };
    format!("{}:{}", f(w), f(h))
}

impl App {
    pub(crate) fn menu_bar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("Datei", |ui| {
                if ui.button("Neues Projekt                 Strg+N").clicked() {
                    self.new_project();
                    ui.close();
                }
                if ui.button("Öffnen …                      Strg+O").clicked() {
                    self.open_dialog();
                    ui.close();
                }
                if ui.button("Medien importieren …          Strg+I").clicked() {
                    self.import_dialog();
                    ui.close();
                }
                ui.separator();
                if ui.button("Sichern                       Strg+S").clicked() {
                    self.save();
                    ui.close();
                }
                if ui.button("Sichern unter …               Strg+Umschalt+S").clicked() {
                    self.save_as();
                    ui.close();
                }
                ui.separator();
                if ui.button("Filmeinstellungen …           E").clicked() {
                    self.show_settings = true;
                    ui.close();
                }
                if ui.add_enabled(self.export_job.is_none() && self.project.duration() > 0.0, egui::Button::new("Exportieren …                Strg+E")).clicked() {
                    self.export_dialog();
                    ui.close();
                }
                if ui.add_enabled(self.project.duration() > 0.0, egui::Button::new("Standbild exportieren …")).clicked() {
                    self.export_still();
                    ui.close();
                }
                if ui.button("Untertitel als SRT exportieren …").clicked() {
                    self.export_srt();
                    ui.close();
                }
                if ui.button("Zwischenspeicher …").clicked() {
                    self.show_cache = true;
                    ui.close();
                }
                ui.separator();
                if ui.button(if self.feat.recorder.is_some() { "Aufnahme beenden" } else { "Sprecher aufnehmen" }).clicked() {
                    self.toggle_recording();
                    ui.close();
                }
                ui.separator();
                if ui.button("Anfangsmarke setzen           I").clicked() {
                    self.set_mark_in();
                    ui.close();
                }
                if ui.button("Endmarke setzen               O").clicked() {
                    self.set_mark_out();
                    ui.close();
                }
                if ui.button("Marken löschen").clicked() {
                    self.clear_marks();
                    ui.close();
                }
                ui.separator();
                if ui.button("Kapitelmarke setzen           M").clicked() {
                    self.add_marker();
                    ui.close();
                }
                if ui.button("Kapitel für YouTube kopieren").clicked() {
                    self.copy_chapters(&ctx);
                    ui.close();
                }
            });
            ui.menu_button("Bearbeiten", |ui| {
                if ui.add_enabled(!self.undo.is_empty(), egui::Button::new("Rückgängig                   Strg+Z")).clicked() {
                    self.do_undo();
                    ui.close();
                }
                if ui.add_enabled(!self.redo.is_empty(), egui::Button::new("Wiederholen                  Strg+Y")).clicked() {
                    self.do_redo();
                    ui.close();
                }
                ui.separator();
                if ui.button("Ausschneiden                  Strg+X").clicked() {
                    self.cut_clips();
                    ui.close();
                }
                if ui.button("Kopieren                      Strg+C").clicked() {
                    self.copy_clips();
                    ui.close();
                }
                if ui.add_enabled(!self.clipboard.is_empty(), egui::Button::new("Einfügen                      Strg+V")).clicked() {
                    self.paste_clips();
                    ui.close();
                }
                if ui.button("Duplizieren                   Strg+D").clicked() {
                    self.duplicate_clips();
                    ui.close();
                }
                if ui.button("Alles auswählen               Strg+A").clicked() {
                    self.select_all();
                    ui.close();
                }
                ui.separator();
                if ui.button("Teilen an Abspielposition     S").clicked() {
                    self.split_at_playhead();
                    ui.close();
                }
                if ui.button("Auswahl löschen               Entf").clicked() {
                    self.delete_selection();
                    ui.close();
                }
                if ui.button("Ripple-Löschen (Lücke schließen)   Umschalt+Entf").clicked() {
                    self.ripple_delete_selection();
                    ui.close();
                }
                if ui.button("Lücke vor Auswahl schließen").clicked() {
                    self.close_gap_before_selection();
                    ui.close();
                }
                ui.separator();
                ui.menu_button("Einfügemodus", |ui| {
                    for m in crate::ops::InsertMode::ALL {
                        if ui.radio(self.insert_mode == m, format!("{}  ({})", m.label(), m.key())).clicked() {
                            self.set_insert_mode(m);
                            ui.close();
                        }
                    }
                });
                ui.menu_button("Mausmodus", |ui| {
                    for m in crate::ops::MouseMode::ALL {
                        if ui.radio(self.mouse_mode == m, format!("{}  ({})", m.label(), m.key())).clicked() {
                            self.set_mouse_mode(m);
                            ui.close();
                        }
                    }
                });
                ui.separator();
                if ui.button("Gruppe bilden                G").clicked() {
                    self.group_selection();
                    ui.close();
                }
                if ui.button("Gruppe auflösen              Umschalt+G").clicked() {
                    self.ungroup_selection();
                    ui.close();
                }
                ui.separator();
                if ui.button("Lückensuche                  Strg+Umschalt+C").clicked() {
                    self.find_gap();
                    ui.close();
                }
                if ui.button("Lücke an Abspielposition schließen").clicked() {
                    self.close_gap_at_playhead();
                    ui.close();
                }
                ui.menu_button("Musikalische Schnittanpassung", |ui| {
                    for (label, every) in [("auf jeden Schlag", 1), ("auf jeden 2. Schlag", 2), ("auf den Taktanfang (4/4)", 4)] {
                        if ui.button(label).clicked() {
                            self.musical_cut_adjustment(every);
                            ui.close();
                        }
                    }
                });
                ui.separator();
                if ui.button("Titel einfügen").clicked() {
                    self.add_title(crate::fx_edit::TitlePreset::Title);
                    ui.close();
                }
                if ui.button("Bauchbinde einfügen").clicked() {
                    self.add_title(crate::fx_edit::TitlePreset::LowerThird);
                    ui.close();
                }
                if ui.button("Abspann einfügen").clicked() {
                    self.add_title(crate::fx_edit::TitlePreset::Credits);
                    ui.close();
                }
                if ui.button("Laufschrift einfügen").clicked() {
                    self.add_title(crate::fx_edit::TitlePreset::Ticker);
                    ui.close();
                }
                if ui.button("Timecode-Titel einfügen").clicked() {
                    self.add_title(crate::fx_edit::TitlePreset::Timecode);
                    ui.close();
                }
                if ui.button("Untertitel einfügen").clicked() {
                    self.add_title(crate::fx_edit::TitlePreset::Subtitle);
                    ui.close();
                }
                if ui.button("Spur hinzufügen").clicked() {
                    self.insert_track_at(self.sel_track + 1);
                    ui.close();
                }
                if ui.add_enabled(self.project.tracks.len() > 1, egui::Button::new("Gewählte Spur löschen")).clicked() {
                    self.remove_track(self.sel_track);
                    ui.close();
                }
                ui.separator();
                if ui.button("Effekte kopieren              Strg+Alt+C").clicked() {
                    self.copy_effects();
                    ui.close();
                }
                if ui.add_enabled(self.fx_clipboard.is_some(), egui::Button::new("Effekte einfügen              Strg+Alt+V")).clicked() {
                    self.paste_effects();
                    ui.close();
                }
                if ui.add_enabled(self.selected.is_some_and(|id| self.can_align_audio(id)), egui::Button::new("Ton an zweiter Aufnahme ausrichten")).clicked() {
                    if let Some(id) = self.selected {
                        self.align_audio(id);
                    }
                    ui.close();
                }
                if ui.add_enabled(self.selected.is_some(), egui::Button::new("Szenen erkennen und teilen   Umschalt+Z")).clicked() {
                    if let Some(id) = self.selected {
                        self.detect_scenes(id);
                    }
                    ui.close();
                }
                if ui.button("Standbild aus Clip einfügen").clicked() {
                    self.freeze_frame();
                    ui.close();
                }
                if ui.add_enabled(self.missing_media() > 0, egui::Button::new("Fehlende Medien suchen …")).clicked() {
                    self.relink_missing();
                    ui.close();
                }
                ui.separator();
                if ui.button("Medien lokal sammeln").on_hover_text("Kopiert alle Medien des Projekts in den Medienordner (VideoSuperbe-Daten\\Medien) und verknüpft sie neu").clicked() {
                    self.collect_media_locally();
                    ui.close();
                }
                let mut local = self.prefs.copy_imports_local;
                if ui.checkbox(&mut local, "Beim Import alles lokal speichern").on_hover_text("Ohne diese Einstellung wird nur kopiert, was sonst verschwinden könnte (Dateien aus Temp-Ordnern, Mail-Anhänge)").changed() {
                    self.prefs.copy_imports_local = local;
                    self.save_prefs();
                }
                if ui.button("Medienordner im Explorer zeigen").clicked() {
                    let dir = crate::media::media_folder(&self.feat.data_dir);
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = std::process::Command::new("explorer.exe").arg(&dir).spawn();
                    ui.close();
                }
            });
            ui.menu_button("Marker", |ui| {
                if ui.button("Zu Zeitcode springen …").clicked() {
                    self.tc_prompt = Some(crate::ui::timecode(self.playhead, self.project.fps));
                    ui.close();
                }
                ui.separator();
                ui.menu_button("Projektmarker setzen", |ui| {
                    for n in 1..=10u32 {
                        if ui.button(format!("Marker {n}   Umschalt+{}", n % 10)).clicked() {
                            self.set_bookmark(n);
                            ui.close();
                        }
                    }
                });
                ui.menu_button("Zu Projektmarker springen", |ui| {
                    for n in 1..=10u32 {
                        let set = self.project.bookmarks.as_ref().is_some_and(|b| b.contains_key(&n.to_string()));
                        if ui.add_enabled(set, egui::Button::new(format!("Marker {n}   Strg+{}", n % 10))).clicked() {
                            self.jump_to_bookmark(n);
                            ui.close();
                        }
                    }
                });
                if ui.button("Nächsten freien Projektmarker setzen   Strg+Enter").clicked() {
                    self.set_next_bookmark();
                    ui.close();
                }
                ui.separator();
                if ui.button("Nächster Marker                W").clicked() {
                    self.jump_to_any_marker(true);
                    ui.close();
                }
                if ui.button("Vorheriger Marker              Q").clicked() {
                    self.jump_to_any_marker(false);
                    ui.close();
                }
                if ui.button("Kapitelmarke an Abspielposition löschen   Strg+Umschalt+Enter").clicked() {
                    self.remove_chapter_near_playhead();
                    ui.close();
                }
                if ui.button("Alle Kapitelmarken löschen").clicked() {
                    self.remove_all_chapters();
                    ui.close();
                }
                ui.separator();
                if ui.button("Nächste Lücke                <").clicked() {
                    self.jump_to_gap(true);
                    ui.close();
                }
                if ui.button("Vorherige Lücke              >").clicked() {
                    self.jump_to_gap(false);
                    ui.close();
                }
                if ui.button("Lücke als Bereich markieren  X").clicked() {
                    self.mark_gap();
                    ui.close();
                }
                if ui.button("Bereich abspielen            #").clicked() {
                    self.play_range();
                    ui.close();
                }
            });
            ui.menu_button("Ansicht", |ui| {
                ui.menu_button("Messinstrumente", |ui| {
                    for m in [crate::scopes::ScopeMode::Off, crate::scopes::ScopeMode::Histogram, crate::scopes::ScopeMode::Parade, crate::scopes::ScopeMode::Both] {
                        if ui.radio(self.scope_mode == m, m.label()).clicked() {
                            self.set_scope(m);
                            ui.close();
                        }
                    }
                });
                if ui.add_enabled(self.project.duration() > 0.0, egui::Button::new("Film im Vollbild abspielen   Alt+Enter")).clicked() {
                    self.toggle_fullscreen(&ctx);
                    ui.close();
                }
                ui.separator();
                if ui.checkbox(&mut self.storyboard, "Storyboard-Ansicht           Strg+Alt+B").clicked() {
                    ui.close();
                }
                if ui.checkbox(&mut self.show_animator, "Keyframe-Animator").on_hover_text("Unter dem Monitor, solange der Reiter „Effekte“ offen ist").clicked() {
                    if self.show_animator {
                        self.inspector_tab = 3;
                    }
                    ui.close();
                }
                if ui.checkbox(&mut self.loop_range, "Abspielbereich als Schleife   Strg+Alt+L").clicked() {
                    ui.close();
                }
                ui.separator();
                ui.menu_button("Zoom", |ui| {
                    let fps = self.project.fps.max(1) as f64;
                    for (label, span) in [("1 Bild   Alt+1", 1.0 / fps), ("5 Bilder   Alt+2", 5.0 / fps), ("1 Sekunde   Alt+3", 1.0), ("10 Sekunden   Alt+4", 10.0), ("1 Minute   Alt+5", 60.0), ("10 Minuten   Strg+Alt+6", 600.0)] {
                        if ui.button(label).clicked() {
                            self.zoom_to_span(span);
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.button("Markierter Bereich   Alt+F").clicked() {
                        self.zoom_to_range();
                        ui.close();
                    }
                    if ui.button("Gewählte Objekte   Strg+Alt+F").clicked() {
                        self.zoom_to_selection();
                        ui.close();
                    }
                    if ui.button("Vergrößern   Strg+↑").clicked() {
                        self.zoom_step(true);
                        ui.close();
                    }
                    if ui.button("Verkleinern   Strg+↓").clicked() {
                        self.zoom_step(false);
                        ui.close();
                    }
                });
                if ui.button("Zoom: ganzen Film zeigen      \\").clicked() {
                    self.zoom_fit();
                    ui.close();
                }
                if ui.checkbox(&mut self.snapping, "Magnetisches Einrasten      N").clicked() {
                    ui.close();
                }
            });
        });
    }

    /// Hauptwerkzeugleiste (wie die Toolbar der Mac-Version): Öffnen, Sichern, Filmformat, Exportieren.
    pub(crate) fn toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal_centered(|ui| {
            ui.add_space(6.0);
            if tool_button(ui, I_OPEN, "Öffnen", "Projekt öffnen (Strg+O)", true).clicked() {
                self.open_dialog();
            }
            if tool_button(ui, I_SAVE, "Sichern", "Projekt sichern (Strg+S)", true).clicked() {
                self.save();
            }
            let fmt = format!("{}×{} · {} fps", self.project.render_width, self.project.render_height, self.project.fps);
            if tool_button(ui, I_ASPECT, &fmt, "Filmeinstellungen (E)", true).clicked() {
                self.show_settings = true;
            }
            let can_export = self.export_job.is_none() && self.project.duration() > 0.0;
            if tool_button(ui, I_EXPORT, "Exportieren", "Als MP4 exportieren (H.264 + AAC)", can_export).clicked() {
                self.export_dialog();
            }
        });
    }

    pub(crate) fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            self.settings_draft = None;
            return;
        }
        let p = &self.project;
        // Entwurf: Seitenverhältnis als Zahlenpaar (nicht als Listenplatz, sonst wird z. B. 2520 × 1080 = 7:3 nicht als 21:9
        // erkannt und fällt auf 16:9 zurück, #2), kurze Seite, Bildrate
        let initial = SettingsDraft { ratio: (p.render_width.max(1) as f64, p.render_height.max(1) as f64), short: p.render_width.min(p.render_height), fps: p.fps };
        let draft = self.settings_draft.get_or_insert(initial);
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new("Filmeinstellungen")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_width(380.0);
                egui::Grid::new("settings").num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| {
                    ui.label("Seitenverhältnis");
                    let preset = RATIOS.iter().find(|r| same_ratio((r.0, r.1), draft.ratio));
                    let custom_label = format!("Eigenes ({})", ratio_text(draft.ratio));
                    egui::ComboBox::from_id_salt("ratio").selected_text(preset.map_or(custom_label, |r| r.2.to_string())).width(260.0).show_ui(ui, |ui| {
                        for r in RATIOS {
                            if ui.selectable_label(preset.is_some_and(|p| p.2 == r.2), r.2).clicked() {
                                draft.ratio = (r.0, r.1);
                            }
                        }
                    });
                    ui.end_row();
                    ui.label("");
                    ui.horizontal(|ui| {
                        // freie Eingabe, z. B. 2,76 : 1 (Ultra Panavision) oder 5 : 4
                        let (mut rw, mut rh) = preset.map_or_else(|| reduce(draft.ratio), |r| (r.0, r.1));
                        let a = ui.add(egui::DragValue::new(&mut rw).range(0.1..=100.0).speed(0.01).max_decimals(2));
                        ui.label(":");
                        let b = ui.add(egui::DragValue::new(&mut rh).range(0.1..=100.0).speed(0.01).max_decimals(2));
                        if a.changed() || b.changed() {
                            draft.ratio = (rw, rh);
                        }
                        ui.weak(format!("= {:.3} : 1", rw / rh.max(0.001)));
                    });
                    ui.end_row();
                    ui.label("Auflösung");
                    let short_label = match draft.short { 720 => "HD (720)".to_string(), 1080 => "Full HD (1080)".to_string(), 1440 => "QHD (1440)".to_string(), 2160 => "4K (2160)".to_string(), v => format!("{v}") };
                    egui::ComboBox::from_id_salt("short").selected_text(short_label).width(260.0).show_ui(ui, |ui| {
                        for (v, n) in [(720, "HD (720)"), (1080, "Full HD (1080)"), (1440, "QHD (1440)"), (2160, "4K (2160)")] {
                            ui.selectable_value(&mut draft.short, v, n);
                        }
                    });
                    ui.end_row();
                    ui.label("Bildrate");
                    egui::ComboBox::from_id_salt("fps").selected_text(format!("{} Bilder/s", draft.fps)).width(260.0).show_ui(ui, |ui| {
                        for f in [24, 25, 30, 50, 60] {
                            ui.selectable_value(&mut draft.fps, f, format!("{f} Bilder/s"));
                        }
                    });
                    ui.end_row();
                });
                let (w, h) = Self::settings_size(draft.ratio, draft.short);
                ui.add_space(6.0);
                ui.label(RichText::new(format!("Zielgröße: {w} × {h} Pixel")).color(SECONDARY).monospace());
                ui.add_space(8.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("OK").clicked() {
                        apply = true;
                    }
                    if ui.button("Abbrechen").clicked() {
                        cancel = true;
                    }
                });
            });
        if apply {
            if let Some(d) = self.settings_draft.take() {
                Self::apply_settings(&mut self.project, d, initial);
            }
            self.show_settings = false;
        } else if cancel {
            self.settings_draft = None;
            self.show_settings = false;
        }
    }

    /// Übernimmt den Entwurf. Ein unverändertes Format bleibt Pixel für Pixel erhalten (OK nach Änderung nur der Bildrate).
    pub(crate) fn apply_settings(p: &mut crate::model::Project, d: SettingsDraft, initial: SettingsDraft) {
        if !(same_ratio(d.ratio, initial.ratio) && d.short == initial.short) {
            (p.render_width, p.render_height) = Self::settings_size(d.ratio, d.short);
        }
        p.fps = d.fps;
    }

    /// Zielgröße aus Seitenverhältnis (Breite : Höhe) und kurzer Seite; beide Seiten gerade.
    pub(crate) fn settings_size(ratio: (f64, f64), short: i64) -> (i64, i64) {
        let (rw, rh) = (ratio.0.max(0.001), ratio.1.max(0.001));
        let long = ((short as f64 * rw.max(rh) / rw.min(rh) / 2.0).round() as i64) * 2;
        let short = (short / 2) * 2;
        if rw >= rh { (long, short) } else { (short, long) }
    }

    /// Dialog zum Umbenennen einer Kapitelmarke.
    pub(crate) fn marker_window(&mut self, ctx: &egui::Context) {
        let Some((id, mut name)) = self.rename_marker.clone() else { return };
        let mut done: Option<bool> = None;
        egui::Window::new("Name der Kapitelmarke").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO).show(ctx, |ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(260.0));
            r.request_focus();
            ui.add_space(6.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("OK").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = Some(true);
                }
                if ui.button("Abbrechen").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    done = Some(false);
                }
            });
        });
        match done {
            Some(true) => {
                if !name.is_empty() {
                    self.rename_marker(id, name);
                }
                self.rename_marker = None;
            }
            Some(false) => self.rename_marker = None,
            None => self.rename_marker = Some((id, name)),
        }
    }

    /// Meldung über nicht importierbare Dateien (statt nur einer Zeile in der Statusleiste).
    pub(crate) fn import_error_window(&mut self, ctx: &egui::Context) {
        if self.import_errors.is_empty() {
            return;
        }
        let mut close = false;
        egui::Window::new("Import nicht möglich")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .max_width(520.0)
            .show(ctx, |ui| {
                ui.set_max_width(500.0);
                for (name, why) in &self.import_errors {
                    ui.label(RichText::new(name).strong());
                    ui.label(why);
                    ui.add_space(6.0);
                }
                if ui.button("OK").clicked() {
                    close = true;
                }
            });
        if close {
            self.import_errors.clear();
        }
    }
}

impl App {
    /// Dialog „Zu Zeitcode springen“ (HH:MM:SS:BB, MM:SS:BB oder Sekunden).
    pub(crate) fn timecode_window(&mut self, ctx: &egui::Context) {
        let Some(mut text) = self.tc_prompt.clone() else { return };
        let mut done: Option<bool> = None;
        egui::Window::new("Zu Zeitcode springen").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO).show(ctx, |ui| {
            ui.label(RichText::new("HH:MM:SS:BB, MM:SS:BB oder Sekunden (z. B. 12,5)").size(11.5));
            let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(220.0));
            r.request_focus();
            ui.add_space(6.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Springen").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = Some(true);
                }
                if ui.button("Abbrechen").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    done = Some(false);
                }
            });
        });
        match done {
            Some(true) => {
                match crate::navigation::parse_timecode(&text, self.project.fps) {
                    Some(t) => {
                        let t = t.max(0.0);
                        self.seek(t);
                        self.tc_prompt = None;
                    }
                    None => self.status = "Zeitcode nicht lesbar (z. B. 00:00:12:15 oder 12,5)".into(),
                }
                if self.tc_prompt.is_some() {
                    self.tc_prompt = Some(text);
                }
            }
            Some(false) => self.tc_prompt = None,
            None => self.tc_prompt = Some(text),
        }
    }
}
