//! Reiter Blenden, Titel und Effekte des Inspektors (InspectorView.swift, neuer Stand): Blendenvorlagen als Kacheln,
//! Titeleditor und Effekte in aufklappbaren Bereichen.

use super::inspector::{arr_to_rgba, caption, rgba_to_arr};
use super::*;
use crate::anim::Channel;
use crate::app::App;
use crate::fx_edit::{TitlePreset, TransitionDrag};
use crate::model::*;
use eframe::egui::{Rect, RichText};
use uuid::Uuid;

const A_COLOR: Color32 = Color32::from_rgb(56, 107, 184);
const B_COLOR: Color32 = Color32::from_rgb(235, 148, 56);

/// Skizze einer Blende (A blau, B orange) bei halbem Fortschritt.
pub fn draw_transition_sketch(p: &egui::Painter, r: Rect, kind: &str) {
    let c = r.center();
    p.rect_filled(r, 3.0, A_COLOR);
    match kind {
        "crossfade" => {
            p.rect_filled(r, 3.0, B_COLOR.gamma_multiply(0.5));
        }
        "dip" => {
            p.rect_filled(Rect::from_min_max(r.min, egui::pos2(c.x, r.max.y)), 3.0, A_COLOR.gamma_multiply(0.6));
            p.rect_filled(Rect::from_min_max(egui::pos2(c.x, r.min.y), r.max), 3.0, B_COLOR.gamma_multiply(0.6));
            p.rect_filled(Rect::from_center_size(c, Vec2::new(r.width() * 0.3, r.height())), 0.0, Color32::BLACK);
        }
        "wipe" => {
            p.rect_filled(Rect::from_min_max(r.min, egui::pos2(c.x, r.max.y)), 3.0, B_COLOR);
        }
        "wipeDown" => {
            p.rect_filled(Rect::from_min_max(r.min, egui::pos2(r.max.x, c.y)), 3.0, B_COLOR);
        }
        "slide" => {
            p.rect_filled(Rect::from_min_max(egui::pos2(c.x, r.min.y), r.max), 3.0, B_COLOR);
            p.line_segment([egui::pos2(c.x, r.min.y), egui::pos2(c.x, r.max.y)], egui::Stroke::new(1.0, Color32::from_black_alpha(160)));
        }
        "zoom" => {
            p.rect_filled(Rect::from_center_size(c, r.size() * 0.62), 2.0, B_COLOR.gamma_multiply(0.85));
        }
        "iris" => {
            p.circle_filled(c, r.height().min(r.width()) * 0.36, B_COLOR);
        }
        "flash" => {
            p.rect_filled(r, 3.0, Color32::from_white_alpha(190));
        }
        _ => {}
    }
    p.rect_stroke(r, 3.0, egui::Stroke::new(1.0, Color32::from_white_alpha(30)), egui::StrokeKind::Inside);
}

fn tile(ui: &mut Ui, w: f32, h: f32, selected: bool, sense: Sense) -> (Rect, Response) {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), sense);
    let fill = if selected { ACCENT.gamma_multiply(0.35) } else if resp.hovered() { Color32::from_rgb(52, 56, 62) } else { TRACK };
    ui.painter().rect_filled(rect, 6.0, fill);
    ui.painter().rect_stroke(rect, 6.0, egui::Stroke::new(1.0, if selected { ACCENT } else { Color32::from_white_alpha(20) }), egui::StrokeKind::Inside);
    (rect, resp)
}

impl App {
    /// Aufklappbarer Bereich; der Zustand bleibt beim Wechseln von Clips und Reitern erhalten.
    fn section(&mut self, ui: &mut Ui, key: &str, title: &str, open: bool, body: impl FnOnce(&mut App, &mut Ui)) {
        egui::CollapsingHeader::new(RichText::new(title).strong().size(13.0)).id_salt(key).default_open(open).show(ui, |ui| {
            ui.add_space(4.0);
            body(self, ui);
            ui.add_space(4.0);
        });
    }

    // ---------- Blenden ----------

