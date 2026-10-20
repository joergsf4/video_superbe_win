//! Szenenerkennung (SceneDetector.swift): harte Schnitte aus dem Helligkeitsunterschied aufeinanderfolgender, stark
//! verkleinerter Bilder.

use crate::app::App;
use crate::features::Bg;
use std::path::Path;
use uuid::Uuid;

const W: usize = 48;
const H: usize = 27;

/// Ein Schnitt ist ein deutlicher, lokal größter Ausschlag (mindestens 22 Stufen und das Dreifache des Umfelds).
pub fn cuts(times: &[f64], diffs: &[f64]) -> Vec<f64> {
    let n = diffs.len();
    let mut r = vec![];
    for i in 0..n {
        let (lo, hi) = (i.saturating_sub(8), (i + 8).min(n.saturating_sub(1)));
        let mut around: Vec<f64> = (lo..=hi).filter(|j| *j != i).map(|j| diffs[j]).collect();
        around.sort_by(|a, b| a.total_cmp(b));
        let base = if around.is_empty() { 0.0 } else { around[around.len() / 2] };
        let is_max = (i.saturating_sub(3)..=(i + 3).min(n - 1)).all(|j| diffs[j] <= diffs[i]);
        if diffs[i] > 22.0 && diffs[i] > 3.0 * base && is_max {
            r.push(times[i]);
        }
    }
    r
}

/// RGBA-Bild → 48×27-Helligkeit `(B + 2G + R) / 4`.
pub fn luma_small(rgba: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; W * H];
    for oy in 0..H {
        let y = ((oy * 2 + 1) * h / (2 * H)).min(h.saturating_sub(1));
        for ox in 0..W {
            let x = ((ox * 2 + 1) * w / (2 * W)).min(w.saturating_sub(1));
            let o = (y * w + x) * 4;
            out[oy * W + ox] = ((rgba[o + 2] as u32 + rgba[o + 1] as u32 * 2 + rgba[o] as u32) / 4) as u8;
        }
    }
    out
}

/// Zeiten (Quellsekunden) der ersten Bilder neuer Szenen im Bereich `range` (blockierend).
pub fn detect(path: &Path, range: (f64, f64)) -> Vec<f64> {
    let Ok(mut dec) = crate::video::Decoder::open_checked(path) else { return vec![] };
    let fps = if dec.info.fps > 1.0 { dec.info.fps } else { 30.0 };
    let dt = 1.0 / fps.min(30.0);
    let (mut times, mut diffs) = (vec![], vec![]);
    let mut prev: Vec<u8> = vec![];
    let mut t = range.0;
    let mut last = f64::NEG_INFINITY;
    while t < range.1 {
        let Some(f) = dec.frame_at(t + dt * 0.3) else { break };
        if f.time > last + 1e-6 {
            last = f.time;
            let cur = luma_small(&f.rgba, f.width, f.height);
            if prev.len() == cur.len() {
                let sum: u64 = cur.iter().zip(&prev).map(|(a, b)| (*a as i32 - *b as i32).unsigned_abs() as u64).sum();
                times.push(f.time);
                diffs.push(sum as f64 / cur.len() as f64);
            }
            prev = cur;
        }
        t += dt;
    }
    cuts(&times, &diffs)
}

impl App {
    /// Erkennt harte Schnitte im Bildclip und teilt den Clip (samt Ton) an allen Treffern; ein Rückgängig-Schritt.
    pub(crate) fn detect_scenes(&mut self, id: Uuid) {
        if self.feat.scene_busy {
            return;
        }
        let Some(c) = self.project.clip(id).cloned().filter(|c| c.part == crate::model::ClipPart::Video && !c.is_title()) else { return };
        let Some(a) = c.asset_id().and_then(|a| self.project.asset(a)).cloned().filter(|a| !a.is_image) else { return };
        self.feat.scene_busy = true;
        self.status = "Suche nach Szenenwechseln …".into();
        let range = (c.source_in, c.source_in + c.duration * c.speed);
        let (tx, path) = (self.feat.tx.clone(), a.path.clone());
        std::thread::spawn(move || {
            crate::video::init();
            let _ = tx.send(Bg::Scenes(id, detect(Path::new(&path), range)));
        });
    }

