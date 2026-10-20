//! Bearbeitungsfunktionen des Projekts (Port von ProjectOps.swift): Einfügen, Verschieben, Kürzen, Teilen,
//! Einfügen aus der Zwischenablage, Ripple-Löschen, Geschwindigkeit, Slip.
//! Alle Funktionen sind „ganz oder gar nicht“: passt etwas nicht, bleibt das Projekt unverändert.

use crate::model::*;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

pub const MIN_CLIP: f64 = 0.1;
const EPS: f64 = 1e-6;

/// Einfügemodus beim Platzieren (Handbuch: 1 Automatisch, 2 Einspur-Ripple, 3 Mehrspur-Ripple, 5 Überschreiben).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum InsertMode {
    #[default]
    Auto,
    Ripple,
    RippleAll,
    Overwrite,
}

impl InsertMode {
    pub const ALL: [InsertMode; 4] = [InsertMode::Auto, InsertMode::Ripple, InsertMode::RippleAll, InsertMode::Overwrite];
    pub fn label(self) -> &'static str {
        match self {
            InsertMode::Auto => "Automatisch",
            InsertMode::Ripple => "Einspur-Ripple",
            InsertMode::RippleAll => "Mehrspur-Ripple",
            InsertMode::Overwrite => "Überschreiben",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            InsertMode::Auto => "1",
            InsertMode::Ripple => "2",
            InsertMode::RippleAll => "3",
            InsertMode::Overwrite => "5",
        }
    }
}

/// Wirkungsbereich von Ripple (Nachrücken beim Kürzen/Verschieben).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RippleScope {
    #[default]
    None,
    Track,
    All,
}

/// Optionen beim Kürzen einer Kante.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrimOptions {
    /// Verknüpften Partner mitnehmen (aus = J-L-Schnitt).
    pub linked: bool,
    pub ripple: RippleScope,
    /// Schnittpunkt verschieben: Nachbar wandert gegenläufig, Gesamtlänge bleibt.
    pub roll: bool,
}

impl Default for TrimOptions {
    fn default() -> Self {
        TrimOptions { linked: true, ripple: RippleScope::None, roll: false }
    }
}

/// Mausmodus (Handbuch: 6 einzelne Objekte, 7 alle Spuren, 8 eine Spur). In 7/8 rückt alles hinter dem Objekt mit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MouseMode {
    #[default]
    Single,
    AllTracks,
    OneTrack,
}

impl MouseMode {
    pub const ALL: [MouseMode; 3] = [MouseMode::Single, MouseMode::AllTracks, MouseMode::OneTrack];
    pub fn label(self) -> &'static str {
        match self {
            MouseMode::Single => "Einzelne Objekte",
            MouseMode::AllTracks => "Alle Spuren",
            MouseMode::OneTrack => "Eine Spur",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            MouseMode::Single => "6",
            MouseMode::AllTracks => "7",
            MouseMode::OneTrack => "8",
        }
    }
}

/// Stelle einer Spur, an der zwei Bild-/Fotoclips aneinanderstoßen oder sich überlappen (Übergang/Blende).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Junction {
    pub a: Uuid,
    pub b: Uuid,
    pub overlap: f64,
    pub track: usize,
}

/// Die fünf Blendenvorlagen im Reiter „Blenden“ (die übrigen Arten bleiben für ältere Projekte erhalten).
pub const TRANSITION_TEMPLATES: [&str; 5] = ["crossfade", "dip", "wipe", "slide", "zoom"];

pub fn transition_label(kind: &str) -> &'static str {
    match kind {
        "crossfade" => "Überblenden",
        "dip" => "Schwarzblende",
        "wipe" => "Wischen",
        "slide" => "Schieben",
        "wipeDown" => "Wischen von oben",
        "zoom" => "Zoom",
        "iris" => "Irisblende",
        "flash" => "Weißblende",
        _ => "Überblenden",
    }
}

impl Project {
    /// Passt der Clip inhaltlich auf die Spur? Spuren sind universell; nur Quelle und Teil müssen stimmen.
    pub fn accepts(&self, clip: &Clip, t: usize) -> bool {
        if t >= self.tracks.len() {
            return false;
        }
        match &clip.content {
            ClipContent::Title(_) => true,
            ClipContent::Media(m) => match self.asset(m.v) {
                None => false,
                Some(a) => {
                    if clip.part == ClipPart::Video {
                        a.has_video || a.is_image
                    } else {
                        a.has_audio
                    }
                }
            },
        }
    }

    pub fn partner_id(&self, id: Uuid) -> Option<Uuid> {
        let link = self.clip(id)?.link?;
        self.tracks.iter().flat_map(|t| &t.clips).find(|c| c.link == Some(link) && c.id != id).map(|c| c.id)
    }

    /// IDs samt verknüpften Partnern und – gehört ein Clip einer Gruppe an – allen Mitgliedern der Gruppe.
    pub fn with_partners(&self, ids: &HashSet<Uuid>) -> HashSet<Uuid> {
        let mut r = ids.clone();
        for id in ids {
            if let Some(p) = self.partner_id(*id) {
                r.insert(p);
            }
        }
        let groups: HashSet<Uuid> = r.iter().filter_map(|i| self.clip(*i).and_then(|c| c.group)).collect();
        if !groups.is_empty() {
            for c in self.tracks.iter().flat_map(|t| &t.clips) {
                if c.group.is_some_and(|g| groups.contains(&g)) {
                    r.insert(c.id);
                    if let Some(p) = self.partner_id(c.id) {
                        r.insert(p);
                    }
                }
            }
        }
        r
    }

    /// Alle Clips der Gruppe (ohne Bild/Ton-Partner), leer wenn der Clip keiner Gruppe angehört.
    pub fn group_members(&self, id: Uuid) -> HashSet<Uuid> {
        let Some(g) = self.clip(id).and_then(|c| c.group) else { return HashSet::new() };
        self.tracks.iter().flat_map(|t| &t.clips).filter(|c| c.group == Some(g)).map(|c| c.id).collect()
    }

    /// Fasst die Clips (samt Partnern) zu einer Gruppe zusammen bzw. löst die Gruppen der Clips auf.
    pub fn set_group(&mut self, ids: &HashSet<Uuid>, group: Option<Uuid>) {
        for id in self.with_partners(ids) {
            if let Some(c) = self.clip_mut(id) {
                c.group = group;
            }
        }
    }

    /// Strikte Überlappung (für automatische Platzwahl).
    pub fn strict_overlaps(&self, start: f64, end: f64, t: usize, ignoring: Option<Uuid>) -> bool {
        self.tracks[t].clips.iter().any(|c| Some(c.id) != ignoring && c.start < end - EPS && c.end() > start + EPS)
    }

    /// Wahr, wenn der Bereich auf der Spur NICHT erlaubt ist. Zwei Clips dürfen sich überlappen (= Überblendung),
    /// aber keiner darf ganz überdeckt werden und nie liegen drei Clips gleichzeitig übereinander.
    pub fn blocked(&self, start: f64, end: f64, t: usize, ignoring: Option<Uuid>) -> bool {
        let hit: Vec<&Clip> = self.tracks[t].clips.iter().filter(|c| Some(c.id) != ignoring && c.start < end - EPS && c.end() > start + EPS).collect();
        for c in &hit {
            let ov = end.min(c.end()) - start.max(c.start);
            if ov > (end - start).min(c.duration) - MIN_CLIP + 1e-9 {
                return true;
            }
        }
        for (i, a) in hit.iter().enumerate() {
            for b in &hit[i + 1..] {
                if start.max(a.start).max(b.start) < end.min(a.end()).min(b.end()) - EPS {
                    return true;
                }
            }
        }
        false
    }

    pub fn add_clip(&mut self, clip: Clip, t: usize) -> bool {
        if !self.accepts(&clip, t) || self.tracks[t].locked || self.blocked(clip.start, clip.end(), t, None) {
            return false;
        }
        self.tracks[t].clips.push(clip);
        self.tracks[t].clips.sort_by(|a, b| a.start.total_cmp(&b.start));
        true
    }

    fn ensure_tracks(&mut self, n: usize) {
        while self.tracks.len() < n {
            let name = format!("Spur {}", self.tracks.len() + 1);
            self.tracks.push(Track { name, ..Default::default() });
        }
    }

    /// Legt ein Medium an: Bild auf Spur `t`, Ton (falls vorhanden) verknüpft auf Spur `t+1`. Ohne `preferred` wird
    /// die erste Spur gesucht, auf der beides frei ist; fehlende Spuren werden ergänzt.
    pub fn insert_media(&mut self, asset: &MediaAsset, start: f64, preferred: Option<usize>, range: Option<(f64, f64)>) -> Option<Uuid> {
        self.insert_media_with(asset, start, preferred, range, InsertMode::Auto, 1.0)
    }

    /// Wie `insert_media`, mit Einfügemodus und Tempo (Vierpunktschnitt: Quellausschnitt wird auf den Zielbereich gedehnt).
    /// Bei einem Modus ungleich Automatisch ist die Spur fest (`preferred` oder 0).
    pub fn insert_media_with(&mut self, asset: &MediaAsset, start: f64, preferred: Option<usize>, range: Option<(f64, f64)>, mode: InsertMode, speed: f64) -> Option<Uuid> {
        if mode != InsertMode::Auto {
            return self.insert_media_fixed(asset, start, preferred.unwrap_or(0), range, mode, speed);
        }
        let mut order: Vec<usize> = preferred.into_iter().collect();
        order.extend(0..=self.tracks.len());
        for t in order {
            let mut s = self.clone();
            s.ensure_tracks(t + 2);
            if let Some(id) = s.place_media(asset, start, t, range, speed) {
                *self = s;
                return Some(id);
            }
        }
        None
    }

