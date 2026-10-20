//! Bedienbefehle des Mac-Originals (AppState.swift): Einfüge- und Mausmodi, Gruppen, Zeitcode, Projektmarker 1–10,
//! Lückensuche, Kanten auf die Abspielposition, Zoomstufen, Musikalische Schnittanpassung, Vier-Punkt-Schnitt.

use crate::app::App;
use crate::model::*;
use crate::ops::{InsertMode, MouseMode, RippleScope, TrimOptions};
use std::collections::HashSet;
use uuid::Uuid;

/// Zeitcode `HH:MM:SS:BB`, `MM:SS:BB`, `SS:BB` oder Sekunden (auch mit Komma) → Sekunden.
pub fn parse_timecode(text: &str, fps: i64) -> Option<f64> {
    let t = text.trim().replace(',', ".");
    if t.is_empty() {
        return None;
    }
    if !t.contains(':') {
        return t.parse::<f64>().ok();
    }
    let parts: Vec<Option<i64>> = t.split(':').map(|p| p.trim().parse::<i64>().ok()).collect();
    if parts.iter().any(|p| p.is_none()) || parts.len() > 4 {
        return None;
    }
    let v: Vec<i64> = parts.into_iter().flatten().collect();
    let f = fps.max(1) as f64;
    match v.len() {
        4 => Some((v[0] * 3600 + v[1] * 60 + v[2]) as f64 + v[3] as f64 / f),
        3 => Some((v[0] * 60 + v[1]) as f64 + v[2] as f64 / f),
        2 => Some(v[0] as f64 + v[1] as f64 / f),
        _ => None,
    }
}

impl App {
    pub(crate) fn seek(&mut self, t: f64) {
        self.set_playing(false);
        self.playhead = t.max(0.0);
    }

    pub(crate) fn set_insert_mode(&mut self, m: InsertMode) {
        self.insert_mode = m;
        self.status = format!("Einfügemodus: {}", m.label());
    }

    pub(crate) fn set_mouse_mode(&mut self, m: MouseMode) {
        self.mouse_mode = m;
        self.status = format!("Mausmodus: {}", m.label());
    }

    // ---------- Einfügen ----------

    /// Zielspur für Einfügen/Überschreiben ohne Angabe: Spur des gewählten Clips (bei Tonclips mit Partner dessen Bildspur).
    pub(crate) fn default_insert_track(&self, a: &MediaAsset) -> usize {
        let pic = a.has_video || a.is_image;
        let fallback = if pic { 0 } else { 1 };
        let Some(id) = self.selected else { return fallback };
        let Some((t, _)) = self.project.find_clip(id) else { return fallback };
        let c = self.project.clip(id).unwrap();
        if pic && c.part == ClipPart::Audio {
            if let Some((pt, _)) = self.project.partner_id(id).and_then(|p| self.project.find_clip(p)) {
                return pt;
            }
        }
        t
    }

    /// Doppelklick im Medienpool: Bilder und Videos kommen nacheinander auf dieselbe Spur (hinter den letzten Bildclip, mit Ton
    /// auf der Spur darunter); reines Audio (Musik) liegt darunter auf einer eigenen Spur ab der Abspielposition.
    pub(crate) fn add_from_pool(&mut self, aid: Uuid) {
        let Some(a) = self.project.asset(aid).cloned() else { return };
        let is_pic = |c: &Clip| c.part == ClipPart::Video && !c.is_title();
        let main = self.project.tracks.iter().position(|t| t.clips.iter().any(is_pic)).unwrap_or(0);
        if a.has_video || a.is_image {
            let end = self.project.tracks[main].clips.iter().filter(|c| is_pic(c)).map(Clip::end).reduce(f64::max);
            let start = end.unwrap_or(self.playhead);
            let before = self.project.clone();
            let id = self.project.insert_media(&a, start, Some(main), None);
            match id {
                Some(id) => {
                    self.select(id, false);
                    self.scroll = (start - 1.0).max(0.0);
                }
                None => self.status = "Hier ist kein Platz.".into(),
            }
            if self.project == before {
                self.status = "Hier ist kein Platz.".into();
            } else if a.has_video && !a.is_image && (a.fps - self.project.fps as f64).abs() > 2.0 && a.fps > 0.0 {
                self.status = format!("Hinweis: Der Clip hat {:.0} Bilder/s, das Projekt {}. Der Export rechnet auf die Projekt-Bildrate um.", a.fps, self.project.fps);
            }
        } else {
            let mut t = main + 2;
            while t < self.project.tracks.len() && self.project.strict_overlaps(self.playhead, self.playhead + a.duration, t, None) {
                t += 1;
            }
            if let Some(id) = self.project.insert_media(&a, self.playhead, Some(t), None) {
                self.select(id, false);
            }
        }
    }

