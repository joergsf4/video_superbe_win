//! Bearbeitungsbefehle für Titel, Effekte und Blenden aus dem Mac-Original: Titelvorlagen (Presets, eigene Vorlagen,
//! Datumstitel), Effekte kopieren/einfügen, LUT wählen, Blenden anwenden.

use crate::anim::{Channel, Keyframe};
use crate::app::App;
use crate::model::*;
use crate::ops::{Junction, TRANSITION_TEMPLATES};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Gespeicherte Titelvorlage (Datei `TitleTemplates.json`, gleiches Format wie am Mac).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TitleTemplate {
    pub id: Uuid,
    pub name: String,
    pub style: TitleStyle,
    pub duration: f64,
    pub x: f64,
    pub y: f64,
}

pub fn load_templates(path: &Path) -> Vec<TitleTemplate> {
    std::fs::read(path).ok().and_then(|d| serde_json::from_slice(d.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(&d)).ok()).unwrap_or_default()
}

pub fn save_templates(path: &Path, list: &[TitleTemplate]) {
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(s) = serde_json::to_string_pretty(list) {
        let _ = std::fs::write(path, s);
    }
}

/// Kopierte Effekte (Bild: Effekte samt Keyframes der Effektkanäle; Ton: Tonbearbeitung).
#[derive(Clone, Debug, Default)]
pub struct FxClipboard {
    pub fx: Option<Effects>,
    pub keys: BTreeMap<Channel, Vec<Keyframe>>,
    pub audio: Option<AudioFx>,
}

/// Drag-Nutzdaten einer Blendenvorlage aus dem Reiter „Blenden“.
#[derive(Clone, Debug)]
pub struct TransitionDrag(pub String);

/// Datum und Uhrzeit einer Dateizeit in lokaler Zeit: (Jahr, Monat, Tag, Stunde, Minute).
pub fn local_datetime(t: std::time::SystemTime) -> Option<(i32, u32, u32, u32, u32)> {
    use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    let secs = t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    let ft = (secs + 11_644_473_600) * 10_000_000;
    let ft = FILETIME { dwLowDateTime: ft as u32, dwHighDateTime: (ft >> 32) as u32 };
    let (mut utc, mut local) = (SYSTEMTIME::default(), SYSTEMTIME::default());
    unsafe {
        FileTimeToSystemTime(&ft, &mut utc).ok()?;
        SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).ok()?;
    }
    Some((local.wYear as i32, local.wMonth as u32, local.wDay as u32, local.wHour as u32, local.wMinute as u32))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitlePreset {
    Title,
    LowerThird,
    Credits,
    Ticker,
    Timecode,
    Subtitle,
}

impl App {
    pub(crate) fn templates_path(&self) -> PathBuf {
        self.feat.data_dir.join("TitleTemplates.json")
    }

