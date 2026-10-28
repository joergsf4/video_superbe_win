//! Masken (Port von Masks.swift): neun Formen und drei Verläufe mit weicher Kante. Die Maske liegt über dem Bild des Clips
//! (vor Position und Größe) und blendet alles außerhalb aus (1 = sichtbar, 0 = ausgeblendet).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::f64::consts::PI;
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MaskKind {
    Rectangle,
    RoundedRect,
    Ellipse,
    Triangle,
    Diamond,
    Hexagon,
    Star,
    Heart,
    Frame,
    LinearGradient,
    MirrorGradient,
    RadialGradient,
}

impl MaskKind {
    pub const SHAPES: [MaskKind; 9] = [
        MaskKind::Rectangle,
        MaskKind::RoundedRect,
        MaskKind::Ellipse,
        MaskKind::Triangle,
        MaskKind::Diamond,
        MaskKind::Hexagon,
        MaskKind::Star,
        MaskKind::Heart,
        MaskKind::Frame,
    ];
    pub const GRADIENTS: [MaskKind; 3] = [MaskKind::LinearGradient, MaskKind::MirrorGradient, MaskKind::RadialGradient];

    pub fn label(self) -> &'static str {
        match self {
            MaskKind::Rectangle => "Rechteck",
            MaskKind::RoundedRect => "Abgerundet",
            MaskKind::Ellipse => "Ellipse",
            MaskKind::Triangle => "Dreieck",
            MaskKind::Diamond => "Raute",
            MaskKind::Hexagon => "Sechseck",
            MaskKind::Star => "Stern",
            MaskKind::Heart => "Herz",
            MaskKind::Frame => "Rahmen",
            MaskKind::LinearGradient => "Linear",
            MaskKind::MirrorGradient => "Spiegel",
            MaskKind::RadialGradient => "Radial",
        }
    }

    pub fn is_gradient(self) -> bool {
        matches!(self, MaskKind::LinearGradient | MaskKind::MirrorGradient | MaskKind::RadialGradient)
    }

    /// Sinnvolle Ausgangswerte beim Wählen der Maske.
    pub fn defaults(self) -> MaskSpec {
        let mut m = MaskSpec { kind: self, ..Default::default() };
        match self {
            MaskKind::Rectangle | MaskKind::RoundedRect => (m.w, m.h) = (1.4, 0.8),
            MaskKind::Frame => (m.w, m.h, m.feather) = (1.7, 0.95, 0.05),
            MaskKind::Ellipse => (m.w, m.h) = (0.85, 0.85),
            MaskKind::Triangle | MaskKind::Hexagon => (m.w, m.h) = (0.9, 0.9),
            MaskKind::Diamond | MaskKind::Star | MaskKind::Heart => (m.w, m.h) = (0.95, 0.95),
            MaskKind::LinearGradient => (m.feather, m.pos) = (0.45, 0.5),
            MaskKind::MirrorGradient => (m.feather, m.w, m.pos) = (0.3, 0.6, 0.5),
            MaskKind::RadialGradient => (m.feather, m.w) = (0.5, 0.8),
        }
        if !self.is_gradient() {
            m.feather = m.feather.max(0.03);
        }
        m
    }
}

/// Einstellungen einer Maske (alle Maße relativ zum Bild, damit sie bei jeder Auflösung gleich aussehen).
/// Alle Felder werden immer geschrieben (die Mac-Version verlangt sie).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MaskSpec {
    pub kind: MaskKind,
    /// Mitte als Anteil der Bildbreite/-höhe (0 = Mitte; x nach rechts, y nach unten)
    pub cx: f64,
    pub cy: f64,
    /// Größe als Anteil der Bildhöhe (gleiche Breite und Höhe ergeben einen Kreis bzw. ein Quadrat)
    pub w: f64,
    pub h: f64,
    /// Drehung in Grad (im Uhrzeigersinn)
    pub rot: f64,
    /// Weiche Kante (Formen) bzw. Weichheit des Übergangs (Verläufe), 0…1
    pub feather: f64,
    pub invert: bool,
    /// Verläufe: Richtung in Grad (0 = von links nach rechts sichtbar) und Lage der Kante 0…1
    pub angle: f64,
    pub pos: f64,
}