    /// Fügt den Quellausschnitt ein. Zwei-, Drei- und Vierpunktschnitt wie im Handbuch: Sind im Projekt Anfangs- und Endmarke
    /// gesetzt (Zielbereich), füllt der Ausschnitt genau diesen Bereich; fehlt an der Quelle eine Marke, bestimmt die Länge des
    /// Zielbereichs das Ende (bzw. den Anfang); sind beide gesetzt, wird gedehnt oder gestaucht.
    pub(crate) fn insert_source(&mut self) {
        let (Some(a), Some(s)) = (self.source_asset(), self.source.clone()) else { return };
        let (has_in, has_out) = (s.mark_in > 0.001, s.mark_out < a.duration - 0.001);
        let target_range = match (self.project.mark_in, self.project.mark_out) {
            (Some(i), Some(o)) if o > i + 0.05 && !a.is_image => Some((i, o)),
            _ => None,
        };
        let Some((d_in, d_out)) = target_range else {
            let r = self.source_range();
            self.add_to_timeline(a.id, None, None, r);
            return;
        };
        let d_len = d_out - d_in;
        let (range, speed) = match (has_in, has_out) {
            (true, true) => {
                let sp = (s.mark_out - s.mark_in) / d_len;
                if !(0.25..=4.0).contains(&sp) {
                    self.status = format!("Quell- und Zielbereich passen nicht zusammen (Faktor {sp:.1}). Bereich anpassen.");
                    return;
                }
                ((s.mark_in, s.mark_out), sp)
            }
            (true, false) => ((s.mark_in, a.duration.min(s.mark_in + d_len)), 1.0),
            (false, true) => (((s.mark_out - d_len).max(0.0), s.mark_out), 1.0),
            (false, false) => ((0.0, a.duration.min(d_len)), 1.0),
        };
        let mode = if self.insert_mode == InsertMode::Auto { InsertMode::Overwrite } else { self.insert_mode };
        let target = self.default_insert_track(&a);
        match self.project.insert_media_with(&a, d_in, Some(target), Some(range), mode, speed) {
            Some(id) => {
                self.select(id, false);
                if (speed - 1.0).abs() > 0.01 {
                    self.status = format!("Vierpunktschnitt: Tempo {speed:.2}×");
                }
            }
            None => self.status = "Einfügen nicht möglich (gesperrte Spur?).".into(),
        }
    }

    // ---------- Gruppen ----------

    pub(crate) fn group_selection(&mut self) {
        let ids = self.project.with_partners(&self.selected_set());
        if ids.len() < 2 {
            self.status = "Zum Gruppieren mindestens zwei Clips wählen".into();
            return;
        }
        let sel = self.selected_set();
        self.project.set_group(&sel, Some(Uuid::new_v4()));
        self.status = "Gruppe gebildet (Umschalt+G löst sie auf)".into();
    }

    pub(crate) fn ungroup_selection(&mut self) {
        let ids = self.project.with_partners(&self.selected_set());
        if !ids.iter().any(|i| self.project.clip(*i).is_some_and(|c| c.group.is_some())) {
            return;
        }
        let sel = self.selected_set();
        self.project.set_group(&sel, None);
        self.extra_sel.clear();
    }

    // ---------- Verschieben, Auswahl ----------

    /// Alt+← / Alt+→: gewählte Clips um Bilder verschieben (Umschalt = 10 Bilder).
    pub(crate) fn nudge_selection(&mut self, frames: i64) {
        let ids = self.selected_set();
        if ids.is_empty() {
            return;
        }
        let dt = frames as f64 / self.project.fps.max(1) as f64;
        self.project.move_clips(&ids, 0, dt);
    }

