//! Inspektor, Stand der Mac-Version 1.2: Symbol-Reiterleiste, Kopfzeile mit Kopieren/Einfügen/Zurücksetzen, Abschnitte als
//! Karten mit Aktiv-Punkt und eigenem Zurücksetzen, Reiter Effekte mit den Unterreitern Bildeffekte, Bewegungseffekte und
//! Foto, Reiter Masken (Formen und Verläufe).

use super::inspector::{arr_to_rgba, caption, rgba_to_arr};
use super::*;
use crate::anim::{AnimPreset, Channel};
use crate::app::App;
use crate::masks::{MaskKind, MaskSpec};
use crate::model::*;
use eframe::egui::{Align2, Rect, RichText, Stroke, StrokeKind};
use uuid::Uuid;

/// Reiter: (Symbol, Beschriftung)
pub(super) const TABS: [(&str, &str); 5] = [("\u{E946}", "Eigenschaften"), ("\u{E8AB}", "Blenden"), ("\u{E8D2}", "Titel"), ("\u{E790}", "Effekte"), ("\u{EA3A}", "Masken")];

impl App {
    /// Reiterleiste: Symbol über kurzer Beschriftung, gleich breit.
    pub(crate) fn inspector_tabs(&mut self, ui: &mut Ui) {
        let full_w = ui.available_width();
        let (bar, _) = ui.allocate_exact_size(Vec2::new(full_w, 44.0), Sense::hover());
        ui.painter().rect_filled(bar, 8.0, TRACK);
        let w = (bar.width() - 6.0) / TABS.len() as f32;
        for (i, (glyph, name)) in TABS.iter().enumerate() {
            let r = Rect::from_min_size(bar.min + Vec2::new(3.0 + i as f32 * w, 3.0), Vec2::new(w, 38.0));
            let resp = ui.interact(r, ui.id().with(("insp_tab", i)), Sense::click()).on_hover_text(*name);
            let on = self.inspector_tab == i;
            if on {
                ui.painter().rect_filled(r, 6.0, ACCENT.gamma_multiply(0.85));
            } else if resp.hovered() {
                ui.painter().rect_filled(r, 6.0, Color32::from_white_alpha(14));
            }
            let col = if on { Color32::WHITE } else { Color32::from_gray(170) };
            ui.painter().text(egui::pos2(r.center().x, r.top() + 13.0), Align2::CENTER_CENTER, *glyph, FontId::new(14.0, icons()), col);
            let fs = if w < 62.0 { 9.0 } else { 10.0 };
            ui.painter().text(egui::pos2(r.center().x, r.bottom() - 9.0), Align2::CENTER_CENTER, *name, FontId::proportional(fs), col);
            if resp.clicked() {
                self.inspector_tab = i;
            }
        }
    }