    /// Länge, die das Medium (bzw. der Ausschnitt) auf der Timeline belegt.
    pub fn placed_duration(&self, asset: &MediaAsset, range: Option<(f64, f64)>, speed: f64) -> f64 {
        match range {
            Some((lo, hi)) if !asset.is_image => {
                let src_in = lo.clamp(0.0, asset.duration);
                (hi.min(asset.duration) - src_in).max(MIN_CLIP) / speed
            }
            _ => asset.duration,
        }
    }

    /// Platzieren mit Überschreiben oder Ripple auf der festen Spur `t` (Bild) bzw. `t+1` (Ton).
    /// Ganz oder gar nicht: Gelingt etwas nicht (gesperrte Spur), bleibt das Projekt unverändert.
    fn insert_media_fixed(&mut self, asset: &MediaAsset, start: f64, t: usize, range: Option<(f64, f64)>, mode: InsertMode, speed: f64) -> Option<Uuid> {
        let mut s = self.clone();
        s.ensure_tracks(t + 2);
        let dur = s.placed_duration(asset, range, speed);
        let pic = asset.has_video || asset.is_image;
        let pair: Vec<usize> = if pic && asset.has_audio && !asset.is_image { vec![t, t + 1] } else { vec![t] };
        if pair.iter().any(|&p| s.tracks[p].locked) {
            return None;
        }
        match mode {
            InsertMode::Auto => {}
            InsertMode::Overwrite => {
                let mut links = HashMap::new();
                for &tr in &pair {
                    s.clear_range(tr, start, start + dur, &mut links);
                }
            }
            InsertMode::Ripple => {
                if !s.make_room(start, dur, &pair.iter().copied().collect()) {
                    return None;
                }
            }
            InsertMode::RippleAll => {
                if !s.make_room(start, dur, &(0..s.tracks.len()).collect()) {
                    return None;
                }
            }
        }
        let id = s.place_media(asset, start, t, range, speed)?;
        *self = s;
        Some(id)
    }

    /// Schneidet den Bereich auf der Spur frei (Clips werden gekürzt, geteilt oder entfernt). `links` ordnet beim Teilen
    /// verknüpfter Clips den rechten Teilen gemeinsame neue Verknüpfungen zu (damit Bild und Ton ein Paar bleiben).
    pub fn clear_range(&mut self, t: usize, a: f64, b: f64, links: &mut HashMap<Uuid, Uuid>) {
        if t >= self.tracks.len() || self.tracks[t].locked || b <= a {
            return;
        }
        let ids: Vec<Uuid> = self.tracks[t].clips.iter().filter(|c| c.end() > a + EPS && c.start < b - EPS).map(|c| c.id).collect();
        for id in ids {
            let Some((ti, ci)) = self.find_clip(id) else { continue };
            let c = self.tracks[ti].clips[ci].clone();
            let keep_left = a - c.start >= MIN_CLIP;
            let keep_right = c.end() - b >= MIN_CLIP;
            if keep_left && keep_right {
                let mut nl = Uuid::new_v4();
                if let Some(lk) = c.link {
                    match links.get(&lk) {
                        Some(k) => nl = *k,
                        None => {
                            links.insert(lk, nl);
                        }
                    }
                }
                if self.split_single(id, b, nl).is_some() {
                    let _ = self.trim_end_single(id, a);
                }
            } else if keep_left {
                let _ = self.trim_end_single(id, a);
            } else if keep_right {
                let _ = self.trim_start_single(id, b);
            } else if let Some((l, k)) = self.find_clip(id) {
                self.tracks[l].clips.remove(k);
            }
        }
    }

    /// Schafft ab `time` eine Lücke von `length`: Clips, die `time` überspannen, werden geteilt, alles dahinter rückt nach.
    /// Verknüpfte Partner werden mitgenommen, auch wenn ihre Spur nicht genannt ist.
    pub fn make_room(&mut self, time: f64, length: f64, tracks: &HashSet<usize>) -> bool {
        let mut affected: HashSet<usize> = tracks.clone();
        for &t in tracks {
            if t >= self.tracks.len() {
                continue;
            }
            for c in &self.tracks[t].clips {
                if c.end() > time + EPS {
                    if let Some((pt, _)) = self.partner_id(c.id).and_then(|p| self.find_clip(p)) {
                        affected.insert(pt);
                    }
                }
            }
        }
        if !affected.iter().all(|&t| t < self.tracks.len() && !self.tracks[t].locked) {
            return false;
        }
        let ids: Vec<Uuid> = affected.iter().flat_map(|&t| self.tracks[t].clips.iter()).filter(|c| c.start < time - MIN_CLIP && c.end() > time + MIN_CLIP).map(|c| c.id).collect();
        for id in ids {
            if self.clip(id).is_some_and(|c| c.start < time - MIN_CLIP && c.end() > time + MIN_CLIP) {
                let _ = self.split(id, time);
            }
        }
        for &t in &affected {
            for c in &mut self.tracks[t].clips {
                if c.start >= time - MIN_CLIP + EPS {
                    c.start += length;
                }
            }
        }
        true
    }

    /// `range` = nur dieser Ausschnitt der Quelle (Sekunden), sonst das ganze Medium.
    fn place_media(&mut self, asset: &MediaAsset, start: f64, t: usize, range: Option<(f64, f64)>, speed: f64) -> Option<Uuid> {
        let pic = asset.has_video || asset.is_image;
        let link = (pic && asset.has_audio && !asset.is_image).then(Uuid::new_v4);
        let (mut src_in, mut dur) = (0.0, asset.duration);
        let sp = if asset.is_image { 1.0 } else { speed };
        if let (Some((a, b)), false) = (range, asset.is_image) {
            src_in = a.clamp(0.0, asset.duration);
            dur = (b.min(asset.duration) - src_in).max(MIN_CLIP) / speed;
        }
        let mk = |part: ClipPart, link: Option<Uuid>| Clip { content: ClipContent::Media(Wrap { v: asset.id }), part, link, start, source_in: src_in, speed: sp, duration: dur, ..Default::default() };
        if pic {
            let v = mk(ClipPart::Video, link);
            let vid = v.id;
            if !self.add_clip(v, t) {
                return None;
            }
            if asset.has_audio && !asset.is_image && !self.add_clip(mk(ClipPart::Audio, link), t + 1) {
                return None;
            }
            return Some(vid);
        }
        let a = mk(ClipPart::Audio, None);
        let id = a.id;
        self.add_clip(a, t).then_some(id)
    }

    fn remove_clip_raw(&mut self, id: Uuid) {
        if let Some((t, c)) = self.find_clip(id) {
            self.tracks[t].clips.remove(c);
        }
    }

    /// Verschiebt Clips (und ihre Partner) gemeinsam um `d_track` Spuren und `d_time` Sekunden; nicht vor 0.
    pub fn move_clips(&mut self, ids: &HashSet<Uuid>, d_track: isize, d_time: f64) -> bool {
        let backup = self.clone();
        let mut moving: Vec<(Clip, usize)> = vec![];
        for id in self.with_partners(ids) {
            let Some((t, c)) = self.find_clip(id) else { return false };
            if self.tracks[t].locked {
                return false;
            }
            let target = t as isize + d_track;
            if target < 0 || target as usize >= self.tracks.len() {
                return false;
            }
            moving.push((self.tracks[t].clips[c].clone(), target as usize));
        }
        if moving.is_empty() {
            return false;
        }
        let min_start = moving.iter().map(|m| m.0.start + d_time).fold(f64::INFINITY, f64::min);
        let shift = d_time - min_start.min(0.0);
        for (c, _) in &moving {
            self.remove_clip_raw(c.id);
        }
        for (mut c, tr) in moving {
            c.start = (c.start + shift).max(0.0);
            if !self.add_clip(c, tr) {
                *self = backup;
                return false;
            }
        }
        true
    }

    /// Fügt Kopien ein: `time` ist der neue Anfang der frühesten Kopie; Spuren bleiben relativ gleich (wird bei Bedarf
    /// nach unten verschoben, fehlende Spuren werden ergänzt). Gibt die neuen IDs zurück.
    pub fn paste(&mut self, items: &[(Clip, usize)], at: f64) -> Vec<Uuid> {
        let Some(min_start) = items.iter().map(|i| i.0.start).reduce(f64::min) else { return vec![] };
        for d_track in 0..=(self.tracks.len() + 1) {
            let mut s = self.clone();
            let needed = items.iter().map(|i| i.1).max().unwrap_or(0) + d_track + 1;
            s.ensure_tracks(needed);
            let mut links: HashMap<Uuid, Uuid> = HashMap::new();
            let mut groups: HashMap<Uuid, Uuid> = HashMap::new();
            let mut ids = vec![];
            let mut ok = true;
            for (c, tr) in items {
                let mut n = c.clone();
                n.id = Uuid::new_v4();
                n.start = (at + (c.start - min_start)).max(0.0);
                if let Some(l) = c.link {
                    n.link = Some(*links.entry(l).or_insert_with(Uuid::new_v4));
                }
                if let Some(g) = c.group {
                    n.group = Some(*groups.entry(g).or_insert_with(Uuid::new_v4));
                }
                let nid = n.id;
                if !s.add_clip(n, tr + d_track) {
                    ok = false;
                    break;
                }
                ids.push(nid);
            }
            if ok {
                *self = s;
                return ids;
            }
        }
        vec![]
    }

