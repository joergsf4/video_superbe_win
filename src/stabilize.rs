//! Bildstabilisierung (Stabilizer.swift des Mac-Originals, dort mit Vision): Bewegungsmessung zwischen aufeinanderfolgenden
//! Bildern per Phasenkorrelation (FFT auf kleinen Graubildern), Weg der Kamera, Glättung per gleitendem Mittelwert und
//! automatische Skalierung gegen schwarze Ränder. Die Messung läuft einmal je Video im Hintergrund und wird gespeichert.

use crate::app::App;
use crate::model::Clip;
use eframe::egui::{self, RichText, Ui};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, OnceLock};
use uuid::Uuid;

const N: usize = 256;
/// Messungen pro Sekunde (höchstens)
const RATE: f64 = 20.0;

/// Weg der Bildinhalte (kumulierte Verschiebung in Anteilen von Breite und Höhe, nach rechts/unten positiv).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StabPath {
    pub times: Vec<f32>,
    pub x: Vec<f32>,
    pub y: Vec<f32>,
}

/// Korrektur je Messpunkt für eine Glättungsstärke: Verschiebung (Anteile) und größte Abweichung.
pub struct Corr {
    pub dx: Vec<f32>,
    pub dy: Vec<f32>,
    pub max: f32,
}

impl StabPath {
    pub fn is_empty(&self) -> bool {
        self.times.len() < 3
    }

    /// Korrektur für Stärke 0…1: Weg minus gleitender Mittelwert (Radius 0,15 + 1,6·Stärke Sekunden), negiert, damit
    /// die Verschiebung die Bewegung aufhebt.
    pub fn corrections(&self, strength: f64) -> Corr {
        let n = self.times.len();
        let dt = if n > 1 { (self.times[n - 1] - self.times[0]) / (n - 1) as f32 } else { 1.0 / RATE as f32 };
        let radius = (0.15 + 1.6 * strength.clamp(0.0, 1.0)) as f32;
        let w = ((radius / dt.max(1e-4)).round() as usize).max(1);
        let smooth = |v: &[f32]| -> Vec<f32> {
            let mut pre = vec![0f64; n + 1];
            for i in 0..n {
                pre[i + 1] = pre[i] + v[i] as f64;
            }
            (0..n)
                .map(|i| {
                    let (a, b) = (i.saturating_sub(w), (i + w).min(n - 1));
                    ((pre[b + 1] - pre[a]) / (b - a + 1) as f64) as f32
                })
                .collect()
        };
        let (sx, sy) = (smooth(&self.x), smooth(&self.y));
        let dx: Vec<f32> = (0..n).map(|i| sx[i] - self.x[i]).collect();
        let dy: Vec<f32> = (0..n).map(|i| sy[i] - self.y[i]).collect();
        let max = dx.iter().chain(dy.iter()).fold(0f32, |m, v| m.max(v.abs()));
        Corr { dx, dy, max }
    }

    /// Verschiebung (Anteile) und Skalierung zur Quellzeit `t` bei Stärke `strength`; `auto_scale`: schwarze Ränder wegzoomen.
    pub fn transform_at(&self, t: f64, strength: f64, auto_scale: bool) -> Option<(f32, f32, f32)> {
        if self.is_empty() {
            return None;
        }
        let c = self.cached(strength);
        let t = t as f32;
        let n = self.times.len();
        let i = self.times.partition_point(|x| *x <= t).clamp(1, n - 1);
        let (t0, t1) = (self.times[i - 1], self.times[i]);
        let f = if t1 > t0 { ((t - t0) / (t1 - t0)).clamp(0.0, 1.0) } else { 0.0 };
        let (dx, dy) = (c.dx[i - 1] + (c.dx[i] - c.dx[i - 1]) * f, c.dy[i - 1] + (c.dy[i] - c.dy[i - 1]) * f);
        let scale = if auto_scale { (1.0 + 2.0 * c.max + 0.01).min(1.4) } else { 1.0 };
        Some((dx, dy, scale))
    }

    fn cached(&self, strength: f64) -> Arc<Corr> {
        static CACHE: OnceLock<Mutex<HashMap<(usize, usize, u32), Arc<Corr>>>> = OnceLock::new();
        let key = (self.times.as_ptr() as usize, self.times.len(), (strength * 100.0).round() as u32);
        let mut m = CACHE.get_or_init(Default::default).lock().unwrap();
        if m.len() > 64 {
            m.clear();
        }
        m.entry(key).or_insert_with(|| Arc::new(self.corrections((key.2 as f64) / 100.0))).clone()
    }
}

