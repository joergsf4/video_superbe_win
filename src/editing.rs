//! Bearbeitungsbefehle der Oberfläche (Auswahl, Zwischenablage, Ripple, Marken, Kapitel, Titel, Einrasten) –
//! Port der Funktionen aus AppState.swift. Die eigentliche Projektlogik steckt in `ops.rs`.

use crate::app::App;
use crate::model::*;
use std::collections::HashSet;
use uuid::Uuid;

impl App {
    // ---------- Auswahl ----------

    /// Hauptauswahl plus weitere Auswahl.
    pub(crate) fn selected_set(&self) -> HashSet<Uuid> {
        let mut s = self.extra_sel.clone();
        if let Some(id) = self.selected {
            if self.project.clip(id).is_some() {
                s.insert(id);
            }
        }
        s.retain(|id| self.project.clip(*id).is_some());
        s
    }

    /// Wählt den Clip; gehört er einer Gruppe an, werden alle Mitglieder mit gewählt (oder abgewählt).
    pub(crate) fn select(&mut self, id: Uuid, additive: bool) {
        let members = self.project.group_members(id);
        if additive {
            let mut set = self.selected_set();
            if set.contains(&id) {
                for m in &members {
                    set.remove(m);
                }
                set.remove(&id);
            } else {
                set.extend(members);
                set.insert(id);
            }
            self.selected = if set.contains(&id) { Some(id) } else { set.iter().next().copied() };
            self.extra_sel = set;
        } else {
            self.selected = Some(id);
            self.extra_sel = members;
            self.extra_sel.remove(&id);
        }
    }

    pub(crate) fn clear_selection(&mut self) {
        self.selected = None;
        self.extra_sel.clear();
    }

    pub(crate) fn select_all(&mut self) {
        let ids: HashSet<Uuid> = self.project.tracks.iter().flat_map(|t| t.clips.iter().map(|c| c.id)).collect();
        self.selected = ids.iter().next().copied();
        self.extra_sel = ids;
    }

    pub(crate) fn select_clips(&mut self, ids: HashSet<Uuid>, additive: bool) {
        let all: HashSet<Uuid> = if additive { self.selected_set().union(&ids).copied().collect() } else { ids };
        self.selected = all.iter().next().copied();
        self.extra_sel = all;
    }

    // ---------- Einfügen ----------

    /// Legt ein Medium an die Abspielposition auf die erste passende freie Spur (oder wie angegeben).
    pub(crate) fn add_to_timeline(&mut self, aid: Uuid, track: Option<usize>, at: Option<f64>, range: Option<(f64, f64)>) {
        let Some(a) = self.project.asset(aid).cloned() else { return };
        let start = at.unwrap_or(self.playhead);
        let mode = self.insert_mode;
        let target = track.or_else(|| (mode != crate::ops::InsertMode::Auto).then(|| self.default_insert_track(&a)));
        let id = self.project.insert_media_with(&a, start, target, range, mode, 1.0);
        match id {
            Some(id) => {
                if a.has_video && !a.is_image && (a.fps - self.project.fps as f64).abs() > 2.0 && a.fps > 0.0 {
                    self.status = format!("Hinweis: Der Clip hat {:.0} Bilder/s, das Projekt {}. Der Export rechnet auf die Projekt-Bildrate um (Filmeinstellungen: Taste E).", a.fps, self.project.fps);
                }
                self.select(id, false);
            }
            None => self.status = if mode == crate::ops::InsertMode::Auto { "Hier ist kein Platz.".into() } else { "Einfügen nicht möglich (gesperrte Spur?).".into() },
        }
    }

    pub(crate) fn add_asset_clip(&mut self, aid: Uuid) {
        self.add_to_timeline(aid, None, None, None);
    }

    pub(crate) fn add_track(&mut self) {
        let n = self.project.tracks.len() + 1;
        self.project.tracks.push(Track { name: format!("Spur {n}"), ..Default::default() });
    }

    /// Fügt eine leere Spur an Position `idx` ein und wählt sie.
    pub(crate) fn insert_track_at(&mut self, idx: usize) {
        let idx = idx.min(self.project.tracks.len());
        let n = self.project.tracks.len() + 1;
        self.project.tracks.insert(idx, Track { name: format!("Spur {n}"), ..Default::default() });
        self.sel_track = idx;
        self.status = format!("Spur {} eingefügt.", idx + 1);
    }