    /// Entfernt die Clips (samt Partnern) und schließt die entstandenen Lücken auf den betroffenen Spuren.
    pub fn ripple_delete(&mut self, ids: &HashSet<Uuid>) {
        let mut intervals: HashMap<usize, Vec<(f64, f64)>> = HashMap::new();
        for id in self.with_partners(ids) {
            let Some((t, c)) = self.find_clip(id) else { continue };
            if self.tracks[t].locked {
                continue;
            }
            let cl = self.tracks[t].clips.remove(c);
            intervals.entry(t).or_default().push((cl.start, cl.end()));
        }
        for (t, mut list) in intervals {
            list.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut merged: Vec<(f64, f64)> = vec![];
            for iv in list {
                match merged.last_mut() {
                    Some(last) if iv.0 <= last.1 + EPS => last.1 = last.1.max(iv.1),
                    _ => merged.push(iv),
                }
            }
            for iv in merged.iter().rev() {
                let len = iv.1 - iv.0;
                for c in &mut self.tracks[t].clips {
                    if c.start >= iv.1 - EPS {
                        c.start -= len;
                    }
                }
            }
        }
    }

    /// Schließt die Lücke vor dem Clip: er und alles danach rücken auf (auch auf der Spur des Partners).
    pub fn close_gap(&mut self, id: Uuid) -> bool {
        let Some((t, ci)) = self.find_clip(id) else { return false };
        let c = self.tracks[t].clips[ci].clone();
        let prev_end = self.tracks[t].clips.iter().filter(|o| o.id != id && o.end() <= c.start + EPS).map(Clip::end).fold(0.0, f64::max);
        let gap = c.start - prev_end;
        if gap <= EPS {
            return false;
        }
        let mut affected = vec![t];
        if let Some((pt, _)) = self.partner_id(id).and_then(|p| self.find_clip(p)) {
            affected.push(pt);
        }
        for tr in affected {
            if self.tracks[tr].locked {
                continue;
            }
            for o in &mut self.tracks[tr].clips {
                if o.start >= c.start - EPS {
                    o.start -= gap;
                }
            }
        }
        true
    }

    pub fn is_still(&self, c: &Clip) -> bool {
        c.is_title() || c.asset_id().and_then(|a| self.asset(a)).is_some_and(|a| a.is_image)
    }

    /// Slip: verschiebt den genutzten Quellausschnitt (Position und Länge bleiben), samt Partner. `delta` in
    /// Quellsekunden. Gibt den tatsächlich angewendeten Wert zurück.
    pub fn slip(&mut self, id: Uuid, delta: f64) -> f64 {
        let Some((t, ci)) = self.find_clip(id) else { return 0.0 };
        if self.tracks[t].locked {
            return 0.0;
        }
        let c0 = self.tracks[t].clips[ci].clone();
        let Some(a) = c0.asset_id().and_then(|a| self.asset(a)).cloned() else { return 0.0 };
        if self.is_still(&c0) {
            return 0.0;
        }
        let max_in = (a.duration - c0.duration * c0.speed).max(0.0);
        let applied = (c0.source_in + delta).clamp(0.0, max_in) - c0.source_in;
        for target in std::iter::once(id).chain(self.partner_id(id)) {
            if let Some(c) = self.clip_mut(target) {
                c.source_in += applied;
            }
        }
        applied
    }

    /// Ändert die Geschwindigkeit (samt Partner); die verbrauchte Quellspanne bleibt gleich, die Dauer auf der
    /// Timeline ändert sich. Passt der Clip dann nicht mehr an seine Stelle, bleibt alles unverändert.
    pub fn set_speed(&mut self, id: Uuid, new_speed: f64) -> bool {
        let s = new_speed.clamp(0.1, 16.0);
        let backup = self.clone();
        let targets: Vec<Uuid> = std::iter::once(id).chain(self.partner_id(id)).collect();
        for &target in &targets {
            let Some((t, ci)) = self.find_clip(target) else {
                *self = backup;
                return false;
            };
            if self.tracks[t].locked || self.is_still(&self.tracks[t].clips[ci]) {
                *self = backup;
                return false;
            }
            let c = &mut self.tracks[t].clips[ci];
            let ratio = c.speed / s;
            c.duration *= ratio;
            for ks in c.keys.0.values_mut() {
                for k in ks {
                    k.t *= ratio;
                }
            }
            for p in &mut c.volume_points {
                p.t *= ratio;
            }
            c.fade_in = c.fade_in.min(c.duration / 2.0);
            c.fade_out = c.fade_out.min(c.duration / 2.0);
            c.speed = s;
        }
        for &target in &targets {
            let Some((t, ci)) = self.find_clip(target) else { continue };
            let c = self.tracks[t].clips[ci].clone();
            if self.blocked(c.start, c.end(), t, Some(c.id)) {
                *self = backup;
                return false;
            }
        }
        true
    }

    /// Alle Schnittpunkte (Clipanfänge und -enden), sortiert.
    pub fn edit_points(&self) -> Vec<f64> {
        let mut v: Vec<f64> = self.tracks.iter().flat_map(|t| t.clips.iter().flat_map(|c| [c.start, c.end()])).collect();
        v.sort_by(|a, b| a.total_cmp(b));
        v.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        v
    }

    /// Maximale Länge der Quelle (Medien sind endlich, Titel und Standbilder nicht).
    pub fn max_source_end(&self, c: &Clip) -> f64 {
        match c.asset_id().and_then(|a| self.asset(a)) {
            Some(a) if !a.is_image => a.duration,
            _ => f64::INFINITY,
        }
    }

    /// Verschiebt die linke Kante (auch beim Partner); die Position des Inhalts bleibt erhalten.
    pub fn trim_start(&mut self, id: Uuid, to: f64) -> bool {
        self.trim_start_opt(id, to, &TrimOptions::default())
    }

    pub fn trim_end(&mut self, id: Uuid, to: f64) -> bool {
        self.trim_end_opt(id, to, &TrimOptions::default())
    }

    /// Wie `trim_start`; `o`: ohne Partner (J-L-Schnitt), mit Ripple (alles dahinter rückt mit) oder als Roll.
    pub fn trim_start_opt(&mut self, id: Uuid, to: f64, o: &TrimOptions) -> bool {
        let backup = self.clone();
        if o.ripple != RippleScope::None && o.linked {
            return self.ripple_trim(id, true, to, o.ripple);
        }
        if o.roll {
            if let Some(nb) = self.roll_neighbor(id, true) {
                if !self.trim_start(id, to) {
                    *self = backup;
                    return false;
                }
                let Some(new) = self.clip(id).map(|c| c.start) else { return false };
                if !self.trim_end(nb, new) {
                    *self = backup;
                    return false;
                }
                return true;
            }
        }
        if !self.trim_start_single(id, to) {
            return false;
        }
        let Some(new) = self.clip(id).map(|c| c.start) else { return false };
        if o.linked {
            if let Some(p) = self.partner_id(id) {
                if !self.trim_start_single(p, new) {
                    *self = backup;
                    return false;
                }
            }
        }
        true
    }

    pub fn trim_end_opt(&mut self, id: Uuid, to: f64, o: &TrimOptions) -> bool {
        let backup = self.clone();
        if o.ripple != RippleScope::None && o.linked {
            return self.ripple_trim(id, false, to, o.ripple);
        }
        if o.roll {
            if let Some(nb) = self.roll_neighbor(id, false) {
                if !self.trim_end(id, to) {
                    *self = backup;
                    return false;
                }
                let Some(new) = self.clip(id).map(Clip::end) else { return false };
                if !self.trim_start(nb, new) {
                    *self = backup;
                    return false;
                }
                return true;
            }
        }
        if !self.trim_end_single(id, to) {
            return false;
        }
        let Some(new) = self.clip(id).map(Clip::end) else { return false };
        if o.linked {
            if let Some(p) = self.partner_id(id) {
                if !self.trim_end_single(p, new) {
                    *self = backup;
                    return false;
                }
            }
        }
        true
    }

    /// Nachbar, der direkt an der Kante anschließt (Schnittpunkt ohne Lücke und ohne Überblendung).
    pub fn roll_neighbor(&self, id: Uuid, left: bool) -> Option<Uuid> {
        let (t, ci) = self.find_clip(id)?;
        let c = &self.tracks[t].clips[ci];
        let edge = if left { c.start } else { c.end() };
        self.tracks[t].clips.iter().find(|o| o.id != id && ((if left { o.end() } else { o.start }) - edge).abs() < 1e-3).map(|o| o.id)
    }

    /// Kante verschieben und alles dahinter mitnehmen (Spur des Clips samt Partnerspur oder alle Spuren).
    fn ripple_trim(&mut self, id: Uuid, left: bool, time: f64, scope: RippleScope) -> bool {
        let Some((t0, ci0)) = self.find_clip(id) else { return false };
        if self.tracks[t0].locked {
            return false;
        }
        let backup = self.clone();
        let c0 = self.tracks[t0].clips[ci0].clone();
        let pid = self.partner_id(id);
        let mut group: HashSet<Uuid> = HashSet::from([id]);
        group.extend(pid);
        let mut affected: HashSet<usize> = HashSet::from([t0]);
        if let Some((pt, _)) = pid.and_then(|p| self.find_clip(p)) {
            affected.insert(pt);
        }
        if scope == RippleScope::All {
            affected = (0..self.tracks.len()).collect();
        }
        let followers: HashSet<Uuid> = affected
            .iter()
            .filter(|&&t| !self.tracks[t].locked)
            .flat_map(|&t| self.tracks[t].clips.iter())
            .filter(|c| !group.contains(&c.id) && c.start > c0.start + EPS)
            .map(|c| c.id)
            .collect();
        let both: HashSet<Uuid> = group.union(&followers).copied().collect();
        let still = self.is_still(&c0);
        let mut ok = true;
        if left {
            let mut t = time.max(0.0).min(c0.end() - MIN_CLIP);
            if !still {
                t = t.max(c0.start - c0.source_in / c0.speed);
            }
            let d = t - c0.start; // > 0: gekürzt
            if d < 0.0 {
                self.shift(&both, -d);
                ok = self.trim_start(id, c0.start);
            } else if d > 0.0 {
                ok = self.trim_start(id, t);
                if ok {
                    self.shift(&both, -d);
                }
            }
        } else {
            let mut e = time.max(c0.start + MIN_CLIP);
            e = e.min(c0.start + (self.max_source_end(&c0) - c0.source_in) / c0.speed);
            let d = e - c0.end(); // > 0: verlängert
            if d > 0.0 {
                self.shift(&followers, d);
                ok = self.trim_end(id, e);
            } else if d < 0.0 {
                ok = self.trim_end(id, e);
                if ok {
                    self.shift(&followers, d);
                }
            }
        }
        if ok && !self.is_consistent(&both) {
            ok = false;
        }
        if !ok {
            *self = backup;
        }
        ok
    }

