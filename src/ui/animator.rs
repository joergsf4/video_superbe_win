//! Keyframe-Animator (wie bei Video deluxe): Werkzeugleiste, Zeitskala des Clips, je Effekt eine Zeile mit
//! Keyframe-Spur (KeyframeAnimatorView.swift), dazu Kurvensymbol und Bézier-Editor (KeyInspector.swift).

use super::*;
use crate::anim::{AnimPreset, Channel, InterpKind, Keyframe};
use crate::app::App;
use crate::model::Clip;
use eframe::egui::{Align2, Pos2, Rect, RichText, Shape, Stroke};
use uuid::Uuid;

pub const LEFT_W: f32 = 218.0;
pub const ROW_H: f32 = 24.0;
const ANIM_BG: Color32 = Color32::from_rgb(51, 54, 59);

#[derive(Clone)]
pub struct Row {
    pub id: String,
    pub label: &'static str,
    pub channels: Vec<Channel>,
    pub child: bool,
    pub group: bool,
}

pub fn default_value(c: Channel) -> f64 {
    match c {
        Channel::Scale | Channel::Contrast | Channel::Saturation => 1.0,
        _ => 0.0,
    }
}

pub fn channel_id(c: Channel) -> String {
    format!("{c:?}")
}

/// Eine Spur erscheint nur, wenn der Kanal angewendet (vom Ausgangswert abweichend) oder animiert ist.
fn is_visible(clip: &Clip, c: Channel) -> bool {
    clip.is_keyed(c) || (clip.static_value(c) - default_value(c)).abs() > 1e-6
}

pub fn visible_transform(clip: &Clip) -> Vec<Channel> {
    Channel::TRANSFORM.iter().copied().filter(|c| is_visible(clip, *c)).collect()
}

pub fn visible_fx(clip: &Clip) -> Vec<Channel> {
    if clip.is_title() {
        vec![]
    } else {
        Channel::FX.iter().copied().filter(|c| is_visible(clip, *c)).collect()
    }
}

pub fn rows_for(clip: &Clip, expanded: bool) -> Vec<Row> {
    let mut r = vec![];
    let tr = visible_transform(clip);
    if !tr.is_empty() {
        r.push(Row { id: "group".into(), label: "Bildgröße/Position", channels: tr.clone(), child: false, group: true });
        if expanded {
            r.extend(tr.iter().map(|c| Row { id: channel_id(*c), label: c.label(), channels: vec![*c], child: true, group: false }));
        }
    }
    r.extend(visible_fx(clip).into_iter().map(|c| Row { id: channel_id(c), label: c.label(), channels: vec![c], child: false, group: false }));
    r
}

pub fn hidden_channels(clip: &Clip) -> Vec<Channel> {
    let all: Vec<Channel> = Channel::TRANSFORM.iter().copied().chain(if clip.is_title() { vec![] } else { Channel::FX.to_vec() }).collect();
    all.into_iter().filter(|c| !is_visible(clip, *c)).collect()
}

/// Gemeinsame Interpolation aller Keyframes einer Zeile (`None` = gemischt oder keine Keyframes).
pub fn common_kind(clip: &Clip, row: &Row) -> Option<(InterpKind, [f64; 4])> {
    let ks: Vec<&Keyframe> = row.channels.iter().flat_map(|c| clip.keys.get(*c).into_iter().flatten()).collect();
    let first = ks.first()?;
    ks.iter().all(|k| k.kind == first.kind && (first.kind != InterpKind::Bezier || k.bz == first.bz)).then(|| (first.kind, first.controls()))
}

/// Alle Keyframe-Zeiten der Kanäle (leer = alle Kanäle), sortiert und ohne Doppelte.
pub fn key_times(clip: &Clip, chs: &[Channel]) -> Vec<f64> {
    let list: Vec<Channel> = if chs.is_empty() { Channel::ALL.to_vec() } else { chs.to_vec() };
    let mut v: Vec<f64> = list.iter().flat_map(|c| clip.keys.get(*c).into_iter().flatten().map(|k| k.t)).collect();
    v.sort_by(|a, b| a.total_cmp(b));
    v.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    v
}

