//! Bild- und Titelfunktionen des Mac-Originals als reine Rechenfunktionen: Greenscreen (ChromaKeyer), 3D-LUT (LUTLoader),
//! Spiegeln/Drehen/Horizont (Compositor.reorient), Titel-Platzhalter und Titelbewegung.

use crate::model::{ChromaKey, Effects};
use std::sync::Arc;

pub fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    if b <= a {
        return if x >= b { 1.0 } else { 0.0 };
    }
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// HSV mit Farbton 0…1.
pub fn rgb_to_hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let d = mx - mn;
    let h = if d < 1e-9 {
        0.0
    } else if mx == r {
        (((g - b) / d) % 6.0 + 6.0) % 6.0 / 6.0
    } else if mx == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    (h, if mx <= 0.0 { 0.0 } else { d / mx }, mx)
}

/// Greenscreen: Deckkraft (0 = ganz ausgeblendet) und Farbe nach Antispill für einen Bildpunkt (RGB 0…1).
/// Grüne, blaue und eigene Farben schlüsseln nach Farbton, Schwarz und Weiß (und Grau) nach Helligkeit.
pub fn chroma_key(r: f32, g: f32, b: f32, k: &ChromaKey) -> (f32, [f32; 3]) {
    let key = rgb_to_hsv(k.color.r as f32, k.color.g as f32, k.color.b as f32);
    let (h, s, v) = rgb_to_hsv(r, g, b);
    let tol = k.tolerance as f32;
    let soft = (k.softness as f32).max(0.001);
    let (d, weight) = if key.1 < 0.2 {
        ((v - key.2).abs(), 1.0 - smoothstep(0.15, 0.45, s))
    } else {
        let dh = (h - key.0).abs();
        (dh.min(1.0 - dh), smoothstep(0.08, 0.28, s) * smoothstep(0.04, 0.18, v))
    };
    let inside = 1.0 - smoothstep(tol, tol + soft, d);
    let a = 1.0 - inside * weight;
    let mut c = [r, g, b];
    let anti = k.antispill as f32;
    if anti > 0.001 && a > 0.0 && key.1 >= 0.2 {
        let amount = (1.0 - smoothstep(tol + soft, tol + soft + 0.06, d)) * weight * anti * 0.9;
        let y = 0.299 * r + 0.587 * g + 0.114 * b;
        for ch in &mut c {
            *ch += (y - *ch) * amount;
        }
    }
    (a, c)
}

// ---------------------------------------------------------------------------------------------
// 3D-LUT (.cube)
// ---------------------------------------------------------------------------------------------

pub struct Lut {
    pub n: usize,
    /// n³ Einträge, R läuft am schnellsten
    pub data: Vec<[f32; 3]>,
}

/// `.cube` einlesen: nur 3D-Tabellen (LUT_3D_SIZE 2…129), Domain wird auf 0…1 skaliert.
pub fn parse_cube(text: &str) -> Option<Lut> {
    let mut n = 0usize;
    let (mut dmin, mut dmax) = ([0f32; 3], [1f32; 3]);
    let mut vals: Vec<[f32; 3]> = vec![];
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') || l.starts_with("TITLE") {
            continue;
        }
        let mut it = l.split_whitespace();
        let key = it.next()?;
        match key {
            "LUT_3D_SIZE" => n = it.next()?.parse().ok()?,
            "LUT_1D_SIZE" => return None,
            "DOMAIN_MIN" | "DOMAIN_MAX" => {
                let v: Vec<f32> = it.filter_map(|x| x.parse().ok()).collect();
                if v.len() != 3 {
                    return None;
                }
                if key == "DOMAIN_MIN" {
                    dmin = [v[0], v[1], v[2]];
                } else {
                    dmax = [v[0], v[1], v[2]];
                }
            }
            _ => {
                let a: f32 = key.parse().ok()?;
                let b: f32 = it.next()?.parse().ok()?;
                let c: f32 = it.next()?.parse().ok()?;
                vals.push([a, b, c]);
            }
        }
    }
    if !(2..=129).contains(&n) || vals.len() != n * n * n {
        return None;
    }
    for v in &mut vals {
        for i in 0..3 {
            v[i] = ((v[i] - dmin[i]) / (dmax[i] - dmin[i]).max(1e-6)).clamp(0.0, 1.0);
        }
    }
    Some(Lut { n, data: vals })
}