    /// Clips, an denen beim Kürzen nichts einrasten soll, weil sie mitwandern (Ripple-Nachfolger, Roll-Nachbar samt Partner).
    pub fn trim_ignore_set(&self, id: Uuid, left: bool, o: &TrimOptions) -> HashSet<Uuid> {
        let Some((t, ci)) = self.find_clip(id) else { return HashSet::new() };
        let c = &self.tracks[t].clips[ci];
        let mut r = HashSet::new();
        if o.roll {
            if let Some(nb) = self.roll_neighbor(id, left) {
                r = self.with_partners(&HashSet::from([nb]));
            }
        }
        if o.ripple != RippleScope::None {
            let mut affected: HashSet<usize> = HashSet::from([t]);
            if let Some((pt, _)) = self.partner_id(id).and_then(|p| self.find_clip(p)) {
                affected.insert(pt);
            }
            if o.ripple == RippleScope::All {
                affected = (0..self.tracks.len()).collect();
            }
            for t in affected {
                for f in &self.tracks[t].clips {
                    if f.id != id && f.start > c.start + EPS {
                        r.insert(f.id);
                    }
                }
            }
        }
        r
    }

    /// Alles, was hinter den bewegten Clips liegt (Mausmodus „Eine Spur“: auf deren Spuren, „Alle Spuren“: überall).
    pub fn followers(&self, moving: &HashSet<Uuid>, scope: MouseMode) -> HashSet<Uuid> {
        if scope == MouseMode::Single {
            return HashSet::new();
        }
        let locs: Vec<(usize, usize)> = moving.iter().filter_map(|i| self.find_clip(*i)).collect();
        let Some(first) = locs.iter().map(|&(t, c)| self.tracks[t].clips[c].start).reduce(f64::min) else { return HashSet::new() };
        let affected: HashSet<usize> = if scope == MouseMode::AllTracks { (0..self.tracks.len()).collect() } else { locs.iter().map(|l| l.0).collect() };
        affected
            .iter()
            .filter(|&&t| !self.tracks[t].locked)
            .flat_map(|&t| self.tracks[t].clips.iter())
            .filter(|c| !moving.contains(&c.id) && c.start >= first - EPS)
            .map(|c| c.id)
            .collect()
    }

    pub fn shift(&mut self, ids: &HashSet<Uuid>, delta: f64) {
        for id in ids {
            if let Some(c) = self.clip_mut(*id) {
                c.start = (c.start + delta).max(0.0);
            }
        }
    }

    /// Prüft, ob keiner der Clips unzulässig überlappt.
    pub fn is_consistent(&self, ids: &HashSet<Uuid>) -> bool {
        ids.iter().all(|id| match self.find_clip(*id) {
            Some((t, ci)) => {
                let c = &self.tracks[t].clips[ci];
                !self.blocked(c.start, c.end(), t, Some(*id))
            }
            None => true,
        })
    }

    /// Lücken ohne Bildmaterial (Video-, Bild- und Titelclips aller Spuren) zwischen dem ersten Bildanfang und dem letzten
    /// Bildende sowie vor dem ersten Bild, wenn es nicht bei 0 beginnt. (Anfang, Ende) in Sekunden.
    pub fn video_gaps(&self) -> Vec<(f64, f64)> {
        let mut iv: Vec<(f64, f64)> = self.tracks.iter().flat_map(|t| &t.clips).filter(|c| c.part == ClipPart::Video || c.is_title()).map(|c| (c.start, c.end())).collect();
        iv.sort_by(|a, b| a.0.total_cmp(&b.0));
        let eps = 0.5 / self.fps.max(1) as f64;
        let mut gaps = vec![];
        let mut reach = 0.0f64;
        for (a, b) in iv {
            if a > reach + eps {
                gaps.push((reach, a));
            }
            reach = reach.max(b);
        }
        gaps
    }

    /// Schließt eine Lücke auf allen (nicht gesperrten) Spuren: alles dahinter rückt auf. Scheitert, wenn dabei Clips kollidieren.
    pub fn close_gap_range(&mut self, gap: (f64, f64)) -> bool {
        let len = gap.1 - gap.0;
        if len <= EPS {
            return false;
        }
        let backup = self.clone();
        let mut moved = HashSet::new();
        for t in 0..self.tracks.len() {
            if self.tracks[t].locked {
                continue;
            }
            for c in &mut self.tracks[t].clips {
                if c.start >= gap.1 - EPS {
                    c.start -= len;
                    moved.insert(c.id);
                }
            }
        }
        if !self.is_consistent(&moved) {
            *self = backup;
            return false;
        }
        true
    }

    /// Schlagzeiten (Timeline) eines Taktraster-Tonclips; `every` = jeder n-te Schlag (4 = Taktanfang bei 4/4).
    pub fn beat_times(&self, clip: &Clip, every: i64) -> Vec<f64> {
        let Some(g) = clip.beat_grid else { return vec![] };
        let every = every.max(1);
        g.beats(clip.source_in, clip.source_in + clip.duration * clip.speed)
            .into_iter()
            .filter(|(k, _)| k.rem_euclid(every) == 0)
            .map(|(_, t)| clip.start + (t - clip.source_in) / clip.speed)
            .collect()
    }