impl Default for MaskSpec {
    fn default() -> Self {
        MaskSpec { kind: MaskKind::Rectangle, cx: 0.0, cy: 0.0, w: 0.8, h: 0.8, rot: 0.0, feather: 0.05, invert: false, angle: 0.0, pos: 0.5 }
    }
}

/// Umriss der Form als Polygon in Einheitskoordinaten (−1…1, y nach unten); `None` für Formen mit eigener Prüfung.
fn polygon(kind: MaskKind) -> Option<Vec<(f64, f64)>> {
    Some(match kind {
        MaskKind::Triangle => vec![(0.0, -1.0), (1.0, 1.0), (-1.0, 1.0)],
        MaskKind::Diamond => vec![(0.0, -1.0), (1.0, 0.0), (0.0, 1.0), (-1.0, 0.0)],
        MaskKind::Hexagon => (0..6).map(|i| {
            let a = i as f64 * PI / 3.0;
            (a.cos(), -a.sin())
        }).collect(),
        MaskKind::Star => (0..10).map(|i| {
            let a = -PI / 2.0 + i as f64 * PI / 5.0;
            let k = if i % 2 == 0 { 1.0 } else { 0.42 };
            (k * a.cos(), k * a.sin())
        }).collect(),
        MaskKind::Heart => (0..=120).map(|i| {
            let t = i as f64 / 120.0 * 2.0 * PI;
            let x = 16.0 * t.sin().powi(3);
            let y = 13.0 * t.cos() - 5.0 * (2.0 * t).cos() - 2.0 * (3.0 * t).cos() - (4.0 * t).cos();
            (x / 16.0, -(y + 2.5) / 14.5)
        }).collect(),
        _ => return None,
    })
}

