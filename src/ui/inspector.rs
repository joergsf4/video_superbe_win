//! Inspektor mit den Reitern Eigenschaften, Blenden, Titel und Effekte (wie InspectorView.swift).

use super::*;
use crate::app::App;
use crate::anim::Channel;
use crate::model::*;
use eframe::egui::{RichText, Rect};

const TABS: [&str; 4] = ["Eigenschaften", "Blenden", "Titel", "Effekte"];

pub(super) fn caption(ui: &mut Ui, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).size(11.5).color(SECONDARY));
}

pub(super) fn rgba_to_arr(c: Rgba) -> [f32; 4] {
    [c.r as f32, c.g as f32, c.b as f32, c.a as f32]
}

pub(super) fn arr_to_rgba(a: [f32; 4]) -> Rgba {
    Rgba { r: a[0] as f64, g: a[1] as f64, b: a[2] as f64, a: a[3] as f64 }
}

impl App {
    pub(crate) fn inspector_panel(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                // Reiter (segmentiert)
                let w = (ui.available_width() - 12.0) / TABS.len() as f32;
                ui.spacing_mut().item_spacing.x = 0.0;
                for (i, name) in TABS.iter().enumerate() {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 24.0), Sense::click());
                    let on = self.inspector_tab == i;
                    let fill = if on { Color32::from_rgb(88, 94, 102) } else if resp.hovered() { Color32::from_rgb(70, 75, 81) } else { BAR };
                    let r = match i {
                        0 => egui::CornerRadius { nw: 5, sw: 5, ne: 0, se: 0 },
                        3 => egui::CornerRadius { nw: 0, sw: 0, ne: 5, se: 5 },
                        _ => egui::CornerRadius::ZERO,
                    };
                    ui.painter().rect_filled(rect, r, fill);
                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, *name, FontId::proportional(11.5), if on { Color32::WHITE } else { Color32::from_gray(190) });
                    if resp.clicked() {
                        self.inspector_tab = i;
                    }
                }
            });
            ui.add_space(10.0);
            egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 0)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let sel = self.selected.and_then(|id| self.project.clip(id).cloned());
                match (self.inspector_tab, sel) {
                    (2, c) => self.title_tab(ui, c),
                    (_, None) => {
                        ui.label(RichText::new("Nichts ausgewählt").color(SECONDARY));
                    }
                    (0, Some(c)) => self.properties_tab(ui, c),
                    (1, Some(c)) => self.blend_tab(ui, c),
                    (_, Some(c)) => self.effects_tab(ui, c),
                }
            });
            ui.add_space(12.0);
        });
    }

    fn properties_tab(&mut self, ui: &mut Ui, c: Clip) {
        let id = c.id;
        let fps = self.project.fps;
        if c.is_title() {
            ui.label(RichText::new("Titel").heading().size(15.0));
        } else if let Some(a) = c.asset_id().and_then(|a| self.project.asset(a)) {
            ui.label(&a.name);
            caption(
                ui,
                if a.has_video || a.is_image {
                    format!("{}×{}{}", a.width as i64, a.height as i64, if a.is_image { String::new() } else { format!(", {} fps", a.fps.round() as i64) })
                } else {
                    format!("Ton, {} s", a.duration as i64)
                },
            );
        }
        ui.add_space(8.0);
        let (title, max) = if c.is_audio() { ("Lautstärke", 2.0) } else { ("Deckkraft", 1.0) };
        let mut v = c.volume;
        if slider_row(ui, title, &mut v, 0.0..=max, pct) {
            if let Some(cl) = self.project.clip_mut(id) {
                cl.volume = v;
            }
        }
        let still = c.asset_id().and_then(|a| self.project.asset(a)).is_some_and(|a| a.is_image) || c.is_title();
        if !still {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Geschwindigkeit").size(12.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("{}×  ·  {:.1} s", (c.speed * 100.0).round() / 100.0, c.duration)).size(12.0).monospace().color(SECONDARY));
                });
            });
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for s in [0.25, 0.5, 1.0, 2.0, 4.0, 8.0] {
                    let on = (c.speed - s).abs() < 1e-6;
                    let label = RichText::new(format!("{s}×")).size(12.0).color(if on { Color32::WHITE } else { Color32::from_gray(210) });
                    if ui.add(egui::Button::new(label).fill(if on { ACCENT } else { BAR }).min_size(Vec2::new(40.0, 22.0))).clicked() {
                        self.set_speed(id, s);
                    }
                }
            });
            ui.horizontal(|ui| {
                caption(ui, "Feineinstellung");
                if ui.small_button("−").clicked() {
                    self.set_speed(id, c.speed / 1.1);
                }
                if ui.small_button("+").clicked() {
                    self.set_speed(id, c.speed * 1.1);
                }
            });
            if c.is_audio() {
                caption(ui, "Hinweis: Beim Ton ist die Geschwindigkeit noch nicht umgesetzt.");
            }
        }
        ui.add_space(8.0);
        ui.label(RichText::new(format!("Start  {}\nDauer  {}", timecode(c.start, fps), timecode(c.duration, fps))).size(11.5).monospace().color(SECONDARY));
    }

    /// Regler mit Keyframe-Raute: animierte Kanäle zeigen den Wert an der Abspielposition und setzen beim Ziehen
    /// einen Keyframe (channelSlider in InspectorView.swift).
    pub(crate) fn channel_slider(&mut self, ui: &mut Ui, clip: &Clip, ch: Channel) {
        let rel = (self.playhead - clip.start).clamp(0.0, clip.duration);
        let mut v = clip.value(ch, rel);
        let on_key = clip.keys.get(ch).is_some_and(|ks| ks.iter().any(|k| (k.t - rel).abs() < 0.02));
        let (lr, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::hover());
        let p = ui.painter();
        p.text(lr.left_center(), egui::Align2::LEFT_CENTER, ch.label(), FontId::proportional(12.0), Color32::from_gray(215));
        // Raute rechts außen
        let dc = egui::pos2(lr.right() - 8.0, lr.center().y);
        let dr = Rect::from_center_size(dc, Vec2::splat(18.0));
        let resp = ui.interact(dr, ui.id().with(("chdiamond", ch as u8)), Sense::click()).on_hover_text("Keyframe an der Abspielposition setzen/entfernen");
        let pts = vec![egui::pos2(dc.x, dc.y - 5.5), egui::pos2(dc.x + 5.5, dc.y), egui::pos2(dc.x, dc.y + 5.5), egui::pos2(dc.x - 5.5, dc.y)];
        let col = if clip.is_keyed(ch) { ACCENT } else { SECONDARY };
        ui.painter().add(egui::Shape::convex_polygon(pts, if on_key { col } else { Color32::TRANSPARENT }, egui::Stroke::new(1.3, col)));
        ui.painter().text(egui::pos2(dc.x - 14.0, lr.center().y), egui::Align2::RIGHT_CENTER, ch.display(v), FontId::monospace(11.5), SECONDARY);
        if resp.clicked() {
            let id = clip.id;
            if let Some(c) = self.project.clip_mut(id) {
                c.toggle_key(ch, rel);
            }
        }
        let (lo, hi) = ch.range();
        ui.spacing_mut().slider_width = (ui.available_width() - 4.0).max(60.0);
        if ui.add(egui::Slider::new(&mut v, lo..=hi).show_value(false)).changed() {
            let id = clip.id;
            if let Some(c) = self.project.clip_mut(id) {
                c.set_channel(ch, v, rel);
            }
        }
        ui.add_space(4.0);
    }
}