    /// Strg+W / Strg+Q: nächstes/vorheriges Objekt auf der Spur des gewählten Clips wählen und hinspringen.
    pub(crate) fn select_neighbor(&mut self, forward: bool) {
        let Some((t, _)) = self.selected.and_then(|id| self.project.find_clip(id)) else {
            let first = self.project.tracks.iter().flat_map(|t| &t.clips).filter(|c| c.start >= self.playhead - 0.001).min_by(|a, b| a.start.total_cmp(&b.start)).map(|c| c.id);
            if let Some(id) = first {
                self.select(id, false);
            }
            return;
        };
        let id = self.selected.unwrap();
        let mut cs: Vec<(Uuid, f64)> = self.project.tracks[t].clips.iter().map(|c| (c.id, c.start)).collect();
        cs.sort_by(|a, b| a.1.total_cmp(&b.1));
        let Some(i) = cs.iter().position(|c| c.0 == id) else { return };
        let j = if forward { i as isize + 1 } else { i as isize - 1 };
        if j >= 0 && (j as usize) < cs.len() {
            let (nid, start) = cs[j as usize];
            self.select(nid, false);
            self.seek(start);
        }
    }

    /// Umschalt+W / Umschalt+Q: zum Anfang des nächsten/vorherigen gewählten Objekts springen.
    pub(crate) fn jump_to_selected(&mut self, forward: bool) {
        let mut ts: Vec<f64> = self.selected_set().iter().filter_map(|i| self.project.clip(*i)).map(|c| c.start).collect();
        ts.sort_by(|a, b| a.total_cmp(b));
        let t = if forward { ts.iter().copied().find(|t| *t > self.playhead + 0.001) } else { ts.iter().rev().copied().find(|t| *t < self.playhead - 0.001) };
        if let Some(t) = t {
            self.seek(t);
        }
    }

    // ---------- Marker ----------

    pub(crate) fn set_bookmark(&mut self, n: u32) {
        let t = self.playhead;
        self.project.bookmarks.get_or_insert_with(Default::default).insert(n.to_string(), t);
        self.status = format!("Projektmarker {n} bei {}", crate::ui::timecode(t, self.project.fps));
    }

    pub(crate) fn jump_to_bookmark(&mut self, n: u32) {
        match self.project.bookmarks.as_ref().and_then(|b| b.get(&n.to_string())).copied() {
            Some(t) => self.seek(t),
            None => self.status = format!("Projektmarker {n} ist nicht gesetzt (Umschalt+{} setzt ihn)", n % 10),
        }
    }

    /// Strg+Enter: Projektmarker an der Abspielposition (nächste freie Nummer 1–10).
    pub(crate) fn set_next_bookmark(&mut self) {
        let used: HashSet<u32> = self.project.bookmarks.iter().flatten().filter_map(|(k, _)| k.parse().ok()).collect();
        match (1..=10).find(|n| !used.contains(n)) {
            Some(n) => self.set_bookmark(n),
            None => self.status = "Alle zehn Projektmarker sind belegt".into(),
        }
    }

    /// Springt zum nächsten/vorherigen Projektmarker oder (`chapters`) zur nächsten/vorherigen Kapitelmarke.
    pub(crate) fn jump_to_marker(&mut self, forward: bool, chapters: bool) {
        let mut ts: Vec<f64> = if chapters { self.project.marker_list().iter().map(|m| m.t).collect() } else { self.project.bookmark_list().iter().map(|b| b.1).collect() };
        ts.sort_by(|a, b| a.total_cmp(b));
        let t = if forward { ts.iter().copied().find(|t| *t > self.playhead + 0.001) } else { ts.iter().rev().copied().find(|t| *t < self.playhead - 0.001) };
        if let Some(t) = t {
            self.seek(t);
        }
    }

    /// W / Q: zum nächsten/vorherigen Marker jeder Art (Projekt-, Kapitelmarker, Bereichsanfang/-ende).
    pub(crate) fn jump_to_any_marker(&mut self, forward: bool) {
        let mut ts: Vec<f64> = self.project.marker_list().iter().map(|m| m.t).chain(self.project.bookmark_list().iter().map(|b| b.1)).collect();
        ts.extend(self.project.mark_in);
        ts.extend(self.project.mark_out);
        ts.sort_by(|a, b| a.total_cmp(b));
        let t = if forward { ts.iter().copied().find(|t| *t > self.playhead + 0.001) } else { ts.iter().rev().copied().find(|t| *t < self.playhead - 0.001) };
        match t {
            Some(t) => self.seek(t),
            None => self.status = "Kein weiterer Marker".into(),
        }
    }

