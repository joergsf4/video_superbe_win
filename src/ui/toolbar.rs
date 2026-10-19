use super::*;
use crate::app::App;
use eframe::egui::{Align2, RichText};

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
                if ui.button("Titel einfügen").clicked() {
                    self.add_title(crate::editing::TitlePreset::Title);
                    ui.close();
                }
                if ui.button("Bauchbinde einfügen").clicked() {
                    self.add_title(crate::editing::TitlePreset::LowerThird);
                    ui.close();
                }
                if ui.button("Untertitel einfügen").clicked() {
                    self.add_title(crate::editing::TitlePreset::Subtitle);
                    ui.close();
                }
                if ui.button("Spur hinzufügen").clicked() {
                    self.add_track();
                    ui.close();
                }
                ui.separator();
                if ui.button("Standbild aus Clip einfügen").clicked() {
                    self.freeze_frame();
                    ui.close();
                }
                if ui.add_enabled(self.missing_media() > 0, egui::Button::new("Fehlende Medien suchen …")).clicked() {
                    self.relink_missing();
                    ui.close();
                }
            });
            ui.menu_button("Ansicht", |ui| {
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
        let ratios: [(i64, i64, &str); 7] = [
            (16, 9, "16:9 Querformat (YouTube)"),
            (9, 16, "9:16 Hochformat (Shorts, Reels)"),
            (1, 1, "1:1 Quadrat"),
            (4, 5, "4:5 Hochformat (Instagram)"),
            (4, 3, "4:3 Querformat"),
            (3, 4, "3:4 Hochformat"),
            (21, 9, "21:9 Kino"),
        ];
        let p = &self.project;
        let draft = self.settings_draft.get_or_insert_with(|| {
            let g = {
                let (mut a, mut b) = (p.render_width, p.render_height);
                while b != 0 {
                    (a, b) = (b, a % b);
                }
                a.max(1)
            };
            let key = (p.render_width / g, p.render_height / g);
            let idx = ratios.iter().position(|r| (r.0, r.1) == key).unwrap_or(0);
            (idx, p.render_width.min(p.render_height), p.fps)
        });
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
                    egui::ComboBox::from_id_salt("ratio").selected_text(ratios[draft.0].2).width(260.0).show_ui(ui, |ui| {
                        for (i, r) in ratios.iter().enumerate() {
                            ui.selectable_value(&mut draft.0, i, r.2);
                        }
                    });
                    ui.end_row();
                    ui.label("Auflösung");
                    let short_label = match draft.1 { 720 => "HD (720)".to_string(), 1080 => "Full HD (1080)".to_string(), 1440 => "QHD (1440)".to_string(), 2160 => "4K (2160)".to_string(), v => format!("{v}") };
                    egui::ComboBox::from_id_salt("short").selected_text(short_label).width(260.0).show_ui(ui, |ui| {
                        for (v, n) in [(720, "HD (720)"), (1080, "Full HD (1080)"), (1440, "QHD (1440)"), (2160, "4K (2160)")] {
                            ui.selectable_value(&mut draft.1, v, n);
                        }
                    });
                    ui.end_row();
                    ui.label("Bildrate");
                    egui::ComboBox::from_id_salt("fps").selected_text(format!("{} Bilder/s", draft.2)).width(260.0).show_ui(ui, |ui| {
                        for f in [24, 25, 30, 50, 60] {
                            ui.selectable_value(&mut draft.2, f, format!("{f} Bilder/s"));
                        }
                    });
                    ui.end_row();
                });
                let (w, h) = Self::settings_size(ratios[draft.0], draft.1);
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
                let (w, h) = Self::settings_size(ratios[d.0], d.1);
                self.project.render_width = w;
                self.project.render_height = h;
                self.project.fps = d.2;
            }
            self.show_settings = false;
        } else if cancel {
            self.settings_draft = None;
            self.show_settings = false;
        }
    }

    /// Zielgröße aus Seitenverhältnis und kurzer Seite (wie `MovieSettingsView.Model.size`).
    pub(crate) fn settings_size(ratio: (i64, i64, &str), short: i64) -> (i64, i64) {
        let long = ((short as f64 * ratio.0.max(ratio.1) as f64 / ratio.0.min(ratio.1) as f64 / 2.0).round() as i64) * 2;
        if ratio.0 >= ratio.1 { (long, short) } else { (short, long) }
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