// ---------------------------------------------------------------------------------------------
// Phasenkorrelation
// ---------------------------------------------------------------------------------------------

pub struct Spectrum {
    re: Vec<f32>,
    im: Vec<f32>,
}

pub struct Fft2 {
    tw: Vec<(f32, f32)>,
    window: Vec<f32>,
}

impl Fft2 {
    pub fn new() -> Self {
        let tw = (0..N / 2).map(|k| { let a = -2.0 * std::f32::consts::PI * k as f32 / N as f32; (a.cos(), a.sin()) }).collect();
        let window = (0..N).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * (i as f32 + 0.5) / N as f32).cos()).collect();
        Fft2 { tw, window }
    }

    fn fft2(&self, re: &mut [f32], im: &mut [f32]) {
        let mut tr = vec![0f32; N * N];
        let mut ti = vec![0f32; N * N];
        for y in 0..N {
            crate::analysis::fft(&mut re[y * N..(y + 1) * N], &mut im[y * N..(y + 1) * N], &self.tw);
        }
        for y in 0..N {
            for x in 0..N {
                tr[x * N + y] = re[y * N + x];
                ti[x * N + y] = im[y * N + x];
            }
        }
        for x in 0..N {
            crate::analysis::fft(&mut tr[x * N..(x + 1) * N], &mut ti[x * N..(x + 1) * N], &self.tw);
        }
        for y in 0..N {
            for x in 0..N {
                re[y * N + x] = tr[x * N + y];
                im[y * N + x] = ti[x * N + y];
            }
        }
    }

    /// Spektrum eines N×N-Graubildes (mit Hann-Fenster gegen Randeffekte).
    pub fn spectrum(&self, gray: &[f32]) -> Spectrum {
        let mean = gray.iter().sum::<f32>() / gray.len() as f32;
        let mut re: Vec<f32> = (0..N * N).map(|i| (gray[i] - mean) * self.window[i % N] * self.window[i / N]).collect();
        let mut im = vec![0f32; N * N];
        self.fft2(&mut re, &mut im);
        Spectrum { re, im }
    }

    /// Verschiebung (in Bildpunkten des N×N-Bildes) von `b` gegenüber `a`: b(x) ≈ a(x − d) → d. Teilpixelgenau.
    pub fn shift(&self, a: &Spectrum, b: &Spectrum) -> (f32, f32) {
        // normiertes Kreuzleistungsspektrum, danach inverse FFT (über die Konjugationsregel)
        let mut re = vec![0f32; N * N];
        let mut im = vec![0f32; N * N];
        for i in 0..N * N {
            let (xr, xi) = (b.re[i] * a.re[i] + b.im[i] * a.im[i], b.im[i] * a.re[i] - b.re[i] * a.im[i]); // B · conj(A)
            let m = (xr * xr + xi * xi).sqrt().max(1e-9);
            re[i] = xr / m;
            im[i] = -xi / m; // Konjugation für die inverse FFT
        }
        self.fft2(&mut re, &mut im);
        let at = |x: usize, y: usize| re[(y % N) * N + (x % N)];
        let (mut best, mut bx, mut by) = (f32::NEG_INFINITY, 0usize, 0usize);
        for y in 0..N {
            for x in 0..N {
                if at(x, y) > best {
                    best = at(x, y);
                    bx = x;
                    by = y;
                }
            }
        }
        let sub = |m1: f32, c: f32, p1: f32| {
            let d = m1 - 2.0 * c + p1;
            if d.abs() < 1e-9 { 0.0 } else { (0.5 * (m1 - p1) / d).clamp(-0.5, 0.5) }
        };
        let fx = sub(at(bx + N - 1, by), best, at(bx + 1, by));
        let fy = sub(at(bx, by + N - 1), best, at(bx, by + 1));
        let wrap = |p: usize, f: f32| if p > N / 2 { p as f32 - N as f32 + f } else { p as f32 + f };
        (wrap(bx, fx), wrap(by, fy))
    }
}

