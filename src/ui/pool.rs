//! Medienpool („Import“): Kacheln mit Vorschaubild, Dauer und Namen; Doppelklick legt in die Timeline,
//! Ziehen auf die Timeline legt an der Zielposition ab.

use super::*;
use crate::app::App;
use crate::model::MediaAsset;
use crate::video;
use eframe::egui::{Align2, Rect, RichText, Stroke, StrokeKind};
use uuid::Uuid;

const TILE_W: f32 = 130.0;
const TILE_H: f32 = 80.0;

impl App {
    /// Vorschaubild eines Mediums. Wird im Hintergrund erzeugt (HEVC-Videos brauchen dafür einen Moment);
    /// bis es fertig ist, liefert die Funktion `None`.
    pub(crate) fn thumb(&mut self, ctx: &egui::Context, a: &MediaAsset) -> Option<egui::TextureHandle> {
        if let Some(t) = self.thumbs.get(&a.id) {
            return t.clone();
        }
        if let Some(rx) = self.thumb_jobs.get(&a.id) {
            use std::sync::mpsc::TryRecvError;
            return match rx.try_recv() {
                Ok(res) => {
                    self.thumb_jobs.remove(&a.id);
                    let tex = res.map(|(w, h, px)| {
                        let ci = egui::ColorImage::from_rgba_unmultiplied([w, h], &px);
                        ctx.load_texture(format!("thumb-{}", a.id), ci, egui::TextureOptions::LINEAR)
                    });
                    self.thumbs.insert(a.id, tex.clone());
                    tex
                }
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    self.thumb_jobs.remove(&a.id);
                    self.thumbs.insert(a.id, None);
                    None
                }
            };
        }
        if self.thumb_jobs.len() >= 2 {
            return None; // höchstens zwei gleichzeitig; die Kachel fragt im nächsten Bild erneut
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let asset = a.clone();
        let wake = ctx.clone();
        std::thread::spawn(move || {
            let res = (|| {
                let rgba = if asset.is_image {
                    image::open(&asset.path).ok()?.thumbnail(260, 160).to_rgba8()
                } else if asset.has_video {
                    let d = video::Decoder::open(std::path::Path::new(&asset.path)).ok()?;
                    let f = d.last.as_ref()?;
                    let img = image::RgbaImage::from_raw(f.width as u32, f.height as u32, f.rgba.clone())?;
                    image::imageops::thumbnail(&img, 260, 160)
                } else {
                    return None;
                };
                Some((rgba.width() as usize, rgba.height() as usize, rgba.into_raw()))
            })();
            let _ = tx.send(res);
            wake.request_repaint();
        });
        self.thumb_jobs.insert(a.id, rx);
        None
    }

