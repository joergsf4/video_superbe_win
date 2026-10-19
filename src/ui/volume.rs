//! Pegellinie im Clip (VolumeLine.swift). Auf Tonclips = Lautstärke (0…200 %), auf Bild-/Titelclips = Deckkraft
//! (0…100 %). Reine Rechenfunktionen, damit sich die Gesten ohne Oberfläche testen lassen.

use crate::model::{Clip, ClipContent, VolumePoint};

pub const INSET: f64 = 7.0;

/// Obergrenze der Pegellinie: Lautstärke bis 200 %, Deckkraft bis 100 %.
pub fn max_level(c: &Clip) -> f64 {
    if c.is_audio() { 2.0 } else { 1.0 }
}

pub fn y_of(gain: f64, height: f64, max_g: f64) -> f64 {
    INSET + (height - 2.0 * INSET) * (1.0 - gain.clamp(0.0, max_g) / max_g)
}

pub fn gain_at_y(y: f64, height: f64, max_g: f64) -> f64 {
    (max_g * (1.0 - (y - INSET) / (height - 2.0 * INSET))).clamp(0.0, max_g)
}

/// Knickpunkte, zwischen denen die Kurve (ohne Ein-/Ausblenden) linear verläuft.
pub fn breakpoints(c: &Clip) -> Vec<f64> {
    if c.volume_points.is_empty() {
        return vec![0.0, c.duration];
    }
    let mut v: Vec<f64> = vec![0.0, c.duration];
    v.extend(c.volume_points.iter().map(|p| p.t).filter(|t| *t > 0.0 && *t < c.duration));
    v.sort_by(|a, b| a.total_cmp(b));
    v.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    v
}

/// Mittelpunkte der Linienstücke (Zeit, Pegel, Index des Stücks zwischen Punkt i und i+1; −1 ohne Punkte).
pub fn segments(c: &Clip) -> Vec<(f64, f64, isize)> {
    if c.volume_points.len() < 2 {
        return vec![(c.duration / 2.0, c.curve_gain(c.duration / 2.0), -1)];
    }
    let mut s = c.volume_points.clone();
    s.sort_by(|a, b| a.t.total_cmp(&b.t));
    s.windows(2).enumerate().map(|(i, w)| ((w[0].t + w[1].t) / 2.0, (w[0].gain + w[1].gain) / 2.0, i as isize)).collect()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Point(usize),
    Whole,
    Segment(isize),
}

/// Ausgangszustand einer Geste.
#[derive(Clone, Debug)]
pub struct Start {
    pub mode: Mode,
    pub points: Vec<VolumePoint>,
    pub volume: f64,
    pub grab_gain: f64,
}

/// Neuer Zustand (Punkte, Grundpegel) nach einer Ziehbewegung.
#[derive(Clone, Debug, PartialEq)]
pub struct Result {
    pub points: Option<Vec<VolumePoint>>,
    pub level: Option<f64>,
}

/// Greift die Linie bei `press_t` (Sekunden): nahe einem Punkt = Punkt ziehen, sonst die ganze Kurve.
pub fn begin_line(c: &Clip, press_t: f64, tol_s: f64) -> Start {
    let mode = match c.volume_points.iter().position(|p| (p.t - press_t).abs() < tol_s) {
        Some(i) => Mode::Point(i),
        None => Mode::Whole,
    };
    Start { mode, points: c.volume_points.clone(), volume: c.volume, grab_gain: if matches!(mode, Mode::Whole) { c.curve_gain(press_t.clamp(0.0, c.duration)) } else { 0.0 } }
}

