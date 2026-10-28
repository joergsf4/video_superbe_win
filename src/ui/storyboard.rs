//! Storyboard (Szenenübersicht, StoryboardView.swift): alle Bilder und Videos der Hauptspur als Raster, per Ziehen
//! umsortieren, Anzeigedauer von Fotos setzen.

use super::*;
use crate::app::App;
use eframe::egui::{Align2, Pos2, Rect, RichText, Stroke, StrokeKind};
use uuid::Uuid;

/// Gezogene Szene (Nutzlast beim Umsortieren).
#[derive(Clone, Copy, Debug)]
pub struct SceneDrag(pub Uuid);

impl App {
    pub(crate) fn storyboard_view(&mut self, ui: &mut Ui) {
        let scenes = self.project.storyboard_scenes();
        let has_photo = scenes.iter().any(|i| self.project.clip(*i).is_some_and(|c| self.project.is_still(c)));
        egui::Frame::new().fill(BAR).inner_margin(egui::Margin::symmetric(14, 0)).show(ui, |ui| {
            ui.set_height(30.0);
            ui.set_width(ui.available_width());
            ui.horizontal_centered(|ui| {
                ui.label(RichText::new("Storyboard").strong());
                ui.label(RichText::new(format!("{} Szenen · Ziehen sortiert um, Doppelklick springt hin", scenes.len())).size(11.0).color(SECONDARY));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Zur Zeitleiste").clicked() {
                        self.storyboard = false;
                    }
                    ui.add_enabled_ui(has_photo, |ui| {
                        ui.menu_button(RichText::new("Anzeigedauer aller Fotos").size(11.5), |ui| {
                            for d in [2.0, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0] {
                                if ui.button(format!("{d:.0} Sekunden")).clicked() {
                                    self.set_all_photo_duration(d);
                                    ui.close();
                                }
                            }
                        });
                    });
                });
            });
        });
        let full = ui.available_rect_before_wrap();
        ui.painter().rect_filled(full, 0.0, BG);
        if scenes.is_empty() {
            ui.painter().text(full.center(), Align2::CENTER_CENTER, "Noch keine Bilder oder Videos im Film.", FontId::proportional(13.0), SECONDARY);
            ui.allocate_rect(full, Sense::hover());
            return;
        }
        let aspect = self.project.render_width as f32 / self.project.render_height.max(1) as f32;
        let card_w = (130.0 * aspect).clamp(110.0, 230.0);
        let thumb_h = card_w / aspect;
        let card_h = thumb_h + 20.0;
        let gap = 14.0;
        let dragging = egui::DragAndDrop::payload::<SceneDrag>(ui.ctx()).map(|p| p.0);
        let mut moved: Option<(Uuid, Uuid)> = None;
        let mut action: Option<(Uuid, u8, f64)> = None;
        egui::ScrollArea::vertical().id_salt("storyboard").auto_shrink([false, false]).show(ui, |ui| {
            let avail = ui.available_width() - 2.0 * 16.0;
            let cols = (((avail + gap) / (card_w + gap)).floor() as usize).max(1);
            let rows = scenes.len().div_ceil(cols);
            let (area, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), rows as f32 * (card_h + gap) + 32.0), Sense::hover());
            let sel = self.selected_set();
            for (i, id) in scenes.iter().enumerate() {
                let Some(c) = self.project.clip(*id).cloned() else { continue };
                let (col, row) = (i % cols, i / cols);
                let r = Rect::from_min_size(area.min + Vec2::new(16.0 + col as f32 * (card_w + gap), 16.0 + row as f32 * (card_h + gap)), Vec2::new(card_w, card_h));
                let thumb_r = Rect::from_min_size(r.min, Vec2::new(card_w, thumb_h));
                let asset = c.asset_id().and_then(|a| self.project.asset(a)).cloned();
                let p = ui.painter().clone();
                p.rect_filled(thumb_r, 6.0, TRACK);
                if let Some(tex) = asset.as_ref().and_then(|a| self.thumb(ui.ctx(), a)) {
                    // füllend, mittig beschnitten
                    let ts = tex.size_vec2();
                    let k = (thumb_r.width() / ts.x).max(thumb_r.height() / ts.y);
                    let (uw, uh) = (thumb_r.width() / (ts.x * k), thumb_r.height() / (ts.y * k));
                    let uv = Rect::from_min_max(Pos2::new((1.0 - uw) / 2.0, (1.0 - uh) / 2.0), Pos2::new((1.0 + uw) / 2.0, (1.0 + uh) / 2.0));
                    p.image(tex.id(), thumb_r, uv, Color32::WHITE);
                }
                let still = self.project.is_still(&c);
                // Nummer oben links, Dauer unten rechts, Blende zum Vorgänger oben rechts
                let nr = Rect::from_min_size(thumb_r.min + Vec2::new(4.0, 4.0), Vec2::new(if i >= 9 { 22.0 } else { 16.0 }, 15.0));
                p.rect_filled(nr, 4.0, ACCENT);
                p.text(nr.center(), Align2::CENTER_CENTER, format!("{}", i + 1), FontId::proportional(10.0), Color32::BLACK);
                let dur = format!("{} {:.1} s", if still { "▣" } else { "▶" }, c.duration);
                let g = p.layout_no_wrap(dur, FontId::proportional(10.0), Color32::WHITE);
                let dr = Rect::from_min_size(thumb_r.right_bottom() - g.size() - Vec2::new(14.0, 10.0), g.size() + Vec2::new(10.0, 4.0));
                p.rect_filled(dr, 4.0, Color32::from_black_alpha(170));
                p.galley(dr.min + Vec2::new(5.0, 2.0), g, Color32::WHITE);
                if let Some(j) = self.project.junction_of(c.id).filter(|j| j.b == c.id && j.overlap > 0.001) {
                    let jr = Rect::from_center_size(thumb_r.right_top() + Vec2::new(-14.0, 14.0), Vec2::splat(20.0));
                    p.circle_filled(jr.center(), 10.0, Color32::from_black_alpha(170));
                    p.text(jr.center(), Align2::CENTER_CENTER, "⇄", FontId::proportional(11.0), Color32::WHITE);
                    let _ = j;
                }
                let name = asset.as_ref().map(|a| a.name.clone()).unwrap_or_else(|| "Szene".into());
                p.text(Pos2::new(r.left() + 2.0, thumb_r.bottom() + 10.0), Align2::LEFT_CENTER, name, FontId::proportional(11.0), SECONDARY);
                let resp = ui.interact(r, ui.id().with(("scene", *id)), Sense::click_and_drag());
                let target = dragging.is_some_and(|d| d != *id) && resp.contains_pointer();
                let selected = sel.contains(id);
                let stroke = if selected { Stroke::new(2.5, Color32::from_rgb(255, 214, 10)) } else if target { Stroke::new(2.5, ACCENT) } else { Stroke::new(1.0, Color32::from_white_alpha(25)) };
                p.rect_stroke(thumb_r, 6.0, stroke, StrokeKind::Inside);
                if target {
                    p.rect_filled(Rect::from_min_size(r.left_top() - Vec2::new(8.0, 0.0), Vec2::new(3.0, thumb_h)), 1.0, ACCENT);
                }
                if resp.drag_started() {
                    egui::DragAndDrop::set_payload(ui.ctx(), SceneDrag(*id));
                }
                if let Some(d) = resp.dnd_release_payload::<SceneDrag>() {
                    if d.0 != *id {
                        moved = Some((d.0, *id));
                    }
                }
                if resp.double_clicked() {
                    action = Some((*id, 1, 0.0));
                } else if resp.clicked() {
                    let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
                    self.select(*id, add);
                }
                let first = scenes.first().copied();
                resp.context_menu(|ui| {
                    if still {
                        ui.menu_button("Anzeigedauer", |ui| {
                            for d in [2.0, 3.0, 4.0, 5.0, 8.0] {
                                if ui.button(format!("{d:.0} Sekunden")).clicked() {
                                    action = Some((*id, 2, d));
                                    ui.close();
                                }
                            }
                        });
                        if ui.button("Diese Dauer für alle Fotos").clicked() {
                            action = Some((*id, 3, c.duration));
                            ui.close();
                        }
                    }
                    if ui.add_enabled(i > 0, egui::Button::new("An den Anfang")).clicked() {
                        if let Some(f) = first {
                            moved = Some((*id, f));
                        }
                        ui.close();
                    }
                    if ui.button("Szene entfernen (Lücke schließen)").clicked() {
                        action = Some((*id, 4, 0.0));
                        ui.close();
                    }
                });
            }
            // Ablegen hinter der letzten Szene: ans Ende
            let rest = ui.interact(Rect::from_min_max(Pos2::new(area.left(), area.bottom() - 30.0), area.max), ui.id().with("scene_end"), Sense::hover());
            if let Some(d) = rest.dnd_release_payload::<SceneDrag>() {
                self.move_scene(d.0, None);
            }
        });
        if let Some((m, before)) = moved {
            self.move_scene(m, Some(before));
        }
        if let Some((id, k, d)) = action {
            match k {
                1 => {
                    self.select(id, false);
                    if let Some(c) = self.project.clip(id) {
                        let t = c.start;
                        self.seek(t);
                    }
                }
                2 => {
                    if !self.project.set_still_duration(&std::collections::HashSet::from([id]), d) {
                        self.status = "Anzeigedauer konnte nicht angepasst werden.".into();
                    }
                }
                3 => self.set_all_photo_duration(d),
                _ => {
                    self.select(id, false);
                    self.ripple_delete_selection();
                }
            }
        }
    }
}