    pub(crate) fn blend_tab(&mut self, ui: &mut Ui, c: Clip) {
        let id = c.id;
        if !c.is_audio() {
            let junction = self.project.junction_of(id);
            let kind = junction.and_then(|j| self.junction_kind(&j));
            ui.label(RichText::new("Übergänge").strong().size(13.0));
            let tile_w = ((ui.available_width() - 12.0) / 3.0).floor();
            for row in App::transition_templates().chunks(3) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for k in row {
                        let sel = kind.as_deref() == Some(*k);
                        let (rect, resp) = tile(ui, tile_w, 62.0, sel, Sense::click_and_drag());
                        draw_transition_sketch(ui.painter(), Rect::from_min_size(rect.min + Vec2::new(5.0, 5.0), Vec2::new(rect.width() - 10.0, 34.0)), k);
                        ui.painter().text(egui::pos2(rect.center().x, rect.bottom() - 11.0), egui::Align2::CENTER_CENTER, crate::ops::transition_label(k), FontId::proportional(10.5), Color32::from_gray(225));
                        if resp.clicked() {
                            if let Some(j) = junction {
                                self.apply_transition(j.a, j.b, Some(k), None);
                            }
                        }
                        if resp.drag_started() {
                            resp.dnd_set_drag_payload(TransitionDrag(k.to_string()));
                        }
                        resp.on_hover_text("Auf den Übergang zweier Clips ziehen (A/B-Symbol) – oder anklicken, um sie am gewählten Übergang anzuwenden");
                    }
                });
                ui.add_space(2.0);
            }
            match junction {
                Some(j) => {
                    ui.horizontal(|ui| {
                        caption(ui, kind.as_deref().map(|k| format!("Blende: {}", crate::ops::transition_label(k))).unwrap_or_else(|| "Harter Schnitt".into()));
                        if kind.is_some() {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("Entfernen").clicked() {
                                    self.apply_transition(j.a, j.b, None, None);
                                }
                                let cur = (j.overlap.clamp(0.5, 4.0) * 2.0).round() / 2.0;
                                egui::ComboBox::from_id_salt("trans_dur").width(64.0).selected_text(format!("{cur} s")).show_ui(ui, |ui| {
                                    for d in [0.5, 1.0, 1.5, 2.0] {
                                        if ui.selectable_label((cur - d).abs() < 1e-9, format!("{d} s")).clicked() {
                                            self.apply_transition(j.a, j.b, kind.as_deref(), Some(d));
                                        }
                                    }
                                });
                            });
                        }
                    });
                }
                None => caption(ui, "Dieser Clip steht allein oder am Anfang. Blenden gibt es zwischen zwei aneinanderstoßenden Clips oder Fotos (A/B-Symbol auf der Spur); hier kannst du ein- und ausblenden."),
            }
            ui.separator();
        }
        ui.label(RichText::new("Ein- und Ausblenden").strong().size(13.0));
        let max = (c.duration / 2.0).min(3.0).max(0.1);
        let (mut fi, mut fo) = (c.fade_in, c.fade_out);
        if slider_row(ui, "Einblenden", &mut fi, 0.0..=max, secs) {
            if let Some(cl) = self.project.clip_mut(id) {
                cl.fade_in = fi;
            }
        }
        ui.add_space(6.0);
        if slider_row(ui, "Ausblenden", &mut fo, 0.0..=max, secs) {
            if let Some(cl) = self.project.clip_mut(id) {
                cl.fade_out = fo;
            }
        }
    }

    // ---------- Titel ----------

    pub(crate) fn title_tab(&mut self, ui: &mut Ui, c: Option<Clip>) {
        let Some(c) = c.filter(|c| c.is_title()) else {
            ui.label(RichText::new("Neuer Titel").strong().size(13.0));
            ui.add_space(4.0);
            let w = ((ui.available_width() - 12.0) / 3.0).floor();
            for row in [[("Titel", TitlePreset::Title), ("Bauchbinde", TitlePreset::LowerThird), ("Abspann", TitlePreset::Credits)], [("Laufschrift", TitlePreset::Ticker), ("Timecode", TitlePreset::Timecode), ("Untertitel", TitlePreset::Subtitle)]] {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for (name, preset) in row {
                        let (rect, resp) = tile(ui, w, 34.0, false, Sense::click());
                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, name, FontId::proportional(12.0), Color32::from_gray(225));
                        if resp.clicked() {
                            self.add_title(preset);
                        }
                    }
                });
                ui.add_space(2.0);
            }
            if !self.title_templates.is_empty() {
                self.section(ui, "titel.vorlagen", "Meine Vorlagen", true, |app, ui| {
                    for t in app.title_templates.clone() {
                        ui.horizontal(|ui| {
                            if ui.button(&t.name).clicked() {
                                app.add_title_from_template(&t);
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("Löschen").on_hover_text("Vorlage löschen").clicked() {
                                    app.delete_title_template(t.id);
                                }
                            });
                        });
                    }
                });
            }
            ui.add_space(6.0);
            caption(ui, "Fügt den Titel an der Abspielposition ein. Danach hier Text, Schrift und Farben bearbeiten; die Position ziehst du im Vorschaumonitor.");
            return;
        };
        let ClipContent::Title(w) = &c.content else { return };
        let id = c.id;
        let mut s = w.v.clone();
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Titel bearbeiten").strong().size(13.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("Neuer Titel", |ui| {
                    for (name, preset) in [("Titel", TitlePreset::Title), ("Bauchbinde", TitlePreset::LowerThird), ("Abspann", TitlePreset::Credits), ("Laufschrift", TitlePreset::Ticker), ("Timecode", TitlePreset::Timecode), ("Untertitel", TitlePreset::Subtitle)] {
                        if ui.button(name).clicked() {
                            self.add_title(preset);
                            ui.close();
                        }
                    }
                    if !self.title_templates.is_empty() {
                        ui.separator();
                        for t in self.title_templates.clone() {
                            if ui.button(&t.name).clicked() {
                                self.add_title_from_template(&t);
                                ui.close();
                            }
                        }
                    }
                });
            });
        });
        caption(ui, "Text (erste Zeile fett, weitere kleiner)");
        changed |= ui.add(egui::TextEdit::multiline(&mut s.text).desired_rows(4).desired_width(f32::INFINITY)).changed();
        caption(ui, "Platzhalter: {datum} {zeit} {timecode} {objektzeit}");
        ui.add_space(4.0);
        self.section(ui, "titel.schrift", "Schrift & Farbe", true, |_, ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Schriftart").size(12.0));
                let cur = s.font.clone().unwrap_or_else(|| "System".into());
                egui::ComboBox::from_id_salt("title_font").selected_text(cur.clone()).show_ui(ui, |ui| {
                    for f in crate::render::installed_fonts() {
                        if ui.selectable_label(cur == f, f).clicked() {
                            s.font = if f == "System" { None } else { Some(f.to_string()) };
                            changed = true;
                        }
                    }
                });
            });
            changed |= slider_row(ui, "Größe", &mut s.font_size, 0.02..=0.25, pct1);
            ui.horizontal(|ui| {
                let mut bold = s.bold.unwrap_or(true);
                if ui.toggle_value(&mut bold, RichText::new("F").strong()).on_hover_text("Fett").changed() {
                    s.bold = Some(bold);
                    changed = true;
                }
                let mut it = s.italic == Some(true);
                if ui.toggle_value(&mut it, RichText::new("K").italics()).on_hover_text("Kursiv").changed() {
                    s.italic = it.then_some(true);
                    changed = true;
                }
                let mut un = s.underline == Some(true);
                if ui.toggle_value(&mut un, RichText::new("U").underline()).on_hover_text("Unterstrichen").changed() {
                    s.underline = un.then_some(true);
                    changed = true;
                }
                ui.add_space(8.0);
                let mut al = if s.left_aligned { 0 } else if s.right_aligned == Some(true) { 2 } else { 1 };
                let before = al;
                for (i, n) in ["Links", "Mitte", "Rechts"].iter().enumerate() {
                    ui.selectable_value(&mut al, i, *n);
                }
                if al != before {
                    s.left_aligned = al == 0;
                    s.right_aligned = (al == 2).then_some(true);
                    changed = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Schriftfarbe").size(12.0));
                let mut col = rgba_to_arr(s.color);
                if ui.color_edit_button_rgba_unmultiplied(&mut col).changed() {
                    s.color = arr_to_rgba(col);
                    changed = true;
                }
            });
            let mut ls = s.line_spacing.unwrap_or(0.0);
            if slider_row(ui, "Zeilenabstand", &mut ls, -0.3..=1.0, pct) {
                s.line_spacing = Some(ls);
                changed = true;
            }
            let mut kn = s.kerning.unwrap_or(0.0);
            if slider_row(ui, "Laufweite", &mut kn, -0.1..=0.5, pct) {
                s.kerning = Some(kn);
                changed = true;
            }
        });
        self.section(ui, "titel.effekte", "Hintergrund, Schatten & Rand", false, |_, ui| {
            ui.horizontal(|ui| {
                changed |= ui.checkbox(&mut s.background, "Hintergrund").changed();
                if s.background {
                    let mut col = rgba_to_arr(s.background_color);
                    if ui.color_edit_button_rgba_unmultiplied(&mut col).changed() {
                        s.background_color = arr_to_rgba(col);
                        changed = true;
                    }
                }
            });
            let mut sh = s.shadow.unwrap_or(!s.background);
            if ui.checkbox(&mut sh, "Schatten").changed() {
                s.shadow = Some(sh);
                changed = true;
            }
            if sh {
                let mut tr = 1.0 - s.shadow_alpha.unwrap_or(0.7);
                if slider_row(ui, "Transparenz", &mut tr, 0.0..=1.0, pct) {
                    s.shadow_alpha = Some(1.0 - tr);
                    changed = true;
                }
                let mut bl = s.shadow_blur.unwrap_or(0.1);
                if slider_row(ui, "Unschärfe", &mut bl, 0.0..=0.5, pct) {
                    s.shadow_blur = Some(bl);
                    changed = true;
                }
            }
            let mut ol = s.outline == Some(true);
            if ui.checkbox(&mut ol, "Umrandung").changed() {
                s.outline = ol.then_some(true);
                changed = true;
            }
            if ol {
                let mut wd = s.outline_width.unwrap_or(4.0);
                if slider_row(ui, "Breite", &mut wd, 1.0..=12.0, |v| format!("{v:.0}")) {
                    s.outline_width = Some(wd);
                    changed = true;
                }
                let mut fill = s.outline_hollow != Some(true);
                if ui.checkbox(&mut fill, "Ausgefüllt").changed() {
                    s.outline_hollow = (!fill).then_some(true);
                    changed = true;
                }
            }
        });
        self.section(ui, "titel.bewegung", "Bewegung", false, |_, ui| {
            let mut cr = s.credits == Some(true);
            if ui.checkbox(&mut cr, "Rollender Abspann").changed() {
                s.credits = cr.then_some(true);
                if cr {
                    s.ticker = None;
                }
                changed = true;
            }
            let mut tk = s.ticker == Some(true);
            if ui.checkbox(&mut tk, "Laufschrift (von rechts nach links)").changed() {
                s.ticker = tk.then_some(true);
                if tk {
                    s.credits = None;
                }
                changed = true;
            }
            if !cr && !tk {
                const ANIMS: [(&str, &str); 6] = [("none", "Keine"), ("flyUp", "Von unten"), ("flyLeft", "Von links"), ("flyRight", "Von rechts"), ("zoom", "Zoom"), ("typewriter", "Schreibmaschine")];
                let label = |k: &Option<String>| ANIMS.iter().find(|a| Some(a.0) == k.as_deref()).map(|a| a.1).unwrap_or("Keine");
                let mut pick = |ui: &mut Ui, id: &str, title: &str, cur: &mut Option<String>, skip_type: bool| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(title).size(12.0));
                        egui::ComboBox::from_id_salt(id).selected_text(label(cur)).show_ui(ui, |ui| {
                            for (k, n) in ANIMS {
                                if skip_type && k == "typewriter" {
                                    continue;
                                }
                                if ui.selectable_label(cur.as_deref().unwrap_or("none") == k, n).clicked() {
                                    *cur = (k != "none").then(|| k.to_string());
                                    changed = true;
                                }
                            }
                        });
                    });
                };
                let (mut ai, mut ao) = (s.anim_in.clone(), s.anim_out.clone());
                pick(ui, "anim_in", "Einlaufen", &mut ai, false);
                pick(ui, "anim_out", "Auslaufen", &mut ao, true);
                s.anim_in = ai;
                s.anim_out = ao;
                if s.anim_in.is_some() || s.anim_out.is_some() {
                    let mut d = s.anim_dur.unwrap_or(0.6);
                    if slider_row(ui, "Dauer", &mut d, 0.2..=2.0, secs) {
                        s.anim_dur = Some(d);
                        changed = true;
                    }
                }
            }
        });
        let clip_for_template = c.clone();
        self.section(ui, "titel.vorlagen2", "Vorlagen", false, |app, ui| {
            if ui.button("Als Vorlage sichern …").clicked() {
                app.template_prompt = Some((clip_for_template.id, "Meine Vorlage".into()));
            }
            if !app.title_templates.is_empty() {
                caption(ui, "Auf diesen Titel anwenden (nur der Text bleibt)");
                for t in app.title_templates.clone() {
                    ui.horizontal(|ui| {
                        if ui.button(&t.name).clicked() {
                            app.apply_title_template(&t, id);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Löschen").clicked() {
                                app.delete_title_template(t.id);
                            }
                        });
                    });
                }
            }
        });
        if changed {
            if let Some(cl) = self.project.clip_mut(id) {
                cl.content = ClipContent::Title(Wrap { v: s });
            }
        }
    }

    /// Eingabe des Namens beim Sichern einer Titelvorlage.
    pub(crate) fn template_prompt_window(&mut self, ctx: &egui::Context) {
        let Some((id, mut name)) = self.template_prompt.clone() else { return };
        let mut done: Option<bool> = None;
        egui::Window::new("Name der Titelvorlage").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(260.0));
            r.request_focus();
            ui.add_space(6.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Sichern").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = Some(true);
                }
                if ui.button("Abbrechen").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    done = Some(false);
                }
            });
        });
        match done {
            Some(true) => {
                if !name.trim().is_empty() {
                    if let Some(c) = self.project.clip(id).cloned() {
                        self.save_title_template(&c, name.trim().to_string());
                    }
                }
                self.template_prompt = None;
            }
            Some(false) => self.template_prompt = None,
            None => self.template_prompt = Some((id, name)),
        }
    }

}

/// Ersatz für `Uuid`-Import-Warnung bei reinen Typannotationen.
#[allow(dead_code)]
fn _id(_: Uuid) {}