/// Kurvensymbol der Interpolation (Linear, Halten, Ease, Bézier).
pub fn curve_icon(ui: &mut Ui, rect: Rect, kind: Option<InterpKind>, bz: [f64; 4], mixed: bool, alpha: u8) {
    let p = ui.painter();
    let pad = 2.5;
    let pt = |x: f64, y: f64| Pos2::new(rect.left() + pad + x as f32 * (rect.width() - 2.0 * pad), rect.bottom() - pad - y as f32 * (rect.height() - 2.0 * pad));
    let col = Color32::from_white_alpha(alpha);
    let mut pts: Vec<Pos2> = vec![];
    match kind {
        Some(InterpKind::Hold) => pts = vec![pt(0.0, 0.0), pt(0.5, 0.0), pt(0.5, 1.0), pt(1.0, 1.0)],
        Some(InterpKind::Linear) => pts = vec![pt(0.0, 0.0), pt(1.0, 1.0)],
        None => {
            for i in 0..=16 {
                let u = i as f64 / 16.0;
                let y = crate::anim::bezier(u, [0.4, 0.9, 0.6, 0.1]);
                pts.push(pt(u, y.clamp(-0.2, 1.2)));
            }
        }
        Some(_) => {
            let c = [bz[0], bz[1].clamp(-0.3, 1.3), bz[2], bz[3].clamp(-0.3, 1.3)];
            for i in 0..=16 {
                let u = i as f64 / 16.0;
                pts.push(pt(u, crate::anim::bezier(u, c)));
            }
        }
    }
    if mixed {
        for w in pts.windows(2) {
            p.extend(Shape::dashed_line(&[w[0], w[1]], Stroke::new(1.5, col), 2.0, 2.0));
        }
    } else {
        p.add(Shape::line(pts, Stroke::new(1.5, col)));
    }
    p.circle_filled(pt(0.0, 0.0), 1.5, Color32::from_white_alpha(alpha / 2 + 30));
    p.circle_filled(pt(1.0, 1.0), 1.5, Color32::from_white_alpha(alpha / 2 + 30));
}

fn diamond(ui: &Ui, c: Pos2, selected: bool, muted: bool) {
    let r = 6.0;
    let pts = vec![Pos2::new(c.x, c.y - r), Pos2::new(c.x + r, c.y), Pos2::new(c.x, c.y + r), Pos2::new(c.x - r, c.y)];
    let fill = if selected { Color32::from_rgb(77, 140, 255) } else { Color32::from_gray(38) };
    let stroke = Stroke::new(1.2, if muted { Color32::from_white_alpha(150) } else { Color32::WHITE });
    ui.painter().add(Shape::convex_polygon(pts, fill, stroke));
}

fn stamp(t: f64, fps: i64) -> String {
    let f = (t * fps as f64).round() as i64;
    format!("{:02}:{:02}", f / fps.max(1), f % fps.max(1))
}

/// Gezeichnete Geometrie des Animators (für Tests und Treffer).
#[derive(Default, Clone)]
pub struct AnimGeom {
    pub lanes: Vec<(String, Rect)>,
}

impl App {
    /// Der Animator erscheint unter dem Monitor, solange der Reiter „Effekte“ offen und ein Bild-/Titelclip gewählt ist.
    pub(crate) fn animator_clip(&self) -> Option<Clip> {
        if self.inspector_tab != 3 || !self.show_animator {
            return None;
        }
        self.selected.and_then(|id| self.project.clip(id)).filter(|c| !c.is_audio()).cloned()
    }

    fn edit_clip(&mut self, id: Uuid, f: impl FnOnce(&mut Clip)) {
        if let Some(c) = self.project.clip_mut(id) {
            f(c);
        }
    }

    /// Höhe des Animators: Werkzeugleiste (28) + Zeitlineal (24) + Zeilen (bis zu 10, danach Scrollen); ohne Zeilen Platz für den Hinweis.
    pub(crate) fn animator_height(&self, clip: &Clip) -> f32 {
        let n = rows_for(clip, self.anim_expanded).len();
        if n == 0 { 28.0 + 24.0 + 58.0 } else { 28.0 + 24.0 + n.min(10) as f32 * ROW_H + 2.0 }
    }