    /// Löscht Spur `idx` samt ihrer Clips; mindestens eine Spur bleibt.
    pub(crate) fn remove_track(&mut self, idx: usize) {
        if self.project.tracks.len() <= 1 || idx >= self.project.tracks.len() {
            self.status = "Die letzte Spur kann nicht gelöscht werden.".into();
            return;
        }
        let ids: Vec<Uuid> = self.project.tracks[idx].clips.iter().map(|c| c.id).collect();
        self.project.tracks.remove(idx);
        // Verknüpfungen zu gelöschten Clips auflösen
        for c in self.project.tracks.iter_mut().flat_map(|t| t.clips.iter_mut()) {
            if c.link.is_some_and(|l| ids.contains(&l)) {
                c.link = None;
            }
        }
        if self.selected.is_some_and(|s| ids.contains(&s)) {
            self.clear_selection();
        }
        self.sel_track = self.sel_track.min(self.project.tracks.len() - 1);
        self.status = if ids.is_empty() { format!("Spur {} gelöscht.", idx + 1) } else { format!("Spur {} samt {} Clip(s) gelöscht – Strg+Z macht es rückgängig.", idx + 1, ids.len()) };
    }

    // ---------- Schneiden, Löschen, Zwischenablage ----------

    /// Teilt an der Abspielposition: die Auswahl, ohne Auswahl alle Clips unter dem Marker.
    pub(crate) fn split_at_playhead(&mut self) {
        let t = self.playhead;
        let sel = self.selected;
        let targets: Vec<Uuid> = self.project.tracks.iter().flat_map(|tr| &tr.clips).filter(|c| c.start < t && c.end() > t && (sel.is_none() || Some(c.id) == sel)).map(|c| c.id).collect();
        let mut refused = false;
        for id in targets {
            // der Partner wurde evtl. schon mit geteilt
            if self.project.clip(id).is_some_and(|c| c.start < t && c.end() > t) && self.project.split(id, t).is_none() {
                refused = true;
            }
        }
        if refused {
            self.status = "Hier kann nicht geteilt werden: Innerhalb einer Blende würde ein Teilstück ganz überdeckt. Weiter vom Übergang entfernt teilen.".into();
        }
    }

    pub(crate) fn delete_selection(&mut self) {
        let ids = self.selected_set();
        if ids.is_empty() {
            return;
        }
        for id in ids {
            self.project.remove_clip(id);
        }
        self.clear_selection();
    }

    fn selected_items(&self) -> Vec<(Clip, usize)> {
        let ids = self.project.with_partners(&self.selected_set());
        let mut v = vec![];
        for (ti, t) in self.project.tracks.iter().enumerate() {
            for c in &t.clips {
                if ids.contains(&c.id) {
                    v.push((c.clone(), ti));
                }
            }
        }
        v
    }

    pub(crate) fn copy_clips(&mut self) {
        let items = self.selected_items();
        if !items.is_empty() {
            self.clipboard = items;
        }
    }

    pub(crate) fn cut_clips(&mut self) {
        self.copy_clips();
        self.delete_selection();
    }

    pub(crate) fn paste_clips(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        let ids = self.project.paste(&self.clipboard.clone(), self.playhead);
        if ids.is_empty() {
            self.status = "Kein Platz zum Einfügen".into();
        } else {
            self.select_clips(ids.into_iter().collect(), false);
        }
    }

    /// Kopie direkt hinter der Auswahl.
    pub(crate) fn duplicate_clips(&mut self) {
        let items = self.selected_items();
        let Some(last) = items.iter().map(|i| i.0.end()).reduce(f64::max) else { return };
        let ids = self.project.paste(&items, last);
        if !ids.is_empty() {
            self.select_clips(ids.into_iter().collect(), false);
        }
    }

    pub(crate) fn ripple_delete_selection(&mut self) {
        let ids = self.selected_set();
        if ids.is_empty() {
            return;
        }
        self.project.ripple_delete(&ids);
        self.clear_selection();
    }

    pub(crate) fn close_gap_before_selection(&mut self) {
        if let Some(id) = self.selected {
            self.project.close_gap(id);
        }
    }

    pub(crate) fn set_speed(&mut self, id: Uuid, v: f64) {
        let before = self.project.clone();
        self.project.set_speed(id, v);
        if self.project == before {
            self.status = "Kein Platz für die geänderte Länge".into();
        }
    }

    // ---------- Navigation ----------

    pub(crate) fn jump_to_edit_point(&mut self, forward: bool) {
        let pts = self.project.edit_points();
        let t = if forward { pts.iter().copied().find(|p| *p > self.playhead + 0.001) } else { pts.iter().rev().copied().find(|p| *p < self.playhead - 0.001) };
        self.set_playing(false);
        match t {
            Some(t) => self.playhead = t,
            None if !forward => self.playhead = 0.0,
            None => {}
        }
    }

    // ---------- Einrasten ----------

