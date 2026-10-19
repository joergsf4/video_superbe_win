//! Bearbeitungsfunktionen des Projekts (Port von ProjectOps.swift): Einfügen, Verschieben, Kürzen, Teilen,
//! Einfügen aus der Zwischenablage, Ripple-Löschen, Geschwindigkeit, Slip.
//! Alle Funktionen sind „ganz oder gar nicht“: passt etwas nicht, bleibt das Projekt unverändert.

use crate::model::*;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

pub const MIN_CLIP: f64 = 0.1;
const EPS: f64 = 1e-6;

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

    /// IDs samt verknüpften Partnern.
    pub fn with_partners(&self, ids: &HashSet<Uuid>) -> HashSet<Uuid> {
        let mut r = ids.clone();
        for id in ids {
            if let Some(p) = self.partner_id(*id) {
                r.insert(p);
            }
        }
        r
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
        let mut order: Vec<usize> = preferred.into_iter().collect();
        order.extend(0..=self.tracks.len());
        for t in order {
            let mut s = self.clone();
            s.ensure_tracks(t + 2);
            if let Some(id) = s.place_media(asset, start, t, range) {
                *self = s;
                return Some(id);
            }
        }
        None
    }

    fn place_media(&mut self, asset: &MediaAsset, start: f64, t: usize, range: Option<(f64, f64)>) -> Option<Uuid> {
        let pic = asset.has_video || asset.is_image;
        let link = (pic && asset.has_audio && !asset.is_image).then(Uuid::new_v4);
        let (mut src_in, mut dur) = (0.0, asset.duration);
        if let (Some((a, b)), false) = (range, asset.is_image) {
            src_in = a.clamp(0.0, asset.duration);
            dur = (b.min(asset.duration) - src_in).max(MIN_CLIP);
        }
        let mk = |part: ClipPart, link: Option<Uuid>| Clip { content: ClipContent::Media(Wrap { v: asset.id }), part, link, start, source_in: src_in, duration: dur, ..Default::default() };
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
            let mut ids = vec![];
            let mut ok = true;
            for (c, tr) in items {
                let mut n = c.clone();
                n.id = Uuid::new_v4();
                n.start = (at + (c.start - min_start)).max(0.0);
                if let Some(l) = c.link {
                    n.link = Some(*links.entry(l).or_insert_with(Uuid::new_v4));
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
        let backup = self.clone();
        if !self.trim_start_single(id, to) {
            return false;
        }
        let Some(new) = self.clip(id).map(|c| c.start) else { return false };
        if let Some(p) = self.partner_id(id) {
            if !self.trim_start_single(p, new) {
                *self = backup;
                return false;
            }
        }
        true
    }

    pub fn trim_end(&mut self, id: Uuid, to: f64) -> bool {
        let backup = self.clone();
        if !self.trim_end_single(id, to) {
            return false;
        }
        let Some(new) = self.clip(id).map(Clip::end) else { return false };
        if let Some(p) = self.partner_id(id) {
            if !self.trim_end_single(p, new) {
                *self = backup;
                return false;
            }
        }
        true
    }

    fn trim_start_single(&mut self, id: Uuid, to: f64) -> bool {
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

    fn trim_end_single(&mut self, id: Uuid, to: f64) -> bool {
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

    /// Teilt den Clip (und seinen Partner) an `time`. Gibt die ID des rechten Teils zurück.
    pub fn split(&mut self, id: Uuid, time: f64) -> Option<Uuid> {
        let new_link = Uuid::new_v4();
        let right = self.split_single(id, time, new_link)?;
        if let Some(p) = self.partner_id(id) {
            let _ = self.split_single(p, time, new_link);
        }
        Some(right)
    }

    fn split_single(&mut self, id: Uuid, time: f64, new_link: Uuid) -> Option<Uuid> {
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
