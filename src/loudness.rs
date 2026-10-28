//! Lautheitsmessung nach ITU-R BS.1770 / EBU R128 (integrierte Lautheit in LUFS mit Gating) und
//! Normalisierung des Exporttons auf einen Zielwert (Standard −14 LUFS wie YouTube und Spotify).

use crate::audio::{Mixer, RATE};
use crate::model::Project;

pub const DEFAULT_TARGET: f64 = -14.0;

/// Ergebnis einer Messung: integrierte Lautheit und höchster Abtastwert (linear, 0…1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Loudness {
    pub lufs: f64,
    pub peak: f64,
}

/// Biquad (Direct Form I) je Kanal.
#[derive(Clone, Copy, Default)]
struct Bq {
    b: [f64; 3],
    a: [f64; 2],
    x: [f64; 2],
    y: [f64; 2],
}

impl Bq {
    fn new(b: [f64; 3], a: [f64; 2]) -> Self {
        Bq { b, a, ..Default::default() }
    }
    fn run(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1] - self.a[0] * self.y[0] - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

/// Streaming-Messung: K-Bewertung (Hochton-Anhebung + Hochpass), Energie je 100-ms-Schritt.
pub struct Meter {
    shelf: [Bq; 2],
    hp: [Bq; 2],
    hop_energy: f64,
    hop_n: usize,
    hops: Vec<f64>,
    peak: f64,
}

const HOP: usize = (RATE / 10) as usize;

impl Meter {
    pub fn new() -> Self {
        // Koeffizienten der Norm für 48 kHz
        let shelf = Bq::new([1.53512485958697, -2.69169618940638, 1.19839281085285], [-1.69065929318241, 0.73248077421585]);
        let hp = Bq::new([1.0, -2.0, 1.0], [-1.99004745483398, 0.99007225036621]);
        Meter { shelf: [shelf; 2], hp: [hp; 2], hop_energy: 0.0, hop_n: 0, hops: vec![], peak: 0.0 }
    }

    /// Stereo-Frames (interleaved, 48 kHz).
    pub fn push(&mut self, frames: &[f32]) {
        for f in frames.chunks_exact(2) {
            let mut e = 0.0;
            for ch in 0..2 {
                self.peak = self.peak.max(f[ch].abs() as f64);
                let y = self.hp[ch].run(self.shelf[ch].run(f[ch] as f64));
                e += y * y;
            }
            self.hop_energy += e;
            self.hop_n += 1;
            if self.hop_n == HOP {
                self.hops.push(self.hop_energy / HOP as f64);
                self.hop_energy = 0.0;
                self.hop_n = 0;
            }
        }
    }

