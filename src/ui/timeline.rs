//! Timeline: Werkzeugleiste, Spurköpfe (Ton, Solo, Sichtbarkeit, Sperre), Lineal mit Kapitel- und Bereichsmarken,
//! Clips mit Vorschaubildern, Wellenform, Pegellinie, Blenden und Anfassern, Abspielmarker
//! (TimelineView.swift; Maße: Kopfbreite 112, Zeilenhöhe 54, Lineal 24).

use super::volume;
use super::*;
use crate::app::App;
use crate::model::*;
use eframe::egui::{Align2, Pos2, Rect, RichText, Shape, Stroke, StrokeKind};
use std::collections::HashSet;
use uuid::Uuid;

pub const HEADER_W: f32 = 112.0;
pub const ROW_H: f32 = 54.0;
pub const RULER_H: f32 = 24.0;
const CLIP_H: f32 = ROW_H - 4.0;
const YELLOW: Color32 = Color32::from_rgb(255, 214, 10);

/// Laufendes Verschieben von Clips (samt Partnern).
#[derive(Clone, Debug)]
pub struct MoveDrag {
    pub primary: Uuid,
    pub ids: HashSet<Uuid>,
    pub dt: f64,
    pub dtrack: isize,
}

/// Laufendes Kürzen einer Kante; `dl`/`dr` = Verschiebung der linken/rechten Kante in Sekunden.
#[derive(Clone, Debug)]
pub struct TrimDrag {
    pub id: Uuid,
    pub ids: HashSet<Uuid>,
    pub left: bool,
    pub dl: f64,
    pub dr: f64,
    /// zuletzt gewünschte (eingerastete) Kantenzeit und die Optionen (J-L-Schnitt, Ripple, Roll) dieser Geste
    pub wanted: f64,
    pub opts: crate::ops::TrimOptions,
}

#[derive(Clone, Debug)]
pub struct VolDrag {
    pub clip: Uuid,
    pub start: volume::Start,
    pub max_g: f64,
}

/// Kantenbild im Monitor beim Kürzen oder Slip (erstes/letztes Bild des Clips).
#[derive(Clone, Debug)]
pub struct EdgePreview {
    pub asset: Uuid,
    pub t: f64,
    pub label: String,
}

#[derive(Default)]
pub struct TlState {
    pub snap_guide: Option<f64>,
    pub hover_track: Option<usize>,
    pub move_drag: Option<MoveDrag>,
    pub trim: Option<TrimDrag>,
    pub slip: Option<(Uuid, f64)>,
    pub marquee: Option<(Pos2, Pos2)>,
    pub vol: Option<VolDrag>,
    pub scrubbing: bool,
    pub edge: Option<EdgePreview>,
    /// geöffnetes Blendenmenü: (Clip A, Clip B, Position)
    pub junction_popup: Option<(Uuid, Uuid, Pos2)>,
}

fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let l2 = ab.length_sq();
    let t = if l2 > 0.0 { (((p - a).x * ab.x + (p - a).y * ab.y) / l2).clamp(0.0, 1.0) } else { 0.0 };
    (p - (a + ab * t)).length()
}

/// UV-Ausschnitt für ein zugeschnittenes Teilrechteck eines Bildes (damit angeschnittene Kacheln nicht gestaucht werden).
fn uv_for(full: Rect, part: Rect, uv: Rect) -> Rect {
    let fx = |x: f32| (x - full.left()) / full.width().max(1.0);
    let fy = |y: f32| (y - full.top()) / full.height().max(1.0);
    Rect::from_min_max(
        egui::pos2(uv.left() + uv.width() * fx(part.left()), uv.top() + uv.height() * fy(part.top())),
        egui::pos2(uv.left() + uv.width() * fx(part.right()), uv.top() + uv.height() * fy(part.bottom())),
    )
}

impl App {
    fn t2x(&self, x0: f32, t: f64) -> f32 {
        x0 + ((t - self.scroll) * self.zoom) as f32
    }

    fn x2t(&self, x0: f32, x: f32) -> f64 {
        self.scroll + (x - x0) as f64 / self.zoom
    }

    pub(crate) fn timeline_panel(&mut self, ui: &mut Ui) {
        self.timeline_toolbar(ui);
        self.timeline_body(ui);
    }