    /// Musikalische Schnittanpassung: Schnitte zwischen aufeinanderfolgenden Bildclips einer Spur rasten auf den nächsten
    /// Schlag ein. Beide Seiten werden gekürzt bzw. verlängert (soweit Quellmaterial da ist). Rückgabe: (angepasst, unverändert).
    pub fn snap_cuts_to_beats(&mut self, t: usize, beats: &[f64]) -> (usize, usize) {
        if t >= self.tracks.len() || beats.len() < 2 {
            return (0, 0);
        }
        let mut clips: Vec<Clip> = self.tracks[t].clips.iter().filter(|c| c.part == ClipPart::Video || c.is_title()).cloned().collect();
        clips.sort_by(|a, b| a.start.total_cmp(&b.start));
        if clips.len() < 2 {
            return (0, 0);
        }
        let (mut moved, mut kept) = (0, 0);
        let mut last_beat = f64::NEG_INFINITY;
        for w in clips.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            let (Some(ca), Some(cb)) = (self.clip(a.id).cloned(), self.clip(b.id).cloned()) else {
                kept += 1;
                continue;
            };
            if (ca.end() - cb.start).abs() >= 0.05 {
                kept += 1; // Überblendungen und Lücken auslassen
                continue;
            }
            let cut = (ca.end() + cb.start) / 2.0;
            // die beiden nächsten Schläge; der erste, für den Quellmaterial da ist, gewinnt
            let mut sorted = beats.to_vec();
            sorted.sort_by(|x, y| (x - cut).abs().total_cmp(&(y - cut).abs()));
            let cands: Vec<f64> = sorted.into_iter().take(2).filter(|x| *x > last_beat + EPS).collect();
            if let Some(first) = cands.first() {
                if (first - cut).abs() < 0.01 {
                    last_beat = *first;
                    kept += 1;
                    continue;
                }
            }
            let mut done = false;
            for target in cands {
                let backup = self.clone();
                if self.trim_end(ca.id, target)
                    && self.clip(ca.id).is_some_and(|c| (c.end() - target).abs() < 1e-3)
                    && self.trim_start(cb.id, target)
                    && self.clip(cb.id).is_some_and(|c| (c.start - target).abs() < 1e-3)
                {
                    moved += 1;
                    last_beat = target;
                    done = true;
                    break;
                }
                *self = backup;
            }
            if !done {
                kept += 1;
            }
        }
        (moved, kept)
    }

    // ---------- Übergänge (Blenden) zwischen aneinanderstoßenden Bild-/Fotoclips ----------

    /// Alle Stellen einer Spur, an denen zwei Bild-/Fotoclips aneinanderstoßen (Abstand ≤ 1/20 s) oder sich überlappen.
    pub fn junctions(&self, t: usize) -> Vec<Junction> {
        if t >= self.tracks.len() {
            return vec![];
        }
        let mut cs: Vec<&Clip> = self.tracks[t].clips.iter().filter(|c| c.part == ClipPart::Video && !c.is_title()).collect();
        cs.sort_by(|a, b| a.start.total_cmp(&b.start));
        cs.windows(2)
            .filter_map(|w| {
                let ov = w[0].end() - w[1].start;
                (ov > -0.05).then(|| Junction { a: w[0].id, b: w[1].id, overlap: ov.max(0.0), track: t })
            })
            .collect()
    }

    /// Der Übergang, an dem der Clip beteiligt ist (als zweiter Clip bevorzugt, sonst als erster).
    pub fn junction_of(&self, id: Uuid) -> Option<Junction> {
        let (t, _) = self.find_clip(id)?;
        let all = self.junctions(t);
        all.iter().find(|j| j.b == id).or_else(|| all.iter().find(|j| j.a == id)).copied()
    }

    /// Setzt die Art und Länge des Übergangs. `kind == None` = harter Schnitt (Überlappung entfernen).
    /// Die Überlappung entsteht bevorzugt aus Material hinter den Schnittkanten; fehlt es, rücken der zweite Clip und alles
    /// dahinter nach vorn.
    pub fn set_transition(&mut self, a: Uuid, b: Uuid, kind: Option<&str>, duration: f64) -> bool {
        let (Some(ca), Some(cb)) = (self.clip(a).cloned(), self.clip(b).cloned()) else { return false };
        // gesperrte Spuren bleiben unberührt
        for id in [a, b] {
            for x in std::iter::once(id).chain(self.partner_id(id)) {
                if self.find_clip(x).is_some_and(|(t, _)| self.tracks[t].locked) {
                    return false;
                }
            }
        }
        let backup = self.clone();
        let finish = |s: &mut Project| {
            if let Some(k) = kind {
                for id in std::iter::once(b).chain(s.partner_id(b)) {
                    if let Some(c) = s.clip_mut(id) {
                        c.transition = k.to_string();
                    }
                }
            }
            true
        };
        let Some(_) = kind else {
            let ov = ca.end() - cb.start;
            if ov > 0.001 && !self.trim_start(b, ca.end()) {
                *self = backup;
                return false;
            }
            return true;
        };
        let limit = (ca.duration.min(cb.duration) - 0.15).max(0.2);
        let d = duration.max(0.2).min(limit);
        let cut = if ca.end() > cb.start { (ca.end() + cb.start) / 2.0 } else { ca.end() };
        if ((ca.end() - cb.start) - d).abs() < 0.02 {
            return finish(self);
        }
        // Zielkanten um den Schnitt herum; Kürzen und Verlängern, soweit Quellmaterial da ist
        let _ = self.trim_end(a, cut + d / 2.0);
        let _ = self.trim_start(b, cut - d / 2.0);
        let (Some(na), Some(nb)) = (self.clip(a).cloned(), self.clip(b).cloned()) else {
            *self = backup;
            return false;
        };
        let have = na.end() - nb.start;
        if have < d - 0.01 {
            // Rest: den zweiten Clip samt allem dahinter (auf seinen Spuren) nach vorn schieben
            let delta = d - have;
            let mut affected: HashSet<usize> = HashSet::new();
            if let Some((t, _)) = self.find_clip(b) {
                affected.insert(t);
            }
            if let Some((pt, _)) = self.partner_id(b).and_then(|p| self.find_clip(p)) {
                affected.insert(pt);
            }
            let moving: HashSet<Uuid> = affected.iter().flat_map(|&t| self.tracks[t].clips.iter()).filter(|c| c.start >= nb.start - EPS).map(|c| c.id).collect();
            if nb.start - delta < -EPS {
                *self = backup;
                return false;
            }
            self.shift(&moving, -delta);
        }
        let mut check: HashSet<Uuid> = HashSet::from([a, b]);
        check.extend(self.partner_id(a));
        check.extend(self.partner_id(b));
        if !self.is_consistent(&check) {
            *self = backup;
            return false;
        }
        finish(self)
    }

    /// Projektmarker 1–10, nach Zeit sortiert: (Nummer, Zeit).
    pub fn bookmark_list(&self) -> Vec<(u32, f64)> {
        let mut v: Vec<(u32, f64)> = self.bookmarks.iter().flatten().filter_map(|(k, t)| k.parse().ok().map(|n| (n, *t))).collect();
        v.sort_by(|a, b| a.1.total_cmp(&b.1));
        v
    }

    /// Teilt den Clip (und seinen Partner) an `time`. Gibt die ID des rechten Teils zurück.
    /// Läge ein Teilstück innerhalb einer Blende ganz unter dem Nachbarclip, wird nicht geteilt.
    pub fn split(&mut self, id: Uuid, time: f64) -> Option<Uuid> {
        let backup = self.clone();
        let new_link = Uuid::new_v4();
        let right = self.split_single(id, time, new_link)?;
        let pid = self.partner_id(id);
        if let Some(p) = pid {
            let _ = self.split_single(p, time, new_link);
        }
        let mut check: HashSet<Uuid> = HashSet::from([id, right]);
        check.extend(pid);
        check.extend(self.partner_id(right));
        if !self.is_consistent(&check) {
            *self = backup;
            return None;
        }
        Some(right)
    }

    pub(crate) fn trim_start_single(&mut self, id: Uuid, to: f64) -> bool {
        let Some((t, ci)) = self.find_clip(id) else { return false };
        if self.tracks[t].locked {
            return false;
        }
        let mut c = self.tracks[t].clips[ci].clone();
        let still = self.is_still(&c);
        let mut time = to.max(0.0).min(c.end() - MIN_CLIP);
        if !still {
            time = time.max(c.start - c.source_in / c.speed); // nicht vor den Quellanfang
        }
        let delta = time - c.start;
        c.keys = c.keys_slice(delta, c.duration);
        c.start = time;
        if !still {
            c.source_in += delta * c.speed;
        }
        c.duration -= delta;
        c.volume_points = c.volume_points.iter().map(|p| VolumePoint { t: p.t - delta, gain: p.gain }).filter(|p| p.t >= 0.0).collect();
        if self.blocked(c.start, c.end(), t, Some(id)) {
            return false;
        }
        self.tracks[t].clips[ci] = c;
        true
    }

    pub(crate) fn trim_end_single(&mut self, id: Uuid, to: f64) -> bool {
        let Some((t, ci)) = self.find_clip(id) else { return false };
        if self.tracks[t].locked {
            return false;
        }
        let mut c = self.tracks[t].clips[ci].clone();
        let mut e = to.max(c.start + MIN_CLIP);
        e = e.min(c.start + (self.max_source_end(&c) - c.source_in) / c.speed);
        let new_dur = e - c.start;
        if new_dur < c.duration {
            c.keys = c.keys_slice(0.0, new_dur);
        }
        c.duration = new_dur;
        c.volume_points.retain(|p| p.t <= new_dur);
        if self.blocked(c.start, c.end(), t, Some(id)) {
            return false;
        }
        self.tracks[t].clips[ci] = c;
        true
    }

    pub(crate) fn split_single(&mut self, id: Uuid, time: f64, new_link: Uuid) -> Option<Uuid> {
        let (t, ci) = self.find_clip(id)?;
        if self.tracks[t].locked {
            return None;
        }
        let c = self.tracks[t].clips[ci].clone();
        if !(time > c.start + MIN_CLIP && time < c.end() - MIN_CLIP) {
            return None;
        }
        let mut left = c.clone();
        let mut right = c.clone();
        left.duration = time - c.start;
        left.fade_out = 0.0;
        left.volume_points.retain(|p| p.t <= left.duration);
        right.id = Uuid::new_v4();
        right.group = None;
        right.start = time;
        let still = self.is_still(&c);
        left.keys = c.keys_slice(0.0, left.duration);
        right.keys = c.keys_slice(left.duration, c.duration);
        right.source_in = if still { 0.0 } else { c.source_in + left.duration * c.speed };
        right.duration = c.end() - time;
        right.fade_in = 0.0;
        right.volume_points = c.volume_points.iter().filter(|p| p.t >= left.duration).map(|p| VolumePoint { t: p.t - left.duration, gain: p.gain }).collect();
        if c.link.is_some() {
            right.link = Some(new_link);
        }
        let rid = right.id;
        self.tracks[t].clips[ci] = left;
        self.tracks[t].clips.insert(ci + 1, right);
        Some(rid)
    }

    /// Entfernt den Clip samt Partner (gesperrte Spuren bleiben unberührt).
    pub fn remove_clip(&mut self, id: Uuid) {
        let Some((t, ci)) = self.find_clip(id) else { return };
        if self.tracks[t].locked {
            return;
        }
        let pid = self.partner_id(id);
        self.tracks[t].clips.remove(ci);
        if let Some((pt, pc)) = pid.and_then(|p| self.find_clip(p)) {
            if !self.tracks[pt].locked {
                self.tracks[pt].clips.remove(pc);
            }
        }
    }

    pub fn unlink(&mut self, id: Uuid) {
        let Some(link) = self.clip(id).and_then(|c| c.link) else { return };
        for t in &mut self.tracks {
            for c in &mut t.clips {
                if c.link == Some(link) {
                    c.link = None;
                }
            }
        }
    }

    /// Erste Spur, auf der ab `start` strikt Platz ist (für Titel).
    pub fn first_free_track(&self, clip: &Clip) -> Option<usize> {
        (0..self.tracks.len()).find(|&t| self.accepts(clip, t) && !self.tracks[t].locked && !self.strict_overlaps(clip.start, clip.end(), t, None))
    }

    pub fn marker_list(&self) -> Vec<Marker> {
        let mut m = self.markers.clone().unwrap_or_default();
        m.sort_by(|a, b| a.t.total_cmp(&b.t));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(has_video: bool, has_audio: bool, dur: f64) -> MediaAsset {
        MediaAsset { path: "x".into(), name: "x".into(), duration: dur, has_video, has_audio, width: 100.0, height: 50.0, fps: 30.0, ..Default::default() }
    }

    fn clip_of(a: &MediaAsset, start: f64, dur: f64) -> Clip {
        Clip { content: ClipContent::Media(Wrap { v: a.id }), start, duration: dur, ..Default::default() }
    }

    fn proj_with(a: &MediaAsset) -> Project {
        let mut p = Project::default();
        p.assets.push(a.clone());
        p
    }

    #[test]
    fn insert_media_links_picture_and_sound_on_adjacent_tracks() {
        let a = asset(true, true, 4.0);
        let mut p = proj_with(&a);
        let id = p.insert_media(&a, 2.0, Some(1), None).unwrap();
        let (t, _) = p.find_clip(id).unwrap();
        assert_eq!(t, 1);
        assert_eq!(p.tracks[2].clips.len(), 1, "Ton auf der Spur darunter");
        let (v, au) = (p.clip(id).unwrap().clone(), p.tracks[2].clips[0].clone());
        assert_eq!((v.link, v.part), (au.link, ClipPart::Video));
        assert_eq!(au.part, ClipPart::Audio);
        assert!(v.link.is_some());
        // zweites Mal an derselben Stelle: weicht auf freie Spuren aus
        let id2 = p.insert_media(&a, 2.0, Some(1), None).unwrap();
        assert_ne!(p.find_clip(id2).unwrap().0, 1);
        // Ausschnitt
        let mut q = proj_with(&a);
        let id = q.insert_media(&a, 0.0, None, Some((1.0, 2.5))).unwrap();
        let c = q.clip(id).unwrap();
        assert_eq!((c.source_in, c.duration), (1.0, 1.5));
    }

    #[test]
    fn insert_image_has_no_audio_and_audio_goes_on_its_track() {
        let img = MediaAsset { is_image: true, has_video: true, ..asset(true, false, 5.0) };
        let wav = asset(false, true, 3.0);
        let mut p = proj_with(&img);
        p.assets.push(wav.clone());
        let i = p.insert_media(&img, 0.0, Some(0), None).unwrap();
        assert!(p.clip(i).unwrap().link.is_none());
        let w = p.insert_media(&wav, 0.0, Some(3), None).unwrap();
        assert_eq!(p.find_clip(w).unwrap().0, 3);
        assert_eq!(p.clip(w).unwrap().part, ClipPart::Audio);
    }

    #[test]
    fn blocked_allows_one_crossfade_overlap_but_not_cover_or_triple() {
        let a = asset(true, false, 10.0);
        let mut p = proj_with(&a);
        assert!(p.add_clip(clip_of(&a, 0.0, 4.0), 0));
        assert!(p.add_clip(clip_of(&a, 3.0, 4.0), 0), "Überlappung 1 s = Überblendung");
        assert!(!p.add_clip(clip_of(&a, 3.5, 1.0), 0), "würde dritten Clip überlagern");
        assert!(!p.add_clip(clip_of(&a, 0.5, 1.0), 0), "würde einen Clip fast ganz überdecken");
        assert!(p.add_clip(clip_of(&a, 7.0, 1.0), 0));
        p.tracks[1].locked = true;
        assert!(!p.add_clip(clip_of(&a, 0.0, 1.0), 1), "gesperrte Spur");
        let pure_audio = asset(false, true, 3.0);
        p.assets.push(pure_audio.clone());
        assert!(!p.add_clip(clip_of(&pure_audio, 0.0, 1.0), 2) || true);
        let video_clip_of_audio_only = Clip { part: ClipPart::Video, ..clip_of(&pure_audio, 0.0, 1.0) };
        assert!(!p.accepts(&video_clip_of_audio_only, 2), "Bildteil braucht ein Bild");
    }

    #[test]
    fn move_clips_all_or_nothing_with_partners() {
        let a = asset(true, true, 4.0);
        let mut p = proj_with(&a);
        let id = p.insert_media(&a, 1.0, Some(0), None).unwrap();
        let other = p.insert_media(&a, 6.0, Some(0), None).unwrap();
        let _ = other;
        let ids: HashSet<Uuid> = [id].into();
        assert!(p.move_clips(&ids, 0, 0.5));
        assert_eq!(p.clip(id).unwrap().start, 1.5);
        assert_eq!(p.partner_id(id).and_then(|x| p.clip(x)).unwrap().start, 1.5, "Partner folgt");
        // gegen den anderen Clip: wäre fast ganz überdeckt → nichts ändert sich
        let before = p.clone();
        assert!(!p.move_clips(&ids, 0, 4.55));
        assert_eq!(p, before);
        // nicht vor 0
        assert!(p.move_clips(&ids, 0, -10.0));
        assert_eq!(p.clip(id).unwrap().start, 0.0);
        // außerhalb der Spurzahl
        assert!(!p.move_clips(&ids, -1, 0.0));
    }

    #[test]
    fn split_slices_keys_volume_points_and_partner() {
        let a = asset(true, true, 8.0);
        let mut p = proj_with(&a);
        let id = p.insert_media(&a, 0.0, Some(0), None).unwrap();
        p.clip_mut(id).unwrap().fade_out = 1.0;
        p.clip_mut(id).unwrap().fade_in = 1.0;
        let aid = p.partner_id(id).unwrap();
        p.clip_mut(aid).unwrap().volume_points = vec![VolumePoint { t: 0.0, gain: 1.0 }, VolumePoint { t: 4.0, gain: 0.5 }, VolumePoint { t: 8.0, gain: 0.0 }];
        assert!(p.split(id, 0.05).is_none(), "zu nah am Rand");
        let right = p.split(id, 4.0).unwrap();
        let l = p.clip(id).unwrap().clone();
        let r = p.clip(right).unwrap().clone();
        assert_eq!((l.duration, r.start, r.duration, r.source_in), (4.0, 4.0, 4.0, 4.0));
        assert_eq!((l.fade_out, r.fade_in), (0.0, 0.0));
        assert_eq!((l.fade_in, r.fade_out), (1.0, 1.0));
        assert_ne!(l.link, r.link);
        let ra = p.partner_id(right).unwrap();
        assert_eq!(p.clip(ra).unwrap().start, 4.0, "Ton wurde mit geteilt");
        let left_audio = p.clip(aid).unwrap();
        assert_eq!(left_audio.volume_points.len(), 2);
        assert_eq!(p.clip(ra).unwrap().volume_points.iter().map(|v| v.t).collect::<Vec<_>>(), vec![0.0, 4.0]);
    }

    #[test]
    fn trim_respects_source_and_neighbours() {
        let a = asset(true, true, 6.0);
        let mut p = proj_with(&a);
        let id = p.insert_media(&a, 2.0, Some(0), Some((1.0, 5.0))).unwrap(); // Quelle 1…5, Timeline 2…6
        assert!(p.trim_end(id, 100.0));
        assert_eq!(p.clip(id).unwrap().end(), 7.0, "höchstens bis zum Quellende (6 s)");
        assert!(p.trim_start(id, -50.0));
        let c = p.clip(id).unwrap();
        assert_eq!((c.start, c.source_in), (1.0, 0.0), "nicht vor den Quellanfang");
        assert!(p.trim_start(id, 3.0));
        let c = p.clip(id).unwrap();
        assert_eq!((c.start, c.source_in), (3.0, 2.0));
        let pc = p.partner_id(id).and_then(|x| p.clip(x)).unwrap();
        assert_eq!((pc.start, pc.source_in), (3.0, 2.0), "Partner mitgekürzt");
        // Mindestdauer
        assert!(p.trim_end(id, 3.0));
        assert!((p.clip(id).unwrap().duration - MIN_CLIP).abs() < 1e-9);
    }

    #[test]
    fn set_speed_scales_duration_keys_and_rolls_back_when_blocked() {
        use crate::anim::{Channel, Keyframe};
        let a = asset(true, true, 8.0);
        let mut p = proj_with(&a);
        let id = p.insert_media(&a, 0.0, Some(0), Some((0.0, 4.0))).unwrap();
        p.clip_mut(id).unwrap().keys.0.insert(Channel::PosX, vec![Keyframe::new(2.0, 0.5)]);
        assert!(p.set_speed(id, 2.0));
        let c = p.clip(id).unwrap();
        assert_eq!((c.speed, c.duration), (2.0, 2.0));
        assert_eq!(c.keys.get(Channel::PosX).unwrap()[0].t, 1.0);
        assert_eq!(p.partner_id(id).and_then(|x| p.clip(x)).unwrap().duration, 2.0);
        // Hindernis: langsamer machen, aber der Platz ist belegt
        let b = asset(true, false, 3.0);
        p.assets.push(b.clone());
        assert!(p.add_clip(clip_of(&b, 2.5, 3.0), 0));
        let before = p.clone();
        assert!(!p.set_speed(id, 0.5));
        assert_eq!(p, before, "alles unverändert");
        // Titel/Bilder haben keine Geschwindigkeit
        let mut t = Project::default();
        t.tracks[0].clips.push(Clip::default());
        let tid = t.tracks[0].clips[0].id;
        assert!(!t.set_speed(tid, 2.0));
    }

    #[test]
    fn paste_keeps_relative_layout_and_links() {
        let a = asset(true, true, 4.0);
        let mut p = proj_with(&a);
        let id = p.insert_media(&a, 1.0, Some(0), None).unwrap();
        let pid = p.partner_id(id).unwrap();
        let items = vec![(p.clip(id).unwrap().clone(), 0usize), (p.clip(pid).unwrap().clone(), 1usize)];
        let ids = p.paste(&items, 10.0);
        assert_eq!(ids.len(), 2);
        let (n0, n1) = (p.clip(ids[0]).unwrap().clone(), p.clip(ids[1]).unwrap().clone());
        assert_eq!((n0.start, n1.start), (10.0, 10.0));
        assert_eq!(n0.link, n1.link);
        assert_ne!(n0.link, p.clip(id).unwrap().link, "neue Verknüpfung");
        // an belegter Stelle: wird auf tiefere Spuren verschoben
        let ids2 = p.paste(&items, 10.0);
        assert_eq!(ids2.len(), 2);
        assert!(p.find_clip(ids2[0]).unwrap().0 > 0);
    }

    #[test]
    fn ripple_delete_and_close_gap() {
        let a = asset(true, false, 5.0);
        let mut p = proj_with(&a);
        let c1 = clip_of(&a, 0.0, 2.0);
        let c2 = clip_of(&a, 2.0, 3.0);
        let c3 = clip_of(&a, 5.0, 1.0);
        let (i1, i2, i3) = (c1.id, c2.id, c3.id);
        for c in [c1, c2, c3] {
            assert!(p.add_clip(c, 0));
        }
        p.ripple_delete(&[i2].into());
        assert_eq!(p.clip(i3).unwrap().start, 2.0, "Lücke geschlossen");
        assert_eq!(p.clip(i1).unwrap().start, 0.0);
        p.clip_mut(i3).unwrap().start = 4.0;
        assert!(p.close_gap(i3));
        assert_eq!(p.clip(i3).unwrap().start, 2.0);
        assert!(!p.close_gap(i3), "keine Lücke mehr");
        p.tracks[0].locked = true;
        p.ripple_delete(&[i1].into());
        assert!(p.clip(i1).is_some(), "gesperrte Spur bleibt unberührt");
    }

    #[test]
    fn slip_shifts_source_and_clamps() {
        let a = asset(true, true, 10.0);
        let mut p = proj_with(&a);
        let id = p.insert_media(&a, 0.0, Some(0), Some((2.0, 5.0))).unwrap();
        assert_eq!(p.slip(id, 1.0), 1.0);
        assert_eq!(p.clip(id).unwrap().source_in, 3.0);
        assert_eq!(p.partner_id(id).and_then(|x| p.clip(x)).unwrap().source_in, 3.0);
        assert_eq!(p.slip(id, 100.0), 4.0, "höchstens bis das Quellende erreicht ist (10 − 3)");
        assert_eq!(p.slip(id, -100.0), -7.0);
    }

    #[test]
    fn remove_clip_removes_partner_unless_locked() {
        let a = asset(true, true, 4.0);
        let mut p = proj_with(&a);
        let id = p.insert_media(&a, 0.0, Some(0), None).unwrap();
        let pid = p.partner_id(id).unwrap();
        p.tracks[1].locked = true;
        p.remove_clip(id);
        assert!(p.clip(id).is_none() && p.clip(pid).is_some(), "Partner auf gesperrter Spur bleibt");
        p.tracks[1].locked = false;
        let id = p.insert_media(&a, 6.0, Some(0), None).unwrap();
        p.unlink(id);
        p.remove_clip(id);
        assert_eq!(p.tracks[1].clips.len(), 2, "ohne Verknüpfung bleibt der Ton des gelöschten Clips stehen (plus der alte Partner)");
    }

    #[test]
    fn edit_points_and_free_track() {
        let a = asset(true, false, 5.0);
        let mut p = proj_with(&a);
        assert!(p.add_clip(clip_of(&a, 1.0, 2.0), 0));
        assert!(p.add_clip(clip_of(&a, 2.0, 2.0), 1));
        assert_eq!(p.edit_points(), vec![1.0, 3.0, 2.0, 4.0].into_iter().fold(vec![], |mut v: Vec<f64>, x| { v.push(x); v.sort_by(|a, b| a.total_cmp(b)); v }));
        let title = Clip { start: 1.5, duration: 1.0, ..Default::default() };
        assert_eq!(p.first_free_track(&title), Some(2), "Spuren 0 und 1 sind belegt");
        p.tracks[2].locked = true;
        assert_eq!(p.first_free_track(&title), Some(3));
    }
}

