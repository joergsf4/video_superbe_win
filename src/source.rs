//! Quellmonitor: Medium aus dem Pool im Monitor ansehen, Bereich wählen (Anfang/Ende) und in die Timeline
//! ziehen oder einfügen (SourceState in AppState.swift, SourceBar in PreviewView.swift).

use crate::app::App;
use crate::model::*;
use crate::ops::MIN_CLIP;
use crate::player::Player;
use std::time::Instant;
use uuid::Uuid;

/// Nutzdaten beim Ziehen eines Mediums (aus dem Pool oder dem Quellmonitor) auf die Timeline.
#[derive(Clone, Debug)]
pub struct DragAsset {
    pub id: Uuid,
    pub range: Option<(f64, f64)>,
}

#[derive(Clone, Debug)]
pub struct SourceState {
    pub asset: Uuid,
    pub position: f64,
    pub mark_in: f64,
    pub mark_out: f64,
    pub playing: bool,
    pub anchor: Option<(Instant, f64)>,
}

impl App {
    pub(crate) fn source_asset(&self) -> Option<MediaAsset> {
        self.source.as_ref().and_then(|s| self.project.asset(s.asset)).cloned()
    }

    pub(crate) fn load_source(&mut self, id: Uuid) {
        let Some(a) = self.project.asset(id).cloned() else { return };
        if !a.is_image && !std::path::Path::new(&a.path).exists() {
            self.status = "Quelle nicht gefunden".into();
            return;
        }
        self.set_playing(false);
        self.source_player = None;
        self.source = Some(SourceState { asset: id, position: 0.0, mark_in: 0.0, mark_out: a.duration, playing: false, anchor: None });
    }

    pub(crate) fn exit_source(&mut self) {
        self.source_player = None;
        self.source = None;
    }

    pub(crate) fn seek_source(&mut self, t: f64) {
        let Some(a) = self.source_asset() else { return };
        if let Some(s) = self.source.as_mut() {
            s.position = t.clamp(0.0, a.duration);
            if s.playing {
                s.anchor = Some((Instant::now(), s.position));
            }
        }
        if self.source.as_ref().is_some_and(|s| s.playing) {
            self.start_source_audio();
        }
    }

    fn start_source_audio(&mut self) {
        let (Some(a), Some(s)) = (self.source_asset(), self.source.as_ref()) else { return };
        if !a.has_audio {
            self.source_player = None;
            return;
        }
        let mut p = Project::default();
        let audio_only = MediaAsset { has_video: false, is_image: false, ..a.clone() };
        p.assets.push(audio_only.clone());
        p.insert_media(&audio_only, 0.0, Some(0), None);
        self.source_player = Some(Player::start(p, s.position));
    }

    pub(crate) fn pause_source(&mut self) {
        if let Some(s) = self.source.as_mut() {
            s.playing = false;
            s.anchor = None;
        }
        self.source_player = None;
    }

    pub(crate) fn toggle_source_play(&mut self) {
        let Some(a) = self.source_asset() else { return };
        if a.is_image {
            return;
        }
        let Some(s) = self.source.clone() else { return };
        if s.playing {
            self.pause_source();
            return;
        }
        self.set_playing(false);
        if s.position >= s.mark_out - 0.05 || s.position < s.mark_in - 0.01 {
            if let Some(m) = self.source.as_mut() {
                m.position = s.mark_in;
            }
        }
        if let Some(m) = self.source.as_mut() {
            m.playing = true;
            m.anchor = Some((Instant::now(), m.position));
        }
        self.start_source_audio();
    }

    pub(crate) fn step_source(&mut self, frames: i64) {
        let Some(a) = self.source_asset() else { return };
        let Some(pos) = self.source.as_ref().map(|s| s.position) else { return };
        self.pause_source();
        self.seek_source(pos + frames as f64 / a.fps.max(1.0));
    }

    pub(crate) fn set_source_in(&mut self) {
        let Some(a) = self.source_asset() else { return };
        if let Some(s) = self.source.as_mut() {
            s.mark_in = s.position;
            if s.mark_out <= s.mark_in + MIN_CLIP {
                s.mark_out = a.duration;
            }
        }
    }

    pub(crate) fn set_source_out(&mut self) {
        if let Some(s) = self.source.as_mut() {
            s.mark_out = s.position;
            if s.mark_in >= s.mark_out - MIN_CLIP {
                s.mark_in = 0.0;
            }
        }
    }