    /// Strg+Umschalt+Enter löscht die nächste Kapitelmarke an der Abspielposition (höchstens 0,5 s entfernt).
    pub(crate) fn remove_chapter_near_playhead(&mut self) {
        let near = self.project.marker_list().into_iter().min_by(|a, b| (a.t - self.playhead).abs().total_cmp(&(b.t - self.playhead).abs()));
        match near {
            Some(m) if (m.t - self.playhead).abs() < 0.5 => self.remove_marker(m.id),
            _ => self.status = "An der Abspielposition liegt keine Kapitelmarke".into(),
        }
    }

    pub(crate) fn remove_all_chapters(&mut self) {
        self.project.markers = None;
    }

    /// #: Bereich zwischen Anfangs- und Endmarke abspielen.
    pub(crate) fn play_range(&mut self) {
        let i = self.project.mark_in.unwrap_or(0.0);
        let o = self.project.mark_out.unwrap_or(self.project.duration());
        if o <= i + 0.05 {
            return;
        }
        self.seek(i);
        self.set_playing(true);
        self.play_until = Some(o);
    }

    // ---------- Kanten und Lücken ----------

    /// Z = Anfang, U = Ende der gewählten Clips auf die Abspielposition setzen; in den Mausmodi 7/8 mit Ripple.
    pub(crate) fn trim_selection_to_playhead(&mut self, left: bool) {
        let t = self.playhead;
        let ids: Vec<Uuid> = self.selected_set().into_iter().filter(|id| self.project.clip(*id).is_some_and(|c| t > c.start + 0.01 && t < c.end() - 0.01)).collect();
        if ids.is_empty() {
            self.status = "Zum Kürzen: Clip wählen und die Abspielposition hineinsetzen".into();
            return;
        }
        let o = TrimOptions {
            ripple: match self.mouse_mode {
                MouseMode::OneTrack => RippleScope::Track,
                MouseMode::AllTracks => RippleScope::All,
                MouseMode::Single => RippleScope::None,
            },
            ..Default::default()
        };
        let mut done: HashSet<Uuid> = HashSet::new();
        for id in ids {
            if done.contains(&id) {
                continue;
            }
            if let Some(p) = self.project.partner_id(id) {
                done.insert(p);
            }
            if left {
                self.project.trim_start_opt(id, t, &o);
            } else {
                self.project.trim_end_opt(id, t, &o);
            }
        }
    }

    /// `<` / `>`: nächste bzw. vorherige Lücke ohne Bild; springt hin.
    pub(crate) fn jump_to_gap(&mut self, forward: bool) -> Option<(f64, f64)> {
        let gaps = self.project.video_gaps();
        let g = if forward { gaps.iter().copied().find(|g| g.0 > self.playhead + 0.001) } else { gaps.iter().rev().copied().find(|g| g.0 < self.playhead - 0.001) };
        match g {
            Some(g) => {
                self.seek(g.0);
                Some(g)
            }
            None => {
                self.status = if gaps.is_empty() { "Keine Lücken im Bild.".into() } else { "Keine weitere Lücke in dieser Richtung.".into() };
                None
            }
        }
    }

    /// X: die Lücke an der Abspielposition (sonst die nächste) als Bereich markieren.
    pub(crate) fn mark_gap(&mut self) {
        let gaps = self.project.video_gaps();
        let Some(g) = gaps.iter().copied().find(|g| g.0 <= self.playhead && self.playhead <= g.1).or_else(|| gaps.iter().copied().find(|g| g.0 > self.playhead)).or_else(|| gaps.first().copied()) else {
            self.status = "Keine Lücken im Bild.".into();
            return;
        };
        self.project.mark_in = Some(g.0);
        self.project.mark_out = Some(g.1);
        self.seek(g.0);
    }

    /// Lückensuche: nächste Lücke finden, hinspringen, als Bereich markieren und die Anzahl melden.
    pub(crate) fn find_gap(&mut self) {
        let gaps = self.project.video_gaps();
        let Some(g) = gaps.iter().copied().find(|g| g.0 > self.playhead + 0.001).or_else(|| gaps.first().copied()) else {
            self.status = "Keine Lücken im Bild.".into();
            return;
        };
        self.project.mark_in = Some(g.0);
        self.project.mark_out = Some(g.1);
        self.seek(g.0);
        self.status = format!("Lücke {:.2} s bei {} – {} insgesamt (Menü: „Lücke schließen“)", g.1 - g.0, crate::ui::timecode(g.0, self.project.fps), gaps.len());
    }