    /// Integrierte Lautheit mit absolutem (−70 LUFS) und relativem Gate (−10 LU); `None` bei Stille.
    pub fn result(&self) -> Option<Loudness> {
        let lufs = |ms: f64| -0.691 + 10.0 * ms.max(1e-12).log10();
        // 400-ms-Blöcke aus je vier Schritten (75 % Überlappung)
        let blocks: Vec<f64> = self.hops.windows(4).map(|w| w.iter().sum::<f64>() / 4.0).collect();
        let blocks = if blocks.is_empty() && !self.hops.is_empty() { vec![self.hops.iter().sum::<f64>() / self.hops.len() as f64] } else { blocks };
        let above: Vec<f64> = blocks.iter().copied().filter(|b| lufs(*b) > -70.0).collect();
        if above.is_empty() {
            return None;
        }
        let rel = lufs(above.iter().sum::<f64>() / above.len() as f64) - 10.0;
        let gated: Vec<f64> = above.into_iter().filter(|b| lufs(*b) > rel).collect();
        if gated.is_empty() {
            return None;
        }
        Some(Loudness { lufs: lufs(gated.iter().sum::<f64>() / gated.len() as f64), peak: self.peak })
    }
}

/// Misst den Projektton im Bereich `range` (Sekunden); `progress(0…1)` gibt `false` zurück, um abzubrechen.
pub fn measure(p: &Project, range: (f64, f64), progress: &mut dyn FnMut(f64) -> bool) -> Option<Loudness> {
    measure_limited(p, range, None, progress)
}

/// Wie `measure`; mit `gain` wird die Mischung vorher verstärkt und durch den Limiter geschickt (so, wie sie beim Export
/// in die Datei kommt).
pub fn measure_limited(p: &Project, range: (f64, f64), gain: Option<f64>, progress: &mut dyn FnMut(f64) -> bool) -> Option<Loudness> {
    let mut mixer = Mixer::new();
    let mut meter = Meter::new();
    let mut limiter = PeakLimiter::new();
    let (a, b) = ((range.0 * RATE as f64).round() as i64, (range.1 * RATE as f64).round() as i64);
    let step = RATE;
    let mut pos = a;
    while pos < b {
        if !progress((pos - a) as f64 / (b - a).max(1) as f64) {
            return None;
        }
        let n = step.min(b - pos) as usize;
        let mut f = mixer.mix(p, pos, n);
        if let Some(g) = gain {
            limiter.process(&mut f, 2, g);
        }
        meter.push(&f);
        pos += n as i64;
    }
    meter.result()
}

/// Verstärkung (linear), mit der `m` auf `target` LUFS kommt; begrenzt, damit die Spitzen nicht über −0,3 dBFS gehen.
pub fn gain_for(m: Loudness, target: f64) -> f64 {
    let g = 10f64.powf((target - m.lufs) / 20.0);
    if m.peak > 1e-6 { g.min(0.966 / m.peak) } else { g }.clamp(0.0, 16.0)
}

/// Verstärkung auf die Ziel-Lautheit ohne Rücksicht auf die Spitzen; `limited`: Die Spitzen lägen danach über −1 dBFS,
/// der Limiter (`PeakLimiter`) fängt sie beim Export ab.
pub fn gain_with_limiter(m: Loudness, target: f64) -> (f64, bool) {
    let g = 10f64.powf((target - m.lufs) / 20.0).clamp(0.0, 16.0);
    (g, m.peak * g > CEILING)
}

/// Ziel-Lautheit samt Limiter: misst nach der Spitzenbegrenzung nach und führt die Verstärkung nach (der Limiter nimmt bei
/// häufigen Spitzen etwas Lautheit weg; höchstens +6 dB über der ersten Schätzung). Rückgabe: (Verstärkung, begrenzt).
pub fn normalize_gain(p: &Project, range: (f64, f64), m: Loudness, target: f64, progress: &mut dyn FnMut(f64) -> bool) -> (f64, bool) {
    let (g0, limited) = gain_with_limiter(m, target);
    let mut g = g0;
    if limited {
        for _ in 0..2 {
            let Some(after) = measure_limited(p, range, Some(g), progress) else { break };
            if (after.lufs - target).abs() <= 0.2 {
                break;
            }
            g = (g * 10f64.powf((target - after.lufs) / 20.0)).min(g0 * 2.0);
        }
    }
    (g, limited)
}

/// Spitze, die der Limiter nicht überschreiten lässt: −1 dBFS.
pub const CEILING: f64 = 0.891_250_938_133_745_5;

/// Spitzenbegrenzer für den Export: hält den Ton unter −1 dBFS. Sofortiges Absenken, weiches Erholen (80 ms), alle
/// Kanäle gemeinsam (das Stereobild bleibt). Er trägt seinen Zustand über die Puffer hinweg.
pub struct PeakLimiter {
    release: f64,
    env: f64,
    /// stärkste Absenkung in dB (zur Anzeige)
    pub max_reduction_db: f64,
}

impl Default for PeakLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl PeakLimiter {
    pub fn new() -> Self {
        PeakLimiter { release: 1.0 - (-1.0 / (0.08 * RATE as f64)).exp(), env: 1.0, max_reduction_db: 0.0 }
    }