    /// Fügt einen Titel an der Abspielposition ein (Titel, Bauchbinde, Abspann, Laufschrift, Timecode, Untertitel).
    pub(crate) fn add_title(&mut self, preset: TitlePreset) {
        let mut clip = Clip { start: self.playhead, duration: 4.0, fade_in: 0.3, fade_out: 0.3, ..Default::default() };
        let dur_rest = (self.project.duration() - self.playhead).max(1.0);
        match preset {
            TitlePreset::Title => {}
            TitlePreset::LowerThird => {
                clip.content = ClipContent::Title(Wrap { v: TitleStyle { text: "Name\nFunktion".into(), font_size: 0.06, background: true, left_aligned: true, ..Default::default() } });
                clip.transform = Transform2D { x: -0.2, y: 0.3, ..Default::default() };
            }
            TitlePreset::Credits => {
                let st = TitleStyle { text: "Abspann\n\nKamera\nSchnitt\nMusik\n\nDanke fürs Zuschauen!".into(), font_size: 0.07, credits: Some(true), outline: Some(true), ..Default::default() };
                clip = Clip { content: ClipContent::Title(Wrap { v: st }), start: self.playhead, duration: 12.0, ..Default::default() };
            }
            TitlePreset::Ticker => {
                let st = TitleStyle { text: "Laufschrift – hier steht dein Text".into(), font_size: 0.06, background: true, ticker: Some(true), ..Default::default() };
                clip = Clip { content: ClipContent::Title(Wrap { v: st }), start: self.playhead, duration: 8.0, ..Default::default() };
                clip.transform = Transform2D { x: 0.0, y: -0.38, ..Default::default() };
            }
            TitlePreset::Timecode => {
                let st = TitleStyle { text: "{timecode}".into(), font_size: 0.045, background: true, font: Some("Consolas".into()), background_color: Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.55 }, ..Default::default() };
                clip = Clip { content: ClipContent::Title(Wrap { v: st }), start: self.playhead, duration: dur_rest, ..Default::default() };
                clip.transform = Transform2D { x: 0.36, y: -0.4, ..Default::default() };
            }
            TitlePreset::Subtitle => {
                // wie die erzeugten Untertitel der Mac-Version: unten im Bild, halbtransparenter dunkler Hintergrund
                let portrait = self.project.render_height > self.project.render_width;
                clip.content = ClipContent::Title(Wrap {
                    v: TitleStyle {
                        text: "Untertitel".into(),
                        font_size: if portrait { 0.032 } else { 0.05 },
                        background: true,
                        background_color: Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.55 },
                        subtitle: Some(true),
                        ..Default::default()
                    },
                });
                clip.transform = Transform2D { x: 0.0, y: if portrait { 0.30 } else { 0.38 }, ..Default::default() };
                clip.duration = 3.0;
                clip.fade_in = 0.0;
                clip.fade_out = 0.0;
            }
        }
        self.add_title_clip(clip);
    }

    pub(crate) fn add_title_clip(&mut self, clip: Clip) {
        let id = clip.id;
        if let Some(t) = self.project.first_free_track(&clip) {
            self.project.add_clip(clip, t);
        } else {
            self.add_track();
            let t = self.project.tracks.len() - 1;
            self.project.add_clip(clip, t);
        }
        self.select(id, false);
    }

    /// „Datum als Titel einblenden“ (Kontextmenü eines Videoclips): Erstellungsdatum der Datei als Titel über dem Clip.
    pub(crate) fn add_date_title(&mut self, id: Uuid) {
        let Some(c) = self.project.clip(id).cloned() else { return };
        let Some(a) = c.asset_id().and_then(|a| self.project.asset(a)).cloned() else { return };
        let Some((y, mo, d, h, mi)) = std::fs::metadata(&a.path).and_then(|m| m.created().or_else(|_| m.modified())).ok().and_then(local_datetime) else {
            self.status = "Kein Datum in der Datei gefunden".into();
            return;
        };
        let st = TitleStyle {
            text: format!("{} um {h:02}:{mi:02}", crate::fxlib::german_date(y, mo, d)),
            font_size: 0.04,
            background: true,
            left_aligned: true,
            background_color: Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.5 },
            bold: Some(false),
            ..Default::default()
        };
        let mut t = Clip { content: ClipContent::Title(Wrap { v: st }), start: c.start, duration: c.duration, fade_in: 0.3, fade_out: 0.3, ..Default::default() };
        t.transform = Transform2D { x: -0.3, y: -0.4, ..Default::default() };
        self.add_title_clip(t);
        self.inspector_tab = 2;
    }

    // ---------- Titelvorlagen ----------

    /// Stil einer Vorlage auf den gewählten Titel übertragen; nur der Text bleibt.
    pub(crate) fn apply_title_template(&mut self, t: &TitleTemplate, id: Uuid) {
        if let Some(c) = self.project.clip_mut(id) {
            if let ClipContent::Title(old) = &c.content {
                let mut n = t.style.clone();
                n.text = old.v.text.clone();
                c.content = ClipContent::Title(Wrap { v: n });
            }
        }
    }

    /// Titel aus einer gespeicherten Vorlage an der Abspielposition einfügen.
    pub(crate) fn add_title_from_template(&mut self, t: &TitleTemplate) {
        let mut clip = Clip { content: ClipContent::Title(Wrap { v: t.style.clone() }), start: self.playhead, duration: t.duration, fade_in: 0.3, fade_out: 0.3, ..Default::default() };
        clip.transform = Transform2D { x: t.x, y: t.y, ..Default::default() };
        if t.style.credits == Some(true) {
            clip.fade_in = 0.0;
            clip.fade_out = 0.0;
        }
        self.add_title_clip(clip);
    }

    /// Stil, Dauer und Position des Titelclips als Vorlage sichern.
    pub(crate) fn save_title_template(&mut self, clip: &Clip, name: String) {
        let ClipContent::Title(w) = &clip.content else { return };
        let mut st = w.v.clone();
        st.subtitle = None;
        self.title_templates.push(TitleTemplate { id: Uuid::new_v4(), name, style: st, duration: clip.duration, x: clip.transform.x, y: clip.transform.y });
        save_templates(&self.templates_path(), &self.title_templates);
    }

    pub(crate) fn delete_title_template(&mut self, id: Uuid) {
        self.title_templates.retain(|t| t.id != id);
        save_templates(&self.templates_path(), &self.title_templates);
    }

    // ---------- Effekte kopieren und einfügen ----------

    /// Strg+Alt+C: Bildeffekte (samt Keyframes der Effektkanäle) bzw. Tonbearbeitung des gewählten Clips merken.
    pub(crate) fn copy_effects(&mut self) {
        let Some(c) = self.selected.and_then(|id| self.project.clip(id)).filter(|c| !c.is_title()).cloned() else {
            self.status = "Zum Kopieren der Effekte einen Bild- oder Tonclip wählen".into();
            return;
        };
        self.fx_clipboard = Some(if c.is_audio() {
            FxClipboard { audio: Some(c.audio_fx.clone()), ..Default::default() }
        } else {
            FxClipboard { fx: Some(c.fx.clone()), keys: c.keys.0.iter().filter(|(ch, _)| Channel::FX.contains(ch)).map(|(ch, v)| (*ch, v.clone())).collect(), audio: None }
        });
        self.status = "Effekte kopiert (Strg+Alt+V fügt sie in die gewählten Clips ein)".into();
    }

    /// Strg+Alt+V: auf alle gewählten Clips gleicher Art anwenden (Ausschnitt und Füllen bleiben beim Ziel).
    pub(crate) fn paste_effects(&mut self) {
        let Some(b) = self.fx_clipboard.clone() else {
            self.status = "Keine kopierten Effekte (Strg+Alt+C)".into();
            return;
        };
        let targets: Vec<Uuid> = self
            .project
            .with_partners(&self.selected_set())
            .into_iter()
            .filter(|id| self.project.clip(*id).is_some_and(|c| !c.is_title() && if b.audio.is_some() { c.is_audio() } else { !c.is_audio() }))
            .collect();
        if targets.is_empty() {
            self.status = "Die gewählten Clips passen nicht zu den kopierten Effekten".into();
            return;
        }
        for id in &targets {
            let Some(c) = self.project.clip_mut(*id) else { continue };
            if let Some(a) = &b.audio {
                c.audio_fx = a.clone();
                continue;
            }
            let Some(fx) = &b.fx else { continue };
            let mut n = fx.clone();
            n.fill = c.fx.fill;
            n.crop_l = c.fx.crop_l;
            n.crop_t = c.fx.crop_t;
            n.crop_r = c.fx.crop_r;
            n.crop_b = c.fx.crop_b;
            c.fx = n;
            for ch in Channel::FX {
                c.keys.0.remove(&ch);
            }
            let dur = c.duration;
            for (ch, ks) in &b.keys {
                let list: Vec<Keyframe> = ks.iter().map(|k| Keyframe { id: Uuid::new_v4(), ..k.clone() }).filter(|k| k.t <= dur).collect();
                if !list.is_empty() {
                    c.keys.0.insert(*ch, list);
                }
            }
        }
        self.status = format!("Effekte eingefügt ({} Clips)", targets.len());
    }

    /// LUT-Datei (.cube) für den Clip wählen.
    pub(crate) fn choose_lut(&mut self, id: Uuid) {
        let Some(p) = rfd::FileDialog::new().add_filter("3D-LUT (.cube)", &["cube"]).set_title("3D-LUT (.cube) als Filmlook wählen").pick_file() else { return };
        self.set_lut(id, &p);
    }

    pub(crate) fn set_lut(&mut self, id: Uuid, path: &Path) {
        if crate::fxlib::load_cube(&path.to_string_lossy()).is_none() {
            self.status = "Die LUT-Datei konnte nicht gelesen werden (nur 3D-LUTs im Format .cube).".into();
            return;
        }
        if let Some(c) = self.project.clip_mut(id) {
            c.fx.lut_path = Some(path.to_string_lossy().into_owned());
            c.fx.lut_amount = Some(c.fx.lut_amount.unwrap_or(1.0));
        }
    }

    // ---------- Blenden ----------

    /// Übergang in Reichweite (48 px) der Zeit `t` für eine Ablage auf der Zeile `row` (Bildspur oder Tonspur des Paares).
    pub(crate) fn junction_near(&self, t: f64, row: usize) -> Option<Junction> {
        let mut best: Option<(Junction, f64)> = None;
        for tr in 0..self.project.tracks.len() {
            for j in self.project.junctions(tr) {
                let (Some(a), Some(b)) = (self.project.clip(j.a), self.project.clip(j.b)) else { continue };
                let partner_row = self.project.partner_id(j.b).and_then(|p| self.project.find_clip(p)).map(|l| l.0);
                if tr != row && partner_row != Some(row) {
                    continue;
                }
                let cx = if j.overlap > 0.0 { (a.end() + b.start) / 2.0 } else { a.end() };
                let d = (cx - t).abs() * self.zoom;
                if d <= 48.0 && best.as_ref().is_none_or(|b| d < b.1) {
                    best = Some((j, d));
                }
            }
        }
        best.map(|b| b.0)
    }

    pub(crate) fn apply_transition(&mut self, a: Uuid, b: Uuid, kind: Option<&str>, duration: Option<f64>) {
        let d = duration.unwrap_or_else(|| {
            let o = self.project.clip(a).map(Clip::end).unwrap_or(0.0) - self.project.clip(b).map(|c| c.start).unwrap_or(0.0);
            if o > 0.05 { o } else { 1.0 }
        });
        if !self.project.set_transition(a, b, kind, d) {
            self.status = "Der Übergang passt hier nicht (zu wenig Platz oder gesperrte Spur).".into();
        }
    }

    /// Art des Übergangs an dieser Stelle (`None` = harter Schnitt).
    pub(crate) fn junction_kind(&self, j: &Junction) -> Option<String> {
        (j.overlap > 0.001).then(|| self.project.clip(j.b).map(|c| c.transition.clone())).flatten()
    }

    pub(crate) fn transition_templates() -> [&'static str; 5] {
        TRANSITION_TEMPLATES
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let ctx = eframe::egui::Context::default();
        let mut a = App::with_ctx(&ctx, None);
        a.feat.data_dir = std::env::temp_dir().join("video-superbe-tests").join(format!("fx-{}", Uuid::new_v4()));
        a.title_templates.clear();
        a
    }

    fn two_clips(a: &mut App) -> (Uuid, Uuid) {
        let asset = MediaAsset { path: "x".into(), name: "x".into(), duration: 20.0, has_video: true, has_audio: true, width: 100.0, height: 50.0, fps: 30.0, ..Default::default() };
        a.project.assets.push(asset.clone());
        let r1 = a.project.insert_media(&asset, 0.0, Some(0), Some((1.0, 5.0))).unwrap();
        let r2 = a.project.insert_media(&asset, 4.0, Some(0), Some((1.0, 5.0))).unwrap();
        (r1, r2)
    }

    #[test]
    fn title_presets_match_the_mac_defaults() {
        let mut a = app();
        a.playhead = 1.0;
        a.add_title(TitlePreset::Credits);
        let c = a.project.clip(a.selected.unwrap()).unwrap();
        let ClipContent::Title(w) = &c.content else { panic!() };
        assert!(w.v.credits == Some(true) && w.v.outline == Some(true) && (c.duration - 12.0).abs() < 1e-9 && (w.v.font_size - 0.07).abs() < 1e-9);
        a.add_title(TitlePreset::Ticker);
        let c = a.project.clip(a.selected.unwrap()).unwrap();
        assert!(matches!(&c.content, ClipContent::Title(w) if w.v.ticker == Some(true)) && (c.duration - 8.0).abs() < 1e-9 && (c.transform.y + 0.38).abs() < 1e-9);
        let (_, _) = two_clips(&mut a);
        a.playhead = 2.0;
        a.add_title(TitlePreset::Timecode);
        let c = a.project.clip(a.selected.unwrap()).unwrap();
        assert!(matches!(&c.content, ClipContent::Title(w) if w.v.text == "{timecode}") && (c.duration - (a.project.duration() - 2.0).max(1.0)).abs() < 1e-6, "Timecode läuft bis zum Filmende");
        assert!((c.transform.x - 0.36).abs() < 1e-9);
    }

    #[test]
    fn templates_save_apply_and_survive_a_restart() {
        let mut a = app();
        a.add_title(TitlePreset::LowerThird);
        let id = a.selected.unwrap();
        let clip = a.project.clip(id).unwrap().clone();
        a.save_title_template(&clip, "Meine Bauchbinde".into());
        assert_eq!(a.title_templates.len(), 1);
        // zweiter Titel mit anderem Text übernimmt den Stil, behält den Text
        a.add_title(TitlePreset::Title);
        let id2 = a.selected.unwrap();
        if let Some(c) = a.project.clip_mut(id2) {
            c.content = ClipContent::Title(Wrap { v: TitleStyle { text: "Mein Text".into(), ..Default::default() } });
        }
        let t = a.title_templates[0].clone();
        a.apply_title_template(&t, id2);
        let ClipContent::Title(w) = &a.project.clip(id2).unwrap().content else { panic!() };
        assert!(w.v.background && w.v.left_aligned && w.v.text == "Mein Text");
        // Datei lesen: gleiches Format wie am Mac
        let loaded = load_templates(&a.templates_path());
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "Meine Bauchbinde");
        let raw = std::fs::read_to_string(a.templates_path()).unwrap();
        assert!(raw.contains("\"duration\"") && raw.contains("\"style\""));
        // Einfügen aus der Vorlage
        a.playhead = 10.0;
        a.add_title_from_template(&t);
        let c = a.project.clip(a.selected.unwrap()).unwrap();
        assert!((c.start - 10.0).abs() < 1e-9 && (c.transform.x + 0.2).abs() < 1e-9);
        a.delete_title_template(t.id);
        assert!(load_templates(&a.templates_path()).is_empty());
    }

    #[test]
    fn effects_copy_and_paste_keep_crop_and_replace_keys() {
        let mut a = app();
        let (r1, r2) = two_clips(&mut a);
        a.project.clip_mut(r1).unwrap().fx = Effects { saturation: 0.0, contrast: 1.3, ..Default::default() };
        a.project.clip_mut(r1).unwrap().keys.0.insert(Channel::Brightness, vec![Keyframe::new(0.0, 0.1), Keyframe::new(2.0, 0.5), Keyframe::new(99.0, 0.9)]);
        a.project.clip_mut(r2).unwrap().fx.crop_l = 0.2;
        a.project.clip_mut(r2).unwrap().fx.fill = true;
        a.select(r1, false);
        a.copy_effects();
        a.select(r2, false);
        a.paste_effects();
        let c = a.project.clip(r2).unwrap();
        assert!(c.fx.saturation == 0.0 && c.fx.contrast == 1.3, "Effekte übernommen");
        assert!(c.fx.crop_l == 0.2 && c.fx.fill, "Ausschnitt und Füllen bleiben beim Ziel");
        assert_eq!(c.keys.get(Channel::Brightness).map(|k| k.len()), Some(2), "Keyframes übernommen, einer hinter dem Clipende verworfen");
        assert_ne!(a.project.clip(r1).unwrap().keys.get(Channel::Brightness).unwrap()[0].id, c.keys.get(Channel::Brightness).unwrap()[0].id, "neue Keyframe-IDs");
        // Titel lassen sich nicht kopieren
        a.add_title(TitlePreset::Title);
        a.copy_effects();
        assert!(a.status.contains("Bild- oder Tonclip"));
    }

    #[test]
    fn applying_a_transition_finds_junctions_and_reports_problems() {
        let mut a = app();
        let (c1, c2) = two_clips(&mut a);
        a.zoom = 100.0;
        let j = a.junction_near(4.0, 0).expect("Übergang bei 4 s auf Spur 1");
        assert!(a.junction_near(4.2, 0).is_some() && a.junction_near(8.0, 0).is_none(), "48 px Reichweite");
        assert_eq!(a.junction_near(4.0, 1).map(|j| j.b), Some(c2), "auch über die Tonspur des Paares");
        assert!(a.junction_kind(&j).is_none(), "ohne Überlappung: harter Schnitt");
        a.apply_transition(j.a, j.b, Some("iris"), Some(1.0));
        let j = a.project.junction_of(c2).unwrap();
        assert_eq!(a.junction_kind(&j).as_deref(), Some("iris"));
        assert!((a.project.clip(c1).unwrap().end() - a.project.clip(c2).unwrap().start - 1.0).abs() < 0.02);
        a.apply_transition(j.a, j.b, None, None);
        assert!(a.project.clip(c1).unwrap().end() - a.project.clip(c2).unwrap().start < 0.002, "Entfernen = harter Schnitt");
        // gesperrte Spur: Meldung statt stiller Änderung
        a.project.tracks[0].locked = true;
        let before = a.project.clone();
        a.apply_transition(c1, c2, Some("wipe"), Some(1.0));
        assert!(a.project == before && a.status.contains("passt hier nicht"));
    }

    #[test]
    fn lut_validation_and_date_helper() {
        let mut a = app();
        let (c1, _) = two_clips(&mut a);
        let bad = std::env::temp_dir().join("video-superbe-tests").join("bad.cube");
        std::fs::write(&bad, "LUT_1D_SIZE 4\n0 0 0\n").unwrap();
        a.set_lut(c1, &bad);
        assert!(a.status.contains("nicht gelesen") && a.project.clip(c1).unwrap().fx.lut_path.is_none());
        let good = std::env::temp_dir().join("video-superbe-tests").join("good.cube");
        let mut s = String::from("LUT_3D_SIZE 2\n");
        for b in 0..2 {
            for g in 0..2 {
                for r in 0..2 {
                    s += &format!("{r} {g} {b}\n");
                }
            }
        }
        std::fs::write(&good, s).unwrap();
        a.set_lut(c1, &good);
        let c = a.project.clip(c1).unwrap();
        assert!(c.fx.lut_path.is_some() && c.fx.lut_amount == Some(1.0));
        let (y, m, d, _, _) = local_datetime(std::time::SystemTime::now()).unwrap();
        assert!((2024..2100).contains(&y) && (1..=12).contains(&m) && (1..=31).contains(&d));
        // Datumstitel aus der Dateizeit (die Testdatei existiert)
        let real = std::env::temp_dir().join("video-superbe-tests").join("good.cube");
        a.project.assets[0].path = real.to_string_lossy().into_owned();
        a.add_date_title(c1);
        let t = a.project.clip(a.selected.unwrap()).unwrap();
        assert!(matches!(&t.content, ClipContent::Title(w) if w.v.text.contains(" um ") && w.v.left_aligned && w.v.bold == Some(false)));
        assert!((t.start - a.project.clip(c1).unwrap().start).abs() < 1e-9 && a.inspector_tab == 2);
    }
}
