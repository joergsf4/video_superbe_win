//! Tonanalyse: Sprachaktivität (für automatisches Abdunkeln der Musik) und Taktraster-Erkennung
//! (VoiceActivity.swift und BeatDetector.swift der Mac-Version). Alles reine Rechenfunktionen auf Abtastwerten.

use crate::model::{BeatGrid, Clip, VolumePoint};

// ---------------------------------------------------------------------------------------------
// Sprachaktivität
// ---------------------------------------------------------------------------------------------

pub const VOICE_HOP: f64 = 0.05;
pub const VOICE_RATE: u32 = 16000;

/// RMS-Pegel in dBFS je 50 ms aus Mono-Abtastwerten (16 kHz); `acc`/`n` tragen Reste zwischen Blöcken.
#[derive(Default)]
pub struct EnvelopeBuilder {
    pub env: Vec<f32>,
    acc: f64,
    n: usize,
}

impl EnvelopeBuilder {
    pub fn push(&mut self, x: &[f32]) {
        let per = (VOICE_RATE as f64 * VOICE_HOP) as usize;
        for v in x {
            self.acc += (*v as f64) * (*v as f64);
            self.n += 1;
            if self.n == per {
                self.env.push((10.0 * (self.acc / self.n as f64).max(1e-12).log10()) as f32);
                self.acc = 0.0;
                self.n = 0;
            }
        }
    }
}

/// Zeitbereiche (Quellsekunden), in denen gesprochen wird: Schwelle adaptiv zwischen Grundrauschen und Spitzen.
pub fn voice_intervals(env: &[f32]) -> Vec<(f64, f64)> {
    if env.len() <= 10 {
        return vec![];
    }
    let mut sorted = env.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let (floor, peak) = (sorted[env.len() / 10] as f64, sorted[env.len() * 95 / 100] as f64);
    if peak <= -50.0 {
        return vec![];
    }
    let threshold = (floor + 8.0).max(peak - 20.0).max(-52.0);
    let mut runs: Vec<(f64, f64)> = vec![];
    let mut start: Option<usize> = None;
    for (i, v) in env.iter().enumerate() {
        if *v as f64 > threshold {
            start.get_or_insert(i);
        } else if let Some(s) = start.take() {
            runs.push((s as f64 * VOICE_HOP, i as f64 * VOICE_HOP));
        }
    }
    if let Some(s) = start {
        runs.push((s as f64 * VOICE_HOP, env.len() as f64 * VOICE_HOP));
    }
    // Lücken < 0,4 s schließen, sehr kurze Ereignisse (Klicks) verwerfen
    let mut merged: Vec<(f64, f64)> = vec![];
    for r in runs {
        match merged.last_mut() {
            Some(l) if r.0 - l.1 < 0.4 => l.1 = r.1,
            _ => merged.push(r),
        }
    }
    merged.into_iter().filter(|r| r.1 - r.0 >= 0.2).collect()
}

/// Sprachabschnitte aller Sprachclips in Timeline-Zeit, auf die Musik begrenzt, zusammengefasst (Lücken < 1,05 s).
/// `voices`: (Clip, Sprachintervalle der zugehörigen Quelle in Quellsekunden).
pub fn speech_on_timeline(music: &Clip, voices: &[(Clip, Vec<(f64, f64)>)]) -> Vec<(f64, f64)> {
    let mut tl: Vec<(f64, f64)> = vec![];
    for (v, ivs) in voices {
        let (a, b) = (v.source_in, v.source_in + v.duration * v.speed);
        for iv in ivs.iter().filter(|iv| iv.1 > a && iv.0 < b) {
            let s = v.start + (iv.0.max(a) - v.source_in) / v.speed;
            let e = v.start + (iv.1.min(b) - v.source_in) / v.speed;
            tl.push((s.max(music.start), e.min(music.end())));
        }
    }
    tl.retain(|r| r.1 > r.0);
    tl.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f64, f64)> = vec![];
    for r in tl {
        match merged.last_mut() {
            Some(l) if r.0 - l.1 < 1.05 => l.1 = l.1.max(r.1),
            _ => merged.push(r),
        }
    }
    merged
}