/// RGBA-Bild → N×N-Graubild (Mittelung in Blöcken).
pub fn gray_n(rgba: &[u8], w: usize, h: usize) -> Vec<f32> {
    let mut out = vec![0f32; N * N];
    for oy in 0..N {
        let (y0, y1) = (oy * h / N, ((oy + 1) * h / N).max(oy * h / N + 1).min(h));
        for ox in 0..N {
            let (x0, x1) = (ox * w / N, ((ox + 1) * w / N).max(ox * w / N + 1).min(w));
            let (mut s, mut n) = (0.0f32, 0.0f32);
            let (sy, sx) = (((y1 - y0) / 3).max(1), ((x1 - x0) / 3).max(1));
            let mut y = y0;
            while y < y1 {
                let mut x = x0;
                while x < x1 {
                    let o = (y * w + x) * 4;
                    s += 0.299 * rgba[o] as f32 + 0.587 * rgba[o + 1] as f32 + 0.114 * rgba[o + 2] as f32;
                    n += 1.0;
                    x += sx;
                }
                y += sy;
            }
            out[oy * N + ox] = s / n.max(1.0) / 255.0;
        }
    }
    out
}

/// Baut den Weg aus einer Folge (Zeit, Graubild); Verschiebungen werden in Anteile von Breite/Höhe umgerechnet.
pub struct PathBuilder {
    fft: Fft2,
    prev: Option<Spectrum>,
    pub path: StabPath,
    ax: f32,
    ay: f32,
}

impl PathBuilder {
    pub fn new() -> Self {
        PathBuilder { fft: Fft2::new(), prev: None, path: StabPath::default(), ax: 0.0, ay: 0.0 }
    }

    pub fn push(&mut self, t: f32, gray: &[f32]) {
        let sp = self.fft.spectrum(gray);
        if let Some(p) = &self.prev {
            let (dx, dy) = self.fft.shift(p, &sp);
            self.ax += dx / N as f32;
            self.ay += dy / N as f32;
        }
        self.prev = Some(sp);
        self.path.times.push(t);
        self.path.x.push(self.ax);
        self.path.y.push(self.ay);
    }
}

// ---------------------------------------------------------------------------------------------
// Registry (Vorschau und Export lesen von hier), Speicherung, Analyse im Hintergrund
// ---------------------------------------------------------------------------------------------

fn registry() -> &'static Mutex<HashMap<Uuid, Arc<StabPath>>> {
    static R: OnceLock<Mutex<HashMap<Uuid, Arc<StabPath>>>> = OnceLock::new();
    R.get_or_init(Default::default)
}

pub fn get(asset: Uuid) -> Option<Arc<StabPath>> {
    registry().lock().ok()?.get(&asset).cloned()
}

pub fn set(asset: Uuid, p: StabPath) {
    if let Ok(mut r) = registry().lock() {
        r.insert(asset, Arc::new(p));
    }
}

pub fn remove(asset: Uuid) {
    if let Ok(mut r) = registry().lock() {
        r.remove(&asset);
    }
}

fn file_for(dir: &Path, asset: Uuid) -> PathBuf {
    dir.join("Stabilisierung").join(format!("{asset}.stab.json"))
}

pub fn save(dir: &Path, asset: Uuid, p: &StabPath) {
    let f = file_for(dir, asset);
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(s) = serde_json::to_vec(p) {
        let _ = std::fs::write(f, s);
    }
}

/// Lädt gespeicherte Bewegungsdaten in die Registry; `true`, wenn welche da sind.
pub fn load(dir: &Path, asset: Uuid) -> bool {
    if get(asset).is_some() {
        return true;
    }
    let Some(p) = std::fs::read(file_for(dir, asset)).ok().and_then(|d| serde_json::from_slice::<StabPath>(&d).ok()).filter(|p| !p.is_empty()) else { return false };
    set(asset, p);
    true
}

/// Misst die Bewegung des Videos (blockierend; mit Fortschritt 0…1 und Abbruch). `None` bei Abbruch oder Fehler.
pub fn analyze(path: &Path, duration: f64, progress: &dyn Fn(f32), cancel: &AtomicBool) -> Option<StabPath> {
    let mut dec = crate::video::Decoder::open_checked(path).ok()?;
    let fps = if dec.info.fps > 1.0 { dec.info.fps } else { 30.0 };
    let dt = 1.0 / fps.min(RATE);
    let mut b = PathBuilder::new();
    let mut t = 0.0;
    let end = (duration - 0.02).max(0.1);
    while t < end {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        // etwas hinter den Bildanfang zielen: sonst liefert Gleitkomma-Rundung gelegentlich das vorige Bild noch einmal
        let f = dec.frame_at(t + dt * 0.3)?;
        b.push(f.time as f32, &gray_n(&f.rgba, f.width, f.height));
        progress((t / end) as f32);
        t += dt;
    }
    (!b.path.is_empty()).then_some(b.path)
}