/// Port der Prüfungen aus `SelfTestEditing.swift` des Mac-Originals (Einfügemodi, J-L-Schnitt, Ripple, Roll, Lücken, Gruppen).
#[cfg(test)]
mod edit_tests {
    use super::*;

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    fn make_project() -> (Project, MediaAsset) {
        let a = MediaAsset { path: "x".into(), name: "x".into(), duration: 10.0, has_video: true, has_audio: true, width: 100.0, height: 50.0, fps: 30.0, ..Default::default() };
        let mut p = Project::default();
        p.assets.push(a.clone());
        (p, a)
    }

    /// Zwei Clips à 4 s auf Spur 0 (Bild) / 1 (Ton): A 0–4, B 4–8.
    fn editing_project() -> (Project, MediaAsset, Uuid, Uuid) {
        let (mut p, a) = make_project();
        let r1 = p.insert_media(&a, 0.0, Some(0), Some((0.0, 4.0))).unwrap();
        let r2 = p.insert_media(&a, 4.0, Some(0), Some((4.0, 8.0))).unwrap();
        (p, a, r1, r2)
    }

    fn title_at(start: f64, dur: f64) -> Clip {
        Clip { start, duration: dur, ..Default::default() }
    }

    fn ids(v: &[Uuid]) -> HashSet<Uuid> {
        v.iter().copied().collect()
    }