/// Lautstärkekurve für die Musik: bei Sprache auf `db` (negativ) abgesenkt, mit 0,25 s Anlauf und 0,6 s Rücklauf.
pub fn ducking_points(music: &Clip, speech: &[(f64, f64)], db: f64) -> Vec<VolumePoint> {
    let base = music.volume;
    let low = base * 10f64.powf(db / 20.0);
    let (attack, release) = (0.25, 0.6);
    let mut pts = vec![VolumePoint { t: 0.0, gain: base }];
    for (s, e) in speech {
        let (rs, re) = (s - music.start, e - music.start);
        let mut add = |t: f64, g: f64| {
            if t > pts.last().map_or(-1.0, |p| p.t) + 0.001 && t < music.duration {
                pts.push(VolumePoint { t, gain: g });
            }
        };
        add((rs - attack).max(0.001), base);
        add(rs, low);
        add(re, low);
        add(re + release, base);
    }
    let last = pts.last().map(|p| if (p.gain - low).abs() < 1e-9 { low } else { base }).unwrap_or(base);
    pts.push(VolumePoint { t: music.duration, gain: last });
    pts
}

// ---------------------------------------------------------------------------------------------
// Taktraster
// ---------------------------------------------------------------------------------------------

pub const BEAT_RATE: u32 = 22050;
const HOP: usize = 256;
const FFT_N: usize = 1024;
/// Zeitversatz zwischen Fluss-Maximum und tatsächlichem Einsatz (halbe Fensterlänge, wie in der Mac-Version).
const LATENCY: f64 = 0.0235;

/// In-place-FFT (Radix 2) über `re`/`im` (Länge Zweierpotenz).
fn fft(re: &mut [f32], im: &mut [f32], tw: &[(f32, f32)]) {
    let n = re.len();
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let step = n / len;
        for s in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (wr, wi) = tw[k * step];
                let (a, b) = (s + k, s + k + len / 2);
                let (xr, xi) = (re[b] * wr - im[b] * wi, re[b] * wi + im[b] * wr);
                re[b] = re[a] - xr;
                im[b] = im[a] - xi;
                re[a] += xr;
                im[a] += xi;
            }
        }
        len <<= 1;
    }
}

/// Onset-Stärke je Frame (Bass bis ca. 900 Hz), normalisiert: (roh, lokales Maximum ±2 Frames).
pub fn onset_envelope(x: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let n = FFT_N;
    if x.len() <= n * 4 {
        return (vec![], vec![]);
    }
    let tw: Vec<(f32, f32)> = (0..n / 2).map(|k| { let a = -2.0 * std::f32::consts::PI * k as f32 / n as f32; (a.cos(), a.sin()) }).collect();
    let window: Vec<f32> = (0..n).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos()).collect();
    let bins = 42;
    let frames = (x.len() - n) / HOP;
    let mut prev = vec![0f32; bins];
    let mut flux = vec![0f32; frames];
    let (mut re, mut im) = (vec![0f32; n], vec![0f32; n]);
    for (f, fl) in flux.iter_mut().enumerate() {
        for i in 0..n {
            re[i] = x[f * HOP + i] * window[i];
            im[i] = 0.0;
        }
        fft(&mut re, &mut im, &tw);
        let mut sum = 0.0;
        for k in 1..bins {
            let m = (re[k] * re[k] + im[k] * im[k]).sqrt();
            sum += (m - prev[k]).max(0.0);
            prev[k] = m;
        }
        *fl = sum;
    }
    let mean = flux.iter().map(|v| *v as f64).sum::<f64>() / frames as f64;
    let sd = (flux.iter().map(|v| (*v as f64 - mean).powi(2)).sum::<f64>() / frames as f64).sqrt();
    if sd < 1e-9 {
        return (vec![], vec![]);
    }
    for v in &mut flux {
        *v = ((*v as f64 - mean) / sd) as f32;
    }
    let loc: Vec<f32> = (0..frames)
        .map(|i| (i.saturating_sub(2)..=(i + 2).min(frames - 1)).map(|j| flux[j]).fold(flux[i], f32::max))
        .collect();
    (flux, loc)
}

fn score(loc: &[f32], period: f64, phase: f64, fps: f64, length: f64) -> f64 {
    let (mut sum, mut n) = (0.0, 0usize);
    let mut t = phase;
    while t < length {
        let i = (t * fps) as usize;
        if i > 2 && i + 2 < loc.len() {
            sum += loc[i] as f64;
            n += 1;
        }
        t += period;
    }
    if n > 8 { sum / n as f64 } else { 0.0 }
}