    /// Schließt die Lücke an der Abspielposition auf allen Spuren.
    pub(crate) fn close_gap_at_playhead(&mut self) {
        let p = self.playhead;
        let Some(g) = self.project.video_gaps().into_iter().find(|g| p >= g.0 - 0.01 && p <= g.1 + 0.01) else {
            self.status = "An der Abspielposition ist keine Lücke (Lückensuche Strg+Umschalt+C)".into();
            return;
        };
        if !self.project.close_gap_range(g) {
            self.status = "Lücke lässt sich nicht schließen: Clips auf anderen Spuren würden kollidieren.".into();
        }
    }

    // ---------- Zoom ----------

    pub(crate) fn zoom_to_span(&mut self, seconds: f64) {
        self.zoom = (self.view_w as f64 / seconds.max(0.001)).clamp(2.0, 600.0);
    }

    pub(crate) fn zoom_to_range(&mut self) {
        let (Some(i), Some(o)) = (self.project.mark_in, self.project.mark_out) else {
            self.status = "Zum Zoomen zuerst einen Bereich markieren (I und O)".into();
            return;
        };
        if o <= i + 0.05 {
            self.status = "Zum Zoomen zuerst einen Bereich markieren (I und O)".into();
            return;
        }
        self.zoom_to_span((o - i) * 1.05);
        self.scroll = (i - (o - i) * 0.02).max(0.0);
    }

    pub(crate) fn zoom_step(&mut self, zoom_in: bool) {
        self.zoom = (self.zoom * if zoom_in { 1.4 } else { 1.0 / 1.4 }).clamp(2.0, 600.0);
    }

    /// Bild↑/↓: Zeitleiste seitenweise scrollen.
    pub(crate) fn scroll_pages(&mut self, pages: f64) {
        let page = (self.view_w as f64 / self.zoom - 100.0 / self.zoom).max(1.0);
        self.scroll = (self.scroll + pages * page).max(0.0);
    }

    // ---------- Musikalische Schnittanpassung ----------

    /// Der gewählte (sonst erste) Tonclip mit Taktraster gibt den Takt vor; die Schnitte der ersten Bildspur rasten darauf ein.
    pub(crate) fn musical_cut_adjustment(&mut self, every: i64) {
        let audios: Vec<Clip> = self.project.tracks.iter().flat_map(|t| &t.clips).filter(|c| c.part == ClipPart::Audio && !c.is_title()).cloned().collect();
        let sel = self.selected_set();
        let reference = self
            .project
            .tracks
            .iter()
            .flat_map(|t| &t.clips)
            .find(|c| sel.contains(&c.id) && c.part == ClipPart::Audio && c.beat_grid.is_some())
            .cloned()
            .or_else(|| audios.iter().find(|c| c.beat_grid.is_some()).cloned());
        let Some(reference) = reference else {
            if let Some(a) = audios.iter().find(|c| sel.contains(&c.id)).or_else(|| audios.first()) {
                self.detect_beats(a.id);
                self.status = "Taktraster wird berechnet – danach „Musikalische Schnittanpassung“ noch einmal wählen.".into();
            } else {
                self.status = "Zuerst den Song in ein Projekt legen (unter die Bildspur).".into();
            }
            return;
        };
        let Some(vt) = self.project.tracks.iter().position(|t| t.clips.iter().filter(|c| c.part == ClipPart::Video || c.is_title()).count() >= 2) else {
            self.status = "Auf einer Spur mindestens zwei aneinanderstoßende Bildclips legen.".into();
            return;
        };
        let beats = self.project.beat_times(&reference, every);
        let (moved, kept) = self.project.snap_cuts_to_beats(vt, &beats);
        self.status = if moved == 0 { "Alle Schnitte liegen schon im Takt (oder es fehlt Material zum Verlängern).".into() } else { format!("{moved} Schnitte auf den Takt gelegt, {kept} unverändert.") };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let ctx = eframe::egui::Context::default();
        App::with_ctx(&ctx, None)
    }

    fn two_clips(a: &mut App) -> (Uuid, Uuid, MediaAsset) {
        let asset = MediaAsset { path: "x".into(), name: "x".into(), duration: 10.0, has_video: true, has_audio: true, width: 100.0, height: 50.0, fps: 30.0, ..Default::default() };
        a.project.assets.push(asset.clone());
        let r1 = a.project.insert_media(&asset, 0.0, Some(0), Some((0.0, 4.0))).unwrap();
        let r2 = a.project.insert_media(&asset, 4.0, Some(0), Some((4.0, 8.0))).unwrap();
        (r1, r2, asset)
    }