    /// Magnetisches Einrasten: Ziel in Reichweite (10 px) oder `None`. Kanten anderer Clips, Abspielmarker, Anfang,
    /// Kapitel- und Bereichsmarken, Beat-Linien. Mit gedrückter Alt-Taste vorübergehend aus.
    pub(crate) fn snap_target(&self, t: f64, exclude: &HashSet<Uuid>, alt: bool) -> Option<f64> {
        if !self.snapping || alt {
            return None;
        }
        let mut pts: Vec<f64> = vec![0.0, self.playhead];
        pts.extend(self.project.marker_list().iter().map(|m| m.t));
        pts.extend(self.project.mark_in);
        pts.extend(self.project.mark_out);
        pts.extend(self.project.bookmark_list().iter().map(|b| b.1));
        for c in self.project.tracks.iter().flat_map(|t| &t.clips).filter(|c| !exclude.contains(&c.id)) {
            pts.push(c.start);
            pts.push(c.end());
            // Andocklinien: Beats von Tonclips mit Taktraster (nächste Beats um die Zielzeit)
            if let Some(g) = &c.beat_grid {
                if t >= c.start - 1.0 && t <= c.end() + 1.0 {
                    let src = c.source_in + (t.clamp(c.start, c.end()) - c.start) * c.speed;
                    let k = ((src - g.phase) / g.period).round();
                    for d in -1..=1 {
                        let bs = g.phase + (k + d as f64) * g.period;
                        let tl = c.start + (bs - c.source_in) / c.speed;
                        if tl >= c.start - 1e-6 && tl <= c.end() + 1e-6 {
                            pts.push(tl);
                        }
                    }
                }
            }
        }
        let tol = 10.0 / self.zoom;
        let best = pts.into_iter().min_by(|a, b| (a - t).abs().total_cmp(&(b - t).abs()))?;
        ((best - t).abs() <= tol).then_some(best)
    }

    // ---------- Marken und Kapitel ----------

    pub(crate) fn set_mark_in(&mut self) {
        self.project.mark_in = Some(self.playhead);
        if self.project.mark_out.is_some_and(|o| o <= self.playhead) {
            self.project.mark_out = None;
        }
    }

    pub(crate) fn set_mark_out(&mut self) {
        self.project.mark_out = Some(self.playhead);
        if self.project.mark_in.is_some_and(|i| i >= self.playhead) {
            self.project.mark_in = None;
        }
    }

    pub(crate) fn clear_marks(&mut self) {
        self.project.mark_in = None;
        self.project.mark_out = None;
    }

    /// Bereich für den Export: zwischen den Marken, sonst `None` (ganzer Film).
    pub(crate) fn export_range(&self) -> Option<(f64, f64)> {
        let i = self.project.mark_in.unwrap_or(0.0);
        let o = self.project.mark_out.unwrap_or(self.project.duration());
        ((self.project.mark_in.is_some() || self.project.mark_out.is_some()) && o > i).then_some((i, o))
    }

    pub(crate) fn add_marker(&mut self) {
        let t = self.playhead;
        let list = self.project.markers.get_or_insert_with(Vec::new);
        if list.iter().any(|m| (m.t - t).abs() < 0.05) {
            return;
        }
        let name = format!("Kapitel {}", list.len() + 1);
        list.push(Marker { id: Uuid::new_v4(), t, name });
    }

    pub(crate) fn remove_marker(&mut self, id: Uuid) {
        if let Some(l) = self.project.markers.as_mut() {
            l.retain(|m| m.id != id);
        }
    }

    pub(crate) fn rename_marker(&mut self, id: Uuid, name: String) {
        if let Some(m) = self.project.markers.as_mut().and_then(|l| l.iter_mut().find(|m| m.id == id)) {
            m.name = name;
        }
    }

    /// Text für die YouTube-Beschreibung („0:00 Intro“ …).
    pub(crate) fn chapters_text(&self) -> String {
        let stamp = |t: f64| {
            let s = t.round() as i64;
            let (h, m, sec) = (s / 3600, s / 60 % 60, s % 60);
            if h > 0 { format!("{h}:{m:02}:{sec:02}") } else { format!("{m}:{sec:02}") }
        };
        self.project.marker_list().iter().map(|m| format!("{} {}", stamp(m.t), m.name)).collect::<Vec<_>>().join("\n")
    }

    /// Kopiert die Kapitel in die Zwischenablage und meldet YouTube-Hinweise in der Statuszeile.
    pub(crate) fn copy_chapters(&mut self, ctx: &eframe::egui::Context) {
        let list = self.project.marker_list();
        if list.is_empty() {
            self.status = "Keine Kapitelmarken (Taste M setzt eine)".into();
            return;
        }
        ctx.copy_text(self.chapters_text());
        let mut hint = String::new();
        if list[0].t.abs() > 0.5 {
            hint += " – Achtung: YouTube verlangt eine Marke bei 0:00";
        }
        if list.len() < 3 {
            hint += " – YouTube braucht mindestens 3 Kapitel";
        }
        self.status = format!("{} Kapitel kopiert{hint}", list.len());
    }
}