/// Linie ziehen: `gain` = Pegel unter dem Zeiger.
pub fn line_drag(s: &Start, gain: f64, max_g: f64) -> Result {
    match s.mode {
        Mode::Point(i) => {
            let mut p = s.points.clone();
            p[i].gain = gain;
            Result { points: Some(p), level: None }
        }
        _ => {
            // ganze Kurve anheben/absenken, relativ zur Griffstelle
            let scale = if s.grab_gain > 0.02 { gain / s.grab_gain } else { 1.0 };
            let max_cur = s.points.iter().map(|p| p.gain).fold(s.volume, f64::max);
            let k = if max_cur * scale > max_g { max_g / max_cur } else { scale };
            let level = if s.grab_gain > 0.02 { (s.volume * k).min(max_g) } else { gain };
            let points = (!s.points.is_empty()).then(|| s.points.iter().map(|p| VolumePoint { t: p.t, gain: (p.gain * k).min(max_g) }).collect());
            Result { points, level: Some(level) }
        }
    }
}

/// Kreis in der Mitte eines Stücks: verschiebt beide Endpunkte (bzw. die ganze Linie) senkrecht um `d_gain`.
pub fn segment_drag(s: &Start, index: isize, d_gain: f64, max_g: f64) -> Result {
    if s.points.len() < 2 {
        return Result { points: None, level: Some((s.volume + d_gain).clamp(0.0, max_g)) };
    }
    let mut p = s.points.clone();
    p.sort_by(|a, b| a.t.total_cmp(&b.t));
    for j in [index as usize, index as usize + 1] {
        p[j].gain = (p[j].gain + d_gain).clamp(0.0, max_g);
    }
    Result { points: Some(p), level: None }
}

/// Doppelklick: Punkt bei `t` entfernen, falls einer in der Nähe ist, sonst setzen.
pub fn double_tap(c: &mut Clip, t: f64, tol_s: f64) {
    if let Some(i) = c.volume_points.iter().position(|p| (p.t - t).abs() < tol_s) {
        c.volume_points.remove(i);
        return;
    }
    let t = t.clamp(0.0, c.duration);
    let g = c.curve_gain(t);
    if c.volume_points.is_empty() {
        // Start- und Endpunkt anlegen, damit jedes Stück seinen eigenen Kreis bekommt
        c.volume_points = vec![VolumePoint { t: 0.0, gain: g }, VolumePoint { t: c.duration, gain: g }];
    }
    c.volume_points.push(VolumePoint { t, gain: g });
    c.volume_points.sort_by(|a, b| a.t.total_cmp(&b.t));
}

/// Wendet das Ergebnis einer Geste auf den Clip an.
pub fn apply(c: &mut Clip, r: &Result) {
    if let Some(p) = &r.points {
        c.volume_points = p.clone();
    }
    if let Some(l) = r.level {
        c.volume = l;
    }
}