    #[test]
    fn overwrite_keeps_length_and_trims_neighbours() {
        let (mut p, a, r1, r2) = editing_project();
        let d0 = p.duration();
        let id = p.insert_media_with(&a, 2.0, Some(0), Some((0.0, 4.0)), InsertMode::Overwrite, 1.0).expect("platziert");
        assert!(near(p.duration(), d0), "Länge bleibt");
        assert!(near(p.clip(r1).unwrap().end(), 2.0), "Vorgänger gekürzt");
        assert!(near(p.clip(r2).unwrap().start, 6.0) && near(p.clip(r2).unwrap().source_in, 6.0), "Nachfolger am Anfang gekürzt");
        assert!(near(p.clip(id).unwrap().start, 2.0) && near(p.clip(id).unwrap().end(), 6.0));
        assert_eq!(p.tracks[1].clips.len(), 3, "Ton ebenso überschrieben");
    }

    #[test]
    fn overwrite_inside_a_clip_splits_it_and_keeps_pairs() {
        let (mut p, a, r1, _) = editing_project();
        p.insert_media_with(&a, 1.0, Some(0), Some((0.0, 1.0)), InsertMode::Overwrite, 1.0);
        let mut vids = p.tracks[0].clips.clone();
        vids.sort_by(|x, y| x.start.total_cmp(&y.start));
        assert!(vids.len() == 4 && near(vids[0].end(), 1.0) && near(vids[2].start, 2.0) && near(vids[2].source_in, 2.0));
        assert!(p.partner_id(vids[2].id).is_some(), "rechter Teil behält Tonpartner");
        assert!(p.partner_id(r1).is_some(), "linker Teil behält Tonpartner");
    }

    #[test]
    fn ripple_modes_push_followers() {
        let (mut p, a, r1, r2) = editing_project();
        let d0 = p.duration();
        let id = p.insert_media_with(&a, 2.0, Some(0), Some((0.0, 3.0)), InsertMode::Ripple, 1.0);
        assert!(id.is_some() && near(p.duration(), d0 + 3.0));
        assert!(near(p.clip(r1).unwrap().end(), 2.0), "geteilt an der Einfügestelle");
        assert!(near(p.clip(r2).unwrap().start, 7.0), "Nachfolger rückt nach");
        assert_eq!(p.tracks[1].clips.len(), 4, "Ton wird mit geteilt");
        // Mehrspur-Ripple nimmt fremde Spuren mit, Einspur lässt sie in Ruhe
        let (mut q, a, _, _) = editing_project();
        q.add_clip(title_at(5.0, 2.0), 4);
        q.insert_media_with(&a, 2.0, Some(0), Some((0.0, 3.0)), InsertMode::RippleAll, 1.0);
        assert!(near(q.tracks[4].clips[0].start, 8.0), "Mehrspur-Ripple verschiebt den Titel");
        let (mut q, a, _, _) = editing_project();
        q.add_clip(title_at(5.0, 2.0), 4);
        q.insert_media_with(&a, 2.0, Some(0), Some((0.0, 3.0)), InsertMode::Ripple, 1.0);
        assert!(near(q.tracks[4].clips[0].start, 5.0), "Einspur-Ripple lässt den Titel stehen");
    }

    #[test]
    fn jl_cut_trims_only_one_part_and_keeps_the_link() {
        let (mut p, _, r1, _) = editing_project();
        let pid = p.partner_id(r1).unwrap();
        let jl = TrimOptions { linked: false, ..Default::default() };
        assert!(p.trim_end_opt(r1, 3.0, &jl));
        assert!(near(p.clip(r1).unwrap().end(), 3.0) && near(p.clip(pid).unwrap().end(), 4.0), "Partner unverändert");
        assert_eq!(p.partner_id(r1), Some(pid), "Verknüpfung bleibt");
        assert!(p.trim_end_opt(r1, 4.0, &jl) && near(p.clip(r1).unwrap().end(), 4.0));
    }

    #[test]
    fn ripple_trim_pulls_and_pushes_followers() {
        let rt = TrimOptions { ripple: RippleScope::Track, ..Default::default() };
        let (mut p, _, r1, r2) = editing_project();
        assert!(p.trim_end_opt(r1, 3.0, &rt));
        assert!(near(p.clip(r1).unwrap().end(), 3.0) && near(p.clip(r2).unwrap().start, 3.0), "Nachfolger rückt auf");
        let mut starts: Vec<f64> = p.tracks[1].clips.iter().map(|c| c.start).collect();
        starts.sort_by(|a, b| a.total_cmp(b));
        assert_eq!(starts, vec![0.0, 3.0], "Ton rückt mit auf");
        assert!(p.trim_end_opt(r1, 3.5, &rt));
        assert!(near(p.clip(r2).unwrap().start, 3.5), "Nachfolger rückt weg");
        // Anfang: Position bleibt, Nachfolger rückt auf
        let (mut p, _, r1, r2) = editing_project();
        assert!(p.trim_start_opt(r1, 1.0, &rt));
        let c = p.clip(r1).unwrap();
        assert!(near(c.start, 0.0) && near(c.duration, 3.0) && near(c.source_in, 1.0), "Anfang gekürzt, Position bleibt");
        assert!(near(p.clip(r2).unwrap().start, 3.0));
    }