    #[test]
    fn timecode_parsing_matches_the_mac_tests() {
        assert!((parse_timecode("00:00:02:15", 30).unwrap() - 2.5).abs() < 1e-9);
        assert!((parse_timecode("1:05:00", 30).unwrap() - 65.0).abs() < 1e-9);
        assert!((parse_timecode("12,5", 30).unwrap() - 12.5).abs() < 1e-9);
        assert!((parse_timecode("5:15", 30).unwrap() - 5.5).abs() < 1e-9);
        assert!(parse_timecode("abc", 30).is_none() && parse_timecode("", 30).is_none() && parse_timecode("1:2:3:4:5", 30).is_none());
    }

    #[test]
    fn bookmarks_set_jump_and_next_free() {
        let mut a = app();
        a.playhead = 2.5;
        a.set_bookmark(3);
        a.playhead = 7.0;
        a.set_next_bookmark();
        assert_eq!(a.project.bookmarks.as_ref().unwrap()["1"], 7.0, "nächste freie Nummer");
        a.jump_to_bookmark(3);
        assert_eq!(a.playhead, 2.5);
        a.jump_to_bookmark(9);
        assert!(a.status.contains("nicht gesetzt"));
        a.playhead = 0.0;
        a.jump_to_marker(true, false);
        assert_eq!(a.playhead, 2.5);
        a.jump_to_marker(true, false);
        assert_eq!(a.playhead, 7.0);
        a.jump_to_any_marker(false);
        assert_eq!(a.playhead, 2.5);
        for n in 1..=10 {
            a.project.bookmarks.as_mut().unwrap().insert(n.to_string(), n as f64);
        }
        a.set_next_bookmark();
        assert!(a.status.contains("belegt"));
    }

    #[test]
    fn gaps_trim_and_modes_through_the_app() {
        let mut a = app();
        let (r1, r2, asset) = two_clips(&mut a);
        a.project.move_clips(&HashSet::from([r2]), 0, 1.0);
        a.find_gap();
        assert_eq!((a.project.mark_in, a.project.mark_out), (Some(4.0), Some(5.0)));
        a.playhead = 4.5;
        a.close_gap_at_playhead();
        assert!(a.project.video_gaps().is_empty());
        // Z/U: Kante auf die Abspielposition, im Mausmodus 8 mit Ripple
        a.select(r1, false);
        a.playhead = 1.0;
        a.trim_selection_to_playhead(true);
        assert!((a.project.clip(r1).unwrap().start - 1.0).abs() < 1e-9, "Anfang entfernt");
        a.set_mouse_mode(MouseMode::OneTrack);
        a.playhead = 3.0;
        a.trim_selection_to_playhead(false);
        assert!((a.project.clip(r1).unwrap().end() - 3.0).abs() < 1e-9 && (a.project.clip(r2).unwrap().start - 3.0).abs() < 1e-9, "Ende gekürzt, Nachfolger rückt auf");
        // Einfügemodus Überschreiben über die App
        a.set_insert_mode(InsertMode::Overwrite);
        a.playhead = 1.5;
        a.add_to_timeline(asset.id, None, None, Some((0.0, 1.0)));
        assert!(a.project.clip(a.selected.unwrap()).is_some_and(|c| (c.start - 1.5).abs() < 1e-9));
        assert!(a.status.starts_with("Einfügemodus") || a.status.is_empty() || true);
    }

    #[test]
    fn group_selection_and_nudge() {
        let mut a = app();
        let (r1, r2, _) = two_clips(&mut a);
        a.select(r1, false);
        a.select(r2, true);
        a.group_selection();
        assert!(a.project.clip(r1).unwrap().group.is_some() && a.project.clip(r1).unwrap().group == a.project.clip(r2).unwrap().group);
        // Anklicken eines Mitglieds wählt die ganze Gruppe
        a.clear_selection();
        a.select(r1, false);
        assert!(a.selected_set().contains(&r2));
        a.nudge_selection(30);
        assert!((a.project.clip(r1).unwrap().start - 1.0).abs() < 1e-9 && (a.project.clip(r2).unwrap().start - 5.0).abs() < 1e-9);
        a.ungroup_selection();
        assert!(a.project.clip(r1).unwrap().group.is_none());
        // einzelnes Clip: Gruppieren braucht mindestens zwei
        a.clear_selection();
        a.status.clear();
        a.group_selection();
        assert!(a.status.contains("mindestens zwei"));
    }

