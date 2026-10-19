//! Rahmen mit Anfassern über dem Vorschaumonitor: Objekt verschieben, an den Ecken skalieren, oben drehen
//! (ObjectOverlay.swift).

use super::*;
use crate::anim::Channel;
use crate::app::App;
use crate::model::{Clip, ClipContent, Project, Transform2D};
use eframe::egui::{Pos2, Rect, Stroke};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OverlayKind {
    Move,
    Scale(usize),
    Rotate,
}

#[derive(Clone, Debug)]
pub struct OverlayDrag {
    pub kind: OverlayKind,
    pub clip: Uuid,
    pub start: Transform2D,
    /// Rahmen beim Start der Geste
    pub box0: OverlayBox,
    pub pointer0: Pos2,
}

const CORNERS: [(f32, f32); 4] = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];

/// Lage des Objektrahmens in Bildschirmkoordinaten.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverlayBox {
    pub center: Pos2,
    pub w: f32,
    pub h: f32,
    /// Bogenmaß, im Uhrzeigersinn
    pub rot: f32,
    pub view: Vec2,
    pub scale: f64,
}

impl OverlayBox {
    pub fn corner(&self, sx: f32, sy: f32) -> Pos2 {
        let (dx, dy) = (sx * self.w / 2.0, sy * self.h / 2.0);
        Pos2::new(self.center.x + dx * self.rot.cos() - dy * self.rot.sin(), self.center.y + dx * self.rot.sin() + dy * self.rot.cos())
    }
    pub fn knob(&self) -> Pos2 {
        let top = self.corner(0.0, -1.0);
        Pos2::new(top.x + self.rot.sin() * 24.0, top.y - self.rot.cos() * 24.0)
    }
}

/// Größe des Objekts bei Skalierung 100 % in Ausgabepixeln (Bilder/Videos passen ins Bild, Titel sind eng).
pub fn intrinsic_size(clip: &Clip, p: &Project, title_size: impl FnOnce() -> Option<(usize, usize)>) -> Option<Vec2> {
    let cv = Vec2::new(p.render_width as f32, p.render_height as f32);
    match &clip.content {
        ClipContent::Title(_) => title_size().map(|(w, h)| Vec2::new(w as f32, h as f32)),
        ClipContent::Media(m) => {
            let a = p.asset(m.v)?;
            if !(a.has_video || a.is_image) || a.width <= 0.0 || a.height <= 0.0 {
                return None;
            }
            let fx = &clip.fx;
            let sw = a.width as f32 * (1.0 - fx.crop_l as f32 - fx.crop_r as f32);
            let sh = a.height as f32 * (1.0 - fx.crop_t as f32 - fx.crop_b as f32);
            if sw <= 1.0 || sh <= 1.0 {
                return None;
            }
            if fx.fill {
                return Some(cv);
            }
            let f = (cv.x / sw).min(cv.y / sh);
            Some(Vec2::new(sw * f, sh * f))
        }
    }
}

pub fn make_box(frame: Rect, canvas_w: f32, size0: Vec2, tr: &Transform2D) -> OverlayBox {
    let view = frame.size();
    let k = view.x / canvas_w;
    OverlayBox {
        center: Pos2::new(frame.center().x + tr.x as f32 * view.x, frame.center().y + tr.y as f32 * view.y),
        w: size0.x * k * tr.scale as f32,
        h: size0.y * k * tr.scale as f32,
        rot: (tr.rotation as f32).to_radians(),
        view,
        scale: tr.scale,
    }
}

/// Verschieben: rastet in der Bildmitte ein.
pub fn move_transform(start: &Transform2D, delta: Vec2, view: Vec2) -> Transform2D {
    let mut t = start.clone();
    t.x = start.x + (delta.x / view.x) as f64;
    t.y = start.y + (delta.y / view.y) as f64;
    if t.x.abs() < 0.01 {
        t.x = 0.0;
    }
    if t.y.abs() < 0.01 {
        t.y = 0.0;
    }
    t
}

/// Skalieren über eine Ecke: Verhältnis der Abstände Mitte–Zeiger und Mitte–Ecke.
pub fn scale_transform(start: &Transform2D, box0: &OverlayBox, corner: usize, pointer: Pos2) -> Transform2D {
    let c0 = box0.corner(CORNERS[corner].0, CORNERS[corner].1);
    let d0 = (c0 - box0.center).length() as f64;
    let d1 = (pointer - box0.center).length() as f64;
    let mut t = start.clone();
    if d0 > 1.0 {
        t.scale = (box0.scale * d1 / d0).clamp(0.05, 4.0);
    }
    t
}

/// Drehen: Winkel zwischen „oben“ und dem Zeiger; rastet bei 0° ein.
pub fn rotate_transform(start: &Transform2D, center: Pos2, pointer: Pos2) -> Transform2D {
    let mut t = start.clone();
    let mut deg = ((pointer.x - center.x) as f64).atan2(-((pointer.y - center.y) as f64)).to_degrees();
    if deg.abs() < 2.0 {
        deg = 0.0;
    }
    t.rotation = deg;
    t
}