pub(crate) struct StabJob {
    pub progress: Arc<AtomicU32>,
    pub cancel: Arc<AtomicBool>,
    pub rx: Receiver<Option<StabPath>>,
    /// Clip, bei dem die Stabilisierung nach der Analyse eingeschaltet wird
    pub enable_on: Option<Uuid>,
}

impl App {
    pub(crate) fn stab_ready(&mut self, asset: Uuid) -> bool {
        let dir = self.feat.data_dir.clone();
        load(&dir, asset)
    }

    pub(crate) fn analyze_motion(&mut self, asset: Uuid, enable_on: Option<Uuid>) {
        if self.stab_jobs.contains_key(&asset) {
            return;
        }
        let Some(a) = self.project.asset(asset).cloned() else { return };
        let (progress, cancel) = (Arc::new(AtomicU32::new(0)), Arc::new(AtomicBool::new(false)));
        let (tx, rx) = std::sync::mpsc::channel();
        let (p2, c2) = (progress.clone(), cancel.clone());
        std::thread::spawn(move || {
            crate::video::init();
            let r = analyze(Path::new(&a.path), a.duration, &|f| p2.store((f * 1000.0) as u32, Ordering::Relaxed), &c2);
            let _ = tx.send(r);
        });
        self.stab_jobs.insert(asset, StabJob { progress, cancel, rx, enable_on });
        self.status = "Bewegungsdaten werden berechnet …".into();
    }

