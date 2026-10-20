//! Ton-Synchronisation (AudioSync.swift): findet die Verschiebung, bei der gleiche Tonereignisse (Klatschen, Klappe,
//! Sprache) zweier Aufnahmen zusammenfallen. Verglichen werden die Spitzenwerte pro Zeitschritt (50 je Sekunde).

use crate::audio::PEAKS_PER_SEC;
use crate::model::Clip;

#[derive(Clone, Copy, Debug)]
pub struct SyncResult {
    /// Sekunden, um die `other` auf der Timeline wandern muss
    pub shift: f64,
    /// Übereinstimmung 0…1
    pub quality: f64,
}

/// Hüllkurve eines Clips auf dem Zeitraster der Timeline: Index i ↔ Zeit `t0 + i / bps`; außerhalb des Clips 0. Danach wird
/// der gleitende Mittelwert (0,5 s) abgezogen, damit Sprünge und Pegelstufen zählen statt der Lautstärke.
pub fn envelope(clip: &Clip, peaks: &[f32], t0: f64, count: usize) -> Vec<f32> {
    let bps = PEAKS_PER_SEC as f64;
    let mut e = vec![0f32; count];
    for (i, v) in e.iter_mut().enumerate() {
        let t = t0 + i as f64 / bps;
        if t < clip.start || t >= clip.end() {
            continue;
        }
        let bin = ((clip.source_in + (t - clip.start) * clip.speed) * bps) as i64;
        if bin >= 0 && (bin as usize) < peaks.len() {
            *v = peaks[bin as usize];
        }
    }
    let w = (bps / 2.0) as usize;
    let mut out = e.clone();
    let mut run = 0f32;
    for i in 0..count {
        run += e[i];
        if i >= w {
            run -= e[i - w];
        }
        out[i] = e[i] - run / (i + 1).min(w) as f32;
    }
    out
}

/// Verschiebung (Sekunden), um die `other` auf der Timeline wandern muss; `None`, wenn nichts Passendes gefunden wird.
pub fn offset(reference: &Clip, ref_peaks: &[f32], other: &Clip, other_peaks: &[f32], max_lag: f64) -> Option<SyncResult> {
    let bps = PEAKS_PER_SEC as f64;
    let n_w = (reference.duration.min(120.0) * bps) as usize;
    let l = (max_lag * bps) as usize;
    if n_w <= bps as usize || ref_peaks.is_empty() || other_peaks.is_empty() {
        return None;
    }
    let r = envelope(reference, ref_peaks, reference.start, n_w);
    let o = envelope(other, other_peaks, reference.start - max_lag, n_w + 2 * l);
    let rr: f32 = r.iter().map(|v| v * v).sum();
    if rr <= 1e-9 {
        return None;
    }
    // Energie des Fensters für jede Lage (Summe der Quadrate von o[k .. k + n_w])
    let mut sq = vec![0f32; o.len() + 1];
    for i in 0..o.len() {
        sq[i + 1] = sq[i] + o[i] * o[i];
    }
    let mut scores = vec![0f32; 2 * l + 1];
    let (mut best, mut best_s) = (f32::NEG_INFINITY, 0i64);
    for s in -(l as i64)..=(l as i64) {
        let k = (l as i64 - s) as usize; // o(t − s) ↔ Index i − s + L
        let dot: f32 = r.iter().zip(&o[k..k + n_w]).map(|(a, b)| a * b).sum();
        let en = sq[k + n_w] - sq[k];
        let score = if en > 1e-9 { dot / (rr * en).sqrt() } else { 0.0 };
        scores[(s + l as i64) as usize] = score;
        if score > best {
            best = score;
            best_s = s;
        }
    }
    // Ein echter Treffer ragt klar aus allen anderen Lagen heraus (außerhalb von ±0,5 s um den Gipfel)
    let mut second = 0f32;
    for s in -(l as i64)..=(l as i64) {
        if (s - best_s).abs() > PEAKS_PER_SEC as i64 / 2 {
            second = second.max(scores[(s + l as i64) as usize]);
        }
    }
    if !(best > 0.2 && best > 1.4 * second) {
        return None;
    }
    // Zwischen den Rasterpunkten interpolieren (Parabel durch die drei Nachbarn)
    let mut sub = 0.0f64;
    if best_s > -(l as i64) && best_s < l as i64 {
        let i = (best_s + l as i64) as usize;
        let (a, b, c) = (scores[i - 1] as f64, scores[i] as f64, scores[i + 1] as f64);
        let d = a - 2.0 * b + c;
        if d.abs() > 1e-9 {
            sub = (0.5 * (a - c) / d).clamp(-0.5, 0.5);
        }
    }
    Some(SyncResult { shift: (best_s as f64 + sub) / bps, quality: best as f64 })
}