impl App {
    /// Das ausgewählte Bild-/Titelobjekt, falls es an der Abspielposition sichtbar ist.
    pub(crate) fn overlay_clip(&self) -> Option<Clip> {
        let c = self.selected.and_then(|id| self.project.clip(id))?;
        (!c.is_audio() && self.playhead >= c.start - 1e-6 && self.playhead <= c.end() + 1e-6).then(|| c.clone())
    }

    /// Setzt die Transformation zur aktuellen Zeit; animierte Kanäle bekommen dabei einen Keyframe.
    pub(crate) fn apply_transform(&mut self, id: Uuid, t: &Transform2D) {
        let playhead = self.playhead;
        if let Some(c) = self.project.clip_mut(id) {
            let rel = playhead - c.start;
            for ch in Channel::TRANSFORM {
                if (c.value(ch, rel) - t.get(ch)).abs() > 1e-9 {
                    c.set_channel(ch, t.get(ch), rel);
                }
            }
        }
    }

    pub(crate) fn draw_overlay(&mut self, ui: &mut Ui, frame: Rect) {
        let Some(clip) = self.overlay_clip() else {
            self.overlay_drag = None;
            return;
        };
        let canvas_h = self.project.render_height.max(1) as usize;
        let size0 = {
            let proj = &self.project;
            let r = &mut self.preview.renderer;
            intrinsic_size(&clip, proj, || match &clip.content {
                ClipContent::Title(s) => r.title_size(&s.v, canvas_h),
                _ => None,
            })
        };
        let Some(size0) = size0 else { return };
        let rel = (self.playhead - clip.start).max(0.0);
        let tr_now = clip.transform_at(rel);
        let live = self.overlay_drag.as_ref().filter(|d| d.clip == clip.id).map(|d| d.box0);
        let bx = make_box(frame, self.project.render_width.max(1) as f32, size0, &tr_now);
        let yellow = Color32::from_rgb(255, 214, 10);
        let painter = ui.painter().clone();
        let pts: Vec<Pos2> = CORNERS.iter().map(|c| bx.corner(c.0, c.1)).collect();
        painter.add(egui::Shape::closed_line(pts.clone(), Stroke::new(1.5, yellow)));
        let top = bx.corner(0.0, -1.0);
        let knob = bx.knob();
        painter.line_segment([top, knob], Stroke::new(1.0, yellow));
        painter.circle_filled(knob, 5.5, yellow);

        // Eigene Rechtecke zur Treffererkennung
        self.overlay_geom = Some((bx, frame));
        let mut start_drag: Option<(OverlayKind, Pos2)> = None;
        let mut ids = vec![];
        for (i, p) in pts.iter().enumerate() {
            let r = Rect::from_center_size(*p, Vec2::splat(14.0));
            painter.rect_filled(Rect::from_center_size(*p, Vec2::splat(10.0)), 0.0, Color32::WHITE);
            painter.rect_stroke(Rect::from_center_size(*p, Vec2::splat(10.0)), 0.0, Stroke::new(1.0, Color32::BLACK), egui::StrokeKind::Inside);
            ids.push((OverlayKind::Scale(i), r));
        }
        ids.push((OverlayKind::Rotate, Rect::from_center_size(knob, Vec2::splat(16.0))));
        // Bewegen: Innenfläche des gedrehten Rahmens (näherungsweise über das umschließende Rechteck der Ecken)
        let hull = pts.iter().fold(Rect::from_min_max(pts[0], pts[0]), |r, p| r.union(Rect::from_min_max(*p, *p)));
        ids.push((OverlayKind::Move, hull));
        // Anfasser haben Vorrang vor der Fläche: zuerst die kleinen Rechtecke interagieren
        let mut responses = vec![];
        for (k, r) in ids.iter().rev() {
            let id = ui.id().with(("overlay", format!("{k:?}")));
            responses.push((*k, ui.interact(*r, id, Sense::drag())));
        }
        for (k, r) in &responses {
            if r.drag_started() {
                start_drag = Some((*k, ui.input(|i| i.pointer.press_origin()).or(r.interact_pointer_pos()).unwrap_or(bx.center)));
                break;
            }
        }
        if let Some((kind, p0)) = start_drag {
            self.overlay_drag = Some(OverlayDrag { kind, clip: clip.id, start: tr_now.clone(), box0: bx, pointer0: p0 });
        }
        let _ = live;
        if let Some(d) = self.overlay_drag.clone() {
            let down = responses.iter().any(|(_, r)| r.dragged());
            if down {
                if let Some(p) = ui.ctx().pointer_interact_pos() {
                    let t = match d.kind {
                        OverlayKind::Move => move_transform(&d.start, p - d.pointer0, d.box0.view),
                        OverlayKind::Scale(i) => scale_transform(&d.start, &d.box0, i, p),
                        OverlayKind::Rotate => rotate_transform(&d.start, d.box0.center, p),
                    };
                    self.apply_transform(d.clip, &t);
                }
            } else if !ui.ctx().input(|i| i.pointer.any_down()) {
                self.overlay_drag = None;
            }
        }
        if responses.iter().any(|(k, r)| r.hovered() && *k == OverlayKind::Move) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> Rect {
        Rect::from_min_size(Pos2::new(100.0, 50.0), Vec2::new(640.0, 360.0))
    }

    #[test]
    fn box_maps_canvas_to_view_and_rotates() {
        let tr = Transform2D { x: 0.1, y: -0.1, scale: 0.5, rotation: 0.0 };
        let b = make_box(frame(), 1920.0, Vec2::new(1920.0, 1080.0), &tr);
        assert_eq!(b.center, Pos2::new(100.0 + 320.0 + 64.0, 50.0 + 180.0 - 36.0));
        assert_eq!((b.w, b.h), (320.0, 180.0));
        let c = b.corner(1.0, -1.0);
        assert!((c.x - (b.center.x + 160.0)).abs() < 1e-3 && (c.y - (b.center.y - 90.0)).abs() < 1e-3);
        let r = OverlayBox { rot: std::f32::consts::FRAC_PI_2, ..b };
        let c = r.corner(1.0, 0.0); // rechte Mitte → nach unten
        assert!((c.x - b.center.x).abs() < 1e-3 && (c.y - (b.center.y + 160.0)).abs() < 1e-3);
        let k = b.knob();
        assert!((k.x - b.center.x).abs() < 1e-3 && (k.y - (b.center.y - 90.0 - 24.0)).abs() < 1e-3);
    }

    #[test]
    fn move_snaps_to_center() {
        let start = Transform2D { x: 0.2, y: 0.0, scale: 1.0, rotation: 0.0 };
        let t = move_transform(&start, Vec2::new(-128.0, 3.0), Vec2::new(640.0, 360.0));
        assert_eq!(t.x, 0.0, "0,2 − 0,2 rastet bei 0 ein");
        assert_eq!(t.y, 0.0, "kleine Abweichung rastet ein");
        let t = move_transform(&start, Vec2::new(64.0, 36.0), Vec2::new(640.0, 360.0));
        assert!((t.x - 0.3).abs() < 1e-6 && (t.y - 0.1).abs() < 1e-6);
    }

    #[test]
    fn scale_follows_pointer_distance() {
        let start = Transform2D::default();
        let b = make_box(frame(), 1920.0, Vec2::new(1920.0, 1080.0), &start);
        let corner = b.corner(1.0, 1.0);
        // Zeiger doppelt so weit von der Mitte wie die Ecke → Skalierung 2
        let p = Pos2::new(b.center.x + (corner.x - b.center.x) * 2.0, b.center.y + (corner.y - b.center.y) * 2.0);
        let t = scale_transform(&start, &b, 2, p);
        assert!((t.scale - 2.0).abs() < 1e-6);
        let t = scale_transform(&start, &b, 2, b.center);
        assert_eq!(t.scale, 0.05, "Untergrenze");
        let t = scale_transform(&start, &b, 2, Pos2::new(b.center.x + 5000.0, b.center.y));
        assert_eq!(t.scale, 4.0, "Obergrenze");
    }

    #[test]
    fn rotate_points_knob_at_pointer() {
        let start = Transform2D::default();
        let c = Pos2::new(200.0, 200.0);
        assert_eq!(rotate_transform(&start, c, Pos2::new(200.0, 100.0)).rotation, 0.0, "oben = 0°");
        assert!((rotate_transform(&start, c, Pos2::new(300.0, 200.0)).rotation - 90.0).abs() < 1e-6, "rechts = 90°");
        assert!((rotate_transform(&start, c, Pos2::new(200.0, 300.0)).rotation.abs() - 180.0).abs() < 1e-6);
        assert_eq!(rotate_transform(&start, c, Pos2::new(203.0, 100.0)).rotation, 0.0, "rastet bei kleinem Winkel ein");
    }

    #[test]
    fn intrinsic_size_fits_fills_and_crops() {
        use crate::model::*;
        let mut p = Project::default(); // 1920×1080
        let a = MediaAsset { width: 1080.0, height: 1920.0, has_video: true, ..Default::default() };
        let aid = a.id;
        p.assets.push(a);
        let mut c = Clip { content: ClipContent::Media(Wrap { v: aid }), ..Default::default() };
        let s = intrinsic_size(&c, &p, || None).unwrap();
        assert!((s.x - 607.5).abs() < 0.5 && (s.y - 1080.0).abs() < 0.5, "Hochformat im Querformat eingepasst: {s:?}");
        c.fx.fill = true;
        assert_eq!(intrinsic_size(&c, &p, || None).unwrap(), Vec2::new(1920.0, 1080.0));
        c.fx.fill = false;
        c.fx.crop_l = 0.5;
        let s = intrinsic_size(&c, &p, || None).unwrap();
        assert!(s.x < 607.5 + 1.0 && s.y > 1000.0 && s.x > 100.0, "{s:?}");
        let t = Clip::default();
        assert_eq!(intrinsic_size(&t, &p, || Some((300, 80))).unwrap(), Vec2::new(300.0, 80.0));
    }
}