fn in_polygon(p: &[(f64, f64)], x: f64, y: f64) -> bool {
    let mut inside = false;
    let mut j = p.len() - 1;
    for i in 0..p.len() {
        let ((xi, yi), (xj, yj)) = (p[i], p[j]);
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Liegt der Punkt (relativ zur Formmitte, ungedreht, Pixel) innerhalb der Form mit den halben Maßen `hw`/`hh`?
fn inside(kind: MaskKind, poly: Option<&[(f64, f64)]>, x: f64, y: f64, hw: f64, hh: f64) -> bool {
    match kind {
        MaskKind::Rectangle => x.abs() <= hw && y.abs() <= hh,
        MaskKind::RoundedRect => {
            let r = (2.0 * hw).min(2.0 * hh) * 0.2;
            let (ax, ay) = (x.abs(), y.abs());
            if ax > hw || ay > hh {
                return false;
            }
            let (dx, dy) = (ax - (hw - r), ay - (hh - r));
            dx <= 0.0 || dy <= 0.0 || dx * dx + dy * dy <= r * r
        }
        MaskKind::Ellipse => hw > 0.0 && hh > 0.0 && (x / hw).powi(2) + (y / hh).powi(2) <= 1.0,
        MaskKind::Frame => {
            let (iw, ih) = (hw - 0.14 * hw, hh - 0.2 * hh);
            (x.abs() <= hw && y.abs() <= hh) && !(x.abs() <= iw && y.abs() <= ih)
        }
        _ => match poly {
            Some(p) if hw > 0.0 && hh > 0.0 => in_polygon(p, x / hw, y / hh),
            _ => false,
        },
    }
}

/// Weichzeichnen (drei Kastenfilter ≈ Gauß) mit Randwiederholung, getrennt nach Zeilen und Spalten.
fn blur(v: &mut [f32], w: usize, h: usize, sigma: f64) {
    if sigma < 0.3 || w == 0 || h == 0 {
        return;
    }
    // Kastenbreite für drei Durchläufe: σ² = n·(r²+r)/3 → r
    let r = (((12.0 * sigma * sigma / 3.0 + 1.0).sqrt() - 1.0) / 2.0).round().max(1.0) as usize;
    let mut tmp = vec![0f32; w.max(h)];
    for _ in 0..3 {
        for y in 0..h {
            box_line(&mut v[y * w..(y + 1) * w], &mut tmp[..w], r, 1);
        }
        let mut col = vec![0f32; h];
        for x in 0..w {
            for y in 0..h {
                col[y] = v[y * w + x];
            }
            box_line(&mut col, &mut tmp[..h], r, 1);
            for y in 0..h {
                v[y * w + x] = col[y];
            }
        }
    }
}

fn box_line(line: &mut [f32], tmp: &mut [f32], r: usize, _stride: usize) {
    let n = line.len();
    let at = |i: isize| line[i.clamp(0, n as isize - 1) as usize];
    let mut sum: f32 = (-(r as isize)..=r as isize).map(at).sum();
    let k = 1.0 / (2 * r + 1) as f32;
    for i in 0..n {
        tmp[i] = sum * k;
        sum += at(i as isize + r as isize + 1) - at(i as isize - r as isize);
    }
    line.copy_from_slice(&tmp[..n]);
}

/// Graustufenbild der Maske in Bildgröße (Werte 0…1, zeilenweise), unzwischengespeichert.
pub fn render(m: &MaskSpec, w: usize, h: usize) -> Vec<f32> {
    let (wf, hf) = (w as f64, h as f64);
    let mut v = vec![0f32; w * h];
    if w == 0 || h == 0 {
        return v;
    }
    if m.kind.is_gradient() {
        let soft = m.feather.max(0.01);
        let ang = m.angle.to_radians();
        let dir = (ang.cos(), ang.sin());
        let len = (wf * ang.cos()).abs() + (hf * ang.sin()).abs();
        let mid = (wf / 2.0 + dir.0 * len * (m.pos - 0.5), hf / 2.0 + dir.1 * len * (m.pos - 0.5));
        let band = m.w.max(0.02) * hf / 2.0;
        let tr = (len * soft / 2.0).max(1.0);
        let r = (m.w * hf / 2.0).max(1.0);
        let inner = r * (1.0 - soft);
        let rc = (wf / 2.0 + m.cx * wf, hf / 2.0 + m.cy * hf);
        for y in 0..h {
            for x in 0..w {
                let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                let val = match m.kind {
                    MaskKind::LinearGradient => {
                        let half = (len * soft / 2.0).max(1e-6);
                        let s = (px - mid.0) * dir.0 + (py - mid.1) * dir.1;
                        (0.5 - s / (2.0 * half)).clamp(0.0, 1.0)
                    }
                    MaskKind::MirrorGradient => {
                        // helles Band um die Mitte (Breite w·Bildhöhe), beidseitig weich auslaufend
                        let s = ((px - mid.0) * dir.0 + (py - mid.1) * dir.1).abs();
                        if s <= band { 1.0 } else { (1.0 - (s - band) / tr).clamp(0.0, 1.0) }
                    }
                    _ => {
                        let d = ((px - rc.0).powi(2) + (py - rc.1).powi(2)).sqrt();
                        let outer = r.max(inner + 1.0);
                        if d <= inner { 1.0 } else { ((outer - d) / (outer - inner)).clamp(0.0, 1.0) }
                    }
                };
                v[y * w + x] = val as f32;
            }
        }
    } else {
        let (hw, hh) = (m.w * hf / 2.0, m.h * hf / 2.0);
        let c = (wf / 2.0 + m.cx * wf, hf / 2.0 + m.cy * hf);
        let (s, co) = m.rot.to_radians().sin_cos();
        let poly = polygon(m.kind);
        // Umschließendes Rechteck der gedrehten Form begrenzt die Prüfung
        let ext = (hw * hw + hh * hh).sqrt() + 2.0;
        let (x0, x1) = (((c.0 - ext).floor().max(0.0)) as usize, ((c.0 + ext).ceil().min(wf)) as usize);
        let (y0, y1) = (((c.1 - ext).floor().max(0.0)) as usize, ((c.1 + ext).ceil().min(hf)) as usize);
        for y in y0..y1 {
            for x in x0..x1 {
                // 2×2 Unterabtastung glättet die Kante
                let mut hit = 0;
                for (ox, oy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                    let (dx, dy) = (x as f64 + ox - c.0, y as f64 + oy - c.1);
                    // Drehung im Uhrzeigersinn zurücknehmen
                    let (lx, ly) = (dx * co + dy * s, -dx * s + dy * co);
                    if inside(m.kind, poly.as_deref(), lx, ly, hw, hh) {
                        hit += 1;
                    }
                }
                v[y * w + x] = hit as f32 / 4.0;
            }
        }
        if m.feather > 0.001 {
            blur(&mut v, w, h, m.feather * 0.08 * hf);
        }
    }
    if m.invert {
        for x in &mut v {
            *x = 1.0 - *x;
        }
    }
    v
}

type Cache = Mutex<HashMap<String, Arc<Vec<f32>>>>;

fn cache() -> &'static Cache {
    static C: OnceLock<Cache> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Maske in Bildgröße, zwischengespeichert (Größe und Einstellungen sind der Schlüssel).
pub fn mask(m: &MaskSpec, w: usize, h: usize) -> Arc<Vec<f32>> {
    let key = format!("{w}x{h}|{m:?}");
    if let Some(v) = cache().lock().unwrap().get(&key) {
        return v.clone();
    }
    let v = Arc::new(render(m, w, h));
    let mut c = cache().lock().unwrap();
    if c.len() > 40 {
        c.clear();
    }
    c.insert(key, v.clone());
    v
}

/// Wendet die Maske auf ein Bild in Leinwandgröße an (RGBA mit vormultipliziertem Alpha, 8 Bit): alles außerhalb wird transparent.
pub fn apply_rgba(img: &mut [u8], w: usize, h: usize, m: &MaskSpec) {
    let mk = mask(m, w, h);
    for (px, &a) in img.chunks_exact_mut(4).zip(mk.iter()) {
        if a >= 0.999 {
            continue;
        }
        for c in px.iter_mut() {
            *c = (*c as f32 * a).round() as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(v: &[f32], w: usize, x: usize, y: usize) -> f32 {
        v[y * w + x]
    }

    #[test]
    fn shapes_hide_the_outside_and_keep_the_middle() {
        for k in MaskKind::SHAPES {
            let m = MaskSpec { feather: 0.0, ..k.defaults() };
            let v = render(&m, 160, 90);
            assert!(at(&v, 160, 1, 1) < 0.01, "{k:?}: Ecke ausgeblendet");
            if k != MaskKind::Frame {
                assert!(at(&v, 160, 80, 50) > 0.99, "{k:?}: Mitte sichtbar");
            } else {
                assert!(at(&v, 160, 80, 45) < 0.01, "Rahmen: Mitte frei");
                assert!(at(&v, 160, 80, 4) > 0.99, "Rahmen: Rand sichtbar");
            }
        }
    }

    #[test]
    fn ellipse_with_equal_size_is_a_circle_and_rotation_is_clockwise() {
        let m = MaskSpec { kind: MaskKind::Ellipse, w: 0.5, h: 0.5, feather: 0.0, ..Default::default() };
        let v = render(&m, 200, 100);
        // Radius 25 px in beide Richtungen
        assert!(at(&v, 200, 100 + 23, 50) > 0.99 && at(&v, 200, 100 + 27, 50) < 0.01);
        assert!(at(&v, 200, 100, 50 + 23) > 0.99 && at(&v, 200, 100, 50 + 27) < 0.01);
        // schmales hohes Rechteck um 90° gedreht wird breit
        let r = MaskSpec { kind: MaskKind::Rectangle, w: 0.1, h: 0.8, rot: 90.0, feather: 0.0, ..Default::default() };
        let v = render(&r, 200, 100);
        assert!(at(&v, 200, 100 + 35, 50) > 0.99 && at(&v, 200, 100, 50 + 35) < 0.01);
        // Dreieck: Spitze oben
        let t = MaskSpec { kind: MaskKind::Triangle, w: 0.8, h: 0.8, feather: 0.0, ..Default::default() };
        let v = render(&t, 200, 100);
        assert!(at(&v, 200, 100, 15) > 0.5 && at(&v, 200, 80, 15) < 0.01 && at(&v, 200, 80, 85) > 0.99);
    }

    #[test]
    fn feather_softens_and_invert_flips() {
        let hard = render(&MaskSpec { kind: MaskKind::Rectangle, w: 1.0, h: 0.5, feather: 0.0, ..Default::default() }, 200, 100);
        let soft = render(&MaskSpec { kind: MaskKind::Rectangle, w: 1.0, h: 0.5, feather: 0.5, ..Default::default() }, 200, 100);
        // an der Kante (x = 50) ist die weiche Maske halb sichtbar, die harte springt
        let e = at(&soft, 200, 50, 50);
        assert!(e > 0.3 && e < 0.7, "{e}");
        assert!(at(&hard, 200, 52, 50) > 0.99 && at(&hard, 200, 48, 50) < 0.01);
        let inv = render(&MaskSpec { kind: MaskKind::Rectangle, w: 1.0, h: 0.5, feather: 0.0, invert: true, ..Default::default() }, 200, 100);
        assert!(at(&inv, 200, 100, 50) < 0.01 && at(&inv, 200, 5, 5) > 0.99);
    }

    #[test]
    fn gradients_run_in_the_set_direction() {
        // linear, 0°: links sichtbar, rechts ausgeblendet, Mitte halb
        let v = render(&MaskKind::LinearGradient.defaults(), 200, 100);
        assert!(at(&v, 200, 2, 50) > 0.99 && at(&v, 200, 197, 50) < 0.01 && (at(&v, 200, 100, 50) - 0.5).abs() < 0.05);
        // 90°: oben sichtbar (Richtung nach unten)
        let v = render(&MaskSpec { angle: 90.0, ..MaskKind::LinearGradient.defaults() }, 200, 100);
        assert!(at(&v, 200, 100, 2) > 0.99 && at(&v, 200, 100, 97) < 0.01);
        // Spiegel: Band in der Mitte
        let v = render(&MaskKind::MirrorGradient.defaults(), 200, 100);
        assert!(at(&v, 200, 100, 50) > 0.99 && at(&v, 200, 1, 50) < 0.2);
        // radial: Mitte sichtbar, Ecke nicht
        let v = render(&MaskKind::RadialGradient.defaults(), 200, 100);
        assert!(at(&v, 200, 100, 50) > 0.99 && at(&v, 200, 1, 1) < 0.01);
    }

    #[test]
    fn apply_makes_outside_transparent_and_json_matches_mac() {
        let mut img = vec![255u8; 20 * 10 * 4];
        apply_rgba(&mut img, 20, 10, &MaskSpec { kind: MaskKind::Ellipse, w: 0.5, h: 0.5, feather: 0.0, ..Default::default() });
        assert_eq!(&img[0..4], &[0, 0, 0, 0]);
        let mid = (5 * 20 + 10) * 4;
        assert_eq!(&img[mid..mid + 4], &[255, 255, 255, 255]);
        let j = serde_json::to_value(MaskKind::RoundedRect.defaults()).unwrap();
        assert_eq!(j["kind"], "roundedRect");
        for k in ["cx", "cy", "w", "h", "rot", "feather", "invert", "angle", "pos"] {
            assert!(j.get(k).is_some(), "{k} fehlt (die Mac-Version verlangt alle Felder)");
        }
        let back: MaskSpec = serde_json::from_value(j).unwrap();
        assert_eq!(back, MaskKind::RoundedRect.defaults());
    }
}