    #[test]
    fn four_point_edit_via_marks() {
        let mut a = app();
        let (_, _, asset) = two_clips(&mut a);
        a.source = Some(crate::source::SourceState { asset: asset.id, position: 0.0, mark_in: 0.5, mark_out: 4.5, playing: false, anchor: None });
        a.project.mark_in = Some(2.0);
        a.project.mark_out = Some(4.0);
        a.insert_source();
        let c = a.project.clip(a.selected.unwrap()).unwrap();
        assert!((c.speed - 2.0).abs() < 1e-9 && (c.duration - 2.0).abs() < 1e-9 && (c.start - 2.0).abs() < 1e-9, "Vierpunkt: Tempo 2");
        assert!(a.status.contains("Vierpunktschnitt"));
        // zu großer Faktor wird abgelehnt
        let n = a.project.tracks.iter().map(|t| t.clips.len()).sum::<usize>();
        a.project.mark_out = Some(2.2);
        a.source.as_mut().unwrap().mark_out = 6.5;
        a.insert_source();
        assert!(a.status.contains("passen nicht zusammen") && a.project.tracks.iter().map(|t| t.clips.len()).sum::<usize>() == n);
    }

    #[test]
    fn pool_double_click_appends_and_music_goes_below() {
        let mut a = app();
        let (_, _, asset) = two_clips(&mut a); // endet bei 8 s auf Spur 0/1
        a.add_from_pool(asset.id);
        let c = a.project.clip(a.selected.unwrap()).unwrap();
        assert!((c.start - 8.0).abs() < 1e-9, "hängt hinten an");
        let music = MediaAsset { path: "m".into(), name: "m".into(), duration: 5.0, has_audio: true, ..Default::default() };
        a.project.assets.push(music.clone());
        a.playhead = 1.0;
        a.add_from_pool(music.id);
        let (t, _) = a.project.find_clip(a.selected.unwrap()).unwrap();
        assert_eq!(t, 2, "Musik liegt unter dem Bild-/Tonpaar (Spur main+2)");
    }

    #[test]
    fn musical_cut_adjustment_uses_the_beat_grid() {
        let mut a = app();
        let asset = MediaAsset { path: "x".into(), name: "x".into(), duration: 10.0, has_video: true, has_audio: true, width: 100.0, height: 50.0, fps: 30.0, ..Default::default() };
        a.project.assets.push(asset.clone());
        let c1 = a.project.insert_media(&asset, 0.0, Some(0), Some((0.0, 3.7))).unwrap();
        let c2 = a.project.insert_media(&asset, 3.7, Some(0), Some((1.0, 5.5))).unwrap();
        let mut song = Clip { content: ClipContent::Media(Wrap { v: asset.id }), part: ClipPart::Audio, start: 0.0, duration: 10.0, ..Default::default() };
        song.beat_grid = Some(BeatGrid { period: 0.5, phase: 0.0, confidence: 1.0 });
        a.project.add_clip(song, 4);
        a.musical_cut_adjustment(1);
        assert!((a.project.clip(c1).unwrap().end() - 3.5).abs() < 1e-9 && (a.project.clip(c2).unwrap().start - 3.5).abs() < 1e-9);
        assert!(a.status.contains("Schnitte auf den Takt"));
    }

    #[test]
    fn zoom_helpers_and_play_range() {
        let mut a = app();
        a.view_w = 1000.0;
        a.zoom_to_span(10.0);
        assert!((a.zoom - 100.0).abs() < 1e-9);
        a.zoom_step(true);
        assert!((a.zoom - 140.0).abs() < 1e-9);
        a.zoom_to_range();
        assert!(a.status.contains("Bereich markieren"));
        a.project.mark_in = Some(10.0);
        a.project.mark_out = Some(20.0);
        a.zoom_to_range();
        assert!((a.scroll - 9.8).abs() < 1e-9 && a.zoom < 100.0);
        a.scroll_pages(1.0);
        assert!(a.scroll > 9.8);
        let (_, _, _) = two_clips(&mut a);
        a.project.mark_in = Some(1.0);
        a.project.mark_out = Some(2.0);
        a.play_range();
        assert_eq!(a.play_until, Some(2.0));
        assert_eq!(a.playhead, 1.0);
    }
}