/// Sucht das Taktraster (Periode 0,30–0,86 s) mit den meisten Treffern auf den Onset-Spitzen.
pub fn find_grid(loc: &[f32], raw: &[f32]) -> Option<BeatGrid> {
    if loc.len() <= 400 {
        return None;
    }
    let fps = BEAT_RATE as f64 / HOP as f64;
    let length = loc.len() as f64 / fps;
    let weight = |period: f64| if (85.0..=170.0).contains(&(60.0 / period)) { 1.0 } else { 0.93 };
    let mut best: Option<(f64, f64, f64)> = None;
    let mut p = 0.30;
    while p <= 0.86 {
        let mut o = 0.0;
        while o < p {
            let s = score(loc, p, o, fps, length) * weight(p);
            if best.is_none_or(|b| s > b.0) {
                best = Some((s, p, o));
            }
            o += 0.012;
        }
        p += 0.002;
    }
    let mut b = best?;
    // Verfeinern
    let (p0, o0) = (b.1, b.2);
    let mut pp = p0 - 0.004;
    while pp <= p0 + 0.004 {
        let mut o = o0 - 0.02;
        while o <= o0 + 0.02 {
            let oo = if o < 0.0 { o + pp } else { o };
            let s = score(loc, pp, oo, fps, length) * weight(pp);
            if s > b.0 {
                b = (s, pp, oo);
            }
            o += 0.004;
        }
        pp += 0.00005;
    }
    let mean = loc.iter().map(|v| *v as f64).sum::<f64>() / loc.len() as f64;
    let conf = b.0 / mean.max(0.1);
    // Phase genau einmessen: ungeglättete Onset-Kurve, 1-ms-Schritte, lineare Interpolation
    let (mut best_phase, mut best_raw) = (b.2, f64::NEG_INFINITY);
    let mut ph = b.2 - 0.035;
    while ph <= b.2 + 0.035 {
        let (mut sum, mut cnt) = (0.0, 0usize);
        let mut t = ph;
        while t < length - 0.1 {
            let pos = t * fps;
            let i = pos as usize;
            if pos >= 2.0 && i + 1 < raw.len() {
                let fr = pos - i as f64;
                sum += raw[i] as f64 * (1.0 - fr) + raw[i + 1] as f64 * fr;
                cnt += 1;
            }
            t += b.1;
        }
        if cnt > 8 && sum / cnt as f64 > best_raw {
            best_raw = sum / cnt as f64;
            best_phase = ph;
        }
        ph += 0.001;
    }
    let mut phase = (best_phase + LATENCY) % b.1;
    if phase < 0.0 {
        phase += b.1;
    }
    Some(BeatGrid { period: b.1, phase, confidence: conf })
}