    fn timeline_toolbar(&mut self, ui: &mut Ui) {
        egui::Frame::new().fill(BAR).inner_margin(egui::Margin::symmetric(12, 0)).show(ui, |ui| {
            ui.set_height(34.0);
            ui.set_width(ui.available_width());
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let sep = |ui: &mut Ui| {
                    ui.add_space(4.0);
                    let r = ui.allocate_exact_size(Vec2::new(1.0, 18.0), Sense::hover()).0;
                    ui.painter().rect_filled(r, 0.0, Color32::from_white_alpha(40));
                    ui.add_space(4.0);
                };
                if icon_button_sized(ui, I_UNDO, "Rückgängig (Strg+Z)", false, 15.0, Vec2::new(28.0, 24.0), !self.undo.is_empty()).clicked() {
                    self.do_undo();
                }
                if icon_button_sized(ui, I_REDO, "Wiederholen (Strg+Y)", false, 15.0, Vec2::new(28.0, 24.0), !self.redo.is_empty()).clicked() {
                    self.do_redo();
                }
                sep(ui);
                if icon_button_sized(ui, I_DELETE, "Löschen (Entf)", false, 15.0, Vec2::new(28.0, 24.0), self.selected.is_some()).clicked() {
                    self.delete_selection();
                }
                if icon_button(ui, I_CUT, "Teilen an Abspielposition (S)", false).clicked() {
                    self.split_at_playhead();
                }
                if icon_button(ui, I_TEXT, "Titel einfügen", false).clicked() {
                    self.add_title(crate::fx_edit::TitlePreset::Title);
                }
                if icon_button(ui, I_FIT, "Zoom: ganzen Film zeigen (\\)", false).clicked() {
                    self.zoom_fit();
                }
                let tip = if self.snapping { "Magnetisches Einrasten: an (Taste N; Alt gedrückt halten = kurz aus)" } else { "Magnetisches Einrasten: aus (Taste N)" };
                if icon_button(ui, I_PIN, tip, self.snapping).clicked() {
                    self.snapping = !self.snapping;
                }
                sep(ui);
                if ui.add(egui::Button::new(RichText::new("+ Spur").size(12.0))).on_hover_text("Spur unter der gewählten Spur einfügen").clicked() {
                    self.insert_track_at(self.sel_track + 1);
                }
                let can_remove = self.project.tracks.len() > 1;
                if ui.add_enabled(can_remove, egui::Button::new(RichText::new("− Spur").size(12.0))).on_hover_text("Gewählte Spur samt Clips löschen (Strg+Z macht es rückgängig). Auch: Rechtsklick auf den Spurkopf.").clicked() {
                    self.remove_track(self.sel_track);
                }
                let rec = self.feat.recorder.is_some();
                if icon_button_sized(ui, I_MIC, if rec { "Aufnahme beenden" } else { "Sprecher aufnehmen (ab der Abspielposition)" }, rec, 15.0, Vec2::new(28.0, 24.0), true).clicked() {
                    self.toggle_recording();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(I_ZOOM_IN).font(FontId::new(15.0, icons())).color(Color32::from_gray(190)));
                    let mut z = self.zoom.log2();
                    ui.spacing_mut().slider_width = 120.0;
                    if ui.add(egui::Slider::new(&mut z, 2.0..=9.0).show_value(false)).changed() {
                        self.zoom = 2f64.powf(z);
                    }
                    ui.label(RichText::new(I_ZOOM_OUT).font(FontId::new(15.0, icons())).color(Color32::from_gray(190)));
                });
            });
        });
    }

    fn alt_down(ui: &Ui) -> bool {
        ui.input(|i| i.modifiers.alt)
    }

    /// Einrasten für Ziehen/Kürzen/Ablegen; setzt die Einrastlinie.
    fn snap_time(&mut self, ui: &Ui, t: f64, exclude: &HashSet<Uuid>) -> f64 {
        self.snap_time_alt(t, exclude, Self::alt_down(ui))
    }

    fn snap_time_alt(&mut self, t: f64, exclude: &HashSet<Uuid>, alt: bool) -> f64 {
        let target = self.snap_target(t, exclude, alt);
        self.tl.snap_guide = target;
        target.unwrap_or(t)
    }

    fn seek_snapped(&mut self, ui: &Ui, x0: f32, x: f32) {
        let view_w = self.view_w;
        let x = x.clamp(x0, x0 + view_w);
        let t = self.x2t(x0, x).max(0.0);
        let target = self.snap_target(t, &HashSet::new(), Self::alt_down(ui));
        self.tl.snap_guide = target;
        self.playhead = target.unwrap_or(t).max(0.0);
        self.set_playing(false);
    }

    fn clip_label(&self, c: &Clip) -> String {
        let mut s = match &c.content {
            ClipContent::Title(w) => format!("T  {}", w.v.text.replace('\n', " ")),
            ClipContent::Media(w) => self.project.asset(w.v).map(|a| a.name.clone()).unwrap_or_else(|| "?".into()),
        };
        if c.group.is_some() {
            s = format!("▣ {s}");
        }
        if (c.speed - 1.0).abs() > 1e-6 {
            s += &format!("  ×{}", (c.speed * 100.0).round() / 100.0);
        }
        if let Some(g) = &c.beat_grid {
            s += &format!("  ♩ {:.0} BPM", g.bpm());
        }
        s
    }

    fn clip_color(c: &Clip) -> Color32 {
        if c.is_title() { TITLE } else if c.is_audio() { AUDIO } else { VIDEO }
    }

    fn timeline_body(&mut self, ui: &mut Ui) {
        const BAR_W: f32 = 11.0;
        let full = ui.available_rect_before_wrap();
        self.tl_rect = full;
        let bg = ui.allocate_rect(full, Sense::click_and_drag());
        let painter = ui.painter_at(full);
        painter.rect_filled(full, 0.0, BG);
        let x0 = full.left() + HEADER_W;
        let top = full.top() + RULER_H;
        let ntr = self.project.tracks.len();
        let content_h = ntr as f32 * ROW_H;
        let view_h = (full.height() - RULER_H - BAR_W).max(1.0);
        self.view_w = (full.width() - HEADER_W).max(100.0);
        let max_scroll = (content_h - view_h).max(0.0);
        self.scroll_y = self.scroll_y.clamp(0.0, max_scroll);
        let area = Rect::from_min_max(Pos2::new(x0, top), full.max);
        let rows = ui.painter_at(area);

        // Zoom / Scrollen
        if bg.hovered() {
            let (scr, ctrl, shift, hover) = ui.input(|i| (i.smooth_scroll_delta, i.modifiers.command, i.modifiers.shift, i.pointer.hover_pos()));
            if ctrl && scr.y != 0.0 {
                let anchor = hover.map(|p| self.x2t(x0, p.x)).unwrap_or(self.playhead);
                let nz = (self.zoom * (1.0 + scr.y as f64 * 0.002)).clamp(4.0, 600.0);
                self.scroll = (anchor - (anchor - self.scroll) * self.zoom / nz).max(0.0);
                self.zoom = nz;
            } else if shift || scr.x != 0.0 || max_scroll == 0.0 {
                self.scroll = (self.scroll - (scr.x + scr.y) as f64 / self.zoom).max(0.0);
            } else {
                self.scroll_y = (self.scroll_y - scr.y).clamp(0.0, max_scroll);
            }
        }
        // Beim Abspielen folgt die Ansicht dem Marker seitenweise.
        if self.playing {
            let x = self.t2x(x0, self.playhead);
            if x > full.right() - 10.0 || x < x0 {
                self.scroll = (self.playhead - 0.05 * self.view_w as f64 / self.zoom).max(0.0);
            }
        }
        let pps = self.zoom as f32;
        let alt = Self::alt_down(ui);

        // Zeilen
        let drag_track_hint = self.tl.move_drag.as_ref().and_then(|d| self.project.find_clip(d.primary).map(|(t, _)| (t as isize + d.dtrack).clamp(0, ntr as isize - 1) as usize));
        self.tl.hover_track = drag_track_hint.or_else(|| bg.hover_pos().filter(|p| p.y >= top && p.x >= full.left()).map(|p| ((p.y - top + self.scroll_y) / ROW_H) as usize).filter(|i| *i < ntr));
        for i in 0..ntr {
            let y = top + i as f32 * ROW_H - self.scroll_y;
            if y + ROW_H < top || y > full.bottom() {
                continue;
            }
            let row = Rect::from_min_size(Pos2::new(x0, y), Vec2::new(full.right() - x0, ROW_H));
            rows.rect_filled(row, 0.0, if i % 2 == 0 { TRACK } else { TRACK_ALT });
            rows.line_segment([Pos2::new(x0, y + ROW_H), Pos2::new(full.right(), y + ROW_H)], Stroke::new(1.0, Color32::from_black_alpha(70)));
            if drag_track_hint == Some(i) {
                rows.rect_filled(row, 0.0, ACCENT.gamma_multiply(0.12));
                rows.rect_stroke(row, 0.0, Stroke::new(1.5, ACCENT.gamma_multiply(0.8)), StrokeKind::Inside);
            }
        }

        // ---------- Clips ----------
        let all: Vec<(usize, Clip)> = self.project.tracks.iter().enumerate().flat_map(|(i, t)| t.clips.iter().cloned().map(move |c| (i, c))).collect();
        let sel_set = self.selected_set();
        let moving: HashSet<Uuid> = self.tl.move_drag.as_ref().map(|d| d.ids.clone()).unwrap_or_default();
        let (trim_ids, trim_dl, trim_dr) = self.tl.trim.as_ref().map(|t| (t.ids.clone(), t.dl, t.dr)).unwrap_or_default();
        let mut commit_move = false;
        let mut commit_trim = false;
        for (ti, c) in &all {
            let ti = *ti;
            let y = top + ti as f32 * ROW_H - self.scroll_y;
            if y + ROW_H < top || y > full.bottom() {
                continue;
            }
            let (tl_, tr_) = if trim_ids.contains(&c.id) { (trim_dl, trim_dr) } else { (0.0, 0.0) };
            let xa = self.t2x(x0, c.start + tl_);
            let w = (((c.duration - tl_ + tr_) * self.zoom) as f32).max(6.0);
            if xa + w < x0 || xa > full.right() {
                continue;
            }
            let rect = Rect::from_min_size(Pos2::new(xa, y + 2.0), Vec2::new(w, CLIP_H));
            let vis = rect.intersect(area);
            let hidden_or_muted = self.project.tracks[ti].hidden || (c.is_audio() && self.project.tracks[ti].muted);
            let mut cp = ui.painter_at(vis);
            if moving.contains(&c.id) {
                cp.set_opacity(0.35);
            } else if hidden_or_muted {
                cp.set_opacity(0.55);
            }
            cp.rect_filled(rect, 3.0, Self::clip_color(c));
            // Wellenform / Vorschaubilder
            if c.is_audio() {
                if let Some(peaks) = c.asset_id().and_then(|a| self.wave(ui.ctx(), a)) {
                    let from = c.source_in + tl_ * c.speed;
                    let pps_src = pps as f64 / c.speed;
                    let bps = crate::audio::PEAKS_PER_SEC as f64;
                    let step = (rect.width() as f64 / 3000.0).max(1.0);
                    let mut mesh = egui::Mesh::default();
                    let mut x = ((vis.left() - rect.left()) as f64).max(0.0);
                    let x_end = ((vis.right() - rect.left()) as f64).min(rect.width() as f64);
                    let col = Color32::from_white_alpha(190);
                    while x < x_end {
                        let i0 = ((from + x / pps_src) * bps) as usize;
                        let i1 = i0.max(((from + (x + step) / pps_src) * bps) as usize) + 1;
                        let m = peaks.get(i0..i1.min(peaks.len())).map(|s| s.iter().copied().fold(0.0f32, f32::max)).unwrap_or(0.0);
                        let h = ((m * 1.4).min(1.0) * CLIP_H * 0.9).max(1.0);
                        let cy = rect.center().y;
                        mesh.add_colored_rect(Rect::from_min_size(Pos2::new(rect.left() + x as f32, cy - h / 2.0), Vec2::new((step * 0.8).max(1.0) as f32, h)), col);
                        x += step;
                    }
                    cp.add(Shape::mesh(mesh));
                }
            } else if let Some(a) = c.asset_id().and_then(|a| self.project.asset(a)).cloned() {
                if let Some(tex) = self.thumb(ui.ctx(), &a) {
                    let tile_w = ROW_H * 16.0 / 9.0;
                    let th = CLIP_H - 14.0;
                    let n = (rect.width() / tile_w).ceil() as usize + 1;
                    let ts = tex.size_vec2();
                    let k = (tile_w / ts.x).max(th / ts.y);
                    let (uw, uh) = (tile_w / (ts.x * k), th / (ts.y * k));
                    let uv = Rect::from_min_max(egui::pos2((1.0 - uw) / 2.0, (1.0 - uh) / 2.0), egui::pos2((1.0 + uw) / 2.0, (1.0 + uh) / 2.0));
                    for i in 0..n {
                        let tx = rect.left() + i as f32 * (tile_w + 1.0);
                        if tx > vis.right() || tx + tile_w < vis.left() {
                            continue;
                        }
                        let tr = Rect::from_min_size(Pos2::new(tx, rect.top() + 14.0), Vec2::new(tile_w, th));
                        let part = tr.intersect(rect);
                        if part.width() > 0.0 {
                            cp.image(tex.id(), part, uv_for(tr, part, uv), Color32::from_white_alpha(230));
                        }
                    }
                }
            }
            cp.text(Pos2::new(rect.left() + 16.0, rect.top() + 3.0), Align2::LEFT_TOP, self.clip_label(c), FontId::proportional(10.5), Color32::WHITE);
            // Beat-Linien
            if let Some(g) = &c.beat_grid {
                let a = c.source_in + tl_ * c.speed;
                let b = a + (c.duration - tl_ + tr_) * c.speed;
                let spacing = g.period / c.speed * pps as f64;
                for (k, t) in g.beats(a, b) {
                    let is_bar = k.rem_euclid(4) == 0;
                    if !(spacing >= 6.0 || (spacing * 4.0 >= 6.0 && is_bar)) {
                        continue;
                    }
                    let x = rect.left() + ((t - a) / c.speed * pps as f64) as f32;
                    let h = if is_bar { CLIP_H } else { CLIP_H * 0.45 };
                    cp.rect_filled(Rect::from_min_size(Pos2::new(x - if is_bar { 1.0 } else { 0.5 }, rect.bottom() - h), Vec2::new(if is_bar { 2.0 } else { 1.0 }, h)), 0.0, Color32::from_rgb(255, 140, 38).gamma_multiply(if is_bar { 1.0 } else { 0.7 }));
                }
            }
            // Überblendzone (Überlappung mit dem Vorgänger)
            let (xin, _) = self.project.overlaps(c.id);
            if xin > 0.0 {
                let cw = (xin * self.zoom) as f32;
                let r = Rect::from_min_size(rect.min, Vec2::new(cw, CLIP_H));
                let line = Stroke::new(1.0, Color32::from_white_alpha(200));
                cp.rect_filled(r, 0.0, Color32::from_white_alpha(46));
                // Zeichen je Art: Kreuz = Überblenden, V = Schwarz-/Weißblende, Pfeil = Wischen/Schieben, Rahmen = Zoom/Iris
                match c.transition.as_str() {
                    "dip" | "flash" => {
                        cp.line_segment([r.left_top(), Pos2::new(r.center().x, r.bottom())], line);
                        cp.line_segment([r.right_top(), Pos2::new(r.center().x, r.bottom())], line);
                    }
                    "wipe" | "wipeDown" | "slide" => {
                        let (a, b) = if c.transition == "wipeDown" { (Pos2::new(r.center().x, r.top() + 6.0), Pos2::new(r.center().x, r.bottom() - 6.0)) } else { (Pos2::new(r.left() + 5.0, r.center().y), Pos2::new(r.right() - 5.0, r.center().y)) };
                        cp.line_segment([a, b], line);
                        let dir = (b - a).normalized();
                        let n = Vec2::new(-dir.y, dir.x);
                        cp.line_segment([b, b - dir * 6.0 + n * 4.0], line);
                        cp.line_segment([b, b - dir * 6.0 - n * 4.0], line);
                    }
                    "zoom" | "iris" => {
                        cp.rect_stroke(r.shrink2(Vec2::new((cw * 0.25).min(20.0), CLIP_H * 0.25)), 2.0, line, StrokeKind::Inside);
                    }
                    _ => {
                        cp.line_segment([r.left_top(), r.right_bottom()], line);
                        cp.line_segment([r.left_bottom(), r.right_top()], line);
                    }
                }
                if cw >= 54.0 {
                    cp.text(Pos2::new(r.center().x, r.bottom() - 9.0), Align2::CENTER_CENTER, crate::ops::transition_label(&c.transition), FontId::proportional(9.0), Color32::WHITE);
                }
            }
            // Pegellinie
            let has_line = volume::has_line(c);
            let max_g = volume::max_level(c);
            let mut line_pts: Vec<Pos2> = vec![];
            if has_line {
                for t in volume::breakpoints(c) {
                    line_pts.push(Pos2::new(rect.left() + (t * self.zoom) as f32, rect.top() + volume::y_of(c.curve_gain(t), CLIP_H as f64, max_g) as f32));
                }
                cp.add(Shape::line(line_pts.clone(), Stroke::new(1.5, YELLOW.gamma_multiply(0.9))));
                for p in &c.volume_points {
                    let pt = Pos2::new(rect.left() + (p.t * self.zoom) as f32, rect.top() + volume::y_of(p.gain, CLIP_H as f64, max_g) as f32);
                    cp.circle(pt, 4.0, Color32::WHITE, Stroke::new(1.0, Color32::BLACK));
                }
            }
            // Ein-/Ausblenden
            let shade = Color32::from_black_alpha(90);
            if c.fade_in > 0.0 {
                let fw = (c.fade_in * self.zoom) as f32;
                cp.add(Shape::convex_polygon(vec![rect.left_top(), rect.left_top() + Vec2::new(fw, 0.0), rect.left_bottom()], shade, Stroke::NONE));
            }
            if c.fade_out > 0.0 {
                let fw = (c.fade_out * self.zoom) as f32;
                cp.add(Shape::convex_polygon(vec![rect.right_top(), rect.right_top() - Vec2::new(fw, 0.0), rect.right_bottom()], shade, Stroke::NONE));
            }
            // Anfasser zum Kürzen
            cp.rect_filled(Rect::from_min_size(rect.left_top() + Vec2::new(2.5, 0.0), Vec2::new(2.0, CLIP_H)), 0.0, Color32::from_white_alpha(90));
            cp.rect_filled(Rect::from_min_size(rect.right_top() + Vec2::new(-4.5, 0.0), Vec2::new(2.0, CLIP_H)), 0.0, Color32::from_white_alpha(90));
            let selected = sel_set.contains(&c.id);
            cp.rect_stroke(rect, 3.0, if selected { Stroke::new(2.0, YELLOW) } else { Stroke::new(1.0, Color32::from_black_alpha(100)) }, StrokeKind::Inside);
            let fi_x = rect.left() + (c.fade_in * self.zoom) as f32;
            let fo_x = rect.right() - (c.fade_out * self.zoom) as f32;
            for kx in [fi_x, fo_x] {
                let r = Rect::from_center_size(Pos2::new(kx, rect.top() + 6.0), Vec2::splat(10.0));
                cp.rect_filled(r, 2.0, Color32::WHITE);
                cp.rect_stroke(r, 2.0, Stroke::new(1.0, Color32::from_black_alpha(150)), StrokeKind::Inside);
            }

            // ---------- Interaktion (spätere Widgets liegen oben) ----------
            let cid = ui.id().with(("clip", c.id));
            let locked = self.project.tracks[ti].locked;
            let body = ui.interact(vis, cid.with("body"), Sense::click_and_drag());
            if body.clicked() {
                let (shift, cmd) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
                self.select(c.id, shift || cmd);
                self.sel_track = ti;
            }
            if body.double_clicked() && has_line && !locked {
                if let Some(p) = body.interact_pointer_pos() {
                    if line_pts.windows(2).any(|w| dist_to_segment(p, w[0], w[1]) < 8.0) {
                        let tp = ((p.x - rect.left()) as f64 / self.zoom).clamp(0.0, c.duration);
                        let tol = 8.0 / self.zoom;
                        if let Some(cl) = self.project.clip_mut(c.id) {
                            volume::double_tap(cl, tp, tol);
                        }
                    }
                }
            }
            let origin = ui.input(|i| i.pointer.press_origin());
            if body.drag_started() && !locked {
                let press = origin.unwrap_or(body.interact_pointer_pos().unwrap_or(rect.center()));
                let near_line = has_line && line_pts.windows(2).any(|w| dist_to_segment(press, w[0], w[1]) < 7.0) && !alt;
                if near_line {
                    let press_t = ((press.x - rect.left()) as f64 / self.zoom).clamp(0.0, c.duration);
                    let start = volume::begin_line(c, press_t, 8.0 / self.zoom);
                    self.tl.vol = Some(VolDrag { clip: c.id, start, max_g });
                } else if alt && !self.project.is_still(c) {
                    self.tl.slip = Some((c.id, 0.0));
                } else {
                    if !sel_set.contains(&c.id) {
                        self.select(c.id, false);
                    }
                    let mut ids = self.project.with_partners(&self.selected_set());
                    // Mausmodus 7/8: alles dahinter rückt mit
                    let fol = self.project.followers(&ids, self.mouse_mode);
                    ids.extend(fol);
                    self.tl.move_drag = Some(MoveDrag { primary: c.id, ids, dt: 0.0, dtrack: 0 });
                }
            }
            if body.dragged() {
                if let (Some(cur), Some(press)) = (ui.input(|i| i.pointer.latest_pos()), origin) {
                    let (dx, dy) = (cur.x - press.x, cur.y - press.y);
                    if let Some(v) = self.tl.vol.clone().filter(|v| v.clip == c.id) {
                        let gain = volume::gain_at_y(cur.y as f64 - rect.top() as f64, CLIP_H as f64, v.max_g);
                        let r = volume::line_drag(&v.start, gain, v.max_g);
                        if let Some(cl) = self.project.clip_mut(c.id) {
                            cl.volume_points = v.start.points.clone();
                            cl.volume = v.start.volume;
                            volume::apply(cl, &r);
                        }
                    } else if let Some((sid, _)) = self.tl.slip.filter(|s| s.0 == c.id) {
                        let mut p = self.project.clone();
                        let applied = p.slip(sid, -((dx as f64) / self.zoom) * c.speed);
                        self.tl.slip = Some((sid, applied));
                        if let (Some(a), Some(nc)) = (c.asset_id(), p.clip(sid)) {
                            if c.part == ClipPart::Video {
                                self.tl.edge = Some(EdgePreview { asset: a, t: nc.source_in, label: format!("Slip {applied:+.2} s · Quelle ab {:.2} s", nc.source_in) });
                            }
                        }
                    } else if let Some(md) = self.tl.move_drag.clone().filter(|m| m.primary == c.id) {
                        // Umschalt: nur zwischen den Spuren verschieben, die Zeit bleibt
                        let only_tracks = ui.input(|i| i.modifiers.shift);
                        let dx = if only_tracks { 0.0 } else { dx };
                        let raw = c.start + dx as f64 / self.zoom;
                        let excl = md.ids.clone();
                        let (s_t, e_t) = (self.snap_target(raw, &excl, alt), self.snap_target(raw + c.duration, &excl, alt));
                        let ds = s_t.map(|t| (t - raw).abs()).unwrap_or(f64::INFINITY);
                        let de = e_t.map(|t| (t - (raw + c.duration)).abs()).unwrap_or(f64::INFINITY);
                        let start = if ds <= de && s_t.is_some() { s_t.unwrap() } else if let Some(e) = e_t { e - c.duration } else { raw };
                        self.tl.snap_guide = if ds <= de { s_t } else { e_t };
                        let dt = start.max(0.0) - c.start;
                        let dtrack = if self.mouse_mode == crate::ops::MouseMode::Single { (dy / ROW_H).round() as isize } else { 0 };
                        self.tl.move_drag = Some(MoveDrag { dt, dtrack, ..md });
                    }
                }
            }
            if body.drag_stopped() {
                if self.tl.vol.as_ref().is_some_and(|v| v.clip == c.id) {
                    self.tl.vol = None;
                } else if let Some((sid, d)) = self.tl.slip.filter(|s| s.0 == c.id) {
                    self.tl.slip = None;
                    self.tl.edge = None;
                    if d.abs() > 1e-9 {
                        self.project.slip(sid, d);
                    }
                } else if self.tl.move_drag.as_ref().is_some_and(|m| m.primary == c.id) {
                    commit_move = true;
                }
            }
            // Blenden-Anfasser
            let max_fade = c.duration - 0.1;
            let fi_resp = ui.interact(Rect::from_center_size(Pos2::new(fi_x, rect.top() + 6.0), Vec2::splat(14.0)), cid.with("fi"), Sense::drag());
            if fi_resp.dragged() && !locked {
                if let Some(p) = ui.input(|i| i.pointer.latest_pos()) {
                    let v = (((p.x - rect.left()) as f64) / self.zoom).clamp(0.0, (max_fade - c.fade_out).max(0.0));
                    if let Some(cl) = self.project.clip_mut(c.id) {
                        cl.fade_in = v;
                    }
                }
            }
            let fo_resp = ui.interact(Rect::from_center_size(Pos2::new(fo_x, rect.top() + 6.0), Vec2::splat(14.0)), cid.with("fo"), Sense::drag());
            if fo_resp.dragged() && !locked {
                if let Some(p) = ui.input(|i| i.pointer.latest_pos()) {
                    let v = (((rect.right() - p.x) as f64) / self.zoom).clamp(0.0, (max_fade - c.fade_in).max(0.0));
                    if let Some(cl) = self.project.clip_mut(c.id) {
                        cl.fade_out = v;
                    }
                }
            }
            if fi_resp.hovered() || fo_resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            // Kürzen
            for left in [true, false] {
                let hr = if left { Rect::from_min_size(rect.left_top(), Vec2::new(7.0, CLIP_H)) } else { Rect::from_min_size(rect.right_top() - Vec2::new(7.0, 0.0), Vec2::new(7.0, CLIP_H)) };
                let hr = Rect::from_min_max(hr.min + Vec2::new(0.0, 12.0), hr.max).intersect(vis.expand(1.0));
                if hr.width() <= 0.0 {
                    continue;
                }
                let resp = ui.interact(hr, cid.with(if left { "tl" } else { "tr" }), Sense::drag());
                if resp.hovered() || resp.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
                if resp.drag_started() && !locked {
                    self.select(c.id, false);
                    // Alt = J-L-Schnitt (nur Bild oder nur Ton), Strg = Roll, sonst Ripple nach Mausmodus
                    let (alt_k, cmd_k) = ui.input(|i| (i.modifiers.alt, i.modifiers.command));
                    let mut opts = crate::ops::TrimOptions::default();
                    if alt_k && self.project.partner_id(c.id).is_some() {
                        opts.linked = false;
                    } else if cmd_k && self.project.roll_neighbor(c.id, left).is_some() {
                        opts.roll = true;
                    } else {
                        opts.ripple = match self.mouse_mode {
                            crate::ops::MouseMode::OneTrack => crate::ops::RippleScope::Track,
                            crate::ops::MouseMode::AllTracks => crate::ops::RippleScope::All,
                            crate::ops::MouseMode::Single => crate::ops::RippleScope::None,
                        };
                    }
                    let mut ids = HashSet::from([c.id]);
                    if opts.linked {
                        ids.extend(self.project.partner_id(c.id));
                    }
                    self.tl.trim = Some(TrimDrag { id: c.id, ids, left, dl: 0.0, dr: 0.0, wanted: if left { c.start } else { c.end() }, opts });
                }
                if resp.dragged() {
                    if let (Some(tr), Some(cur), Some(press)) = (self.tl.trim.clone().filter(|t| t.id == c.id && t.left == left), ui.input(|i| i.pointer.latest_pos()), origin) {
                        let dt = (cur.x - press.x) as f64 / self.zoom;
                        let edge = if left { c.start } else { c.end() } + dt;
                        let mut excl: HashSet<Uuid> = tr.ids.clone();
                        excl.extend(self.project.trim_ignore_set(c.id, left, &tr.opts));
                        // beim J-L-Schnitt hält Alt den Magneten nicht an
                        let wanted = self.snap_time_alt(edge, &excl, alt && tr.opts.linked);
                        let mut p = self.project.clone();
                        let ok = if left { p.trim_start_opt(c.id, wanted, &tr.opts) } else { p.trim_end_opt(c.id, wanted, &tr.opts) };
                        if ok {
                            if let Some(nc) = p.clip(c.id) {
                                let mut t2 = tr.clone();
                                t2.dl = nc.start - c.start;
                                t2.dr = nc.end() - c.end();
                                t2.wanted = wanted;
                                self.tl.trim = Some(t2);
                                if let (Some(a), ClipPart::Video) = (nc.asset_id(), c.part) {
                                    let src = if left { nc.source_in } else { nc.source_in + nc.duration * nc.speed - 1.0 / self.project.fps.max(1) as f64 };
                                    let lab = format!("{}{}", if left { "Erstes Bild: " } else { "Letztes Bild: " }, timecode(if left { nc.start } else { nc.end() }, self.project.fps));
                                    self.tl.edge = Some(EdgePreview { asset: a, t: src, label: lab });
                                }
                            }
                        }
                    }
                }
                if resp.drag_stopped() && self.tl.trim.as_ref().is_some_and(|t| t.id == c.id && t.left == left) {
                    commit_trim = true;
                }
            }
            // Mitte der Linienstücke: senkrecht ziehen
            if has_line && !locked {
                for (st, g, idx) in volume::segments(c) {
                    let pt = Pos2::new(rect.left() + (st * self.zoom) as f32, rect.top() + volume::y_of(g, CLIP_H as f64, max_g) as f32);
                    if !vis.contains(pt) {
                        continue;
                    }
                    cp.circle(pt, 6.0, YELLOW, Stroke::new(1.5, Color32::from_black_alpha(180)));
                    let sr = ui.interact(Rect::from_center_size(pt, Vec2::splat(14.0)), cid.with(("seg", idx)), Sense::drag());
                    if sr.drag_started() {
                        let start = volume::Start { mode: volume::Mode::Segment(idx), points: c.volume_points.clone(), volume: c.volume, grab_gain: 0.0 };
                        self.tl.vol = Some(VolDrag { clip: c.id, start, max_g });
                    }
                    if sr.dragged() {
                        if let (Some(v), Some(cur), Some(press)) = (self.tl.vol.clone().filter(|v| v.clip == c.id), ui.input(|i| i.pointer.latest_pos()), origin) {
                            let d = -((cur.y - press.y) as f64) / (CLIP_H as f64 - 2.0 * volume::INSET) * v.max_g;
                            let r = volume::segment_drag(&v.start, idx, d, v.max_g);
                            if let Some(cl) = self.project.clip_mut(c.id) {
                                cl.volume_points = v.start.points.clone();
                                cl.volume = v.start.volume;
                                volume::apply(cl, &r);
                            }
                        }
                    }
                    if sr.drag_stopped() {
                        self.tl.vol = None;
                    }
                }
            }
            // Kontextmenü
            let cc = c.clone();
            body.context_menu(|ui| {
                if cc.is_audio() {
                    if cc.beat_grid.is_none() {
                        if ui.button("Beat-Raster erkennen").clicked() {
                            self.detect_beats(cc.id);
                            ui.close();
                        }
                    } else {
                        if ui.button("Beat-Raster neu erkennen").clicked() {
                            self.remove_beats(cc.id);
                            self.detect_beats(cc.id);
                            ui.close();
                        }
                        if ui.button("Tempo verdoppeln (×2)").clicked() {
                            self.scale_beats(cc.id, 2.0);
                            ui.close();
                        }
                        if ui.button("Tempo halbieren (÷2)").clicked() {
                            self.scale_beats(cc.id, 0.5);
                            ui.close();
                        }
                        if ui.button("Beat an der Abspielposition ausrichten").clicked() {
                            self.align_beats(cc.id);
                            ui.close();
                        }
                        if ui.button("Beat-Raster entfernen").clicked() {
                            self.remove_beats(cc.id);
                            ui.close();
                        }
                    }
                    ui.menu_button("Musik bei Sprache abdunkeln (Ducking)", |ui| {
                        for db in [-8.0, -12.0, -18.0] {
                            if ui.button(format!("−{:.0} dB", -db)).clicked() {
                                self.apply_ducking(cc.id, db);
                                ui.close();
                            }
                        }
                        if !cc.volume_points.is_empty() {
                            ui.separator();
                            if ui.button("Lautstärkekurve entfernen").clicked() {
                                self.clear_ducking(cc.id);
                                ui.close();
                            }
                        }
                    });
                    ui.separator();
                }
                if cc.part == ClipPart::Video && !cc.is_title() && ui.button("Szenen erkennen und teilen").clicked() {
                    self.detect_scenes(cc.id);
                    ui.close();
                }
                if self.can_align_audio(cc.id) && ui.button("Ton an zweiter Aufnahme ausrichten").clicked() {
                    self.align_audio(cc.id);
                    ui.close();
                }
                if !cc.is_audio() && !cc.is_title() && ui.button("Datum als Titel einblenden").clicked() {
                    self.add_date_title(cc.id);
                    ui.close();
                }
                if cc.group.is_some() && ui.button("Gruppe auflösen").clicked() {
                    self.select(cc.id, false);
                    self.ungroup_selection();
                    ui.close();
                }
                if cc.link.is_some() && ui.button("Verknüpfung lösen").clicked() {
                    self.project.unlink(cc.id);
                    ui.close();
                }
                if ui.button("Löschen").clicked() {
                    self.select(cc.id, false);
                    self.delete_selection();
                    ui.close();
                }
            });
        }

        // Verschieben/Kürzen beenden (nach der Schleife: das Modell wird erst jetzt geändert)
        if commit_move {
            if let Some(md) = self.tl.move_drag.take() {
                self.project.move_clips(&md.ids, md.dtrack, md.dt);
            }
            self.tl.snap_guide = None;
        }
        if commit_trim {
            if let Some(tr) = self.tl.trim.take() {
                if tr.dl.abs() > 1e-9 || tr.dr.abs() > 1e-9 {
                    if tr.left {
                        self.project.trim_start_opt(tr.id, tr.wanted, &tr.opts);
                    } else {
                        self.project.trim_end_opt(tr.id, tr.wanted, &tr.opts);
                    }
                }
            }
            self.tl.snap_guide = None;
            self.tl.edge = None;
        }

        // ---------- Übergänge: A/B-Symbol, Ablage von Blendenvorlagen, Auswahlmenü ----------
        // Nur abholen, wenn wirklich eine Blendenvorlage gezogen wird (das Abholen verbraucht jede Ablage, auch Medien)
        let dragging_transition = egui::DragAndDrop::payload::<crate::fx_edit::TransitionDrag>(ui.ctx()).is_some();
        let trans_hover = if dragging_transition { bg.dnd_hover_payload::<crate::fx_edit::TransitionDrag>() } else { None };
        let trans_drop = if dragging_transition { bg.dnd_release_payload::<crate::fx_edit::TransitionDrag>() } else { None };
        let pointer = ui.input(|i| i.pointer.latest_pos());
        let drag_target: Option<crate::ops::Junction> = match pointer {
            Some(pos) if (trans_hover.is_some() || trans_drop.is_some()) && pos.x >= x0 && pos.y >= top && ntr > 0 => {
                let row = (((pos.y - top + self.scroll_y) / ROW_H).floor().max(0.0) as usize).min(ntr - 1);
                self.junction_near(self.x2t(x0, pos.x), row)
            }
            _ => None,
        };
        if let (Some(d), Some(j)) = (&trans_drop, drag_target) {
            self.apply_transition(j.a, j.b, Some(&d.0), None);
        }
        for tr in 0..ntr {
            let y = top + tr as f32 * ROW_H - self.scroll_y;
            if y + ROW_H < top || y > full.bottom() {
                continue;
            }
            for j in self.project.junctions(tr) {
                let (Some(a), Some(b)) = (self.project.clip(j.a).cloned(), self.project.clip(j.b).cloned()) else { continue };
                let cut = if j.overlap > 0.0 { (a.end() + b.start) / 2.0 } else { a.end() };
                let x = self.t2x(x0, cut);
                if x < x0 + 12.0 || x > full.right() - 12.0 {
                    continue;
                }
                let chip = Rect::from_center_size(Pos2::new(x, y + 2.0 + 9.0), Vec2::new(22.0, 14.0));
                let target = drag_target.is_some_and(|d| d.b == j.b);
                rows.rect_filled(chip, 3.0, if target { ACCENT } else { Color32::from_black_alpha(if j.overlap > 0.001 { 200 } else { 140 }) });
                rows.text(chip.center(), Align2::CENTER_CENTER, "A/B", FontId::proportional(8.5), if j.overlap > 0.001 && !target { Color32::from_rgb(255, 214, 10) } else { Color32::WHITE });
                let resp = ui.interact(chip, ui.id().with(("junction", j.b)), Sense::click());
                if resp.clicked() {
                    self.tl.junction_popup = Some((j.a, j.b, chip.left_bottom() + Vec2::new(0.0, 2.0)));
                }
                let kind = self.junction_kind(&j);
                resp.on_hover_text(match kind {
                    Some(k) => format!("{} · {:.1} s\nKlick: Blende ändern", crate::ops::transition_label(&k), j.overlap),
                    None => "Harter Schnitt\nKlick: Blende einfügen".to_string(),
                });
            }
        }
        if let Some((pa, pb, pos)) = self.tl.junction_popup {
            let mut close = false;
            let area = egui::Area::new(ui.id().with("junction_popup")).order(egui::Order::Foreground).fixed_pos(pos).show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(170.0);
                    ui.label(RichText::new("Blende").strong());
                    for k in crate::ops::TRANSITION_TEMPLATES {
                        if ui.button(crate::ops::transition_label(k)).clicked() {
                            self.apply_transition(pa, pb, Some(k), None);
                            close = true;
                        }
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        for d in [0.5, 1.0, 1.5, 2.0] {
                            if ui.small_button(format!("{d} s")).clicked() {
                                let kind = self.project.junction_of(pb).and_then(|j| self.junction_kind(&j));
                                self.apply_transition(pa, pb, Some(kind.as_deref().unwrap_or("crossfade")), Some(d));
                                close = true;
                            }
                        }
                    });
                    if ui.button("Harter Schnitt").clicked() {
                        self.apply_transition(pa, pb, None, None);
                        close = true;
                    }
                });
            });
            let outside = ui.input(|i| i.pointer.primary_pressed() && i.pointer.latest_pos().is_some_and(|p| !area.response.rect.contains(p)));
            if close || outside || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.tl.junction_popup = None;
            }
        }

        // Ziel-Anzeige beim Verschieben: Kopien der Clips am Zielplatz
        if let Some(md) = self.tl.move_drag.clone() {
            for (ti, c) in all.iter().filter(|(_, c)| md.ids.contains(&c.id)) {
                let t2 = *ti as isize + md.dtrack;
                if t2 < 0 || t2 as usize >= ntr {
                    continue;
                }
                let y = top + t2 as f32 * ROW_H - self.scroll_y;
                let xa = self.t2x(x0, c.start + md.dt);
                let r = Rect::from_min_size(Pos2::new(xa, y + 2.0), Vec2::new(((c.duration * self.zoom) as f32).max(6.0), CLIP_H));
                let gp = rows.clone();
                gp.rect_filled(r, 3.0, Self::clip_color(c).gamma_multiply(0.9));
                gp.rect_stroke(r, 3.0, Stroke::new(2.0, YELLOW), StrokeKind::Inside);
                let name = if c.is_title() { "T".to_string() } else { c.asset_id().and_then(|a| self.project.asset(a)).map(|a| a.name.clone()).unwrap_or_default() };
                gp.text(r.left_top() + Vec2::new(16.0, 3.0), Align2::LEFT_TOP, name, FontId::proportional(10.0), Color32::WHITE);
            }
        }

        // Ablegen aus dem Medienpool: Vorschau und Einfügen
        let dnd = bg.dnd_hover_payload::<crate::source::DragAsset>();
        let dropped: Option<crate::source::DragAsset> = bg.dnd_release_payload::<crate::source::DragAsset>().map(|a| (*a).clone());
        if let (Some(da), Some(pos)) = (dnd.as_ref().map(|a| (**a).clone()).or(dropped.clone()), ui.input(|i| i.pointer.latest_pos())) {
            let aid = da.id;
            if pos.x >= x0 && pos.y >= top {
                if let Some(a) = self.project.asset(aid).cloned() {
                    let ti = (((pos.y - top + self.scroll_y) / ROW_H).floor().max(0.0) as usize).min(ntr - 1);
                    let raw = self.x2t(x0, pos.x).max(0.0);
                    let start = self.snap_time(ui, raw, &HashSet::new()).max(0.0);
                    self.tl.hover_track = Some(ti);
                    let both = a.has_video && a.has_audio && !a.is_image;
                    let ghost_dur = da.range.map(|r| r.1 - r.0).unwrap_or(a.duration);
                    let ghost = |track: usize, label: &str| {
                        let y = top + track as f32 * ROW_H - self.scroll_y;
                        let xa = self.t2x(x0, start);
                        let r = Rect::from_min_max(Pos2::new(xa, y + 2.0), Pos2::new((xa + (ghost_dur * self.zoom) as f32).max(xa + 6.0), y + ROW_H - 2.0));
                        rows.rect_filled(r, 3.0, ACCENT.gamma_multiply(0.25));
                        rows.rect_stroke(r, 3.0, Stroke::new(1.5, ACCENT), StrokeKind::Inside);
                        rows.text(r.left_top() + Vec2::new(4.0, 3.0), Align2::LEFT_TOP, label, FontId::proportional(10.5), Color32::WHITE);
                    };
                    ghost(ti, &format!("Spur {}", ti + 1));
                    if both {
                        ghost(ti + 1, "Ton");
                    }
                    if let Some(d) = dropped {
                        self.add_to_timeline(d.id, Some(ti), Some(start), d.range);
                        self.sel_track = ti;
                        self.tl.snap_guide = None;
                    }
                }
            }
        }

        // Einrastlinie
        if let Some(g) = self.tl.snap_guide {
            if self.tl.move_drag.is_some() || self.tl.trim.is_some() || dnd.is_some() || self.tl.scrubbing {
                let x = self.t2x(x0, g);
                painter.line_segment([Pos2::new(x, full.top()), Pos2::new(x, full.bottom())], Stroke::new(1.5, YELLOW));
            }
        }

        // Auswahlrahmen
        if let Some((a, b)) = self.tl.marquee {
            let r = Rect::from_two_pos(a, b);
            painter.rect_filled(r, 0.0, YELLOW.gamma_multiply(0.12));
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, YELLOW.gamma_multiply(0.8)), StrokeKind::Inside);
        }

        // Spurköpfe (stehen beim waagerechten Scrollen still)
        let head_clip = ui.painter_at(Rect::from_min_max(Pos2::new(full.left(), top), Pos2::new(x0, full.bottom())));
        let mut track_action: Option<(usize, u8)> = None;
        for i in 0..ntr {
            let y = top + i as f32 * ROW_H - self.scroll_y;
            if y + ROW_H < top || y > full.bottom() {
                continue;
            }
            let r = Rect::from_min_size(Pos2::new(full.left(), y), Vec2::new(HEADER_W, ROW_H));
            head_clip.rect_filled(r, 0.0, if i % 2 == 0 { PANEL } else { HEADER_ALT });
            head_clip.line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, Color32::from_black_alpha(70)));
            let hot = self.tl.hover_track == Some(i) || self.sel_track == i;
            head_clip.text(Pos2::new(r.left() + 17.0, r.center().y), Align2::CENTER_CENTER, format!("{}", i + 1), FontId::proportional(if hot { 19.0 } else { 16.0 }), if hot { ACCENT } else { SECONDARY });
            let (muted, solo, hidden, locked) = {
                let t = &self.project.tracks[i];
                (t.muted, t.solo == Some(true), t.hidden, t.locked)
            };
            let mut x = r.left() + 30.0;
            let cy = r.center().y;
            let hs = ui.interact(r.intersect(head_clip.clip_rect()), ui.id().with(("trkhead", i)), Sense::click());
            if hs.clicked() {
                self.sel_track = i;
            }
            hs.context_menu(|ui| {
                if ui.button("Spur darüber einfügen").clicked() {
                    track_action = Some((i, 0));
                    ui.close();
                }
                if ui.button("Spur darunter einfügen").clicked() {
                    track_action = Some((i, 1));
                    ui.close();
                }
                if ui.add_enabled(ntr > 1, egui::Button::new("Spur löschen")).clicked() {
                    track_action = Some((i, 2));
                    ui.close();
                }
            });
            let mut flag = |ui: &mut Ui, id: &str, w: f32, tip: &str, draw: &dyn Fn(&egui::Painter, Rect, bool)| -> bool {
                let rr = Rect::from_center_size(Pos2::new(x + w / 2.0, cy), Vec2::new(w + 4.0, 22.0));
                x += w + 5.0;
                if rr.right() > r.right() || rr.bottom() < top || rr.top() > full.bottom() {
                    return false;
                }
                let rs = ui.interact(rr, ui.id().with(("trk", i, id)), Sense::click()).on_hover_text(tip);
                draw(&head_clip, rr, rs.hovered());
                rs.clicked()
            };
            let ic = |g: &'static str, on: bool| move |p: &egui::Painter, rr: Rect, hover: bool| {
                p.text(rr.center(), Align2::CENTER_CENTER, g, FontId::new(14.0, icons()), if on { ACCENT } else if hover { Color32::WHITE } else { SECONDARY });
            };
            let mut toggled = None;
            if flag(ui, "m", 15.0, "Ton aus/an", &ic(if muted { I_MUTE } else { I_VOLUME }, muted)) {
                toggled = Some(0);
            }
            if flag(ui, "s", 15.0, "Solo: nur diese Spur ist zu hören", &|p, rr, hover| {
                let b = Rect::from_center_size(rr.center(), Vec2::splat(15.0));
                if solo {
                    p.rect_filled(b, 3.0, ACCENT);
                } else if hover {
                    p.rect_filled(b, 3.0, Color32::from_white_alpha(25));
                }
                p.text(b.center(), Align2::CENTER_CENTER, "S", FontId::proportional(11.0), if solo { Color32::WHITE } else { SECONDARY });
            }) {
                toggled = Some(1);
            }
            if flag(ui, "h", 15.0, "Spur im Bild ein-/ausblenden", &ic(if hidden { I_HIDE } else { I_VIEW }, hidden)) {
                toggled = Some(2);
            }
            if flag(ui, "l", 15.0, "Spur sperren", &ic(if locked { I_LOCK } else { I_UNLOCK }, locked)) {
                toggled = Some(3);
            }
            if let Some(k) = toggled {
                let t = &mut self.project.tracks[i];
                match k {
                    0 => t.muted = !t.muted,
                    1 => t.solo = if t.solo == Some(true) { None } else { Some(true) },
                    2 => t.hidden = !t.hidden,
                    _ => t.locked = !t.locked,
                }
            }
        }

        if let Some((i, k)) = track_action {
            match k {
                0 => self.insert_track_at(i),
                1 => self.insert_track_at(i + 1),
                _ => self.remove_track(i),
            }
        }

        // ---------- Lineal ----------
        let ruler = Rect::from_min_size(full.min, Vec2::new(full.width(), RULER_H));
        painter.rect_filled(ruler, 0.0, BAR);
        let rp = ui.painter_at(Rect::from_min_max(Pos2::new(x0, full.top()), Pos2::new(full.right(), top)));
        let steps = [0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0];
        let step = steps.iter().copied().find(|s| s * self.zoom >= 70.0).unwrap_or(600.0);
        let mut t = (self.scroll / step).floor() * step;
        while self.t2x(x0, t) < full.right() {
            let x = self.t2x(x0, t);
            rp.line_segment([Pos2::new(x, ruler.bottom() - 8.0), Pos2::new(x, ruler.bottom())], Stroke::new(1.0, Color32::GRAY));
            let txt = if step < 1.0 { format!("{}.{}", clock(t.floor()), ((t.fract() * 10.0).round() as i64) % 10) } else { clock(t) };
            rp.text(Pos2::new(x + 3.0, ruler.top() + 8.0), Align2::LEFT_CENTER, txt, FontId::proportional(9.5), SECONDARY);
            t += step;
        }
        // Bereichsmarken (Export): getönter Bereich und Anfangsbalken
        let (mi, mo) = (self.project.mark_in, self.project.mark_out);
        if mi.is_some() || mo.is_some() {
            let xa = self.t2x(x0, mi.unwrap_or(0.0));
            let xb = self.t2x(x0, mo.unwrap_or(self.project.duration()));
            rp.rect_filled(Rect::from_min_max(Pos2::new(xa, ruler.top()), Pos2::new(xb.max(xa + 2.0), ruler.bottom())), 0.0, ACCENT.gamma_multiply(0.28));
            if mi.is_some() {
                rp.rect_filled(Rect::from_min_size(Pos2::new(xa, ruler.top()), Vec2::new(2.0, RULER_H)), 0.0, ACCENT);
            }
            if mo.is_some() {
                rp.rect_filled(Rect::from_min_size(Pos2::new(xb - 2.0, ruler.top()), Vec2::new(2.0, RULER_H)), 0.0, ACCENT);
            }
        }
        // Kapitelmarken
        let mut marker_action: Option<(Uuid, bool)> = None;
        let mut seek_to: Option<f64> = None;
        for m in self.project.marker_list() {
            let x = self.t2x(x0, m.t);
            if x < x0 - 80.0 || x > full.right() {
                continue;
            }
            let cyan = Color32::from_rgb(0, 220, 230);
            rp.add(Shape::convex_polygon(vec![Pos2::new(x, ruler.top() + 12.0), Pos2::new(x + 8.0, ruler.top() + 16.0), Pos2::new(x, ruler.top() + 20.0)], cyan, Stroke::NONE));
            rp.line_segment([Pos2::new(x, ruler.top() + 12.0), Pos2::new(x, ruler.top() + 24.0)], Stroke::new(1.0, cyan));
            rp.text(Pos2::new(x + 10.0, ruler.top() + 16.0), Align2::LEFT_CENTER, &m.name, FontId::proportional(9.5), cyan);
            let g = rp.layout_no_wrap(m.name.clone(), FontId::proportional(9.5), cyan);
            let mr = Rect::from_min_size(Pos2::new(x - 2.0, ruler.top() + 12.0), Vec2::new(g.size().x + 14.0, 12.0)).intersect(Rect::from_min_max(Pos2::new(x0, ruler.top()), ruler.max));
            if mr.width() > 0.0 {
                let r = ui.interact(mr, ui.id().with(("marker", m.id)), Sense::click());
                if r.clicked() {
                    seek_to = Some(m.t);
                }
                r.on_hover_text(format!("{} · {}\nKlick: hinspringen · Rechtsklick: umbenennen/löschen", m.name, timecode(m.t, self.project.fps))).context_menu(|ui| {
                    if ui.button("Umbenennen …").clicked() {
                        marker_action = Some((m.id, true));
                        ui.close();
                    }
                    if ui.button("Löschen").clicked() {
                        marker_action = Some((m.id, false));
                        ui.close();
                    }
                });
            }
        }
        // Projektmarker 1–10: orangefarbene Kästchen mit der Nummer
        let mut bm_delete: Option<u32> = None;
        for (n, bt) in self.project.bookmark_list() {
            let x = self.t2x(x0, bt);
            if x < x0 - 12.0 || x > full.right() + 6.0 {
                continue;
            }
            let r = Rect::from_min_size(Pos2::new(x - 6.0, ruler.top() + 1.0), Vec2::new(12.0, 11.0));
            let orange = Color32::from_rgb(255, 140, 38);
            rp.rect_filled(r, 2.0, orange);
            rp.text(r.center(), Align2::CENTER_CENTER, n.to_string(), FontId::proportional(8.5), Color32::BLACK);
            let hit = r.intersect(Rect::from_min_max(Pos2::new(x0, ruler.top()), ruler.max));
            if hit.width() > 0.0 {
                let resp = ui.interact(hit, ui.id().with(("bookmark", n)), Sense::click());
                if resp.clicked() {
                    seek_to = Some(bt);
                }
                resp.on_hover_text(format!("Projektmarker {n} · {}\nKlick: hinspringen · Rechtsklick: löschen", timecode(bt, self.project.fps))).context_menu(|ui| {
                    if ui.button("Projektmarker löschen").clicked() {
                        bm_delete = Some(n);
                        ui.close();
                    }
                });
            }
        }
        if let Some(n) = bm_delete {
            if let Some(b) = self.project.bookmarks.as_mut() {
                b.remove(&n.to_string());
                if b.is_empty() {
                    self.project.bookmarks = None;
                }
            }
        }
        if let Some(t) = seek_to {
            self.playhead = t;
            self.set_playing(false);
        }
        if let Some((id, rename)) = marker_action {
            if rename {
                if let Some(m) = self.project.markers.as_ref().and_then(|l| l.iter().find(|m| m.id == id)) {
                    self.rename_marker = Some((id, m.name.clone()));
                }
            } else {
                self.remove_marker(id);
            }
        }
        painter.rect_filled(Rect::from_min_size(full.min, Vec2::new(HEADER_W, RULER_H)), 0.0, BAR);

        // Abspielmarker
        let px = self.t2x(x0, self.playhead);
        if px >= x0 - 1.0 && px <= full.right() {
            let bottom = (top + content_h - self.scroll_y).clamp(top + 1.0, full.bottom());
            ui.painter_at(Rect::from_min_max(Pos2::new(x0, full.top()), full.max)).line_segment([Pos2::new(px, top), Pos2::new(px, bottom.max(top + 20.0))], Stroke::new(1.5, ACCENT));
            ui.painter_at(Rect::from_min_max(Pos2::new(x0 - 8.0, full.top()), full.max)).add(Shape::convex_polygon(vec![Pos2::new(px - 8.0, top - 12.0), Pos2::new(px + 8.0, top - 12.0), Pos2::new(px, top)], ACCENT, Stroke::NONE));
        } else {
            // außerhalb: Pfeil am Rand, Klick springt hin
            let left = px < x0;
            let r = Rect::from_min_size(Pos2::new(if left { x0 } else { full.right() - 22.0 }, full.top()), Vec2::new(22.0, RULER_H));
            painter.rect_filled(r, 0.0, BAR.gamma_multiply(0.9));
            painter.text(r.center(), Align2::CENTER_CENTER, if left { "◀" } else { "▶" }, FontId::proportional(13.0), ACCENT);
            if ui.interact(r, ui.id().with("offscreen"), Sense::click()).on_hover_text("Abspielmarker liegt außerhalb – klicken, um dorthin zu springen").clicked() {
                self.scroll = (self.playhead - 0.5 * self.view_w as f64 / self.zoom).max(0.0);
            }
        }

        // ---------- Bildlaufleisten ----------
        let view_secs = self.view_w as f64 / self.zoom;
        let total_secs = (self.project.duration() + view_secs * 0.5).max(self.scroll + view_secs).max(1.0);
        let v_on = max_scroll > 0.0;
        let hbar = Rect::from_min_max(Pos2::new(x0, full.bottom() - BAR_W), Pos2::new(full.right() - if v_on { BAR_W } else { 0.0 }, full.bottom()));
        let vbar = Rect::from_min_max(Pos2::new(full.right() - BAR_W, top), Pos2::new(full.right(), full.bottom() - BAR_W));
        {
            let draw = |ui: &Ui, rs: &egui::Response, track: Rect, thumb: Rect| {
                let p = ui.painter_at(track);
                p.rect_filled(track, 0.0, Color32::from_black_alpha(90));
                let col = if rs.dragged() { 150 } else if rs.hovered() { 110 } else { 70 };
                p.rect_filled(thumb.shrink(2.0), 3.0, Color32::from_white_alpha(col));
            };
            // waagerecht (Zeit)
            let tw = hbar.width();
            let thumb_w = (view_secs / total_secs * tw as f64).clamp(28.0, tw as f64) as f32;
            let thumb_x = hbar.left() + (self.scroll / total_secs * tw as f64) as f32;
            let thumb = Rect::from_min_size(Pos2::new(thumb_x.min(hbar.right() - thumb_w), hbar.top()), Vec2::new(thumb_w, BAR_W));
            let rs = ui.interact(hbar, ui.id().with("hscroll"), Sense::click_and_drag());
            if rs.dragged() {
                self.scroll = (self.scroll + rs.drag_delta().x as f64 / tw as f64 * total_secs).clamp(0.0, (total_secs - view_secs).max(0.0));
            } else if rs.clicked() {
                if let Some(p) = rs.interact_pointer_pos().filter(|p| !thumb.contains(*p)) {
                    self.scroll = ((p.x - hbar.left()) as f64 / tw as f64 * total_secs - view_secs / 2.0).clamp(0.0, (total_secs - view_secs).max(0.0));
                }
            }
            let rs = rs.on_hover_text("Zeitleiste scrollen (auch Umschalt+Mausrad)");
            draw(ui, &rs, hbar, thumb);
            // senkrecht (Spuren)
            if v_on {
                let th = vbar.height();
                let thumb_h = (view_h / content_h * th).clamp(28.0, th);
                let thumb_y = vbar.top() + self.scroll_y / content_h * th;
                let thumb = Rect::from_min_size(Pos2::new(vbar.left(), thumb_y.min(vbar.bottom() - thumb_h)), Vec2::new(BAR_W, thumb_h));
                let rs = ui.interact(vbar, ui.id().with("vscroll"), Sense::click_and_drag());
                if rs.dragged() {
                    self.scroll_y = (self.scroll_y + rs.drag_delta().y / th * content_h).clamp(0.0, max_scroll);
                } else if rs.clicked() {
                    if let Some(p) = rs.interact_pointer_pos().filter(|p| !thumb.contains(*p)) {
                        self.scroll_y = ((p.y - vbar.top()) / th * content_h - view_h / 2.0).clamp(0.0, max_scroll);
                    }
                }
                let rs = rs.on_hover_text("Spuren scrollen (Mausrad)");
                draw(ui, &rs, vbar, thumb);
            }
        }
        let on_bar = |p: Pos2| hbar.contains(p) || (v_on && vbar.contains(p));

        // ---------- Hintergrund: Lineal, Marquee, Auswahl aufheben ----------
        let press = ui.input(|i| i.pointer.press_origin());
        if bg.drag_started() {
            if let Some(p) = press.filter(|p| !on_bar(*p)) {
                if p.x >= x0 && p.y < top {
                    self.tl.scrubbing = true;
                } else if p.x >= x0 && p.y >= top {
                    self.tl.marquee = Some((p, p));
                }
            }
        }
        if bg.dragged() {
            if let Some(p) = bg.interact_pointer_pos() {
                if self.tl.scrubbing {
                    self.seek_snapped(ui, x0, p.x);
                } else if let Some((a, _)) = self.tl.marquee {
                    self.tl.marquee = Some((a, p));
                    let r = Rect::from_two_pos(a, p);
                    let mut hit = HashSet::new();
                    for (ti, c) in &all {
                        let y0 = top + *ti as f32 * ROW_H - self.scroll_y;
                        let cr = Rect::from_min_size(Pos2::new(self.t2x(x0, c.start), y0), Vec2::new(((c.duration * self.zoom) as f32).max(1.0), ROW_H));
                        if cr.intersects(r) {
                            hit.insert(c.id);
                        }
                    }
                    let shift = ui.input(|i| i.modifiers.shift);
                    self.select_clips(hit, shift);
                }
            }
        }
        if bg.drag_stopped() {
            self.tl.scrubbing = false;
            self.tl.marquee = None;
            self.tl.snap_guide = None;
        }
        if bg.clicked() {
            if let Some(p) = bg.interact_pointer_pos().filter(|p| !on_bar(*p)) {
                if p.y < top {
                    if p.x >= x0 {
                        self.seek_snapped(ui, x0, p.x);
                        self.tl.snap_guide = None;
                    }
                } else {
                    let i = ((p.y - top + self.scroll_y) / ROW_H) as usize;
                    if i < ntr {
                        self.sel_track = i;
                    }
                    if p.x >= x0 {
                        self.clear_selection();
                    }
                }
            }
        }
    }
}