    pub(crate) fn animator_panel(&mut self, ui: &mut Ui, clip: Clip) {
        let full = ui.available_rect_before_wrap();
        ui.allocate_rect(full, Sense::hover());
        ui.painter().rect_filled(full, 0.0, ANIM_BG);
        let id = clip.id;
        let dur = clip.duration.max(0.1);
        let rel = (self.playhead - clip.start).clamp(0.0, dur);
        let fps = self.project.fps;
        let rows = rows_for(&clip, self.anim_expanded);
        let sel_channels: Vec<Channel> = self.anim_row.as_ref().and_then(|r| rows.iter().find(|x| &x.id == r)).map(|r| r.channels.clone()).unwrap_or_default();
        self.anim_geom.lanes.clear();

        // ---- Werkzeugleiste
        let bar = Rect::from_min_size(full.min, Vec2::new(full.width(), 28.0));
        ui.painter().rect_filled(bar, 0.0, Color32::from_black_alpha(64));
        let mut tb = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(Vec2::new(8.0, 2.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
        tb.spacing_mut().item_spacing.x = 6.0;
        let ui2 = &mut tb;
        if icon_button(ui2, "\u{E892}", "Voriger Keyframe", false).clicked() {
            let ts = key_times(&clip, &sel_channels);
            if let Some(t) = ts.iter().rev().find(|t| **t < rel - 0.02) {
                self.playhead = clip.start + t;
                self.set_playing(false);
            }
        }
        if icon_button(ui2, "\u{E893}", "Nächster Keyframe", false).clicked() {
            let ts = key_times(&clip, &sel_channels);
            if let Some(t) = ts.iter().find(|t| **t > rel + 0.02) {
                self.playhead = clip.start + t;
                self.set_playing(false);
            }
        }
        if icon_button(ui2, "\u{E7AD}", "Keyframe an der Abspielposition setzen", false).clicked() {
            let chs = if sel_channels.is_empty() { visible_transform(&clip).into_iter().chain(visible_fx(&clip)).collect() } else { sel_channels.clone() };
            self.edit_clip(id, |c| chs.iter().for_each(|ch| c.add_key(*ch, rel)));
        }
        if icon_button(ui2, "\u{E711}", "Gewählten Keyframe löschen", false).clicked() {
            if let Some(k) = self.selected_key.take() {
                self.edit_clip(id, |c| {
                    for ch in Channel::ALL {
                        let t = c.keys.get(ch).and_then(|ks| ks.iter().find(|x| x.id == k)).map(|x| x.t);
                        if let Some(t) = t {
                            c.remove_key(ch, t);
                        }
                    }
                });
            } else {
                self.edit_clip(id, |c| sel_channels.iter().for_each(|ch| c.remove_key(*ch, rel)));
            }
        }
        if icon_button(ui2, "\u{E73A}", "Keyframe auf allen animierten Spuren setzen", false).clicked() {
            self.edit_clip(id, |c| {
                for ch in Channel::ALL {
                    if c.is_keyed(ch) {
                        c.add_key(ch, rel);
                    }
                }
            });
        }
        let hidden = hidden_channels(&clip);
        ui2.add_enabled_ui(!hidden.is_empty(), |ui| {
            ui.menu_button(RichText::new("+ Spur hinzufügen").size(12.0), |ui| {
                for ch in &hidden {
                    if ui.button(ch.label()).clicked() {
                        let ch = *ch;
                        self.edit_clip(id, |c| c.set_animated(ch, true, rel));
                        self.anim_row = Some(if ch.is_transform() { "group".into() } else { channel_id(ch) });
                        ui.close();
                    }
                }
            });
        });
        ui2.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button(RichText::new("Voreinstellung").size(12.0), |ui| {
                for p in AnimPreset::ALL {
                    if ui.button(p.label()).clicked() {
                        self.apply_preset_all(p, id);
                        ui.close();
                    }
                }
            });
        });

        // ---- Zeitskala
        let ruler_top = bar.bottom();
        let lane_x0 = full.left() + LEFT_W;
        let lane_w = (full.width() - LEFT_W).max(10.0);
        let ruler = Rect::from_min_size(Pos2::new(lane_x0, ruler_top), Vec2::new(lane_w, 24.0));
        let left_hdr = Rect::from_min_size(Pos2::new(full.left(), ruler_top), Vec2::new(LEFT_W, 24.0));
        let unit = [0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0].into_iter().find(|s| s / dur * 600.0 >= 40.0).unwrap_or(60.0);
        {
            let p = ui.painter();
            p.rect_filled(ruler, 0.0, Color32::from_black_alpha(38));
            let tc = timecode(rel, fps);
            let g = p.layout_no_wrap(tc, FontId::monospace(11.0), Color32::WHITE);
            let b = Rect::from_min_size(Pos2::new(left_hdr.left() + 6.0, left_hdr.center().y - 9.0), Vec2::new(g.size().x + 12.0, 18.0));
            p.rect_filled(b, 0.0, Color32::from_black_alpha(90));
            p.rect_stroke(b, 0.0, Stroke::new(1.0, Color32::from_white_alpha(50)), egui::StrokeKind::Inside);
            p.galley(b.min + Vec2::new(6.0, 2.0), g, Color32::WHITE);
            p.text(Pos2::new(left_hdr.right() - 8.0, left_hdr.center().y), Align2::RIGHT_CENTER, format!("Einheit: {unit} s"), FontId::proportional(9.5), SECONDARY);
            let mut t = 0.0;
            while t <= dur + 1e-6 {
                let x = ruler.left() + (t / dur) as f32 * lane_w;
                p.line_segment([Pos2::new(x, ruler.bottom() - 8.0), Pos2::new(x, ruler.bottom())], Stroke::new(1.0, Color32::GRAY));
                p.text(Pos2::new(x + 3.0, ruler.top() + 8.0), Align2::LEFT_CENTER, format!("{t} s"), FontId::proportional(9.5), SECONDARY);
                t += unit;
            }
        }
        let ruler_resp = ui.interact(ruler, ui.id().with("anim_ruler"), Sense::click_and_drag());
        if ruler_resp.is_pointer_button_down_on() {
            if let Some(pos) = ruler_resp.interact_pointer_pos() {
                self.playhead = clip.start + ((pos.x - ruler.left()) / lane_w).clamp(0.0, 1.0) as f64 * dur;
                self.set_playing(false);
            }
        }

        // ---- Zeilen
        let rows_top = ruler.bottom();
        let rows_rect = Rect::from_min_max(Pos2::new(full.left(), rows_top), full.max);
        let clip_ui = ui.new_child(egui::UiBuilder::new().max_rect(rows_rect));
        let mut ui3 = clip_ui;
        ui3.set_clip_rect(rows_rect.intersect(ui.clip_rect()));
        if rows.is_empty() {
            let r = Rect::from_min_size(rows_rect.min + Vec2::new(12.0, 8.0), Vec2::new(rows_rect.width() - 24.0, 60.0));
            ui3.allocate_ui(r.size(), |ui| {
                ui.label(RichText::new("Keine angewendeten oder animierten Effekte. Ändere einen Effekt, aktiviere ihn im Reiter „Effekte“ mit der Raute, bewege das Objekt im Monitor oder wähle oben „Spur hinzufügen“.").size(11.5).color(SECONDARY));
            });
        }
        let mut popup_for: Option<(String, Pos2)> = None;
        for (i, row) in rows.iter().enumerate() {
            let y = rows_top + i as f32 * ROW_H;
            if y > full.bottom() {
                break;
            }
            let row_rect = Rect::from_min_size(Pos2::new(full.left(), y), Vec2::new(LEFT_W, ROW_H));
            let selected = self.anim_row.as_deref() == Some(row.id.as_str());
            ui3.painter().rect_filled(row_rect, 0.0, if selected { Color32::from_rgb(64, 107, 191) } else { Color32::from_white_alpha(15) });
            ui3.painter().line_segment([row_rect.left_bottom(), row_rect.right_bottom()], Stroke::new(1.0, Color32::from_black_alpha(90)));
            let keyed = row.channels.iter().any(|c| clip.is_keyed(*c));
            let common = common_kind(&clip, row);
            // Name anklicken wählt die Zeile
            let name_resp = ui3.interact(row_rect, ui3.id().with(("anim_row", &row.id)), Sense::click());
            if name_resp.clicked() {
                self.anim_row = Some(row.id.clone());
            }
            // Kurvensymbol
            let icon_rect = Rect::from_min_size(row_rect.min + Vec2::new(4.0, 2.0), Vec2::splat(20.0));
            let ir = ui3.interact(icon_rect, ui3.id().with(("anim_icon", &row.id)), if keyed { Sense::click() } else { Sense::hover() });
            ui3.painter().rect_filled(icon_rect, 4.0, Color32::from_black_alpha(70));
            curve_icon(&mut ui3, icon_rect, common.map(|c| c.0), common.map(|c| c.1).unwrap_or([0.42, 0.0, 0.58, 1.0]), keyed && common.is_none(), if keyed { 255 } else { 80 });
            if ir.on_hover_text(if keyed { "Interpolation dieser Spur: Linear, Halten, Ease In/Out, Bézier" } else { "Erst einen Keyframe setzen" }).clicked() {
                popup_for = Some((row.id.clone(), icon_rect.right_top()));
            }
            let mut x = icon_rect.right() + 4.0;
            if row.group {
                let cr = Rect::from_min_size(Pos2::new(x, y + 2.0), Vec2::new(14.0, 20.0));
                let r = ui3.interact(cr, ui3.id().with(("anim_chev", &row.id)), Sense::click());
                ui3.painter().text(cr.center(), Align2::CENTER_CENTER, if self.anim_expanded { "▾" } else { "▸" }, FontId::proportional(12.0), Color32::WHITE);
                if r.clicked() {
                    self.anim_expanded = !self.anim_expanded;
                }
            }
            x += if row.child { 26.0 } else { 14.0 };
            let cb = Rect::from_min_size(Pos2::new(x, y + 5.0), Vec2::splat(14.0));
            let cr = ui3.interact(cb.expand(2.0), ui3.id().with(("anim_cb", &row.id)), Sense::click()).on_hover_text("Animation ein/aus");
            ui3.painter().rect_filled(cb, 2.0, if keyed { ACCENT } else { Color32::from_black_alpha(90) });
            ui3.painter().rect_stroke(cb, 2.0, Stroke::new(1.0, Color32::from_white_alpha(120)), egui::StrokeKind::Inside);
            if keyed {
                ui3.painter().text(cb.center(), Align2::CENTER_CENTER, "✓", FontId::proportional(11.0), Color32::WHITE);
            }
            if cr.clicked() {
                let on = !keyed;
                let chs = row.channels.clone();
                self.edit_clip(id, |c| chs.iter().for_each(|ch| c.set_animated(*ch, on, rel)));
                self.anim_row = Some(row.id.clone());
            }
            ui3.painter().with_clip_rect(Rect::from_min_max(Pos2::new(cb.right() + 6.0, y), Pos2::new(row_rect.right() - 24.0, y + ROW_H))).text(
                Pos2::new(cb.right() + 6.0, row_rect.center().y),
                Align2::LEFT_CENTER,
                row.label,
                FontId::proportional(11.0),
                Color32::WHITE,
            );
            // Zeilenmenü
            let mr = Rect::from_min_size(Pos2::new(row_rect.right() - 22.0, y + 3.0), Vec2::splat(18.0));
            let mresp = ui3.interact(mr, ui3.id().with(("anim_menu", &row.id)), Sense::click());
            ui3.painter().text(mr.center(), Align2::CENTER_CENTER, "▾", FontId::proportional(11.0), Color32::WHITE);
            let chs = row.channels.clone();
            egui::Popup::menu(&mresp).show(|ui| {
                if ui.button("Alle Keyframes entfernen").clicked() {
                    self.edit_clip(id, |c| chs.iter().for_each(|ch| c.set_animated(*ch, false, rel)));
                    ui.close();
                }
                if ui.button("Zurücksetzen").clicked() {
                    self.edit_clip(id, |c| {
                        for ch in &chs {
                            c.keys.0.remove(ch);
                            c.set_static(*ch, default_value(*ch));
                        }
                    });
                    ui.close();
                }
            });

            // Spur mit Keyframes
            let lane = Rect::from_min_size(Pos2::new(lane_x0, y), Vec2::new(lane_w, ROW_H));
            self.anim_geom.lanes.push((row.id.clone(), lane));
            ui3.painter().rect_filled(lane, 0.0, Color32::from_white_alpha(if row.group { 26 } else { 13 }));
            ui3.painter().line_segment([lane.left_bottom(), lane.right_bottom()], Stroke::new(1.0, Color32::from_black_alpha(90)));
            let lane_resp = ui3.interact(lane, ui3.id().with(("anim_lane", &row.id)), Sense::click_and_drag());
            if lane_resp.is_pointer_button_down_on() {
                if let Some(pos) = lane_resp.interact_pointer_pos() {
                    self.playhead = clip.start + ((pos.x - lane.left()) / lane_w).clamp(0.0, 1.0) as f64 * dur;
                    self.set_playing(false);
                    self.anim_row = Some(row.id.clone());
                }
            }
            if row.channels.len() == 1 {
                let ch = row.channels[0];
                for k in clip.keys.get(ch).cloned().unwrap_or_default() {
                    let cx = lane.left() + (k.t / dur) as f32 * lane_w + if self.selected_key == Some(k.id) { 14.0 } else { 0.0 };
                    let kr = Rect::from_center_size(Pos2::new(cx, lane.center().y), Vec2::splat(16.0));
                    let kresp = ui3.interact(kr, ui3.id().with(("anim_key", &row.id, k.id)), Sense::click_and_drag());
                    let sel = self.selected_key == Some(k.id);
                    diamond(&ui3, kr.center(), sel, false);
                    if sel {
                        ui3.painter().text(kr.center() + Vec2::new(10.0, 0.0), Align2::LEFT_CENTER, stamp(k.t, fps), FontId::proportional(10.0), Color32::WHITE);
                    }
                    if kresp.is_pointer_button_down_on() {
                        self.selected_key = Some(k.id);
                        self.anim_row = Some(row.id.clone());
                    }
                    if kresp.dragged() {
                        if let Some(pos) = kresp.interact_pointer_pos() {
                            let nt = (((pos.x - lane.left()) / lane_w).clamp(0.0, 1.0) as f64) * dur;
                            let kid = k.id;
                            self.edit_clip(id, |c| {
                                if let Some(ks) = c.keys.0.get_mut(&ch) {
                                    if let Some(i) = ks.iter().position(|x| x.id == kid) {
                                        ks[i].t = nt;
                                        ks.sort_by(|a, b| a.t.total_cmp(&b.t));
                                    }
                                }
                            });
                            self.playhead = clip.start + nt;
                            self.set_playing(false);
                        }
                    }
                }
            } else {
                for t in key_times(&clip, &row.channels) {
                    let c = Pos2::new(lane.left() + (t / dur) as f32 * lane_w, lane.center().y);
                    let kr = Rect::from_center_size(c, Vec2::splat(16.0));
                    let r = ui3.interact(kr, ui3.id().with(("anim_gkey", &row.id, t.to_bits())), Sense::click());
                    diamond(&ui3, c, false, true);
                    if r.clicked() {
                        self.playhead = clip.start + t;
                        self.set_playing(false);
                        self.anim_row = Some(row.id.clone());
                    }
                }
            }
        }

        // Abspielmarke und Dreieck
        let px = lane_x0 + (rel / dur) as f32 * lane_w;
        ui.painter().line_segment([Pos2::new(px, ruler.top() + 8.0), Pos2::new(px, full.bottom())], Stroke::new(1.5, ACCENT));
        ui.painter().add(Shape::convex_polygon(vec![Pos2::new(px - 5.5, ruler.top()), Pos2::new(px + 5.5, ruler.top()), Pos2::new(px, ruler.top() + 8.0)], ACCENT, Stroke::NONE));

        // Interpolations-Auswahl
        if let Some((row_id, anchor)) = popup_for {
            self.interp_popup = if self.interp_popup.as_deref() == Some(row_id.as_str()) { None } else { Some(row_id) };
            self.interp_anchor = anchor;
        }
        if let Some(row_id) = self.interp_popup.clone() {
            if let Some(row) = rows.iter().find(|r| r.id == row_id).cloned() {
                self.interp_window(ui.ctx(), &clip, &row);
            } else {
                self.interp_popup = None;
            }
        }
    }