    pub(crate) fn pool_panel(&mut self, ui: &mut Ui) {
        use crate::media::{PoolFilter, PoolSort, PoolView};
        let panel = ui.max_rect();
        self.pool.hover = ui.ctx().pointer_hover_pos().is_some_and(|p| panel.contains(p)) && !ui.ctx().egui_wants_keyboard_input();
        // Kopfzeile
        let bar = egui::Frame::new().fill(BAR).inner_margin(egui::Margin::symmetric(18, 0));
        bar.show(ui, |ui| {
            ui.set_height(30.0);
            ui.horizontal_centered(|ui| {
                ui.label(RichText::new("Import").strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if tool_button(ui, I_ADD, "Importieren", "Videos, Bilder oder Ton importieren (Strg+I)", true).clicked() {
                        self.import_dialog();
                    }
                });
            });
        });
        // Suche, Filter, Sortierung, Ansicht
        let mut prefs_changed = false;
        egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::symmetric(10, 3)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.add(egui::TextEdit::singleline(&mut self.pool.search).hint_text("Suchen").desired_width((ui.available_width() - 150.0).clamp(60.0, 180.0)));
                let f = self.pool.filter;
                egui::ComboBox::from_id_salt("pool_filter").width(28.0).selected_text(RichText::new("\u{E71C}").font(FontId::new(12.0, icons())).color(if f == PoolFilter::All { SECONDARY } else { ACCENT })).show_ui(ui, |ui| {
                    for x in PoolFilter::ALL {
                        ui.selectable_value(&mut self.pool.filter, x, x.label());
                    }
                }).response.on_hover_text(format!("Filter: {}", self.pool.filter.label()));
                egui::ComboBox::from_id_salt("pool_sort").width(28.0).selected_text(RichText::new("\u{E8CB}").font(FontId::new(12.0, icons())).color(SECONDARY)).show_ui(ui, |ui| {
                    for s in PoolSort::ALL {
                        if ui.selectable_value(&mut self.pool.sort, s, s.label()).clicked() {
                            prefs_changed = true;
                        }
                    }
                    ui.separator();
                    if ui.radio(self.pool.ascending, "Aufsteigend").clicked() {
                        self.pool.ascending = true;
                        prefs_changed = true;
                    }
                    if ui.radio(!self.pool.ascending, "Absteigend").clicked() {
                        self.pool.ascending = false;
                        prefs_changed = true;
                    }
                }).response.on_hover_text(format!("Sortieren: {}", self.pool.sort.label()));
                for (v, g) in [(PoolView::Large, "\u{E80A}"), (PoolView::Small, "\u{E8A9}"), (PoolView::List, "\u{E8FD}"), (PoolView::Details, "\u{E8EF}")] {
                    if icon_button_sized(ui, g, v.label(), self.pool.view == v, 12.0, Vec2::new(22.0, 20.0), true).clicked() {
                        self.pool.view = v;
                        prefs_changed = true;
                    }
                }
            });
        });
        if prefs_changed {
            self.save_prefs();
        }

        let ctx = ui.ctx().clone();
        let mut add: Option<Uuid> = None;
        let mut remove: Option<std::collections::HashSet<Uuid>> = None;
        let mut view: Option<Uuid> = None;
        let mut append: Vec<Uuid> = vec![];
        let mut clicked: Option<(Uuid, bool, bool)> = None;
        let mut drag_start: Option<Uuid> = None;
        let mut marquee_hit: Option<Vec<Uuid>> = None;
        self.pool_tiles.clear();
        self.pool.frames.clear();
        let assets = self.pool_assets();
        let pv = self.pool.view;
        let (tw, th, name_h) = match pv {
            PoolView::Large => (TILE_W, TILE_H, 20.0),
            PoolView::Small => (86.0, 52.0, 16.0),
            _ => (0.0, 0.0, 0.0),
        };
        egui::ScrollArea::vertical().id_salt("pool_scroll").auto_shrink([false, false]).show(ui, |ui| {
            // Hintergrund für den Auswahlrahmen und Klick ins Leere
            let bg_rect = Rect::from_min_size(ui.cursor().min, Vec2::new(ui.available_width(), ui.available_height().max(40.0)));
            let bg = ui.interact(bg_rect, ui.id().with("pool_bg"), Sense::click_and_drag());
            if self.project.assets.is_empty() {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("Videos hierher ziehen oder „Importieren“ wählen").color(SECONDARY));
                });
            } else if assets.is_empty() {
                ui.add_space(30.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("Keine Medien passen zu Suche und Filter").color(SECONDARY));
                });
            }
            let mut items: Vec<(MediaAsset, egui::Response, Rect)> = vec![];
            match pv {
                PoolView::Large | PoolView::Small => {
                    let avail = ui.available_width() - 24.0;
                    let cols = ((avail + 10.0) / (tw + 10.0)).floor().max(1.0) as usize;
                    ui.add_space(12.0);
                    for chunk in assets.chunks(cols) {
                        ui.horizontal(|ui| {
                            ui.add_space(12.0);
                            ui.spacing_mut().item_spacing.x = 10.0;
                            for a in chunk {
                                let (rect, resp) = ui.allocate_exact_size(Vec2::new(tw, th + name_h), Sense::click_and_drag());
                                let img = Rect::from_min_size(rect.min, Vec2::new(tw, th));
                                let selected = self.pool.selection.contains(&a.id);
                                if selected {
                                    ui.painter().rect_filled(rect.expand(3.0), 6.0, ACCENT.gamma_multiply(0.25));
                                }
                                ui.painter().rect_filled(img, 4.0, Color32::BLACK);
                                let thumb = self.thumb(&ctx, a);
                                if thumb.is_none() && !self.thumbs.contains_key(&a.id) {
                                    ctx.request_repaint_after(std::time::Duration::from_millis(150));
                                }
                                if let Some(t) = thumb {
                                    let s = t.size_vec2();
                                    let k = (tw / s.x).max(th / s.y);
                                    let (uv_w, uv_h) = (tw / (s.x * k), th / (s.y * k));
                                    let uv = Rect::from_min_max(egui::pos2((1.0 - uv_w) / 2.0, (1.0 - uv_h) / 2.0), egui::pos2((1.0 + uv_w) / 2.0, (1.0 + uv_h) / 2.0));
                                    ui.painter().image(t.id(), img, uv, Color32::WHITE);
                                } else {
                                    let g = if a.has_video || a.is_image { I_VIEW } else { I_VOLUME };
                                    ui.painter().text(img.center(), Align2::CENTER_CENTER, g, FontId::new(if pv == PoolView::Large { 22.0 } else { 16.0 }, icons()), Color32::from_gray(150));
                                }
                                let gal = ui.painter().layout_no_wrap(clock(a.duration), FontId::proportional(if pv == PoolView::Large { 10.0 } else { 9.0 }), Color32::WHITE);
                                let tag = Rect::from_min_size(egui::pos2(img.right() - gal.size().x - 9.0, img.bottom() - gal.size().y - 7.0), gal.size() + Vec2::new(6.0, 4.0));
                                ui.painter().rect_filled(tag, 2.0, Color32::from_black_alpha(160));
                                ui.painter().galley(tag.min + Vec2::new(3.0, 2.0), gal, Color32::WHITE);
                                if selected {
                                    ui.painter().rect_stroke(img, 4.0, Stroke::new(2.0, ACCENT), StrokeKind::Inside);
                                } else if resp.hovered() || resp.dragged() {
                                    ui.painter().rect_stroke(img, 4.0, Stroke::new(1.5, ACCENT.gamma_multiply(0.7)), StrokeKind::Inside);
                                }
                                ui.painter().with_clip_rect(Rect::from_min_size(rect.min + Vec2::new(0.0, th), Vec2::new(tw, name_h))).text(
                                    egui::pos2(rect.left() + 1.0, rect.top() + th + 3.0),
                                    Align2::LEFT_TOP,
                                    &a.name,
                                    FontId::proportional(if pv == PoolView::Large { 11.5 } else { 10.0 }),
                                    Color32::from_gray(215),
                                );
                                self.pool_tiles.push((a.id, img));
                                items.push((a.clone(), resp, rect));
                            }
                        });
                        ui.add_space(if pv == PoolView::Large { 12.0 } else { 8.0 });
                    }
                }
                PoolView::List | PoolView::Details => {
                    ui.add_space(4.0);
                    let w = ui.available_width() - 8.0;
                    let cols: [(f32, &str, PoolSort); 4] = [(0.18, "Typ", PoolSort::Type), (0.16, "Dauer", PoolSort::Duration), (0.16, "Größe", PoolSort::Size), (0.20, "Geändert", PoolSort::Date)];
                    if pv == PoolView::Details {
                        // Spaltenköpfe (Klick sortiert, erneuter Klick dreht die Richtung)
                        let (hr, _) = ui.allocate_exact_size(Vec2::new(w, 18.0), Sense::hover());
                        let mut x = hr.left() + 22.0;
                        let name_w = w * 0.30 - 22.0;
                        let heads: Vec<(f32, &str, PoolSort)> = std::iter::once((name_w, "Name", PoolSort::Name)).chain(cols.iter().map(|c| (w * c.0, c.1, c.2))).collect();
                        for (cw, label, s) in heads {
                            let r = Rect::from_min_size(egui::pos2(x, hr.top()), Vec2::new(cw, 18.0));
                            let on = self.pool.sort == s;
                            let txt = if on { format!("{label} {}", if self.pool.ascending { "▲" } else { "▼" }) } else { label.to_string() };
                            ui.painter().text(r.left_center(), Align2::LEFT_CENTER, txt, FontId::proportional(10.5), if on { ACCENT } else { SECONDARY });
                            if ui.interact(r, ui.id().with(("poolhead", label)), Sense::click()).clicked() {
                                if on {
                                    self.pool.ascending = !self.pool.ascending;
                                } else {
                                    self.pool.sort = s;
                                    self.pool.ascending = true;
                                }
                                prefs_changed = true;
                            }
                            x += cw;
                        }
                    }
                    for a in &assets {
                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 20.0), Sense::click_and_drag());
                        let selected = self.pool.selection.contains(&a.id);
                        if selected {
                            ui.painter().rect_filled(rect, 3.0, ACCENT.gamma_multiply(0.35));
                        } else if resp.hovered() {
                            ui.painter().rect_filled(rect, 3.0, Color32::from_white_alpha(12));
                        }
                        let g = if a.is_image { "\u{EB9F}" } else if a.has_video { "\u{E714}" } else { "\u{E8D6}" };
                        ui.painter().text(egui::pos2(rect.left() + 10.0, rect.center().y), Align2::CENTER_CENTER, g, FontId::new(12.0, icons()), SECONDARY);
                        let name_w = if pv == PoolView::Details { w * 0.30 - 22.0 } else { w - 80.0 };
                        ui.painter().with_clip_rect(Rect::from_min_size(egui::pos2(rect.left() + 22.0, rect.top()), Vec2::new(name_w - 4.0, 20.0))).text(egui::pos2(rect.left() + 22.0, rect.center().y), Align2::LEFT_CENTER, &a.name, FontId::proportional(11.5), Color32::from_gray(220));
                        if pv == PoolView::Details {
                            let (size, date) = self.pool.file_info(a);
                            let vals = [crate::media::kind_label(a).to_string(), if a.is_image { "–".into() } else { clock(a.duration) }, crate::media::format_bytes(size), date_text(date)];
                            let mut x = rect.left() + 22.0 + name_w;
                            for (c, v) in cols.iter().zip(vals) {
                                ui.painter().text(egui::pos2(x, rect.center().y), Align2::LEFT_CENTER, v, FontId::proportional(10.5), SECONDARY);
                                x += w * c.0;
                            }
                        } else {
                            ui.painter().text(egui::pos2(rect.right() - 4.0, rect.center().y), Align2::RIGHT_CENTER, clock(a.duration), FontId::proportional(10.5), SECONDARY);
                        }
                        self.pool_tiles.push((a.id, rect));
                        items.push((a.clone(), resp, rect));
                    }
                }
            }
            for (a, resp, rect) in items {
                self.pool.frames.push((a.id, rect));
                if resp.clicked() {
                    let (ctrl, shift) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
                    clicked = Some((a.id, ctrl, shift));
                }
                if resp.drag_started() {
                    drag_start = Some(a.id);
                    resp.dnd_set_drag_payload(crate::source::DragAsset { id: a.id, range: None });
                }
                if resp.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                }
                if resp.double_clicked() {
                    append = vec![a.id];
                }
                if Some(a.id) == self.pool.focus && self.pool.hover {
                    ui.painter().rect_stroke(rect.expand(1.0), 4.0, Stroke::new(1.0, Color32::from_white_alpha(90)), StrokeKind::Outside);
                }
                let n_sel = self.pool.selection.len();
                let multi = n_sel > 1 && self.pool.selection.contains(&a.id);
                resp.on_hover_text(format!(
                    "{}\n{}\nKlick: wählen · Strg/Umschalt: mehrere · Doppelklick: hinten anhängen · Ziehen: auf die Timeline",
                    a.name,
                    if a.is_image { format!("Bild {}×{}", a.width as i64, a.height as i64) } else if a.has_video { format!("Video {}×{}, {:.0} fps", a.width as i64, a.height as i64, a.fps) } else { "Ton".to_string() }
                ))
                .context_menu(|ui| {
                    if !multi && ui.button("Im Monitor ansehen (Bereich wählen)").clicked() {
                        view = Some(a.id);
                        ui.close();
                    }
                    if ui.button(if multi { format!("{n_sel} Medien hinten anhängen") } else { "Hinten anhängen".into() }).clicked() {
                        append = if multi { self.pool_selected_assets().iter().map(|x| x.id).collect() } else { vec![a.id] };
                        ui.close();
                    }
                    if !multi && ui.button("An Abspielposition einfügen").clicked() {
                        add = Some(a.id);
                        ui.close();
                    }
                    if ui.button(if multi { format!("{n_sel} Medien aus dem Pool entfernen") } else { "Aus dem Pool entfernen".into() }).clicked() {
                        remove = Some(if multi { self.pool.selection.clone() } else { std::collections::HashSet::from([a.id]) });
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Alle auswählen  (Strg+A)").clicked() {
                        self.select_all_assets();
                        ui.close();
                    }
                });
            }
            // Auswahlrahmen im leeren Bereich; Klick ins Leere hebt die Auswahl auf
            if bg.drag_started() {
                self.pool.marquee = ui.input(|i| i.pointer.press_origin()).map(|p| (p, p));
            }
            if let (Some((a, _)), Some(p)) = (self.pool.marquee, bg.interact_pointer_pos()) {
                self.pool.marquee = Some((a, p));
                let r = Rect::from_two_pos(a, p);
                ui.painter().rect_filled(r, 0.0, ACCENT.gamma_multiply(0.12));
                ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, ACCENT), StrokeKind::Inside);
                marquee_hit = Some(self.pool.frames.iter().filter(|(_, fr)| fr.intersects(r)).map(|(i, _)| *i).collect());
            }
            if bg.drag_stopped() {
                self.pool.marquee = None;
            }
            if bg.clicked() {
                self.pool.selection.clear();
            }
        });
        if prefs_changed {
            self.save_prefs();
        }
        if let Some(hit) = marquee_hit {
            let add = ui.input(|i| i.modifiers.command || i.modifiers.shift);
            if !add {
                self.pool.selection.clear();
            }
            self.pool.selection.extend(hit);
        }
        if let Some((id, ctrl, shift)) = clicked {
            self.pool_click(id, ctrl, shift);
        }
        if let Some(id) = drag_start {
            // Ziehen eines gewählten Mediums nimmt die ganze Auswahl mit (in Poolreihenfolge)
            if self.pool.selection.contains(&id) && self.pool.selection.len() > 1 {
                self.pool.dragging = self.pool_selected_assets().iter().map(|a| a.id).collect();
            } else {
                self.pool.selection = std::collections::HashSet::from([id]);
                self.pool.anchor = Some(id);
                self.pool.dragging = vec![id];
            }
        }
        if let Some(id) = add {
            self.add_asset_clip(id);
        }
        for id in append {
            self.add_from_pool(id);
        }
        if let Some(id) = view {
            self.load_source(id);
        }
        if let Some(ids) = remove {
            self.remove_assets(&ids);
        }
    }
}

/// Änderungsdatum (Sekunden seit 1970, UTC) als „TT.MM.JJJJ“.
fn date_text(secs: u64) -> String {
    if secs == 0 {
        return "–".into();
    }
    let days = (secs / 86400) as i64;
    // Zivilkalender aus Tagen seit 1970 (Algorithmus von H. Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{d:02}.{m:02}.{y}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn date_text_is_german() {
        assert_eq!(super::date_text(0), "–");
        assert_eq!(super::date_text(1_700_000_000), "14.11.2023");
        assert_eq!(super::date_text(951_782_400), "29.02.2000");
    }
}