/// Komplette Erkennung aus Mono-Abtastwerten bei 22,05 kHz.
pub fn detect_grid(x: &[f32]) -> Option<BeatGrid> {
    let (raw, loc) = onset_envelope(x);
    find_grid(&loc, &raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Clip, ClipContent, Wrap};
    use uuid::Uuid;

    fn env_with_speech(spans: &[(f64, f64)], total: f64) -> Vec<f32> {
        let n = (total / VOICE_HOP) as usize;
        (0..n)
            .map(|i| {
                let t = i as f64 * VOICE_HOP;
                if spans.iter().any(|s| t >= s.0 && t < s.1) { -20.0 } else { -70.0 }
            })
            .collect()
    }

    #[test]
    fn finds_speech_spans_merges_gaps_and_drops_clicks() {
        let env = env_with_speech(&[(1.0, 3.0), (3.2, 4.0), (6.0, 6.1), (8.0, 9.0)], 12.0);
        let v = voice_intervals(&env);
        assert_eq!(v.len(), 2, "{v:?}");
        assert!((v[0].0 - 1.0).abs() < 0.06 && (v[0].1 - 4.0).abs() < 0.06, "Lücke 0,2 s geschlossen: {v:?}");
        assert!((v[1].0 - 8.0).abs() < 0.06 && (v[1].1 - 9.0).abs() < 0.06);
        assert!(voice_intervals(&vec![-80.0; 100]).is_empty(), "Stille: keine Sprache");
        assert!(voice_intervals(&[-20.0; 5]).is_empty(), "zu kurz");
    }

    #[test]
    fn envelope_builder_measures_rms_in_dbfs() {
        let mut b = EnvelopeBuilder::default();
        let x: Vec<f32> = (0..16000).map(|i| 0.5 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 16000.0).sin()).collect();
        b.push(&x[..5000]);
        b.push(&x[5000..]);
        assert_eq!(b.env.len(), 20);
        assert!((b.env[5] - (-9.03)).abs() < 0.4, "Sinus 0,5 = −9 dBFS RMS: {}", b.env[5]);
    }

    fn music() -> Clip {
        Clip { content: ClipContent::Media(Wrap { v: Uuid::new_v4() }), start: 10.0, duration: 20.0, volume: 0.8, ..Default::default() }
    }

    #[test]
    fn ducking_curve_follows_speech_with_attack_and_release() {
        let m = music();
        let voice = Clip { start: 12.0, duration: 6.0, source_in: 1.0, speed: 1.0, ..music() };
        // Sprache in der Quelle bei 2–4 s und 20–22 s (der zweite Abschnitt liegt hinter dem Clip-Ende)
        let sp = speech_on_timeline(&m, &[(voice, vec![(2.0, 4.0), (20.0, 22.0)])]);
        assert_eq!(sp.len(), 1);
        assert!((sp[0].0 - 13.0).abs() < 1e-9 && (sp[0].1 - 15.0).abs() < 1e-9, "{sp:?}");
        let pts = ducking_points(&m, &sp, -12.0);
        let low = 0.8 * 10f64.powf(-12.0 / 20.0);
        // Start, Anlauf, abgesenkt (2×), Rücklauf, Ende
        assert_eq!(pts.len(), 6, "{pts:?}");
        assert_eq!(pts[0], VolumePoint { t: 0.0, gain: 0.8 });
        assert!((pts[1].t - 2.75).abs() < 1e-9 && pts[1].gain == 0.8);
        assert!((pts[2].t - 3.0).abs() < 1e-9 && (pts[2].gain - low).abs() < 1e-9);
        assert!((pts[3].t - 5.0).abs() < 1e-9 && (pts[3].gain - low).abs() < 1e-9);
        assert!((pts[4].t - 5.6).abs() < 1e-9 && pts[4].gain == 0.8);
        assert_eq!(pts[5], VolumePoint { t: 20.0, gain: 0.8 });
        // dicht beieinanderliegende Sprache wird zusammengefasst
        let two = speech_on_timeline(&m, &[(Clip { start: 10.0, duration: 10.0, ..music() }, vec![(1.0, 2.0), (2.8, 3.5)])]);
        assert_eq!(two.len(), 1);
    }

    /// Klick-Spur: kurze tiefe Stöße im festen Abstand
    fn clicks(bpm: f64, offset: f64, secs: f64) -> Vec<f32> {
        let n = (secs * BEAT_RATE as f64) as usize;
        let mut x = vec![0f32; n];
        let period = 60.0 / bpm;
        let mut t = offset;
        let mut seed = 12345u32;
        while t < secs {
            let s = (t * BEAT_RATE as f64) as usize;
            for i in 0..2000.min(n.saturating_sub(s)) {
                let env = (-(i as f32) / 350.0).exp();
                x[s + i] += 0.8 * env * (2.0 * std::f32::consts::PI * 70.0 * i as f32 / BEAT_RATE as f32).sin();
            }
            t += period;
        }
        // leichtes Rauschen
        for v in &mut x {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            *v += ((seed >> 16) as f32 / 65535.0 - 0.5) * 0.01;
        }
        x
    }

    #[test]
    fn detects_tempo_and_phase_of_a_click_track() {
        let g = detect_grid(&clicks(120.0, 0.31, 24.0)).expect("Raster");
        assert!((g.period - 0.5).abs() < 0.004, "Periode {}", g.period);
        assert!((g.bpm() - 120.0).abs() < 1.0);
        // Phase: Beat k bei phase + k·period soll auf den Klicks (0,31 + k·0,5) liegen
        let off = ((g.phase - 0.31) / 0.5).round() * 0.5 + 0.31 - g.phase;
        assert!(off.abs() < 0.03, "Phasenfehler {off}");
        let g2 = detect_grid(&clicks(100.0, 0.1, 24.0)).expect("Raster");
        assert!((g2.period - 0.6).abs() < 0.005, "Periode {}", g2.period);
        // Stille und zu kurze Signale: kein Raster
        assert!(detect_grid(&vec![0.0; 22050 * 20]).is_none());
        assert!(detect_grid(&vec![0.1; 1000]).is_none());
    }

    #[test]
    fn fft_finds_the_right_bin() {
        let n = 1024;
        let tw: Vec<(f32, f32)> = (0..n / 2).map(|k| { let a = -2.0 * std::f32::consts::PI * k as f32 / n as f32; (a.cos(), a.sin()) }).collect();
        let mut re: Vec<f32> = (0..n).map(|i| (2.0 * std::f32::consts::PI * 40.0 * i as f32 / n as f32).sin()).collect();
        let mut im = vec![0f32; n];
        fft(&mut re, &mut im, &tw);
        let mags: Vec<f32> = (0..n / 2).map(|k| (re[k] * re[k] + im[k] * im[k]).sqrt()).collect();
        let peak = mags.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap().0;
        assert_eq!(peak, 40);
        assert!((mags[40] - 512.0).abs() < 1.0);
    }
}