    fn interp_window(&mut self, ctx: &egui::Context, clip: &Clip, row: &Row) {
        let common = common_kind(clip, row);
        let mut choose: Option<InterpKind> = None;
        let resp = egui::Area::new(egui::Id::new("interp_popup")).order(egui::Order::Foreground).fixed_pos(self.interp_anchor + Vec2::new(6.0, -10.0)).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(200.0);
                ui.label(RichText::new(format!("Interpolation: {}", row.label)).strong().size(12.0));
                ui.add_space(4.0);
                for k in InterpKind::ALL {
                    let (rect, r) = ui.allocate_exact_size(Vec2::new(200.0, 28.0), Sense::click());
                    if r.hovered() {
                        ui.painter().rect_filled(rect, 4.0, Color32::from_white_alpha(20));
                    }
                    let icon = Rect::from_min_size(rect.min + Vec2::new(4.0, 3.0), Vec2::splat(22.0));
                    let bz = if k == InterpKind::Bezier { common.map(|c| c.1).unwrap_or([0.42, 0.0, 0.58, 1.0]) } else { Keyframe::with_kind(0.0, 0.0, k).controls() };
                    curve_icon(ui, icon, Some(k), bz, false, 255);
                    ui.painter().text(Pos2::new(icon.right() + 8.0, rect.center().y), Align2::LEFT_CENTER, k.label(), FontId::proportional(12.5), Color32::WHITE);
                    if common.map(|c| c.0) == Some(k) {
                        ui.painter().text(Pos2::new(rect.right() - 8.0, rect.center().y), Align2::RIGHT_CENTER, "✓", FontId::proportional(13.0), Color32::WHITE);
                    }
                    if r.clicked() {
                        choose = Some(k);
                    }
                }
            });
        });
        let clicked_outside = ctx.input(|i| i.pointer.any_click()) && ctx.pointer_interact_pos().is_some_and(|p| !resp.response.rect.contains(p));
        if let Some(k) = choose {
            self.set_row_interp(clip.id, row, k);
            self.interp_popup = None;
        } else if clicked_outside && !ctx.input(|i| i.pointer.any_pressed() && false) {
            // Klick außerhalb schließt (der Klick auf das Symbol selbst wird oben als Umschalter behandelt)
            if ctx.pointer_interact_pos().is_some_and(|p| !Rect::from_min_size(self.interp_anchor - Vec2::new(20.0, 0.0), Vec2::new(24.0, 24.0)).contains(p)) {
                self.interp_popup = None;
            }
        }
    }

    pub(crate) fn set_row_interp(&mut self, id: Uuid, row: &Row, kind: InterpKind) {
        let chs = row.channels.clone();
        self.edit_clip(id, |c| c.set_interpolation(&chs, kind));
        self.anim_row = Some(row.id.clone());
        if kind == InterpKind::Bezier {
            let first = self.project.clip(id).and_then(|c| chs.iter().filter_map(|ch| c.keys.get(*ch).and_then(|ks| ks.first())).next().map(|k| k.id));
            if let Some(k) = first {
                self.selected_key = Some(k); // Kurveneditor im Reiter „Effekte“
                self.inspector_tab = 3;
            }
        }
    }

    /// Bézier-Editor: zeigt die Beschleunigungskurve; bei `editable` sind die beiden Griffe ziehbar.
    pub(crate) fn bezier_editor(ui: &mut Ui, controls: [f64; 4], editable: bool) -> Option<[f64; 4]> {
        let s = ui.available_width().min(150.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(s), Sense::hover());
        let pad = 14.0;
        let inner = s - 2.0 * pad;
        let pt = |x: f64, y: f64| Pos2::new(rect.left() + pad + x as f32 * inner, rect.top() + pad + ((1.3 - y) / 1.6) as f32 * inner);
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, TRACK);
        for y in [0.0, 1.0] {
            painter.extend(Shape::dashed_line(&[pt(0.0, y), pt(1.0, y)], Stroke::new(1.0, Color32::from_white_alpha(40)), 3.0, 3.0));
        }
        let mut c = controls;
        let curve: Vec<Pos2> = (0..=32).map(|i| pt(i as f64 / 32.0, crate::anim::bezier(i as f64 / 32.0, c))).collect();
        painter.add(Shape::line(curve, Stroke::new(2.0, ACCENT)));
        let mut changed = false;
        if editable {
            for i in 0..2 {
                let hp = pt(c[i * 2], c[i * 2 + 1]);
                let anchor = if i == 0 { pt(0.0, 0.0) } else { pt(1.0, 1.0) };
                painter.line_segment([anchor, hp], Stroke::new(1.0, Color32::from_white_alpha(130)));
                let r = ui.interact(Rect::from_center_size(hp, Vec2::splat(16.0)), ui.id().with(("bez_handle", i)), Sense::drag());
                painter.circle_filled(hp, 6.0, Color32::WHITE);
                if r.dragged() {
                    if let Some(p) = r.interact_pointer_pos() {
                        c[i * 2] = (((p.x - rect.left() - pad) / inner) as f64).clamp(0.0, 1.0);
                        c[i * 2 + 1] = (1.3 - ((p.y - rect.top() - pad) / inner) as f64 * 1.6).clamp(-0.3, 1.3);
                        changed = true;
                    }
                }
            }
        }
        changed.then_some(c)
    }

    /// Keyframe-Inspektor im Reiter „Effekte“ (Interpolation und Kurve des gewählten Keyframes).
    pub(crate) fn key_inspector(&mut self, ui: &mut Ui, clip: &Clip) {
        let Some(kid) = self.selected_key else { return };
        let Some((ch, key)) = Channel::ALL.iter().find_map(|ch| clip.keys.get(*ch).and_then(|ks| ks.iter().find(|k| k.id == kid)).map(|k| (*ch, k.clone()))) else { return };
        ui.add_space(6.0);
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Keyframe · {}", ch.label())).strong().size(12.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, I_DELETE, "Keyframe löschen", false).clicked() {
                    self.edit_clip(clip.id, |c| {
                        if let Some(ks) = c.keys.0.get_mut(&ch) {
                            ks.retain(|k| k.id != kid);
                            if ks.is_empty() {
                                c.keys.0.remove(&ch);
                            }
                        }
                    });
                    self.selected_key = None;
                }
            });
        });
        let mut kind = key.kind;
        egui::ComboBox::from_id_salt("key_kind").selected_text(kind.label()).show_ui(ui, |ui| {
            for k in InterpKind::ALL {
                ui.selectable_value(&mut kind, k, k.label());
            }
        });
        if kind != key.kind {
            self.edit_clip(clip.id, |c| {
                if let Some(k) = c.keys.0.get_mut(&ch).and_then(|ks| ks.iter_mut().find(|k| k.id == kid)) {
                    if kind == InterpKind::Bezier && k.kind != InterpKind::Bezier {
                        k.bz = k.controls();
                    }
                    k.kind = kind;
                }
            });
        }
        if key.kind != InterpKind::Hold {
            if let Some(n) = App::bezier_editor(ui, key.controls(), key.kind == InterpKind::Bezier) {
                self.edit_clip(clip.id, |c| {
                    if let Some(k) = c.keys.0.get_mut(&ch).and_then(|ks| ks.iter_mut().find(|k| k.id == kid)) {
                        k.bz = n;
                    }
                });
            }
            if key.kind == InterpKind::Bezier {
                ui.label(RichText::new(format!("cubic-bezier({:.2}, {:.2}, {:.2}, {:.2})", key.bz[0], key.bz[1], key.bz[2], key.bz[3])).size(10.5).monospace().color(SECONDARY));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip() -> Clip {
        use crate::model::{ClipContent, Wrap};
        Clip { content: ClipContent::Media(Wrap { v: Uuid::new_v4() }), duration: 6.0, ..Default::default() }
    }

    #[test]
    fn rows_show_only_applied_or_animated_channels() {
        let mut c = clip();
        assert!(rows_for(&c, true).is_empty());
        assert_eq!(hidden_channels(&c).len(), 11);
        c.transform.x = 0.2;
        c.fx.blur = 3.0;
        let rows = rows_for(&c, false);
        assert_eq!(rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["group", "Blur"]);
        let rows = rows_for(&c, true);
        assert_eq!(rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["group", "PosX", "Blur"]);
        assert!(rows[1].child && rows[0].group);
        c.set_animated(Channel::Scale, true, 0.0);
        assert!(rows_for(&c, false)[0].channels.contains(&Channel::Scale), "animierter Kanal wird sichtbar, auch bei Standardwert");
        let mut t = clip();
        t.content = crate::model::ClipContent::Title(crate::model::Wrap { v: Default::default() });
        t.fx.blur = 3.0;
        assert!(rows_for(&t, false).is_empty(), "Titel haben keine Bildeffekte");
    }

    #[test]
    fn common_kind_and_key_times() {
        let mut c = clip();
        c.set_animated(Channel::PosX, true, 1.0);
        c.add_key(Channel::PosX, 3.0);
        c.set_animated(Channel::PosY, true, 3.0);
        let row = Row { id: "group".into(), label: "x", channels: vec![Channel::PosX, Channel::PosY], child: false, group: true };
        assert_eq!(common_kind(&c, &row).unwrap().0, InterpKind::EaseInOut);
        c.set_interpolation(&[Channel::PosX], InterpKind::Linear);
        assert!(common_kind(&c, &row).is_none(), "gemischt");
        assert_eq!(key_times(&c, &[]), vec![1.0, 3.0]);
        assert_eq!(key_times(&c, &[Channel::PosY]), vec![3.0]);
    }
}