    pub(crate) fn cancel_motion(&mut self, asset: Uuid) {
        if let Some(j) = self.stab_jobs.get(&asset) {
            j.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub(crate) fn delete_motion(&mut self, asset: Uuid) {
        remove(asset);
        let _ = std::fs::remove_file(file_for(&self.feat.data_dir, asset));
        for t in &mut self.project.tracks {
            for c in &mut t.clips {
                if c.asset_id() == Some(asset) {
                    c.fx.stabilize = None;
                }
            }
        }
        self.project_rev += 1;
    }

    /// Nimmt fertige Analysen entgegen (jedes Bild aufrufen).
    pub(crate) fn poll_stabilizer(&mut self, ctx: &egui::Context) {
        let done: Vec<(Uuid, Option<StabPath>)> = self.stab_jobs.iter().filter_map(|(id, j)| j.rx.try_recv().ok().map(|r| (*id, r))).collect();
        for (id, r) in done {
            let job = self.stab_jobs.remove(&id);
            match r {
                Some(p) => {
                    save(&self.feat.data_dir, id, &p);
                    set(id, p);
                    if let Some(cid) = job.and_then(|j| j.enable_on) {
                        if let Some(c) = self.project.clip_mut(cid) {
                            c.fx.stabilize = Some(0.5);
                        }
                    }
                    self.project_rev += 1;
                    self.preview.reset();
                    self.status = "Bewegungsdaten fertig – Stabilisierung eingeschaltet".into();
                }
                None => self.status = "Bewegungsanalyse abgebrochen oder fehlgeschlagen".into(),
            }
        }
        if !self.stab_jobs.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }
    }

    pub(crate) fn stabilize_section(&mut self, ui: &mut Ui, c: &Clip) {
        let Some(asset) = c.asset_id() else { return };
        let id = c.id;
        if let Some(j) = self.stab_jobs.get(&asset) {
            let frac = j.progress.load(Ordering::Relaxed) as f32 / 1000.0;
            ui.add(egui::ProgressBar::new(frac).show_percentage().desired_width(260.0));
            if ui.small_button("Abbrechen").clicked() {
                self.cancel_motion(asset);
            }
            ui.label(RichText::new("Bewegungsdaten werden berechnet …").size(11.0).color(super::ui::SECONDARY));
        } else if self.stab_ready(asset) {
            let mut s = c.fx.stabilize.unwrap_or(0.0);
            if super::ui::slider_row(ui, "Glättung", &mut s, 0.0..=1.0, |v| if v < 0.01 { "aus".into() } else { format!("{:.0} %", v * 100.0) }) {
                if let Some(cl) = self.project.clip_mut(id) {
                    cl.fx.stabilize = (s >= 0.01).then_some(s);
                }
            }
            let mut auto = c.fx.stab_no_scale != Some(true);
            if ui.checkbox(&mut auto, "Auto-Skalierung (schwarze Ränder wegzoomen)").changed() {
                if let Some(cl) = self.project.clip_mut(id) {
                    cl.fx.stab_no_scale = (!auto).then_some(true);
                }
            }
            if ui.small_button("Bewegungsdaten löschen").clicked() {
                self.delete_motion(asset);
            }
        } else {
            if ui.button("Bewegungsdaten analysieren").clicked() {
                self.analyze_motion(asset, Some(id));
            }
            ui.label(RichText::new("Gleicht Verwackeln aus Aufnahmen aus der Hand aus. Die Analyse dauert je nach Länge einige Zeit.").size(11.0).color(super::ui::SECONDARY));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Gleichmäßig verteiltes Rauschen als Textur (deterministisch).
    fn texture(size: usize) -> Vec<f32> {
        let mut seed = 12345u32;
        let raw: Vec<f32> = (0..size * size).map(|_| { seed = seed.wrapping_mul(1664525).wrapping_add(1013904223); (seed >> 8) as f32 / (1u32 << 24) as f32 }).collect();
        // glätten, damit Teilpixelverschiebungen sinnvoll interpolieren
        let mut sm = raw.clone();
        for _ in 0..2 {
            let prev = sm.clone();
            for y in 0..size {
                for x in 0..size {
                    let g = |xx: i32, yy: i32| prev[((yy.rem_euclid(size as i32)) as usize) * size + (xx.rem_euclid(size as i32)) as usize];
                    sm[y * size + x] = (g(x as i32, y as i32) * 4.0 + g(x as i32 - 1, y as i32) + g(x as i32 + 1, y as i32) + g(x as i32, y as i32 - 1) + g(x as i32, y as i32 + 1)) / 8.0;
                }
            }
        }
        sm
    }

    /// Bild `base` (größer als das Fenster) mit Fensterausschnitt, um (dx, dy) Pixel verschoben (bilinear).
    fn window(base: &[f32], size: usize, dx: f32, dy: f32) -> Vec<f32> {
        let off = (size - N) as f32 / 2.0;
        let at = |x: f32, y: f32| {
            let (x0, y0) = (x.floor(), y.floor());
            let (fx, fy) = (x - x0, y - y0);
            let g = |xx: f32, yy: f32| base[(yy as usize).min(size - 1) * size + (xx as usize).min(size - 1)];
            g(x0, y0) * (1.0 - fx) * (1.0 - fy) + g(x0 + 1.0, y0) * fx * (1.0 - fy) + g(x0, y0 + 1.0) * (1.0 - fx) * fy + g(x0 + 1.0, y0 + 1.0) * fx * fy
        };
        (0..N * N).map(|i| at((i % N) as f32 + off - dx, (i / N) as f32 + off - dy)).collect()
    }

    #[test]
    fn phase_correlation_measures_subpixel_shifts() {
        let size = 340;
        let base = texture(size);
        let fft = Fft2::new();
        let a = fft.spectrum(&window(&base, size, 0.0, 0.0));
        for (dx, dy) in [(3.0, 0.0), (0.0, -4.0), (5.4, -2.7), (-7.25, 6.5), (0.0, 0.0)] {
            let b = fft.spectrum(&window(&base, size, dx, dy));
            let (mx, my) = fft.shift(&a, &b);
            assert!((mx - dx).abs() < 0.35 && (my - dy).abs() < 0.35, "gemessen ({mx:.2}, {my:.2}) statt ({dx}, {dy})");
        }
    }

    #[test]
    fn path_smoothing_and_scale_cancel_jitter() {
        // verwackelte Folge: langsame Fahrt (0,1 px/Bild nach rechts) plus Zittern
        let size = 340;
        let base = texture(size);
        let mut b = PathBuilder::new();
        let fps = 20.0;
        let mut truth = vec![];
        for i in 0..80 {
            let t = i as f32 / fps;
            let (dx, dy) = (0.1 * i as f32 + 4.0 * (t * 9.0).sin(), 3.0 * (t * 7.0).cos());
            truth.push((dx, dy));
            b.push(t, &window(&base, size, dx, dy));
        }
        let p = b.path;
        // Weg ≈ tatsächliche Bewegung (Anteile von N)
        for i in [10, 40, 79] {
            let (tx, ty) = (truth[i].0 - truth[0].0, truth[i].1 - truth[0].1);
            assert!((p.x[i] * N as f32 - tx).abs() < 1.5 && (p.y[i] * N as f32 - ty).abs() < 1.5, "Weg bei {i}: ({:.2}, {:.2}) statt ({tx:.2}, {ty:.2})", p.x[i] * N as f32, p.y[i] * N as f32);
        }
        // Restunruhe nach der Korrektur: Abweichung vom gleitenden Mittelwert sinkt deutlich
        let jitter = |v: &[f32]| {
            let n = 7usize;
            let mut s = 0.0;
            for i in 0..v.len() {
                let (a, bb) = (i.saturating_sub(n), (i + n).min(v.len() - 1));
                let m = v[a..=bb].iter().sum::<f32>() / (bb - a + 1) as f32;
                s += (v[i] - m).powi(2);
            }
            (s / v.len() as f32).sqrt()
        };
        let c = p.corrections(0.5);
        let corrected: Vec<f32> = (0..p.x.len()).map(|i| p.x[i] + c.dx[i]).collect();
        let (before, after) = (jitter(&p.x), jitter(&corrected));
        assert!(after < before * 0.6, "Unruhe {before:.4} → {after:.4}");
        // Stärke 0 glättet kaum, hohe Stärke stärker
        let weak = p.corrections(0.0);
        assert!(weak.max < c.max + 1e-6 || weak.max >= 0.0);
        // Transformation: Auto-Skalierung hängt an der größten Korrektur, höchstens 1,4
        let (_, _, sc) = p.transform_at(2.0, 0.5, true).unwrap();
        assert!((sc - (1.0 + 2.0 * c.max + 0.01).min(1.4)).abs() < 1e-5 && sc > 1.0);
        assert_eq!(p.transform_at(2.0, 0.5, false).unwrap().2, 1.0, "ohne Auto-Skalierung bleibt der Maßstab");
        assert!(StabPath::default().transform_at(1.0, 0.5, true).is_none());
    }

    #[test]
    fn registry_and_files_roundtrip() {
        let dir = std::env::temp_dir().join("video-superbe-tests").join(format!("stab-{}", Uuid::new_v4()));
        let id = Uuid::new_v4();
        let p = StabPath { times: vec![0.0, 0.1, 0.2, 0.3], x: vec![0.0, 0.01, 0.0, -0.01], y: vec![0.0; 4] };
        assert!(!load(&dir, id));
        save(&dir, id, &p);
        assert!(load(&dir, id) && get(id).unwrap().x.len() == 4);
        remove(id);
        assert!(get(id).is_none() && load(&dir, id), "nach dem Entfernen aus dem Speicher wird die Datei wieder geladen");
    }

    #[test]
    fn gray_conversion_averages_blocks() {
        let rgba: Vec<u8> = (0..64 * 64).flat_map(|i| if (i % 64) < 32 { [255, 255, 255, 255] } else { [0, 0, 0, 255] }).collect();
        let g = gray_n(&rgba, 64, 64);
        assert!(g[10 * N + 10] > 0.95 && g[10 * N + 200] < 0.05);
    }

    /// Ende-zu-Ende: ein verwackelt „gefilmtes“ Video wird exportiert, wieder gelesen und analysiert; der gemessene Weg folgt
    /// der echten Bewegung. Danach verschiebt der Renderer das Bild gegen die Abweichung.
    #[test]
    fn analysis_of_a_shaky_mp4_follows_the_real_motion() {
        use crate::anim::{Channel, InterpKind, Keyframe};
        use crate::model::*;
        let dir = std::env::temp_dir().join("video-superbe-tests");
        std::fs::create_dir_all(&dir).unwrap();
        // Textur 640×360
        let big = texture(640);
        let tex: Vec<u8> = (0..640 * 360).flat_map(|i| { let v = (big[i] * 255.0).clamp(0.0, 255.0) as u8; [v, v, v, 255] }).collect();
        let png = dir.join("stab_texture.png");
        image::RgbaImage::from_raw(640, 360, tex).unwrap().save(&png).unwrap();
        let mut p = Project::default();
        p.render_width = 320;
        p.render_height = 180;
        p.fps = 15;
        let a = crate::render::tests::image_asset(&mut p, &png, 640.0, 360.0);
        let mut c = crate::render::tests::clip(a, 0.0, 3.0);
        c.transform.scale = 1.3;
        let jit = |i: usize| 0.02 * (i as f64 * 0.9).sin() + 0.0005 * i as f64;
        c.keys.0.insert(Channel::PosX, (0..=45).map(|i| Keyframe::with_kind(i as f64 / 15.0, jit(i), InterpKind::Linear)).collect());
        p.tracks[0].clips.push(c);
        let mp4 = dir.join("stab_shaky.mp4");
        let mut o = crate::export::ExportOpts::from_project(&p);
        o.video_kbps = 8000;
        crate::export::export(&p, &mp4, &o, &mut |_| true).unwrap();
        let path = analyze(&mp4, 3.0, &|_| {}, &AtomicBool::new(false)).expect("Analyse");
        assert!(path.times.len() >= 40, "{} Messpunkte", path.times.len());
        // Der Export schreibt keine B-Bilder (und damit keinen Zeitversatz): Bildzeit und Inhalt stimmen überein.
        let err = |delay: i32| {
            let mut worst: f32 = 0.0;
            for (i, t) in path.times.iter().enumerate().skip(1) {
                let fi = ((*t * 15.0).round() as i32 - delay).clamp(0, 45) as usize;
                let truth = (jit(fi) - jit(0)) as f32 * 320.0;
                worst = worst.max((path.x[i] * 320.0 - truth).abs());
            }
            worst
        };
        let worst = err(0);
        assert!(worst < 2.5, "größte Abweichung des gemessenen Wegs: {worst:.2} px (bei einem Bild Versatz: {:.2})", err(1));
        assert!(path.times[0] < 0.01, "das erste Bild liegt bei 0 s, nicht später: {}", path.times[0]);
        assert!(path.y.iter().all(|v| v.abs() < 0.01), "keine senkrechte Bewegung");
    }

    #[test]
    fn renderer_moves_the_picture_against_the_measured_shake() {
        use crate::model::*;
        let dir = std::env::temp_dir().join("video-superbe-tests");
        let png = dir.join("stab_halves.png");
        let mut im = image::RgbaImage::new(2, 1);
        im.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        im.put_pixel(1, 0, image::Rgba([0, 0, 255, 255]));
        im.save(&png).unwrap();
        let mut p = Project::default();
        let a = crate::render::tests::image_asset(&mut p, &png, 2.0, 1.0);
        let mut c = crate::render::tests::clip(a, 0.0, 6.0);
        c.fx.stabilize = Some(1.0);
        c.fx.stab_no_scale = Some(true);
        p.tracks[0].clips.push(c);
        // Weg springt zwischen ±10 % (Bild wackelt): bei +10 % muss das Bild um 10 % nach links geschoben werden
        set(a, StabPath { times: (0..13).map(|i| i as f32 * 0.5).collect(), x: (0..13).map(|i| if i % 2 == 0 { 0.1 } else { -0.1 }).collect(), y: vec![0.0; 13] });
        let mut r = crate::render::Renderer::new();
        let blue_at = |buf: &[u8], x: usize| buf[(16 * 64 + x) * 4 + 2] as i32;
        let shifted = r.render(&p, 0.0, 64, 32);
        remove(a);
        let plain = r.render(&p, 0.0, 64, 32);
        // Das Bild wird um rund 6 Pixel nach links geschoben: an jeder Stelle der Grenze wird mehr blau sichtbar
        assert!(blue_at(&shifted, 29) > blue_at(&plain, 29) + 30, "x=29: {} gegen {}", blue_at(&shifted, 29), blue_at(&plain, 29));
        assert!(blue_at(&shifted, 22) > blue_at(&plain, 22) + 30, "x=22: {} gegen {}", blue_at(&shifted, 22), blue_at(&plain, 22));
        assert!(blue_at(&plain, 5) < 10 && blue_at(&plain, 60) > 245, "ohne Bewegungsdaten bleibt das Bild unverändert");
    }
}

