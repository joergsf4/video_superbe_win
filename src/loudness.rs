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
    let mut mixer = Mixer::new();
    let mut meter = Meter::new();
    let (a, b) = ((range.0 * RATE as f64).round() as i64, (range.1 * RATE as f64).round() as i64);
    let step = RATE;
    let mut pos = a;
    while pos < b {
        if !progress((pos - a) as f64 / (b - a).max(1) as f64) {
            return None;
        }
        let n = step.min(b - pos) as usize;
        meter.push(&mixer.mix(p, pos, n));
        pos += n as i64;
    }
    meter.result()
}

/// Verstärkung (linear), mit der `m` auf `target` LUFS kommt; begrenzt, damit die Spitzen nicht über −0,3 dBFS gehen.
pub fn gain_for(m: Loudness, target: f64) -> f64 {
    let g = 10f64.powf((target - m.lufs) / 20.0);
    if m.peak > 1e-6 { g.min(0.966 / m.peak) } else { g }.clamp(0.0, 16.0)
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