    /// Kopfzeile für Effekte und Masken: Clipname, Animator ein/aus, Kopieren/Einfügen/Zurücksetzen.
    pub(crate) fn clip_header(&mut self, ui: &mut Ui, c: &Clip) {
        let id = c.id;
        ui.horizontal(|ui| {
            let name = match &c.content {
                ClipContent::Title(w) => format!("Titel: {}", w.v.text.lines().next().unwrap_or("")),
                ClipContent::Media(m) => self.project.asset(m.v).map(|a| a.name.clone()).unwrap_or_else(|| "Objekt".into()),
            };
            ui.add(egui::Label::new(RichText::new(name).size(11.5).color(SECONDARY)).truncate());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button(RichText::new("\u{E712}").font(FontId::new(13.0, icons())), |ui| {
                    if ui.button("Effekte kopieren   Strg+Alt+C").clicked() {
                        self.select(id, false);
                        self.copy_effects();
                        ui.close();
                    }
                    if ui.add_enabled(self.fx_clipboard.as_ref().is_some_and(|b| b.fx.is_some()), egui::Button::new("Effekte einfügen   Strg+Alt+V")).clicked() {
                        self.paste_effects();
                        ui.close();
                    }
                    ui.separator();
                    let any = c.fx != Effects::default() || !c.keys.is_empty() || c.transform != Transform2D::default();
                    if ui.add_enabled(any, egui::Button::new("Alle Effekte zurücksetzen")).clicked() {
                        if let Some(cl) = self.project.clip_mut(id) {
                            cl.fx = Effects::default();
                        }
                        ui.close();
                    }
                })
                .response
                .on_hover_text("Effekte kopieren, einfügen, zurücksetzen");
                if self.inspector_tab == 3 {
                    let on = self.show_animator;
                    let r = ui.add(egui::Button::new(RichText::new("◆ Animator").size(11.0).color(if on { ACCENT } else { SECONDARY })).frame(false));
                    if r.on_hover_text(if on { "Keyframe-Animator ausblenden" } else { "Keyframe-Animator einblenden: Effekte über die Zeit verändern" }).clicked() {
                        self.show_animator = !on;
                    }
                }
            });
        });
        ui.add_space(4.0);
    }

    /// Abschnitt als Karte: aufklappbar (Zustand bleibt erhalten), Aktiv-Punkt bei Abweichung von den Standardwerten,
    /// Knopf zum Zurücksetzen der Gruppe.
    pub(crate) fn card(&mut self, ui: &mut Ui, key: &'static str, title: &str, open: bool, reset: Option<(Uuid, FxGroup, bool)>, body: impl FnOnce(&mut App, &mut Ui)) {
        let is_open = *self.section_open.get(key).unwrap_or(&open);
        egui::Frame::new().fill(TRACK.gamma_multiply(0.6)).corner_radius(8.0).inner_margin(egui::Margin::same(9)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let (hr, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::hover());
            let head = ui.interact(hr, ui.id().with(("card", key)), Sense::click());
            let p = ui.painter();
            p.text(hr.left_center() + Vec2::new(4.0, 0.0), Align2::CENTER_CENTER, if is_open { "⏷" } else { "⏵" }, FontId::proportional(10.0), SECONDARY);
            let g = p.layout_no_wrap(title.to_string(), FontId::proportional(13.0), Color32::from_gray(230));
            let tw = g.size().x;
            p.galley(hr.left_center() + Vec2::new(14.0, -g.size().y / 2.0), g, Color32::from_gray(230));
            let active = reset.is_some_and(|r| r.2);
            if active {
                p.circle_filled(hr.left_center() + Vec2::new(22.0 + tw, 0.0), 3.0, ACCENT);
            }
            if head.clicked() {
                self.section_open.insert(key, !is_open);
            }
            if let Some((id, grp, true)) = reset {
                let rr = Rect::from_center_size(hr.right_center() - Vec2::new(8.0, 0.0), Vec2::splat(18.0));
                let rs = ui.interact(rr, ui.id().with(("card_reset", key)), Sense::click()).on_hover_text("Auf Standardwerte zurücksetzen");
                ui.painter().text(rr.center(), Align2::CENTER_CENTER, "\u{E72C}", FontId::new(12.0, icons()), if rs.hovered() { Color32::WHITE } else { SECONDARY });
                if rs.clicked() {
                    if let Some(c) = self.project.clip_mut(id) {
                        c.reset_fx(grp);
                    }
                }
            }
            if is_open {
                ui.add_space(6.0);
                body(self, ui);
            }
        });
        ui.add_space(6.0);
    }

    fn grp(c: &Clip, g: FxGroup) -> Option<(Uuid, FxGroup, bool)> {
        Some((c.id, g, c.is_modified(g)))
    }

    // ---------- Effekte ----------

    pub(crate) fn effects_tab(&mut self, ui: &mut Ui, c: Clip) {
        let id = c.id;
        if c.is_audio() {
            ui.label(RichText::new("Tonbearbeitung").strong().size(13.0));
            caption(ui, "Wirkt sofort beim Abspielen und im Export.");
            let mut fx = c.audio_fx.clone();
            let mut ch = ui.checkbox(&mut fx.wind_filter, "Windfilter (tiefe Frequenzen unter 120 Hz dämpfen)").changed();
            ui.add_space(4.0);
            ui.label(RichText::new("Rauschminderung").size(12.0));
            ch |= ui.add(egui::Slider::new(&mut fx.denoise, 0.0..=1.0).show_value(false)).changed();
            caption(ui, format!("{:.0} %", fx.denoise * 100.0));
            if ch {
                if let Some(cl) = self.project.clip_mut(id) {
                    cl.audio_fx = fx;
                }
            }
            return;
        }
        self.clip_header(ui, &c);
        let is_photo = c.asset_id().and_then(|a| self.project.asset(a)).is_some_and(|a| a.is_image);
        if c.is_title() {
            // Titel: nur Bewegung (Position, Größe, Zoom, Voreinstellungen)
            self.motion_effects(ui, &c);
        } else {
            let subs: Vec<&str> = if is_photo { vec!["Bildeffekte", "Bewegungseffekte", "Foto"] } else { vec!["Bildeffekte", "Bewegungseffekte"] };
            if self.effects_sub >= subs.len() {
                self.effects_sub = 0;
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let w = (ui.available_width() - 2.0 * (subs.len() as f32 - 1.0)) / subs.len() as f32;
                for (i, s) in subs.iter().enumerate() {
                    let on = self.effects_sub == i;
                    if ui.add(egui::Button::new(RichText::new(*s).size(11.5).color(if on { Color32::WHITE } else { Color32::from_gray(190) })).fill(if on { Color32::from_rgb(88, 94, 102) } else { BAR }).min_size(Vec2::new(w, 22.0))).clicked() {
                        self.effects_sub = i;
                    }
                }
            });
            ui.add_space(8.0);
            match self.effects_sub {
                1 => self.motion_effects(ui, &c),
                2 => self.photo_effects(ui, &c),
                _ => self.image_effects(ui, &c),
            }
        }
        let c2 = self.project.clip(id).cloned().unwrap_or(c);
        self.key_inspector(ui, &c2);
    }

    fn image_effects(&mut self, ui: &mut Ui, c: &Clip) {
        let id = c.id;
        // Voreinstellungen (Looks): ändern nur Farbe, Schärfe und Vignette; Ausschnitt, Stabilisierung, Ausrichtung,
        // Greenscreen, LUT, Maske und Foto-Effekte bleiben
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (name, look) in Effects::looks() {
                if ui.add(egui::Button::new(RichText::new(name).size(11.0)).min_size(Vec2::new(0.0, 20.0))).clicked() {
                    if let Some(cl) = self.project.clip_mut(id) {
                        let k = cl.fx.clone();
                        cl.fx = Effects {
                            fill: k.fill,
                            crop_l: k.crop_l,
                            crop_t: k.crop_t,
                            crop_r: k.crop_r,
                            crop_b: k.crop_b,
                            flip_h: k.flip_h,
                            flip_v: k.flip_v,
                            quarter_turns: k.quarter_turns,
                            horizon: k.horizon,
                            chroma: k.chroma,
                            lut_path: k.lut_path,
                            lut_amount: k.lut_amount,
                            stabilize: k.stabilize,
                            stab_no_scale: k.stab_no_scale,
                            mask: k.mask,
                            photo: k.photo,
                            extra: k.extra,
                            ..look
                        };
                    }
                }
            }
        });
        ui.add_space(6.0);
        let cc = c.clone();
        self.card(ui, "fx.farbe", "Farbe & Licht", true, Self::grp(c, FxGroup::Color), |app, ui| {
            for chn in [Channel::Brightness, Channel::Contrast, Channel::Saturation, Channel::Temperature] {
                app.channel_slider(ui, &cc, chn);
            }
        });
        let cc = c.clone();
        self.card(ui, "fx.schaerfe", "Schärfe & Vignette", false, Self::grp(c, FxGroup::Sharp), |app, ui| {
            for chn in [Channel::Sharpness, Channel::Blur, Channel::Vignette] {
                app.channel_slider(ui, &cc, chn);
            }
        });
        let cc = c.clone();
        self.card(ui, "fx.lut", "Filmlook (LUT)", false, Self::grp(c, FxGroup::Lut), |app, ui| {
            ui.horizontal(|ui| {
                caption(ui, "LUT (.cube)");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| match &cc.fx.lut_path {
                    Some(p) => {
                        if ui.small_button("Entfernen").clicked() {
                            if let Some(cl) = app.project.clip_mut(id) {
                                cl.fx.lut_path = None;
                                cl.fx.lut_amount = None;
                            }
                        }
                        ui.label(RichText::new(std::path::Path::new(p).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()).size(12.0));
                    }
                    None => {
                        if ui.button("LUT laden …").clicked() {
                            app.choose_lut(id);
                        }
                    }
                });
            });
            if cc.fx.lut_path.is_some() {
                let mut a = cc.fx.lut_amount.unwrap_or(1.0);
                if slider_row(ui, "LUT-Stärke", &mut a, 0.0..=1.0, pct) {
                    if let Some(cl) = app.project.clip_mut(id) {
                        cl.fx.lut_amount = Some(a);
                    }
                }
            }
        });
        let cc = c.clone();
        self.card(ui, "fx.chroma", "Chroma Key (Greenscreen)", false, Self::grp(c, FxGroup::Chroma), |app, ui| app.chroma_section(ui, &cc));
        let cc = c.clone();
        self.card(ui, "fx.ausschnitt", "Ausschnitt", false, Self::grp(c, FxGroup::Crop), |app, ui| {
            let mut fx = cc.fx.clone();
            let mut ch = ui.checkbox(&mut fx.fill, "Füllen statt Einpassen").changed();
            caption(ui, "Abschneiden");
            ch |= slider_row(ui, "Links", &mut fx.crop_l, 0.0..=0.8, pct);
            ch |= slider_row(ui, "Rechts", &mut fx.crop_r, 0.0..=0.8, pct);
            ch |= slider_row(ui, "Oben", &mut fx.crop_t, 0.0..=0.8, pct);
            ch |= slider_row(ui, "Unten", &mut fx.crop_b, 0.0..=0.8, pct);
            if ch {
                if let Some(cl) = app.project.clip_mut(id) {
                    (cl.fx.fill, cl.fx.crop_l, cl.fx.crop_r, cl.fx.crop_t, cl.fx.crop_b) = (fx.fill, fx.crop_l, fx.crop_r, fx.crop_t, fx.crop_b);
                }
            }
        });
    }

    fn chroma_section(&mut self, ui: &mut Ui, c: &Clip) {
        let id = c.id;
        let mut on = c.fx.chroma.is_some();
        if ui.checkbox(&mut on, "Farbbereich ausblenden").changed() {
            if let Some(cl) = self.project.clip_mut(id) {
                cl.fx.chroma = on.then(ChromaKey::default);
            }
        }
        if let Some(mut k) = c.fx.chroma.clone() {
            let mut ch = false;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                caption(ui, "Farbe");
                let mut col = rgba_to_arr(k.color);
                if ui.color_edit_button_rgba_unmultiplied(&mut col).changed() {
                    k.color = arr_to_rgba(col);
                    ch = true;
                }
                for (n, r, g, b) in [("Grün", 0.1, 0.75, 0.2), ("Blau", 0.1, 0.25, 0.85), ("Schwarz", 0.0, 0.0, 0.0), ("Weiß", 1.0, 1.0, 1.0)] {
                    if ui.small_button(n).clicked() {
                        k.color = Rgba { r, g, b, a: 1.0 };
                        ch = true;
                    }
                }
            });
            ch |= slider_row(ui, "Schwellwert", &mut k.tolerance, 0.01..=0.4, |v| format!("{:.0} %", v * 250.0));
            ch |= slider_row(ui, "Überblendbereich", &mut k.softness, 0.005..=0.3, |v| format!("{:.0} %", v * 333.0));
            ch |= slider_row(ui, "Antispill", &mut k.antispill, 0.0..=1.0, pct);
            caption(ui, "Das Hintergrundvideo muss auf einer Spur oberhalb (kleinere Nummer) des Green-Screen-Videos liegen.");
            if ch {
                if let Some(cl) = self.project.clip_mut(id) {
                    cl.fx.chroma = Some(k);
                }
            }
        }
        caption(ui, "Person freistellen (Mac) ist in der Windows-Version nicht enthalten.");
    }

    fn motion_effects(&mut self, ui: &mut Ui, c: &Clip) {
        let id = c.id;
        self.card(ui, "fx.zoom", "Zoom auf Bereich", true, None, |app, ui| {
            caption(ui, "Zieh im Monitor einen Rahmen auf: Der Film zoomt über die Clipdauer genau auf diesen Bereich – ohne das Bild von Hand zu vergrößern.");
            ui.horizontal(|ui| {
                if ui.button(RichText::new("Hinein auf Bereich …").size(11.5)).clicked() {
                    app.begin_zoom_pick(id, false);
                }
                if ui.button(RichText::new("Heraus aus Bereich …").size(11.5)).clicked() {
                    app.begin_zoom_pick(id, true);
                }
            });
        });
        let cc = c.clone();
        self.card(ui, "fx.position", "Position & Größe", true, Self::grp(c, FxGroup::Motion), |app, ui| {
            for ch in Channel::TRANSFORM {
                app.channel_slider(ui, &cc, ch);
            }
        });
        self.card(ui, "fx.bewegung", "Bewegung (Voreinstellungen)", true, None, |app, ui| {
            caption(ui, "Setzt Keyframes für Position und Größe. Bei Mehrfachauswahl für alle gewählten Clips. Feinschliff im Keyframe-Animator.");
            let w = ((ui.available_width() - 12.0) / 3.0).floor();
            for row in AnimPreset::ALL.chunks(3) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for p in row {
                        let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 44.0), Sense::click());
                        ui.painter().rect_filled(r, 6.0, if resp.hovered() { Color32::from_rgb(52, 56, 62) } else { TRACK });
                        ui.painter().rect_stroke(r, 6.0, Stroke::new(1.0, Color32::from_white_alpha(20)), StrokeKind::Inside);
                        ui.painter().text(egui::pos2(r.center().x, r.top() + 15.0), Align2::CENTER_CENTER, p.glyph(), FontId::new(14.0, icons()), Color32::from_gray(220));
                        ui.painter().text(egui::pos2(r.center().x, r.bottom() - 10.0), Align2::CENTER_CENTER, p.short(), FontId::proportional(10.0), Color32::from_gray(220));
                        if resp.on_hover_text(p.label()).clicked() {
                            app.apply_preset_all(*p, id);
                        }
                    }
                });
                ui.add_space(4.0);
            }
        });
        if c.is_title() {
            return;
        }
        if c.part == ClipPart::Video && c.asset_id().and_then(|a| self.project.asset(a)).is_some_and(|a| !a.is_image) {
            let cc = c.clone();
            self.card(ui, "fx.stab", "Stabilisieren", false, Self::grp(c, FxGroup::Stab), |app, ui| app.stabilize_section(ui, &cc));
        }
        let cc = c.clone();
        self.card(ui, "fx.ausrichtung", "Ausrichtung", false, Self::grp(c, FxGroup::Orient), |app, ui| {
            let mut fx = cc.fx.clone();
            let mut ch = false;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                caption(ui, "Spiegeln / Drehen");
                if ui.selectable_label(fx.flip_h == Some(true), "↔ Waagerecht").on_hover_text("Waagerecht spiegeln").clicked() {
                    fx.flip_h = (fx.flip_h != Some(true)).then_some(true);
                    ch = true;
                }
                if ui.selectable_label(fx.flip_v == Some(true), "↕ Senkrecht").on_hover_text("Senkrecht spiegeln").clicked() {
                    fx.flip_v = (fx.flip_v != Some(true)).then_some(true);
                    ch = true;
                }
                if ui.button("↺ 90°").on_hover_text("90° nach links drehen").clicked() {
                    let q = (fx.quarter_turns.unwrap_or(0) + 3).rem_euclid(4);
                    fx.quarter_turns = (q != 0).then_some(q);
                    ch = true;
                }
                if ui.button("↻ 90°").on_hover_text("90° nach rechts drehen").clicked() {
                    let q = (fx.quarter_turns.unwrap_or(0) + 1).rem_euclid(4);
                    fx.quarter_turns = (q != 0).then_some(q);
                    ch = true;
                }
            });
            let mut hz = fx.horizon.unwrap_or(0.0);
            if slider_row(ui, "Horizont begradigen", &mut hz, -20.0..=20.0, deg) {
                fx.horizon = (hz.abs() >= 0.05).then_some(hz);
                ch = true;
            }
            if fx.horizon.is_some() {
                ui.checkbox(&mut app.show_guides, "Hilfslinien zeigen");
                caption(ui, "Es wird automatisch hineingezoomt, damit keine schwarzen Ränder entstehen.");
            }
            if ch {
                if let Some(cl) = app.project.clip_mut(id) {
                    (cl.fx.flip_h, cl.fx.flip_v, cl.fx.quarter_turns, cl.fx.horizon) = (fx.flip_h, fx.flip_v, fx.quarter_turns, fx.horizon);
                }
            }
        });
    }

    /// Foto: Auto-Verbessern und Rauschentfernung; bei Mehrfachauswahl für alle gewählten Fotos.
    fn photo_effects(&mut self, ui: &mut Ui, c: &Clip) {
        let id = c.id;
        let p = c.fx.photo.clone().unwrap_or_default();
        self.card(ui, "fx.photo", "Verbessern (Licht, Farbe, Rauschen)", true, Self::grp(c, FxGroup::PhotoEnhance), |app, ui| {
            let fmt = |v: f64| if v < 0.01 { "aus".to_string() } else { format!("{:.0} %", v * 100.0) };
            let (mut e, mut d) = (p.enhance.unwrap_or(0.0), p.denoise.unwrap_or(0.0));
            let ce = slider_row(ui, "Auto-Verbessern", &mut e, 0.0..=1.0, fmt);
            let cd = slider_row(ui, "Rauschentfernung", &mut d, 0.0..=1.0, fmt);
            if ce || cd {
                let ids: Vec<Uuid> = app.effect_targets(id).into_iter().filter(|i| app.project.clip(*i).and_then(|c| c.asset_id()).and_then(|a| app.project.asset(a)).is_some_and(|a| a.is_image)).collect();
                for i in ids {
                    if let Some(cl) = app.project.clip_mut(i) {
                        let mut ph = cl.fx.photo.clone().unwrap_or_default();
                        if ce {
                            ph.enhance = (e >= 0.01).then_some(e);
                        }
                        if cd {
                            ph.denoise = (d >= 0.01).then_some(d);
                        }
                        cl.fx.photo = (!ph.is_empty()).then_some(ph);
                    }
                }
            }
            caption(ui, "Belichtung, Kontrast und Farbstich automatisch; Rauschen bei wenig Licht klassisch gemindert (kein ML). Hintergrund-Effekte und die Smart-Kamerafahrt brauchen Apples Motiverkennung und gibt es nur auf dem Mac.");
        });
        if p.bg != "none" {
            caption(ui, format!("Hintergrund-Effekt „{}“ aus der Mac-Version bleibt in der Datei erhalten, wird hier aber nicht gerechnet.", p.bg));
        }
    }

    // ---------- Masken ----------

    pub(crate) fn mask_tab(&mut self, ui: &mut Ui, c: Clip) {
        if c.is_audio() {
            caption(ui, "Masken gelten für Bild, Video und Titel. Ein Tonclip hat kein Bild.");
            return;
        }
        self.clip_header(ui, &c);
        let id = c.id;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Masken").strong().size(13.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(c.fx.mask.is_some(), egui::Button::new(RichText::new("Maske entfernen").size(11.5)).frame(false)).clicked() {
                    if let Some(cl) = self.project.clip_mut(id) {
                        cl.fx.mask = None;
                    }
                }
            });
        });
        for (title, kinds) in [("Formen", &MaskKind::SHAPES[..]), ("Verläufe", &MaskKind::GRADIENTS[..])] {
            caption(ui, title);
            let w = ((ui.available_width() - 12.0) / 3.0).floor();
            for row in kinds.chunks(3) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for k in row {
                        let on = c.fx.mask.as_ref().is_some_and(|m| m.kind == *k);
                        let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 50.0), Sense::click());
                        ui.painter().rect_filled(r, 6.0, if resp.hovered() { Color32::from_rgb(52, 56, 62) } else { TRACK });
                        ui.painter().rect_stroke(r, 6.0, Stroke::new(if on { 2.0 } else { 1.0 }, if on { ACCENT } else { Color32::from_white_alpha(20) }), StrokeKind::Inside);
                        let tr = Rect::from_center_size(egui::pos2(r.center().x, r.top() + 18.0), Vec2::new(40.0, 26.0));
                        let tex = self.mask_thumb(ui.ctx(), *k);
                        ui.painter().image(tex.id(), tr, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                        ui.painter().text(egui::pos2(r.center().x, r.bottom() - 9.0), Align2::CENTER_CENTER, k.label(), FontId::proportional(10.0), Color32::from_gray(220));
                        if resp.clicked() {
                            if let Some(cl) = self.project.clip_mut(id) {
                                let mut d = k.defaults();
                                if let Some(o) = &cl.fx.mask {
                                    d.invert = o.invert;
                                }
                                cl.fx.mask = Some(d);
                            }
                        }
                    }
                });
                ui.add_space(4.0);
            }
        }
        let Some(m) = c.fx.mask.clone() else {
            caption(ui, "Wähle eine Form oder einen Verlauf. Alles außerhalb der Maske wird ausgeblendet.");
            return;
        };
        ui.separator();
        let mut nm = m.clone();
        let mut ch = false;
        let p100 = |v: f64| format!("{:.0} %", v * 100.0);
        if m.kind.is_gradient() {
            ch |= slider_row(ui, "Weichheit", &mut nm.feather, 0.01..=1.0, p100);
            ch |= slider_row(ui, "Richtung", &mut nm.angle, -180.0..=180.0, |v| format!("{v:.0}°"));
            if m.kind == MaskKind::MirrorGradient {
                ch |= slider_row(ui, "Breite", &mut nm.w, 0.02..=2.0, p100);
            }
            if m.kind == MaskKind::RadialGradient {
                ch |= slider_row(ui, "Radius", &mut nm.w, 0.05..=2.0, p100);
                ch |= slider_row(ui, "Mitte X", &mut nm.cx, -0.5..=0.5, p100);
                ch |= slider_row(ui, "Mitte Y", &mut nm.cy, -0.5..=0.5, p100);
            } else {
                ch |= slider_row(ui, "Lage", &mut nm.pos, 0.0..=1.0, p100);
            }
        } else {
            ch |= slider_row(ui, "Weiche Kante", &mut nm.feather, 0.0..=1.0, p100);
            ch |= slider_row(ui, "Breite", &mut nm.w, 0.05..=2.5, p100);
            ch |= slider_row(ui, "Höhe", &mut nm.h, 0.05..=2.5, p100);
            ch |= slider_row(ui, "Position X", &mut nm.cx, -0.8..=0.8, p100);
            ch |= slider_row(ui, "Position Y", &mut nm.cy, -0.5..=0.5, p100);
            ch |= slider_row(ui, "Drehung", &mut nm.rot, -180.0..=180.0, |v| format!("{v:.0}°"));
        }
        ch |= ui.checkbox(&mut nm.invert, "Invertieren").changed();
        if ch {
            if let Some(cl) = self.project.clip_mut(id) {
                cl.fx.mask = Some(nm);
            }
        }
        caption(ui, "Die Maske liegt über dem Bild des Clips und bewegt sich mit Position, Größe und Drehung.");
    }

    /// Kleines Vorschaubild einer Maske für die Kacheln (einmal erzeugt).
    fn mask_thumb(&mut self, ctx: &egui::Context, k: MaskKind) -> egui::TextureHandle {
        if let Some(t) = self.mask_thumbs.get(&k) {
            return t.clone();
        }
        let (w, h) = (80usize, 52usize);
        let spec = MaskSpec { feather: if k.is_gradient() { k.defaults().feather } else { 0.0 }, ..k.defaults() };
        let v = crate::masks::render(&spec, w, h);
        let px: Vec<u8> = v.iter().flat_map(|a| {
            let g = (a * 255.0) as u8;
            [g, g, g, 255]
        }).collect();
        let tex = ctx.load_texture(format!("mask-{k:?}"), egui::ColorImage::from_rgba_unmultiplied([w, h], &px), egui::TextureOptions::LINEAR);
        self.mask_thumbs.insert(k, tex.clone());
        tex
    }
}