impl Lut {
    /// Trilineare Interpolation auf den (Gamma-kodierten) Werten ohne Farbraumwandlung; `amount` mischt mit dem Original.
    pub fn apply(&self, c: [f32; 3], amount: f32) -> [f32; 3] {
        if amount <= 0.001 {
            return c;
        }
        let n = self.n;
        let m = (n - 1) as f32;
        let p = [c[0].clamp(0.0, 1.0) * m, c[1].clamp(0.0, 1.0) * m, c[2].clamp(0.0, 1.0) * m];
        // p ≥ 0: Abschneiden entspricht floor (ohne Bibliotheksaufruf)
        let i0 = [p[0] as usize, p[1] as usize, p[2] as usize];
        let f = [p[0] - i0[0] as f32, p[1] - i0[1] as f32, p[2] - i0[2] as f32];
        let (r1, g1, b1) = ((i0[0] + 1).min(n - 1), (i0[1] + 1).min(n - 1) * n, (i0[2] + 1).min(n - 1) * n * n);
        let (r0, g0, b0) = (i0[0].min(n - 1), i0[1].min(n - 1) * n, i0[2].min(n - 1) * n * n);
        let d = &self.data;
        // acht Eckpunkte, Gewichte einmal berechnet
        let corners = [
            (d[r0 + g0 + b0], (1.0 - f[0]) * (1.0 - f[1]) * (1.0 - f[2])),
            (d[r0 + g0 + b1], (1.0 - f[0]) * (1.0 - f[1]) * f[2]),
            (d[r0 + g1 + b0], (1.0 - f[0]) * f[1] * (1.0 - f[2])),
            (d[r0 + g1 + b1], (1.0 - f[0]) * f[1] * f[2]),
            (d[r1 + g0 + b0], f[0] * (1.0 - f[1]) * (1.0 - f[2])),
            (d[r1 + g0 + b1], f[0] * (1.0 - f[1]) * f[2]),
            (d[r1 + g1 + b0], f[0] * f[1] * (1.0 - f[2])),
            (d[r1 + g1 + b1], f[0] * f[1] * f[2]),
        ];
        let mut out = [0f32; 3];
        for ch in 0..3 {
            let acc: f32 = corners.iter().map(|(v, w)| v[ch] * w).sum();
            out[ch] = if amount >= 0.999 { acc } else { c[ch] + (acc - c[ch]) * amount };
        }
        out
    }
}

pub fn load_cube(path: &str) -> Option<Arc<Lut>> {
    parse_cube(&std::fs::read_to_string(path).ok()?).map(Arc::new)
}

// ---------------------------------------------------------------------------------------------
// Spiegeln, Drehen, Horizont
// ---------------------------------------------------------------------------------------------