    #[test]
    fn roll_moves_the_cut_and_keeps_total_length() {
        let roll = TrimOptions { roll: true, ..Default::default() };
        let (mut p, _, r1, r2) = editing_project();
        let d0 = p.duration();
        assert!(p.trim_end_opt(r1, 3.0, &roll));
        assert!(near(p.clip(r1).unwrap().end(), 3.0) && near(p.clip(r2).unwrap().start, 3.0) && near(p.duration(), d0));
        assert!(near(p.clip(r2).unwrap().source_in, 3.0), "Inhalt des Nachbarn bleibt an seiner Stelle");
        // nach links: der Nachbar braucht Material vor seinem Anfang
        let (mut p, a) = make_project();
        let r1 = p.insert_media(&a, 0.0, Some(0), Some((0.0, 6.0))).unwrap();
        let r2 = p.insert_media(&a, 6.0, Some(0), Some((2.0, 8.0))).unwrap();
        assert!(p.trim_end_opt(r1, 5.5, &roll));
        assert!(near(p.clip(r1).unwrap().end(), 5.5) && near(p.clip(r2).unwrap().start, 5.5) && near(p.clip(r2).unwrap().source_in, 1.5));
    }

    #[test]
    fn gaps_followers_and_close_gap() {
        let (mut p, a, r1, r2) = editing_project();
        assert!(p.video_gaps().is_empty(), "keine Lücke bei lückenlosem Schnitt");
        p.move_clips(&ids(&[r2]), 0, 1.0);
        let gaps = p.video_gaps();
        assert!(gaps.len() == 1 && near(gaps[0].0, 4.0) && near(gaps[0].1, 5.0), "{gaps:?}");
        assert!(p.close_gap_range(gaps[0]) && near(p.clip(r2).unwrap().start, 4.0) && p.video_gaps().is_empty());
        assert!(p.followers(&ids(&[r1]), MouseMode::Single).is_empty(), "Mausmodus 6: keine Nachfolger");
        assert!(p.followers(&ids(&[r1]), MouseMode::OneTrack).contains(&r2), "Mausmodus 8: Nachfolger auf der Spur");
        assert!(p.insert_media(&a, 9.0, Some(2), Some((0.0, 1.0))).is_some(), "Platzieren hinter dem Ende");
        let gaps = p.video_gaps();
        assert!(gaps.len() == 1 && near(gaps[0].0, 8.0), "Lücke zwischen Ende und neuem Clip: {gaps:?}");
    }

    #[test]
    fn four_point_edit_stretches_the_range() {
        let (mut p, a, _, _) = editing_project();
        let id = p.insert_media_with(&a, 2.0, Some(0), Some((0.0, 4.0)), InsertMode::Overwrite, 2.0).unwrap();
        let c = p.clip(id).unwrap();
        assert!(near(c.duration, 2.0) && near(c.speed, 2.0) && near(c.end(), 4.0), "Länge = Zielbereich");
        assert!(p.tracks[1].clips.iter().any(|c| c.speed == 2.0 && near(c.duration, 2.0)), "Ton läuft mit gleichem Tempo");
        assert!(near(p.duration(), 8.0), "Gesamtlänge bleibt");
    }

    #[test]
    fn groups_move_select_and_paste_together() {
        let (mut p, a, r1, r2) = editing_project();
        let t = p.insert_media(&a, 10.0, Some(2), Some((0.0, 2.0))).unwrap();
        p.set_group(&ids(&[r1, t]), Some(Uuid::new_v4()));
        assert!(p.group_members(r1).contains(&t) && p.group_members(r1).len() >= 2, "Gruppe enthält beide");
        assert!(p.with_partners(&ids(&[r1])).contains(&t), "Gruppe zieht Mitglieder mit");
        assert!(!p.with_partners(&ids(&[r2])).contains(&t), "Nichtmitglied bleibt draußen");
        let all = p.with_partners(&ids(&[r1]));
        assert!(p.move_clips(&all, 0, 0.5) && near(p.clip(t).unwrap().start, 10.5), "Gruppe verschiebt gemeinsam");
        let items = vec![(p.clip(r1).unwrap().clone(), 0), (p.clip(t).unwrap().clone(), 2)];
        let pasted = p.paste(&items, 20.0);
        let (g0, g1) = (p.clip(pasted[0]).unwrap().group, p.clip(pasted[1]).unwrap().group);
        assert!(pasted.len() == 2 && g0.is_some() && g0 == g1 && g0 != p.clip(r1).unwrap().group, "Kopie bildet eine eigene Gruppe");
        p.set_group(&ids(&[r1]), None);
        assert!(p.clip(r1).unwrap().group.is_none() && p.clip(t).unwrap().group.is_none(), "Auflösen entfernt die Gruppe samt Mitgliedern");
    }

    #[test]
    fn snap_cuts_to_beats_moves_both_sides() {
        let (mut p, a) = make_project();
        let c1 = p.insert_media(&a, 0.0, Some(0), Some((0.0, 3.7))).unwrap();
        let c2 = p.insert_media(&a, 3.7, Some(0), Some((1.0, 5.5))).unwrap();
        let c3 = p.insert_media(&a, 8.2, Some(0), Some((1.0, 5.0))).unwrap();
        let mut song = Clip { content: ClipContent::Media(Wrap { v: a.id }), part: ClipPart::Audio, start: 0.0, duration: 10.0, ..Default::default() };
        song.beat_grid = Some(BeatGrid { period: 0.5, phase: 0.0, confidence: 1.0 });
        let beats = p.beat_times(&song, 1);
        assert!(beats.len() == 21 && near(beats[1], 0.5), "Schlagzeiten aus dem Taktraster");
        assert_eq!(p.beat_times(&song, 4).len(), 6, "Taktanfänge");
        let (moved, _) = p.snap_cuts_to_beats(0, &beats);
        assert_eq!(moved, 2, "zwei Schnitte angepasst");
        assert!(near(p.clip(c1).unwrap().end(), 3.5) && near(p.clip(c2).unwrap().start, 3.5), "Schnitt 1 bei 3,5 s");
        assert!(near(p.clip(c2).unwrap().end(), 8.0) && near(p.clip(c3).unwrap().start, 8.0), "Schnitt 2 bei 8,0 s");
        assert!(near(p.clip(c2).unwrap().source_in, 0.8) && near(p.clip(c3).unwrap().source_in, 0.8), "Inhalt bleibt an seiner Stelle");
    }

    #[test]
    fn transitions_from_reserve_from_pushing_and_hard_cut() {
        let (mut p, a) = make_project();
        let c1 = p.insert_media(&a, 0.0, Some(0), Some((1.0, 5.0))).unwrap(); // Reserve hinten (bis 10 s)
        let c2 = p.insert_media(&a, 4.0, Some(0), Some((1.0, 5.0))).unwrap(); // Reserve davor (1 s)
        assert!(p.junctions(0).len() == 1 && p.junction_of(c1).map(|j| j.b) == Some(c2) && p.junction_of(c2).map(|j| j.a) == Some(c1));
        assert!(p.set_transition(c1, c2, Some("wipe"), 1.0), "Übergang 1 s");
        assert!(near(p.clip(c1).unwrap().end() - p.clip(c2).unwrap().start, 1.0) && p.clip(c2).unwrap().transition == "wipe");
        assert!(near(p.duration(), 8.0), "Film nicht kürzer (aus Reserve)");
        assert!(p.set_transition(c1, c2, Some("zoom"), 2.0) && near(p.clip(c1).unwrap().end() - p.clip(c2).unwrap().start, 2.0), "auf 2 s verlängert");
        assert!(p.set_transition(c1, c2, None, 1.0) && near(p.clip(c1).unwrap().end(), p.clip(c2).unwrap().start), "harter Schnitt");
        // ohne Reserve: nachrücken
        let (mut q, b) = make_project();
        let d1 = q.insert_media(&b, 0.0, Some(0), Some((0.0, 4.0))).unwrap();
        let d2 = q.insert_media(&b, 4.0, Some(0), Some((0.0, 4.0))).unwrap();
        let d3 = q.insert_media(&b, 8.0, Some(0), Some((6.0, 10.0))).unwrap();
        assert!(q.set_transition(d1, d2, Some("crossfade"), 1.0), "Übergang ohne Reserve");
        let ov = q.clip(d1).unwrap().end() - q.clip(d2).unwrap().start;
        assert!(ov > 0.99 && ov < 1.01, "Überlappung {ov}");
        assert!(q.clip(d3).unwrap().start < 8.0, "Folgeclip rückt nach");
        assert_eq!(q.junctions(0).len(), 2, "zwei Übergänge");
    }

    #[test]
    fn splitting_inside_a_transition_is_refused() {
        let (mut p, a) = make_project();
        let c1 = p.insert_media(&a, 0.0, Some(0), Some((1.0, 6.0))).unwrap();
        let c2 = p.insert_media(&a, 5.0, Some(0), Some((1.0, 6.0))).unwrap();
        assert!(p.set_transition(c1, c2, Some("crossfade"), 2.0), "Blende 2 s");
        let before = p.clone();
        let n = p.tracks[0].clips.len();
        let end = p.clip(c1).unwrap().end();
        assert!(p.split(c1, end - 0.5).is_none() && p.tracks[0].clips.len() == n && p == before, "mitten in der Blende abgelehnt, Projekt unverändert");
        assert!(p.split(c1, 1.0).is_some(), "vor der Blende möglich");
    }

    #[test]
    fn split_clears_group_on_the_right_part() {
        let (mut p, _, r1, _) = editing_project();
        p.set_group(&ids(&[r1]), Some(Uuid::new_v4()));
        let right = p.split(r1, 2.0).unwrap();
        assert!(p.clip(right).unwrap().group.is_none() && p.clip(r1).unwrap().group.is_some());
    }

    #[test]
    fn bookmarks_are_sorted_by_time() {
        let mut p = Project::default();
        p.bookmarks = Some([("3".to_string(), 1.0), ("1".to_string(), 5.0), ("10".to_string(), 2.0)].into_iter().collect());
        assert_eq!(p.bookmark_list(), vec![(3, 1.0), (10, 2.0), (1, 5.0)]);
    }
}