    /// Wendet erkannte Szenenwechsel an (Zeiten in Quellsekunden).
    pub(crate) fn apply_scene_cuts(&mut self, id: Uuid, src_times: Vec<f64>) {
        self.feat.scene_busy = false;
        let Some(cur) = self.project.clip(id).cloned() else { return };
        let mut times: Vec<f64> = src_times.iter().map(|t| cur.start + (t - cur.source_in) / cur.speed).filter(|t| *t > cur.start + 0.3 && *t < cur.end() - 0.3).collect();
        times.sort_by(|a, b| b.total_cmp(a));
        if times.is_empty() {
            self.status = "Keine Szenenwechsel gefunden.".into();
            return;
        }
        for t in &times {
            // der geteilte Clip behält die ID des linken Teils: von hinten nach vorn teilen
            self.project.split(id, *t);
        }
        self.status = format!("{} Szenenwechsel gefunden, Clip geteilt.", times.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuts_match_the_mac_test_values() {
        let mut diffs = vec![1.5; 60];
        diffs[10] = 80.0;
        diffs[30] = 45.0;
        diffs[31] = 20.0;
        diffs[50] = 15.0; // zu schwach
        let times: Vec<f64> = (0..60).map(|i| i as f64 * 0.1).collect();
        assert_eq!(cuts(&times, &diffs), vec![1.0, 3.0]);
        assert!(cuts(&[], &[]).is_empty());
        assert!(cuts(&[0.0], &[100.0]).len() <= 1);
    }

    #[test]
    fn luma_of_solid_colours() {
        let px = |r: u8, g: u8, b: u8| -> Vec<u8> { (0..64 * 36).flat_map(|_| [r, g, b, 255]).collect() };
        assert_eq!(luma_small(&px(255, 255, 255), 64, 36)[100], 255);
        assert_eq!(luma_small(&px(0, 0, 0), 64, 36)[100], 0);
        let g = luma_small(&px(0, 255, 0), 64, 36)[100];
        assert!((125..=130).contains(&g), "Grün zählt doppelt: {g}");
    }

    /// Ende-zu-Ende: ein Video aus drei Farbblöcken wird exportiert, die Schnitte werden gefunden und der Clip geteilt.
    #[test]
    fn detects_scene_changes_in_an_exported_video_and_splits_the_clip() {
        use crate::model::*;
        use crate::render::tests::{clip, image_asset, solid_png};
        let mut p = Project::default();
        p.render_width = 320;
        p.render_height = 180;
        p.fps = 15;
        for (i, rgb) in [[230, 230, 230], [20, 20, 20], [200, 40, 40]].iter().enumerate() {
            let png = solid_png(&format!("scene_{i}.png"), 320, 180, *rgb);
            let a = image_asset(&mut p, &png, 320.0, 180.0);
            p.tracks[0].clips.push(clip(a, i as f64 * 2.0, 2.0));
        }
        let mp4 = std::env::temp_dir().join("video-superbe-tests").join("scenes.mp4");
        crate::export::export(&p, &mp4, &crate::export::ExportOpts::from_project(&p), &mut |_| true).unwrap();
        let found = detect(&mp4, (0.0, 6.0));
        assert_eq!(found.len(), 2, "zwei Schnitte: {found:?}");
        assert!((found[0] - 2.0).abs() < 0.15 && (found[1] - 4.0).abs() < 0.15, "bei 2 s und 4 s: {found:?}");
        // am Clip anwenden
        let ctx = eframe::egui::Context::default();
        let mut app = App::with_ctx(&ctx, None);
        app.import_file(mp4.clone());
        let aid = app.project.assets.iter().find(|a| !a.is_image).unwrap().id;
        app.add_to_timeline(aid, None, Some(0.0), None);
        let cid = app.selected.unwrap();
        app.apply_scene_cuts(cid, found);
        let n = app.project.tracks[0].clips.len();
        assert_eq!(n, 3, "Clip in drei Szenen geteilt");
        assert!(app.status.contains("2 Szenenwechsel"));
    }
}