    /// Gewählter Ausschnitt der Quelle (`None` = ganzes Medium).
    pub(crate) fn source_range(&self) -> Option<(f64, f64)> {
        let (s, a) = (self.source.as_ref()?, self.source_asset()?);
        (!a.is_image && (s.mark_in > 0.001 || s.mark_out < a.duration - 0.001)).then_some((s.mark_in, s.mark_out))
    }

    /// Fügt den gewählten Ausschnitt an der Abspielposition ein.
    pub(crate) fn insert_source(&mut self) {
        let Some(s) = self.source.as_ref().map(|s| s.asset) else { return };
        let r = self.source_range();
        self.add_to_timeline(s, None, None, r);
    }

    /// Lässt die Quellwiedergabe weiterlaufen (jedes Bild).
    pub(crate) fn tick_source(&mut self) {
        let Some(s) = self.source.clone() else { return };
        if !s.playing {
            return;
        }
        let Some((t0, p0)) = s.anchor else { return };
        let mut pos = p0 + t0.elapsed().as_secs_f64();
        let mut stop = false;
        if pos >= s.mark_out - 0.01 {
            pos = s.mark_out;
            stop = true;
        }
        if let Some(m) = self.source.as_mut() {
            m.position = pos;
        }
        if stop {
            self.pause_source();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_and_range_follow_the_swift_rules() {
        let ctx = eframe::egui::Context::default();
        let mut app = App::with_ctx(&ctx, None);
        let png = crate::render::tests::solid_png("src_img.png", 8, 8, [1, 1, 1]);
        app.import_file(png.clone());
        let wav = crate::audio::tests::write_sine_wav("src_tone.wav", 440.0, 10.0, 48000, 0.2);
        app.import_file(wav);
        let wav_id = app.project.assets.last().unwrap().id;
        app.load_source(wav_id);
        assert_eq!(app.source_range(), None, "ganzes Medium");
        app.seek_source(3.0);
        app.set_source_in();
        app.seek_source(6.0);
        app.set_source_out();
        assert_eq!(app.source_range(), Some((3.0, 6.0)));
        // Ende vor dem Anfang setzt den Anfang auf 0 zurück
        app.seek_source(2.0);
        app.set_source_out();
        assert_eq!(app.source.as_ref().unwrap().mark_in, 0.0);
        // Anfang hinter dem Ende schiebt das Ende ans Medienende
        app.seek_source(9.0);
        app.set_source_in();
        assert_eq!(app.source.as_ref().unwrap().mark_out, 10.0);
        app.seek_source(100.0);
        assert_eq!(app.source.as_ref().unwrap().position, 10.0, "begrenzt auf die Dauer");
        // Einfügen legt den Ausschnitt an die Abspielposition
        app.source.as_mut().unwrap().mark_in = 3.0;
        app.source.as_mut().unwrap().mark_out = 5.0;
        app.playhead = 12.0;
        app.insert_source();
        let c = app.project.clip(app.selected.unwrap()).unwrap();
        assert_eq!((c.start, c.source_in, c.duration), (12.0, 3.0, 2.0));
        // Bilder haben keinen Bereich und keine Wiedergabe
        let img_id = app.project.assets.iter().find(|a| a.is_image).unwrap().id;
        app.load_source(img_id);
        assert_eq!(app.source_range(), None);
        app.toggle_source_play();
        assert!(!app.source.as_ref().unwrap().playing);
        app.exit_source();
        assert!(app.source.is_none());
    }

    #[test]
    fn playback_runs_and_stops_at_the_end_mark() {
        let ctx = eframe::egui::Context::default();
        let mut app = App::with_ctx(&ctx, None);
        let wav = crate::audio::tests::write_sine_wav("src_tone2.wav", 440.0, 2.0, 48000, 0.2);
        app.import_file(wav);
        let id = app.project.assets.last().unwrap().id;
        app.load_source(id);
        app.source.as_mut().unwrap().mark_in = 0.2;
        app.source.as_mut().unwrap().mark_out = 0.5;
        app.toggle_source_play();
        assert!(app.source.as_ref().unwrap().playing);
        assert!((app.source.as_ref().unwrap().position - 0.2).abs() < 1e-9, "startet am Anfangspunkt");
        std::thread::sleep(std::time::Duration::from_millis(450));
        app.tick_source();
        let s = app.source.as_ref().unwrap();
        assert!(!s.playing && (s.position - 0.5).abs() < 1e-9, "hält am Endpunkt: {} {}", s.playing, s.position);
        app.step_source(-3);
        assert!((app.source.as_ref().unwrap().position - 0.5 + 3.0 / 30.0).abs() < 0.04 || app.source.as_ref().unwrap().position < 0.5);
    }
}