    /// Verstärkt die (verschränkten) Abtastwerte um `gain` und begrenzt die Spitzen.
    pub fn process(&mut self, s: &mut [f32], channels: usize, gain: f64) {
        let ch = channels.max(1);
        for f in s.chunks_exact_mut(ch) {
            let peak = f.iter().fold(0.0f64, |m, v| m.max((*v as f64 * gain).abs()));
            let want = if peak > CEILING { CEILING / peak } else { 1.0 };
            self.env = if want < self.env { want } else { self.env + (want - self.env) * self.release };
            if self.env < 1.0 {
                self.max_reduction_db = self.max_reduction_db.max(-20.0 * self.env.log10());
            }
            for v in f.iter_mut() {
                *v = (*v as f64 * gain * self.env) as f32;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::tests::write_sine_wav;
    use crate::model::*;

    fn sine_meter(amp: f32, hz: f64, secs: f64) -> Meter {
        let mut m = Meter::new();
        let n = (RATE as f64 * secs) as usize;
        let mut f = Vec::with_capacity(n * 2);
        for i in 0..n {
            let v = amp * (2.0 * std::f64::consts::PI * hz * i as f64 / RATE as f64).sin() as f32;
            f.extend([v, v]);
        }
        m.push(&f);
        m
    }

    #[test]
    fn matches_the_reference_value_of_the_standard() {
        // Norm: Sinus 997 Hz mit −20 dBFS auf beiden Kanälen = −20,0 LUFS (Toleranz 0,1 LU)
        let l = sine_meter(0.1, 997.0, 6.0).result().unwrap();
        assert!((l.lufs + 20.0).abs() < 0.15, "{}", l.lufs);
        // doppelt so laut (+6 dB) = +6 LU
        let l2 = sine_meter(0.2, 997.0, 6.0).result().unwrap();
        assert!((l2.lufs - l.lufs - 6.02).abs() < 0.05, "{} {}", l2.lufs, l.lufs);
        assert!((l2.peak - 0.2).abs() < 1e-3);
    }

    #[test]
    fn silence_and_quiet_parts_are_gated() {
        assert!(Meter::new().result().is_none());
        let mut m = sine_meter(0.0, 440.0, 2.0);
        assert!(m.result().is_none(), "Stille hat keine Lautheit");
        // 2 s laut + 8 s sehr leise (−80 dB): das Gate lässt die leisen Teile weg
        let loud = sine_meter(0.1, 997.0, 2.0);
        m = sine_meter(0.00001, 997.0, 8.0);
        let mut both = Meter::new();
        both.hops = loud.hops.clone();
        both.hops.extend(m.hops.iter());
        both.peak = 0.1;
        let l = both.result().unwrap();
        assert!((l.lufs + 20.0).abs() < 0.5, "{}", l.lufs);
    }

    #[test]
    fn gain_reaches_target_but_respects_peaks() {
        let g = gain_for(Loudness { lufs: -20.0, peak: 0.1 }, -14.0);
        assert!((g - 1.995).abs() < 0.01, "{g}");
        let limited = gain_for(Loudness { lufs: -30.0, peak: 0.9 }, -14.0);
        assert!(limited <= 0.966 / 0.9 + 1e-9 && limited > 1.0, "{limited}");
    }

    #[test]
    fn limiter_holds_peaks_below_minus_1_dbfs_and_keeps_quiet_parts() {
        let mut l = PeakLimiter::new();
        // laute Spitze mit Verstärkung 4: wird auf −1 dBFS gehalten
        let mut s: Vec<f32> = (0..4800).flat_map(|i| {
            let v = 0.5 * (2.0 * std::f64::consts::PI * 997.0 * i as f64 / RATE as f64).sin() as f32;
            [v, v]
        }).collect();
        l.process(&mut s, 2, 4.0);
        let peak = s.iter().fold(0.0f32, |m, v| m.max(v.abs())) as f64;
        assert!(peak <= CEILING + 1e-6, "{peak}");
        assert!(l.max_reduction_db > 6.0);
        // nach dem Erholen (1 s leise): unverändert verstärkt
        let mut q = vec![0.01f32; 2 * RATE as usize];
        l.process(&mut q, 2, 2.0);
        assert!((q[q.len() - 1] - 0.02).abs() < 1e-4, "{}", q[q.len() - 1]);
        let (g, limited) = gain_with_limiter(Loudness { lufs: -30.0, peak: 0.9 }, -14.0);
        assert!(limited && (g - 6.31).abs() < 0.01, "{g}");
        assert!(!gain_with_limiter(Loudness { lufs: -20.0, peak: 0.1 }, -14.0).1);
    }

    #[test]
    fn normalizing_with_limiter_reaches_the_target() {
        // Sinus mit kurzen lauten Spitzen: ohne Limiter käme man wegen der Spitzen nicht auf −14 LUFS
        let path = std::env::temp_dir().join("video-superbe-tests").join("loud_peaky.wav");
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        let n = 6 * RATE as usize;
        let mut d: Vec<u8> = vec![];
        d.extend(b"RIFF");
        d.extend(((36 + n * 4) as u32).to_le_bytes());
        d.extend(b"WAVEfmt ");
        d.extend(16u32.to_le_bytes());
        d.extend(1u16.to_le_bytes());
        d.extend(2u16.to_le_bytes());
        d.extend((RATE as u32).to_le_bytes());
        d.extend((RATE as u32 * 4).to_le_bytes());
        d.extend(4u16.to_le_bytes());
        d.extend(16u16.to_le_bytes());
        d.extend(b"data");
        d.extend(((n * 4) as u32).to_le_bytes());
        for i in 0..n {
            let base = 0.05 * (2.0 * std::f64::consts::PI * 997.0 * i as f64 / RATE as f64).sin();
            let click = if i % (RATE as usize / 2) < 40 { 0.9 } else { 0.0 };
            let v = ((base + click).clamp(-1.0, 1.0) * 32767.0) as i16;
            d.extend(v.to_le_bytes());
            d.extend(v.to_le_bytes());
        }
        std::fs::write(&path, d).unwrap();
        let mut p = Project::default();
        let a = MediaAsset { path: path.to_string_lossy().into_owned(), name: "t".into(), duration: 6.0, has_audio: true, ..Default::default() };
        let aid = a.id;
        p.assets.push(a);
        p.tracks[0].clips.push(Clip { content: ClipContent::Media(Wrap { v: aid }), part: ClipPart::Audio, start: 0.0, duration: 6.0, ..Default::default() });
        let m = measure(&p, (0.0, 6.0), &mut |_| true).unwrap();
        let (g, limited) = normalize_gain(&p, (0.0, 6.0), m, -14.0, &mut |_| true);
        assert!(limited);
        let after = measure_limited(&p, (0.0, 6.0), Some(g), &mut |_| true).unwrap();
        assert!((after.lufs + 14.0).abs() < 0.5, "Ziel erreicht: {}", after.lufs);
        assert!(after.peak <= CEILING + 1e-3, "Spitze {}", after.peak);
        // die alte Begrenzung über die Verstärkung bliebe weit darunter
        let old = measure_limited(&p, (0.0, 6.0), Some(gain_for(m, -14.0)), &mut |_| true).unwrap();
        assert!(old.lufs < -17.0, "{}", old.lufs);
    }

    #[test]
    fn measures_a_project() {
        let wav = write_sine_wav("loud_tone.wav", 997.0, 4.0, 48000, 0.1);
        let mut p = Project::default();
        let a = MediaAsset { path: wav.to_string_lossy().into_owned(), name: "t".into(), duration: 4.0, has_audio: true, ..Default::default() };
        let aid = a.id;
        p.assets.push(a);
        p.tracks[0].clips.push(Clip { content: ClipContent::Media(Wrap { v: aid }), part: ClipPart::Audio, start: 0.0, duration: 4.0, ..Default::default() });
        let mut last = 0.0;
        let l = measure(&p, (0.0, 4.0), &mut |f| {
            assert!(f >= last);
            last = f;
            true
        })
        .unwrap();
        assert!((l.lufs + 20.0).abs() < 0.4, "{}", l.lufs);
        assert!(measure(&p, (0.0, 4.0), &mut |_| false).is_none(), "Abbruch");
    }
}