impl crate::app::App {
    /// Bild- oder Tonclip → Tonclip (bei verknüpften Bild-Clips der Partner).
    fn audio_clip_of(&self, id: uuid::Uuid) -> Option<Clip> {
        let c = self.project.clip(id)?;
        if c.is_audio() {
            return Some(c.clone());
        }
        self.project.partner_id(id).and_then(|p| self.project.clip(p)).filter(|c| c.is_audio()).cloned()
    }

    /// Zwei gewählte Clips mit Ton: `moving` kann so verschoben werden, dass sein Ton zum Ton des anderen passt.
    pub(crate) fn can_align_audio(&self, moving: uuid::Uuid) -> bool {
        let sel = self.selected_set();
        if sel.len() > 4 || !sel.contains(&moving) {
            return false;
        }
        let audios: std::collections::HashSet<uuid::Uuid> = sel.iter().filter_map(|i| self.audio_clip_of(*i).map(|c| c.id)).collect();
        audios.len() == 2 && self.audio_clip_of(moving).is_some()
    }

    pub(crate) fn align_audio(&mut self, moving: uuid::Uuid) {
        let Some(mc) = self.audio_clip_of(moving) else { return };
        let sel = self.selected_set();
        let Some(other) = sel.iter().filter_map(|i| self.audio_clip_of(*i)).find(|c| c.id != mc.id) else { return };
        let (Some(ma), Some(oa)) = (mc.asset_id(), other.asset_id()) else { return };
        let (Some(mp), Some(op)) = (self.waveforms.get(&ma).cloned(), self.waveforms.get(&oa).cloned()) else {
            self.status = "Die Wellenform ist noch nicht berechnet – kurz warten und erneut versuchen.".into();
            return;
        };
        let Some(r) = offset(&other, &op, &mc, &mp, 30.0) else {
            self.status = "Keine gemeinsamen Tonereignisse gefunden (die Aufnahmen sollten sich um höchstens 30 s unterscheiden).".into();
            return;
        };
        let target = (mc.start + r.shift).max(0.0);
        let dt = target - mc.start;
        let ids = std::collections::HashSet::from([mc.id]);
        self.status = if self.project.move_clips(&ids, 0, dt) { format!("Ton ausgerichtet: {dt:+.3} s (Übereinstimmung {:.0} %)", r.quality * 100.0) } else { "Ausrichten nicht möglich: Der Platz ist belegt.".into() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ClipContent, ClipPart, Wrap};
    use uuid::Uuid;

    fn audio_clip(start: f64, dur: f64) -> Clip {
        Clip { content: ClipContent::Media(Wrap { v: Uuid::new_v4() }), part: ClipPart::Audio, start, duration: dur, ..Default::default() }
    }

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> f32 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (self.0 >> 40) as f32 / (1u64 << 24) as f32
        }
    }

    #[test]
    fn finds_a_known_delay_and_rejects_noise() {
        // dieselben Ereignisse, 1,3 s später aufgenommen → Verschiebung −1,3 s
        let mut rng = Rng(7);
        let bps = PEAKS_PER_SEC;
        let mut reference = vec![0.02f32; 20 * bps];
        for _ in 0..40 {
            let i = (rng.next() * (reference.len() - 10) as f32) as usize;
            for k in 0..(2 + (rng.next() * 4.0) as usize) {
                reference[i + k] = 0.4 + rng.next() * 0.6;
            }
        }
        let d = (1.3 * bps as f64) as usize;
        let mut other = vec![0.02f32; reference.len() + d];
        for i in 0..reference.len() {
            other[i + d] = reference[i];
        }
        let (c1, c2) = (audio_clip(0.0, 20.0), audio_clip(0.0, 20.0));
        let r = offset(&c1, &reference, &c2, &other, 30.0).expect("Treffer");
        assert!((r.shift + 1.3).abs() < 0.03, "Ton-Sync findet −1,3 s ({})", r.shift);
        assert!(r.quality > 0.5);
        let noise: Vec<f32> = (0..reference.len()).map(|_| rng.next() * 0.05).collect();
        assert!(offset(&c1, &reference, &c2, &noise, 30.0).is_none(), "Rauschen wird abgelehnt");
        // zu kurz oder leer
        assert!(offset(&audio_clip(0.0, 0.5), &reference, &c2, &other, 30.0).is_none());
        assert!(offset(&c1, &[], &c2, &other, 30.0).is_none());
    }

    #[test]
    fn envelope_follows_speed_and_clip_bounds() {
        let peaks: Vec<f32> = (0..500).map(|i| if i == 100 { 1.0 } else { 0.0 }).collect();
        let mut c = audio_clip(2.0, 4.0);
        c.speed = 2.0; // Quelle läuft doppelt so schnell: Bin 100 (Quellzeit 2 s) liegt auf der Timeline bei 2 + 1 = 3 s
        let e = envelope(&c, &peaks, 0.0, 400);
        let i = e.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap().0;
        assert_eq!(i, 150, "Spitze bei 3,0 s auf dem Raster");
        assert!(e[..90].iter().all(|v| v.abs() < 1e-6), "vor dem Clip null");
    }

    #[test]
    fn app_aligns_a_second_recording_to_the_reference() {
        use crate::model::MediaAsset;
        let ctx = eframe::egui::Context::default();
        let mut app = crate::app::App::with_ctx(&ctx, None);
        let mut rng = Rng(11);
        let bps = PEAKS_PER_SEC;
        let mut reference = vec![0.02f32; 20 * bps];
        for _ in 0..40 {
            let i = (rng.next() * (reference.len() - 10) as f32) as usize;
            for k in 0..3 {
                reference[i + k] = 0.5 + rng.next() * 0.5;
            }
        }
        let d = (1.3 * bps as f64) as usize;
        let mut other = vec![0.02f32; reference.len() + d];
        other[d..].copy_from_slice(&reference);
        let mk = |name: &str| MediaAsset { path: name.into(), name: name.into(), duration: 25.0, has_audio: true, ..Default::default() };
        let (ra, oa) = (mk("ref.wav"), mk("other.wav"));
        let (rid, oid) = (ra.id, oa.id);
        app.project.assets.push(ra);
        app.project.assets.push(oa);
        app.waveforms.insert(rid, std::sync::Arc::new(reference));
        app.waveforms.insert(oid, std::sync::Arc::new(other));
        let mut c1 = audio_clip(3.0, 20.0);
        c1.content = ClipContent::Media(Wrap { v: rid });
        let mut c2 = audio_clip(3.0, 20.0);
        c2.content = ClipContent::Media(Wrap { v: oid });
        let (i1, i2) = (c1.id, c2.id);
        app.project.add_clip(c1, 1);
        app.project.add_clip(c2, 3);
        assert!(!app.can_align_audio(i2), "ohne Auswahl nicht möglich");
        app.select(i1, false);
        app.select(i2, true);
        assert!(app.can_align_audio(i2));
        app.align_audio(i2);
        assert!(app.status.starts_with("Ton ausgerichtet"), "{}", app.status);
        assert!((app.project.clip(i2).unwrap().start - 1.7).abs() < 0.04, "um −1,3 s verschoben: {}", app.project.clip(i2).unwrap().start);
        assert!((app.project.clip(i1).unwrap().start - 3.0).abs() < 1e-9, "die Referenz bleibt");
    }
}