pub fn has_line(c: &Clip) -> bool {
    !matches!(c.content, ClipContent::Title(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ClipPart, Wrap};
    use uuid::Uuid;

    fn audio() -> Clip {
        Clip { content: ClipContent::Media(Wrap { v: Uuid::new_v4() }), part: ClipPart::Audio, duration: 10.0, ..Default::default() }
    }

    #[test]
    fn y_and_gain_roundtrip() {
        for g in [0.0, 0.5, 1.0, 1.7, 2.0] {
            assert!((gain_at_y(y_of(g, 50.0, 2.0), 50.0, 2.0) - g).abs() < 1e-9);
        }
        assert_eq!(y_of(2.0, 50.0, 2.0), INSET);
        assert_eq!(y_of(0.0, 50.0, 2.0), 50.0 - INSET);
        assert_eq!(gain_at_y(-100.0, 50.0, 1.0), 1.0, "oben begrenzt");
        assert_eq!(gain_at_y(500.0, 50.0, 1.0), 0.0, "unten begrenzt");
    }

    #[test]
    fn max_level_differs_for_audio_and_picture() {
        assert_eq!(max_level(&audio()), 2.0);
        assert_eq!(max_level(&Clip { part: ClipPart::Video, ..audio() }), 1.0);
    }

    #[test]
    fn double_tap_adds_points_and_removes_them_again() {
        let mut c = audio();
        double_tap(&mut c, 4.0, 0.1);
        assert_eq!(c.volume_points.iter().map(|p| p.t).collect::<Vec<_>>(), vec![0.0, 4.0, 10.0], "Start, Klickstelle, Ende");
        assert!(c.volume_points.iter().all(|p| p.gain == 1.0));
        double_tap(&mut c, 4.05, 0.1);
        assert_eq!(c.volume_points.len(), 2, "Doppelklick auf einen Punkt entfernt ihn");
        assert_eq!(segments(&c).len(), 1);
    }

    #[test]
    fn whole_line_drag_scales_curve_and_clamps() {
        let mut c = audio();
        c.volume = 1.0;
        let s = begin_line(&c, 5.0, 0.1);
        assert_eq!(s.mode, Mode::Whole);
        let r = line_drag(&s, 0.5, 2.0);
        assert_eq!(r.level, Some(0.5));
        assert!(r.points.is_none());
        let r = line_drag(&s, 5.0, 2.0);
        assert_eq!(r.level, Some(2.0), "bei 200 % begrenzt");
        // mit Punkten: alle skalieren, Obergrenze verhindert Verzerren
        c.volume_points = vec![VolumePoint { t: 0.0, gain: 1.0 }, VolumePoint { t: 10.0, gain: 1.5 }];
        let s = begin_line(&c, 2.0, 0.1);
        let r = line_drag(&s, 2.0, 2.0);
        let pts = r.points.unwrap();
        assert!(pts.iter().all(|p| p.gain <= 2.0 + 1e-9));
        assert!((pts[1].gain / pts[0].gain - 1.5).abs() < 1e-6, "Verhältnis bleibt");
    }

    #[test]
    fn point_and_segment_drag() {
        let mut c = audio();
        c.volume_points = vec![VolumePoint { t: 0.0, gain: 1.0 }, VolumePoint { t: 5.0, gain: 1.0 }, VolumePoint { t: 10.0, gain: 1.0 }];
        let s = begin_line(&c, 5.02, 0.1);
        assert_eq!(s.mode, Mode::Point(1));
        let r = line_drag(&s, 0.3, 2.0);
        assert_eq!(r.points.unwrap()[1].gain, 0.3);
        let s = Start { mode: Mode::Segment(0), points: c.volume_points.clone(), volume: 1.0, grab_gain: 0.0 };
        let r = segment_drag(&s, 0, 0.5, 2.0);
        let p = r.points.unwrap();
        assert_eq!((p[0].gain, p[1].gain, p[2].gain), (1.5, 1.5, 1.0));
        let r = segment_drag(&s, 0, 9.0, 2.0);
        assert_eq!(r.points.unwrap()[0].gain, 2.0);
        // ohne Punkte hebt das Mittelstück die Grundlautstärke
        let plain = Start { mode: Mode::Segment(-1), points: vec![], volume: 0.8, grab_gain: 0.0 };
        assert_eq!(segment_drag(&plain, -1, -0.3, 2.0).level, Some(0.5));
        let mut c2 = audio();
        apply(&mut c2, &segment_drag(&plain, -1, 0.4, 2.0));
        assert!((c2.volume - 1.2).abs() < 1e-9 || (c2.volume - 1.4).abs() < 1e-9);
    }

    #[test]
    fn breakpoints_and_titles() {
        let mut c = audio();
        assert_eq!(breakpoints(&c), vec![0.0, 10.0]);
        c.volume_points = vec![VolumePoint { t: 3.0, gain: 1.0 }, VolumePoint { t: 12.0, gain: 1.0 }];
        assert_eq!(breakpoints(&c), vec![0.0, 3.0, 10.0], "Punkte außerhalb zählen nicht");
        assert!(!has_line(&Clip::default()), "Titel haben keine Pegellinie");
    }
}