/// Kleinster Zoom, bei dem das gedrehte Bild das ganze Rechteck wieder ausfüllt.
pub fn horizon_zoom(w: f64, h: f64, a: f64) -> f64 {
    let (c, s) = (a.cos().abs(), a.sin().abs());
    (c + s * h / w).max(c + s * w / h) * if a.abs() < 1e-6 { 1.0 } else { 1.015 }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orient {
    pub flip_h: bool,
    pub flip_v: bool,
    pub turns: u32,
    /// Grad, positiv = im Uhrzeigersinn
    pub horizon: f64,
}

impl Orient {
    /// `None`, wenn nichts zu tun ist (Spiegeln/Drehen/Horizont aus).
    pub fn from_fx(fx: &Effects) -> Option<Orient> {
        let o = Orient {
            flip_h: fx.flip_h == Some(true),
            flip_v: fx.flip_v == Some(true),
            turns: fx.quarter_turns.unwrap_or(0).rem_euclid(4) as u32,
            horizon: fx.horizon.unwrap_or(0.0),
        };
        (o.flip_h || o.flip_v || o.turns != 0 || o.horizon.abs() > 0.001).then_some(o)
    }

    /// Größe des Bildes nach dem Drehen.
    pub fn dims(&self, sw: f32, sh: f32) -> (f32, f32) {
        if self.turns % 2 == 1 { (sh, sw) } else { (sw, sh) }
    }

    /// Bildpunkt (px, py) im gespiegelten/gedrehten/begradigten Bild → Bildpunkt in der Quelle (Pixelkoordinaten, y nach unten).
    pub fn to_source(&self, px: f32, py: f32, sw: f32, sh: f32) -> (f32, f32) {
        let (ow, oh) = self.dims(sw, sh);
        let (mut x, mut y) = (px, py);
        if self.horizon.abs() > 0.001 {
            let a = self.horizon.to_radians();
            let zoom = horizon_zoom(ow as f64, oh as f64, a) as f32;
            let (cx, cy) = (ow / 2.0, oh / 2.0);
            let (dx, dy) = (x - cx, y - cy);
            let (c, s) = (a.cos() as f32, a.sin() as f32);
            x = cx + (dx * c + dy * s) / zoom;
            y = cy + (-dx * s + dy * c) / zoom;
        }
        let (mut w, mut h) = (ow, oh);
        for _ in 0..self.turns {
            let (nx, ny) = (y, w - x);
            x = nx;
            y = ny;
            std::mem::swap(&mut w, &mut h);
        }
        if self.flip_v {
            y = h - y;
        }
        if self.flip_h {
            x = w - x;
        }
        (x, y)
    }
}

// ---------------------------------------------------------------------------------------------
// Titel: Platzhalter und Bewegung
// ---------------------------------------------------------------------------------------------

fn tc(t: f64, fps: i64) -> String {
    let fps = fps.max(1);
    let f = (t * fps as f64).round() as i64;
    format!("{:02}:{:02}:{:02}:{:02}", f / (fps * 3600), f / (fps * 60) % 60, f / fps % 60, f % fps)
}

/// Aktuelles Datum und Uhrzeit (lokal): (Jahr, Monat, Tag, Stunde, Minute).
pub fn local_now() -> (i32, u32, u32, u32, u32) {
    use windows::Win32::System::SystemInformation::GetLocalTime;
    let t = unsafe { GetLocalTime() };
    (t.wYear as i32, t.wMonth as u32, t.wDay as u32, t.wHour as u32, t.wMinute as u32)
}

pub fn german_date(y: i32, m: u32, d: u32) -> String {
    const MONATE: [&str; 12] = ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"];
    format!("{d}. {} {y}", MONATE[(m.clamp(1, 12) - 1) as usize])
}

/// Platzhalter in Titeln: {datum}, {zeit}, {timecode}, {filmzeit}, {objektzeit}.
pub fn expand_tokens(text: &str, time: f64, fps: i64, object_time: Option<f64>) -> String {
    if !text.contains('{') {
        return text.to_string();
    }
    let (y, mo, d, h, mi) = local_now();
    text.replace("{datum}", &german_date(y, mo, d))
        .replace("{zeit}", &format!("{h:02}:{mi:02}"))
        .replace("{timecode}", &tc(time, fps))
        .replace("{filmzeit}", &tc(time, fps))
        .replace("{objektzeit}", &tc(object_time.unwrap_or(0.0), fps))
}

pub fn ease(x: f64) -> f64 {
    let t = x.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Bewegung eines Titels beim Ein-/Auslaufen: (Versatz x, Versatz y in Pixeln, y nach unten, Skalierung, Deckkraft).
/// `rest` 1 = ganz draußen, 0 = an Ort und Stelle.
pub fn title_motion(kind: &str, rest: f64, w: f64, h: f64) -> (f64, f64, f64, f64) {
    if rest <= 0.0 {
        return (0.0, 0.0, 1.0, 1.0);
    }
    let alpha = 1.0 - rest;
    match kind {
        "flyUp" => (0.0, rest * h * 0.2, 1.0, alpha),
        "flyLeft" => (-rest * w * 0.3, 0.0, 1.0, alpha),
        "flyRight" => (rest * w * 0.3, 0.0, 1.0, alpha),
        "zoom" => (0.0, 0.0, 1.0 - 0.4 * rest, alpha),
        _ => (0.0, 0.0, 1.0, 1.0),
    }
}

/// Foto-Effekte (Mac: „Foto KI“ ohne Motiverkennung): `denoise` 0…1 glättet Rauschen kantenerhaltend (Sigma-Filter),
/// `enhance` 0…1 blendet eine automatische Korrektur ein: Tonwerte spreizen (0,5 %/99,5 %), Farbstich zur Hälfte
/// ausgleichen (Grauwelt), Mitten auf mittlere Helligkeit ziehen, Sättigung leicht anheben. RGBA, Alpha bleibt.
pub fn photo_fx(px: &mut [u8], w: usize, h: usize, enhance: f64, denoise: f64) {
    let n = w * h;
    if n == 0 || px.len() < n * 4 {
        return;
    }
    if denoise > 0.001 {
        let sigma = ((0.6 + 1.6 * denoise) * w.max(h) as f64 / 1000.0).max(0.6) as f32;
        let mut blurred: Vec<f32> = px.iter().map(|v| *v as f32).collect();
        // drei Kastenfilter je Richtung ≈ Gauß, Kanal für Kanal
        let r = ((((12.0 * sigma * sigma / 3.0 + 1.0).sqrt() - 1.0) / 2.0).round() as usize).max(1);
        for c in 0..4 {
            let mut ch: Vec<f32> = (0..n).map(|i| blurred[i * 4 + c]).collect();
            box3(&mut ch, w, h, r);
            for i in 0..n {
                blurred[i * 4 + c] = ch[i];
            }
        }
        let sr = (6.0 + 22.0 * denoise as f32).powi(2) * 2.0;
        let k = denoise as f32;
        for i in 0..n {
            let o = i * 4;
            let d2: f32 = (0..3).map(|c| (px[o + c] as f32 - blurred[o + c]).powi(2)).sum::<f32>() / 3.0;
            // Abweichungen in Größe des Rauschens werden geglättet, Kanten (große Abweichung) bleiben
            let wgt = (-d2 / sr).exp() * k.min(1.0);
            for c in 0..3 {
                px[o + c] = (px[o + c] as f32 * (1.0 - wgt) + blurred[o + c] * wgt).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    if enhance > 0.001 {
        let mut hist = [0u32; 256];
        let mut sum = [0f64; 3];
        for p in px.chunks_exact(4) {
            let l = (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) as usize;
            hist[l.min(255)] += 1;
            for c in 0..3 {
                sum[c] += p[c] as f64;
            }
        }
        let pct = |q: f64| {
            let target = (n as f64 * q) as u32;
            let mut acc = 0u32;
            for (i, v) in hist.iter().enumerate() {
                acc += v;
                if acc > target {
                    return i as f32;
                }
            }
            255.0
        };
        // Spreizung der Helligkeit, höchstens 2,2fach (flache oder einfarbige Bilder werden nicht verzerrt)
        let (p_lo, p_hi) = (pct(0.005), pct(0.995));
        let range = (p_hi - p_lo).max(255.0 / 2.2);
        let lo = ((p_lo + p_hi) / 2.0 - range / 2.0).clamp(0.0, 255.0 - range);
        let mean = [sum[0] / n as f64, sum[1] / n as f64, sum[2] / n as f64];
        let gray = (mean[0] + mean[1] + mean[2]) / 3.0;
        // halber Grauwelt-Abgleich, begrenzt (bunte Motive bleiben bunt)
        let wb: Vec<f32> = mean.iter().map(|m| (1.0 + 0.5 * (gray / m.max(1.0) - 1.0)).clamp(0.85, 1.15) as f32).collect();
        // Mittlere Helligkeit nach dem Spreizen → Gamma, das sie Richtung 0,46 zieht (höchstens moderat)
        let mid = ((gray as f32 - lo) / range).clamp(0.05, 0.95);
        let gamma = ((0.46f32).ln() / mid.ln()).clamp(0.7, 1.4);
        let k = enhance as f32;
        for p in px.chunks_exact_mut(4) {
            let c = [p[0] as f32 * wb[0], p[1] as f32 * wb[1], p[2] as f32 * wb[2]];
            let l = (0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]).max(0.5);
            let l2 = ((l - lo) / range).clamp(0.0, 1.0).powf(gamma) * 255.0;
            let s = l2 / l;
            for i in 0..3 {
                // Farben gemeinsam skalieren (Farbton bleibt), Sättigung leicht anheben
                let v = (l2 + (c[i] * s - l2) * 1.12).clamp(0.0, 255.0);
                p[i] = (p[i] as f32 * (1.0 - k) + v * k).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// Drei Kastenfilter (Radius `r`) waagerecht und senkrecht, Rand wiederholt.
fn box3(v: &mut [f32], w: usize, h: usize, r: usize) {
    let mut line = vec![0f32; w.max(h)];
    let mut tmp = vec![0f32; w.max(h)];
    let pass = |src: &[f32], dst: &mut [f32], len: usize| {
        let at = |i: isize| src[i.clamp(0, len as isize - 1) as usize];
        let mut s: f32 = (-(r as isize)..=r as isize).map(at).sum();
        let k = 1.0 / (2 * r + 1) as f32;
        for i in 0..len {
            dst[i] = s * k;
            s += at(i as isize + r as isize + 1) - at(i as isize - r as isize);
        }
    };
    for _ in 0..3 {
        for y in 0..h {
            line[..w].copy_from_slice(&v[y * w..(y + 1) * w]);
            pass(&line[..w], &mut tmp[..w], w);
            v[y * w..(y + 1) * w].copy_from_slice(&tmp[..w]);
        }
        for x in 0..w {
            for y in 0..h {
                line[y] = v[y * w + x];
            }
            pass(&line[..h], &mut tmp[..h], h);
            for y in 0..h {
                v[y * w + x] = tmp[y];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Rgba;

    #[test]
    fn photo_enhance_stretches_dull_pictures_and_denoise_smooths_noise() {
        // flaues, bläuliches Bild: Werte 90…150
        let (w, h) = (64usize, 48usize);
        let mut px: Vec<u8> = (0..w * h).flat_map(|i| {
            let v = (90 + (i % w) * 60 / w) as u8;
            [v, v, v.saturating_add(20), 255]
        }).collect();
        let range = |p: &[u8]| {
            let l: Vec<u8> = p.chunks(4).map(|c| c[1]).collect();
            (*l.iter().max().unwrap() as i32) - (*l.iter().min().unwrap() as i32)
        };
        let before = range(&px);
        let orig = px.clone();
        photo_fx(&mut px, w, h, 1.0, 0.0);
        assert!(range(&px) > before + 40, "Kontrast gespreizt: {} → {}", before, range(&px));
        let blue_cast = |p: &[u8]| p.chunks(4).map(|c| c[2] as f64 - c[0] as f64).sum::<f64>() / (w * h) as f64;
        assert!(blue_cast(&px) < blue_cast(&orig), "Farbstich gemindert");
        assert!(px.chunks(4).all(|c| c[3] == 255), "Alpha bleibt");
        // halbe Stärke liegt dazwischen
        let mut half = orig.clone();
        photo_fx(&mut half, w, h, 0.5, 0.0);
        assert!(range(&half) > before && range(&half) < range(&px));
        // Rauschen auf grauer Fläche wird kleiner, eine harte Kante bleibt
        let mut noisy: Vec<u8> = (0..w * h).flat_map(|i| {
            let n = ((i * 7919) % 23) as i32 - 11;
            let base = if i % w < w / 2 { 60 } else { 200 };
            let v = (base + n).clamp(0, 255) as u8;
            [v, v, v, 255]
        }).collect();
        let var = |p: &[u8], x0: usize, x1: usize| {
            let v: Vec<f64> = (0..h).flat_map(|y| (x0..x1).map(move |x| (y, x))).map(|(y, x)| p[(y * w + x) * 4] as f64).collect();
            let m = v.iter().sum::<f64>() / v.len() as f64;
            v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / v.len() as f64
        };
        let v0 = var(&noisy, 4, 26);
        photo_fx(&mut noisy, w, h, 0.0, 1.0);
        assert!(var(&noisy, 4, 26) < v0 * 0.6, "Rauschen gemindert: {v0} → {}", var(&noisy, 4, 26));
        let (l, r) = (noisy[(10 * w + w / 2 - 3) * 4] as i32, noisy[(10 * w + w / 2 + 2) * 4] as i32);
        assert!(r - l > 100, "Kante bleibt: {l} {r}");
    }

    #[test]
    fn greenscreen_keys_green_and_keeps_skin_red_and_grey() {
        let k = ChromaKey::default();
        assert!(chroma_key(0.1, 0.75, 0.2, &k).0 < 0.05, "Grün weg");
        assert!(chroma_key(0.9, 0.7, 0.6, &k).0 > 0.95, "Hautton bleibt");
        assert!(chroma_key(0.8, 0.1, 0.1, &k).0 > 0.95, "Rot bleibt");
        assert!(chroma_key(0.5, 0.5, 0.5, &k).0 > 0.95, "Grau bleibt");
        let black = ChromaKey { color: Rgba { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }, ..Default::default() };
        assert!(chroma_key(0.02, 0.02, 0.02, &black).0 < 0.05 && chroma_key(0.8, 0.8, 0.8, &black).0 > 0.95 && chroma_key(0.8, 0.1, 0.1, &black).0 > 0.95, "Schwarz-Schlüssel nach Helligkeit");
        let white = ChromaKey { color: Rgba { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, ..Default::default() };
        assert!(chroma_key(0.98, 0.98, 0.98, &white).0 < 0.05 && chroma_key(0.2, 0.2, 0.2, &white).0 > 0.95, "Weiß-Schlüssel nach Helligkeit");
        // Antispill: grünlicher Saum nahe am Schlüssel wird entsättigt
        let (a, c) = chroma_key(0.55, 0.70, 0.45, &ChromaKey { tolerance: 0.05, softness: 0.05, antispill: 1.0, ..Default::default() });
        assert!(a > 0.5 && c[1] < 0.70, "Antispill dämpft Grün: a={a} g={}", c[1]);
    }

    #[test]
    fn cube_files_parse_and_apply() {
        let mut ident = String::from("LUT_3D_SIZE 2\n");
        let mut inv = String::from("TITLE \"inv\"\nLUT_3D_SIZE 2\n");
        for b in 0..2 {
            for g in 0..2 {
                for r in 0..2 {
                    ident += &format!("{r} {g} {b}\n");
                    inv += &format!("{} {} {}\n", 1 - r, 1 - g, 1 - b);
                }
            }
        }
        assert!(parse_cube(&ident).is_some() && parse_cube("LUT_3D_SIZE 2\n0 0 0\n").is_none() && parse_cube("LUT_1D_SIZE 4\n").is_none());
        let id = parse_cube(&ident).unwrap();
        let o = id.apply([0.3, 0.6, 0.9], 1.0);
        assert!((o[0] - 0.3).abs() < 1e-4 && (o[1] - 0.6).abs() < 1e-4 && (o[2] - 0.9).abs() < 1e-4, "Identität: {o:?}");
        let lut = parse_cube(&inv).unwrap();
        let a = lut.apply([0.8, 0.4, 0.1], 1.0);
        assert!((a[0] * 255.0 - 51.0).abs() <= 3.0 && (a[1] * 255.0 - 153.0).abs() <= 3.0 && (a[2] * 255.0 - 230.0).abs() <= 3.0, "Umkehr-LUT: {a:?}");
        let h = lut.apply([0.8, 0.4, 0.1], 0.5);
        assert!((h[0] * 255.0 - 128.0).abs() <= 4.0, "Stärke 50 %: {}", h[0] * 255.0);
        // DOMAIN skaliert
        let scaled = "DOMAIN_MIN 0 0 0\nDOMAIN_MAX 2 2 2\nLUT_3D_SIZE 2\n0 0 0\n2 0 0\n0 2 0\n2 2 0\n0 0 2\n2 0 2\n0 2 2\n2 2 2\n";
        let s = parse_cube(scaled).unwrap();
        assert!((s.apply([1.0, 1.0, 1.0], 1.0)[0] - 1.0).abs() < 1e-6, "Wertebereich auf 0…1 skaliert");
    }

    #[test]
    fn orientation_flips_turns_and_maps_back_to_the_source() {
        // 2×1-Bild: links rot, rechts blau. Spiegeln: rechts des Ergebnisses = Quelle links.
        let o = Orient::from_fx(&Effects { flip_h: Some(true), ..Default::default() }).unwrap();
        let (x, _) = o.to_source(0.5, 0.5, 2.0, 1.0);
        assert!((x - 1.5).abs() < 1e-5, "Pixel links im Ergebnis stammt von rechts: {x}");
        // 90° im Uhrzeigersinn: aus 2×1 wird 1×2, links → oben
        let o = Orient::from_fx(&Effects { quarter_turns: Some(1), ..Default::default() }).unwrap();
        assert_eq!(o.dims(2.0, 1.0), (1.0, 2.0));
        let (x, _) = o.to_source(0.5, 0.5, 2.0, 1.0); // oben im Ergebnis
        assert!(x < 1.0, "oben = Quelle links: {x}");
        let (x, _) = o.to_source(0.5, 1.5, 2.0, 1.0); // unten
        assert!(x > 1.0, "unten = Quelle rechts: {x}");
        assert!(Orient::from_fx(&Effects { quarter_turns: Some(4), ..Default::default() }).is_none(), "vier Drehungen = keine");
        assert!(Orient::from_fx(&Effects::default()).is_none());
        // Horizont: Zoom deckt die Ränder, 0° ohne Zoom
        assert!((horizon_zoom(1920.0, 1080.0, 0.0) - 1.0).abs() < 1e-9);
        let hz = horizon_zoom(1920.0, 1080.0, 5f64.to_radians());
        assert!(hz > 1.1 && hz < 1.32, "5° zoomt um {hz}");
        // Alle Ecken des begradigten Bildes liegen innerhalb der Quelle
        let o = Orient::from_fx(&Effects { horizon: Some(5.0), ..Default::default() }).unwrap();
        for (px, py) in [(0.0, 0.0), (64.0, 0.0), (0.0, 36.0), (64.0, 36.0)] {
            let (x, y) = o.to_source(px, py, 64.0, 36.0);
            assert!((-0.5..=64.5).contains(&x) && (-0.5..=36.5).contains(&y), "Ecke ({px},{py}) → ({x},{y}) liegt in der Quelle");
        }
        // Spiegeln beides + Drehen kombiniert bleibt umkehrbar im Bereich
        let o = Orient { flip_h: true, flip_v: true, turns: 3, horizon: 0.0 };
        let (x, y) = o.to_source(0.3, 1.7, 2.0, 1.0);
        assert!((0.0..=2.0).contains(&x) && (0.0..=1.0).contains(&y));
    }

    #[test]
    fn tokens_motion_and_dates() {
        assert_eq!(expand_tokens("{timecode}", 2.5, 30, None), "00:00:02:15");
        assert_eq!(expand_tokens("{objektzeit} / {filmzeit}", 3.0, 25, Some(1.2)), "00:00:01:05 / 00:00:03:00");
        assert_eq!(expand_tokens("kein Platzhalter", 0.0, 30, None), "kein Platzhalter");
        assert_ne!(expand_tokens("{datum}", 0.0, 30, None), "{datum}");
        assert_eq!(german_date(2026, 10, 20), "20. Oktober 2026");
        assert!((ease(0.5) - 0.5).abs() < 1e-9 && ease(0.0) == 0.0 && ease(1.0) == 1.0);
        let (dx, dy, z, a) = title_motion("flyUp", 1.0, 1000.0, 500.0);
        assert!(dx == 0.0 && (dy - 100.0).abs() < 1e-9 && z == 1.0 && a == 0.0);
        let (dx, ..) = title_motion("flyLeft", 0.5, 1000.0, 500.0);
        assert!((dx + 150.0).abs() < 1e-9);
        let (_, _, z, a) = title_motion("zoom", 0.5, 1000.0, 500.0);
        assert!((z - 0.8).abs() < 1e-9 && (a - 0.5).abs() < 1e-9);
        assert_eq!(title_motion("flyUp", 0.0, 10.0, 10.0), (0.0, 0.0, 1.0, 1.0), "an Ort und Stelle: keine Bewegung");
    }
}
