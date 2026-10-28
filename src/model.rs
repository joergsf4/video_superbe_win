//! Projektmodell, JSON-kompatibel zur macOS-App (Swift `Codable`, `*.mvd.json`).
//! Unbekannte Felder bleiben über `extra` erhalten.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use uuid::Uuid;

fn is_none<T>(o: &Option<T>) -> bool {
    o.is_none()
}
fn one() -> f64 {
    1.0
}
fn identity() -> [f64; 6] {
    [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
}
fn new_id() -> Uuid {
    Uuid::new_v4()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MediaAsset {
    pub id: Uuid,
    pub path: String,
    pub name: String,
    pub duration: f64,
    pub has_video: bool,
    pub has_audio: bool,
    pub width: f64,
    pub height: f64,
    pub fps: f64,
    pub transform: [f64; 6],
    pub is_image: bool,
    #[serde(skip_serializing_if = "is_none")]
    pub is_hdr: Option<bool>,
}

impl Default for MediaAsset {
    fn default() -> Self {
        Self {
            id: new_id(),
            path: String::new(),
            name: String::new(),
            duration: 0.0,
            has_video: false,
            has_audio: false,
            width: 0.0,
            height: 0.0,
            fps: 0.0,
            transform: identity(),
            is_image: false,
            is_hdr: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rgba {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Rgba {
    pub const WHITE: Rgba = Rgba { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TitleStyle {
    pub text: String,
    /// Schrifthöhe als Bruchteil der Bildhöhe.
    pub font_size: f64,
    pub color: Rgba,
    pub background: bool,
    pub background_color: Rgba,
    pub left_aligned: bool,
    #[serde(skip_serializing_if = "is_none")]
    pub font: Option<String>,
    #[serde(skip_serializing_if = "is_none")]
    pub outline: Option<bool>,
    #[serde(skip_serializing_if = "is_none")]
    pub subtitle: Option<bool>,
    /// Schatten: `None` = Standard (Schatten, wenn kein Hintergrund), sonst ausdrücklich an/aus.
    #[serde(skip_serializing_if = "is_none")]
    pub shadow: Option<bool>,
    /// Einlaufen/Auslaufen: none, flyUp, flyLeft, flyRight, zoom, typewriter (Dauer `anim_dur`, Standard 0,6 s).
    #[serde(skip_serializing_if = "is_none")]
    pub anim_in: Option<String>,
    #[serde(skip_serializing_if = "is_none")]
    pub anim_out: Option<String>,
    #[serde(skip_serializing_if = "is_none")]
    pub anim_dur: Option<f64>,
    /// Rollender Abspann (von unten nach oben über die ganze Clipdauer) und Laufschrift (von rechts nach links).
    #[serde(skip_serializing_if = "is_none")]
    pub credits: Option<bool>,
    #[serde(skip_serializing_if = "is_none")]
    pub ticker: Option<bool>,
    /// `None` = Standard (erste Zeile fett)
    #[serde(skip_serializing_if = "is_none")]
    pub bold: Option<bool>,
    #[serde(skip_serializing_if = "is_none")]
    pub italic: Option<bool>,
    #[serde(skip_serializing_if = "is_none")]
    pub underline: Option<bool>,
    #[serde(skip_serializing_if = "is_none")]
    pub right_aligned: Option<bool>,
    /// Zeilenabstand, Anteil der Schrifthöhe (−0,3 … 1)
    #[serde(skip_serializing_if = "is_none")]
    pub line_spacing: Option<f64>,
    /// Laufweite, Anteil der Schrifthöhe (−0,1 … 0,5)
    #[serde(skip_serializing_if = "is_none")]
    pub kerning: Option<f64>,
    /// Transparenz (Standard 0,7) und Unschärfe (Anteil der Schrifthöhe, Standard 0,1) des Schattens
    #[serde(skip_serializing_if = "is_none")]
    pub shadow_alpha: Option<f64>,
    #[serde(skip_serializing_if = "is_none")]
    pub shadow_blur: Option<f64>,
    /// Breite der Umrandung (Standard 4) und „nur Rahmen“
    #[serde(skip_serializing_if = "is_none")]
    pub outline_width: Option<f64>,
    #[serde(skip_serializing_if = "is_none")]
    pub outline_hollow: Option<bool>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for TitleStyle {
    fn default() -> Self {
        Self {
            text: "Titel".into(),
            font_size: 0.08,
            color: Rgba::WHITE,
            background: false,
            background_color: Rgba { r: 0.05, g: 0.15, b: 0.35, a: 0.85 },
            left_aligned: false,
            font: None,
            outline: None,
            subtitle: None,
            shadow: None,
            anim_in: None,
            anim_out: None,
            anim_dur: None,
            credits: None,
            ticker: None,
            bold: None,
            italic: None,
            underline: None,
            right_aligned: None,
            line_spacing: None,
            kerning: None,
            shadow_alpha: None,
            shadow_blur: None,
            outline_width: None,
            outline_hollow: None,
            extra: Map::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Effects {
    pub brightness: f64,
    pub contrast: f64,
    pub saturation: f64,
    pub temperature: f64,
    pub sharpness: f64,
    pub blur: f64,
    pub vignette: f64,
    pub fill: bool,
    pub crop_l: f64,
    pub crop_t: f64,
    pub crop_r: f64,
    pub crop_b: f64,
    #[serde(skip_serializing_if = "is_none")]
    pub cutout: Option<bool>,
    #[serde(skip_serializing_if = "is_none")]
    pub cutout_quality: Option<i64>,
    #[serde(skip_serializing_if = "is_none")]
    pub cutout_soft: Option<f64>,
    #[serde(skip_serializing_if = "is_none")]
    pub cutout_shift: Option<f64>,
    /// Spiegeln und Drehen in 90°-Schritten (im Uhrzeigersinn, 0…3)
    #[serde(skip_serializing_if = "is_none")]
    pub flip_h: Option<bool>,
    #[serde(skip_serializing_if = "is_none")]
    pub flip_v: Option<bool>,
    #[serde(skip_serializing_if = "is_none")]
    pub quarter_turns: Option<i64>,
    /// Horizont begradigen: Drehwinkel in Grad (−20…20), es wird automatisch hineingezoomt
    #[serde(skip_serializing_if = "is_none")]
    pub horizon: Option<f64>,
    /// Greenscreen
    #[serde(skip_serializing_if = "is_none")]
    pub chroma: Option<ChromaKey>,
    /// 3D-LUT (.cube) und Stärke (0…1, Standard 1)
    #[serde(skip_serializing_if = "is_none")]
    pub lut_path: Option<String>,
    #[serde(skip_serializing_if = "is_none")]
    pub lut_amount: Option<f64>,
    /// Bildstabilisierung: Glättungsstärke 0…1 (`None` = aus); Auto-Skalierung abschaltbar
    #[serde(skip_serializing_if = "is_none")]
    pub stabilize: Option<f64>,
    #[serde(skip_serializing_if = "is_none")]
    pub stab_no_scale: Option<bool>,
    /// Maske (Form oder Verlauf) über dem Bild des Clips
    #[serde(skip_serializing_if = "is_none")]
    pub mask: Option<crate::masks::MaskSpec>,
    /// Foto-Effekte (Auto-Verbessern, Rauschen); der Hintergrund-Effekt der Mac-Version bleibt erhalten, wird hier aber nicht gerechnet
    #[serde(skip_serializing_if = "is_none")]
    pub photo: Option<PhotoFx>,
    /// Unbekannte Felder (z. B. `cutoutPick` der Mac-Version) bleiben erhalten
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Greenscreen: Schlüsselfarbe, Schwellwert (0…0,4), Überblendbereich (0…0,3) und Antispill (0…1).
/// Alle Felder werden immer geschrieben (die Mac-Version verlangt sie).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ChromaKey {
    pub color: Rgba,
    pub tolerance: f64,
    pub softness: f64,
    pub antispill: f64,
}

impl Default for ChromaKey {
    fn default() -> Self {
        Self { color: Rgba { r: 0.1, g: 0.75, b: 0.2, a: 1.0 }, tolerance: 0.12, softness: 0.08, antispill: 0.5 }
    }
}

impl Default for Effects {
    fn default() -> Self {
        Self {
            brightness: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            temperature: 0.0,
            sharpness: 0.0,
            blur: 0.0,
            vignette: 0.0,
            fill: false,
            crop_l: 0.0,
            crop_t: 0.0,
            crop_r: 0.0,
            crop_b: 0.0,
            cutout: None,
            cutout_quality: None,
            cutout_soft: None,
            cutout_shift: None,
            flip_h: None,
            flip_v: None,
            quarter_turns: None,
            horizon: None,
            chroma: None,
            lut_path: None,
            lut_amount: None,
            stabilize: None,
            stab_no_scale: None,
            mask: None,
            photo: None,
            extra: Map::new(),
        }
    }
}

/// Foto-Effekte. `bg`, `bgAmount` und `bgColor` werden immer geschrieben (die Mac-Version verlangt sie).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PhotoFx {
    /// Auto-Verbessern (Belichtung, Farbe), 0…1
    #[serde(skip_serializing_if = "is_none")]
    pub enhance: Option<f64>,
    /// Rauschentfernung bei schlechtem Licht, 0…1
    #[serde(skip_serializing_if = "is_none")]
    pub denoise: Option<f64>,
    /// Hintergrund hinter dem Motiv (Mac: Motiverkennung mit Vision): none, blur, mono, dim, color
    pub bg: String,
    pub bg_amount: f64,
    pub bg_color: Vec<f64>,
}

impl Default for PhotoFx {
    fn default() -> Self {
        PhotoFx { enhance: None, denoise: None, bg: "none".into(), bg_amount: 0.7, bg_color: vec![0.08, 0.10, 0.16] }
    }
}

impl PhotoFx {
    pub fn is_empty(&self) -> bool {
        self.enhance.unwrap_or(0.0) <= 0.001 && self.denoise.unwrap_or(0.0) <= 0.001 && self.bg == "none"
    }
}

/// Gruppen von Effekten, die einzeln auf die Standardwerte zurückgesetzt werden können (ein Abschnitt im Inspektor je Gruppe).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FxGroup {
    Color,
    Sharp,
    Lut,
    Chroma,
    Crop,
    Stab,
    Orient,
    Motion,
    PhotoEnhance,
    Mask,
}

impl Clip {
    /// Setzt die Gruppe auf die Standardwerte zurück (samt zugehöriger Keyframes).
    pub fn reset_fx(&mut self, g: FxGroup) {
        use crate::anim::Channel as Ch;
        let d = Effects::default();
        let mut clear = |chs: &[Ch]| {
            for c in chs {
                self.keys.0.remove(c);
            }
        };
        match g {
            FxGroup::Color => {
                clear(&[Ch::Brightness, Ch::Contrast, Ch::Saturation, Ch::Temperature]);
                (self.fx.brightness, self.fx.contrast, self.fx.saturation, self.fx.temperature) = (d.brightness, d.contrast, d.saturation, d.temperature);
            }
            FxGroup::Sharp => {
                clear(&[Ch::Sharpness, Ch::Blur, Ch::Vignette]);
                (self.fx.sharpness, self.fx.blur, self.fx.vignette) = (d.sharpness, d.blur, d.vignette);
            }
            FxGroup::Lut => (self.fx.lut_path, self.fx.lut_amount) = (None, None),
            FxGroup::Chroma => self.fx.chroma = None,
            FxGroup::Crop => (self.fx.fill, self.fx.crop_l, self.fx.crop_t, self.fx.crop_r, self.fx.crop_b) = (d.fill, 0.0, 0.0, 0.0, 0.0),
            FxGroup::Stab => (self.fx.stabilize, self.fx.stab_no_scale) = (None, None),
            FxGroup::Orient => (self.fx.flip_h, self.fx.flip_v, self.fx.quarter_turns, self.fx.horizon) = (None, None, None, None),
            FxGroup::Motion => {
                clear(&Ch::TRANSFORM);
                self.transform = Transform2D::default();
            }
            FxGroup::PhotoEnhance => {
                if let Some(p) = self.fx.photo.as_mut() {
                    (p.enhance, p.denoise) = (None, None);
                }
            }
            FxGroup::Mask => self.fx.mask = None,
        }
        if self.fx.photo.as_ref().is_some_and(PhotoFx::is_empty) {
            self.fx.photo = None;
        }
    }

    /// Weicht die Gruppe von den Standardwerten ab?
    pub fn is_modified(&self, g: FxGroup) -> bool {
        let mut c = self.clone();
        c.reset_fx(g);
        c != *self
    }
}

impl Effects {
    /// true, wenn kein Look (Helligkeit … Weichzeichner) eingestellt ist; Ausschnitt und „Füllen“ zählen nicht.
    pub fn is_default_look(&self) -> bool {
        let d = Effects::default();
        Effects { fill: d.fill, crop_l: d.crop_l, crop_t: d.crop_t, crop_r: d.crop_r, crop_b: d.crop_b, ..self.clone() } == d
    }

    /// Voreinstellungen („Looks“) wie in der Mac-Version.
    pub fn looks() -> Vec<(&'static str, Effects)> {
        let e = Effects::default;
        vec![
            ("Lebendig", Effects { contrast: 1.12, saturation: 1.3, sharpness: 0.4, ..e() }),
            ("Warm", Effects { saturation: 1.1, temperature: 0.45, ..e() }),
            ("Kühl", Effects { saturation: 1.05, temperature: -0.45, ..e() }),
            ("Schwarzweiß", Effects { contrast: 1.15, saturation: 0.0, ..e() }),
            ("Kino", Effects { contrast: 1.2, saturation: 0.85, temperature: 0.12, vignette: 0.45, ..e() }),
        ]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Wrap<T> {
    #[serde(rename = "_0")]
    pub v: T,
}

/// Swift kodiert Enums mit Wert als `{"media":{"_0":"<uuid>"}}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClipContent {
    Media(Wrap<Uuid>),
    Title(Wrap<TitleStyle>),
}

/// Stützpunkt der Lautstärkekurve; `t` in Sekunden ab Clipanfang
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct VolumePoint {
    pub t: f64,
    pub gain: f64,
}

/// Erkanntes oder gesetztes Taktraster der Tonquelle (Quellzeit)
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BeatGrid {
    pub period: f64,
    pub phase: f64,
    #[serde(default)]
    pub confidence: f64,
}

impl BeatGrid {
    pub fn bpm(&self) -> f64 {
        60.0 / self.period
    }

    /// Beats im Quellzeitbereich [a, b] als (Nummer, Zeit).
    pub fn beats(&self, a: f64, b: f64) -> Vec<(i64, f64)> {
        if b < a || self.period <= 0.05 {
            return vec![];
        }
        let k0 = ((a - self.phase) / self.period - 1e-9).ceil() as i64;
        let k1 = ((b - self.phase) / self.period + 1e-9).floor() as i64;
        if k1 < k0 || k1 - k0 >= 20000 {
            return vec![];
        }
        (k0..=k1).map(|k| (k, self.phase + k as f64 * self.period)).collect()
    }
}

/// Kapitelmarke (Name + Zeit)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    #[serde(default = "new_id")]
    pub id: Uuid,
    pub t: f64,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClipPart {
    #[default]
    Video,
    Audio,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Transform2D {
    pub x: f64,
    pub y: f64,
    pub scale: f64,
    pub rotation: f64,
}

impl Default for Transform2D {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0, scale: 1.0, rotation: 0.0 }
    }
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AudioFx {
    pub wind_filter: bool,
    pub denoise: f64,
    /// Rückwärts (Bild und Ton); die Mac-Version verlangt den Schlüssel immer
    pub reverse: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Clip {
    pub id: Uuid,
    pub content: ClipContent,
    pub part: ClipPart,
    #[serde(skip_serializing_if = "is_none")]
    pub link: Option<Uuid>,
    /// Freie Gruppe (G / Umschalt+G): gemeinsam wählen, verschieben und kopieren
    #[serde(skip_serializing_if = "is_none")]
    pub group: Option<Uuid>,
    pub start: f64,
    pub source_in: f64,
    pub speed: f64,
    pub duration: f64,
    /// Audio: Lautstärke (0…2). Video/Titel: Deckkraft (0…1).
    pub volume: f64,
    pub volume_points: Vec<VolumePoint>,
    pub transform: Transform2D,
    pub fx: Effects,
    #[serde(rename = "audioFX")]
    pub audio_fx: AudioFx,
    /// Rückwärts abspielen
    #[serde(skip_serializing_if = "is_none")]
    pub reversed: Option<bool>,
    #[serde(skip_serializing_if = "is_none")]
    pub beat_grid: Option<BeatGrid>,
    pub transition: String,
    /// Keyframes je Kanal (siehe `anim.rs`)
    pub keys: crate::anim::Keys,
    pub fade_in: f64,
    pub fade_out: f64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Clip {
    fn default() -> Self {
        Self {
            id: new_id(),
            content: ClipContent::Title(Wrap { v: TitleStyle::default() }),
            part: ClipPart::Video,
            link: None,
            group: None,
            start: 0.0,
            source_in: 0.0,
            speed: one(),
            duration: 3.0,
            volume: one(),
            volume_points: vec![],
            transform: Transform2D::default(),
            fx: Effects::default(),
            audio_fx: AudioFx::default(),
            reversed: None,
            beat_grid: None,
            transition: "crossfade".into(),
            keys: Default::default(),
            fade_in: 0.0,
            fade_out: 0.0,
            extra: Map::new(),
        }
    }
}

impl Clip {
    pub fn end(&self) -> f64 {
        self.start + self.duration
    }
    pub fn asset_id(&self) -> Option<Uuid> {
        match &self.content {
            ClipContent::Media(w) => Some(w.v),
            _ => None,
        }
    }
    pub fn is_title(&self) -> bool {
        matches!(self.content, ClipContent::Title(_))
    }
    pub fn is_audio(&self) -> bool {
        self.part == ClipPart::Audio && !self.is_title()
    }
    /// Faktor aus Ein-/Ausblenden bzw. Überblendung (`xin`/`xout` = Überlappung mit Nachbarclip, Sekunden).
    pub fn fade_factor(&self, t: f64, xin: f64, xout: f64) -> f64 {
        let mut f: f64 = 1.0;
        let (fi, fo) = (self.fade_in.max(xin), self.fade_out.max(xout));
        if fi > 0.0 {
            f = f.min(t / fi);
        }
        if fo > 0.0 {
            f = f.min((self.duration - t) / fo);
        }
        f.clamp(0.0, 1.0)
    }

    /// Lautstärkekurve (ohne Ein-/Ausblenden): `volumePoints` = [{t, gain}], linear interpoliert.
    pub fn curve_gain(&self, t: f64) -> f64 {
        let mut pts: Vec<(f64, f64)> = self.volume_points.iter().map(|v| (v.t, v.gain)).collect();
        if pts.is_empty() {
            return self.volume;
        }
        pts.sort_by(|a, b| a.0.total_cmp(&b.0));
        if t <= pts[0].0 {
            return pts[0].1;
        }
        for w in pts.windows(2) {
            if t <= w[1].0 {
                let f = if w[1].0 > w[0].0 { (t - w[0].0) / (w[1].0 - w[0].0) } else { 1.0 };
                return w[0].1 + (w[1].1 - w[0].1) * f;
            }
        }
        pts[pts.len() - 1].1
    }

    pub fn gain(&self, t: f64, xin: f64, xout: f64) -> f64 {
        self.curve_gain(t) * self.fade_factor(t, xin, xout)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Track {
    pub id: Uuid,
    pub name: String,
    pub muted: bool,
    #[serde(skip_serializing_if = "is_none")]
    pub solo: Option<bool>,
    pub hidden: bool,
    pub locked: bool,
    /// Spurfarbe (Name aus `TRACK_COLORS`), Lautstärke und Deckkraft der ganzen Spur (`None` = 100 %)
    #[serde(skip_serializing_if = "is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "is_none")]
    pub volume: Option<f64>,
    #[serde(skip_serializing_if = "is_none")]
    pub opacity: Option<f64>,
    pub clips: Vec<Clip>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Track {
    fn default() -> Self {
        Self {
            id: new_id(),
            name: String::new(),
            muted: false,
            solo: None,
            hidden: false,
            locked: false,
            color: None,
            volume: None,
            opacity: None,
            clips: vec![],
            extra: Map::new(),
        }
    }
}

/// Auswahl für die Spurfarbe: (Name in der Datei, Beschriftung, Farbe).
pub const TRACK_COLORS: [(&str, &str, [u8; 3]); 6] = [
    ("red", "Rot", [235, 77, 61]),
    ("orange", "Orange", [242, 153, 51]),
    ("yellow", "Gelb", [245, 204, 51]),
    ("green", "Grün", [89, 199, 97]),
    ("blue", "Blau", [64, 140, 245]),
    ("purple", "Violett", [176, 97, 222]),
];

impl Track {
    /// Vom Nutzer vergebener Name (nicht der automatische „Spur N“).
    pub fn custom_name(&self) -> Option<&str> {
        (!self.name.is_empty() && !self.name.starts_with("Spur ")).then_some(self.name.as_str())
    }
    pub fn gain(&self) -> f64 {
        self.volume.unwrap_or(1.0)
    }
    pub fn alpha(&self) -> f64 {
        self.opacity.unwrap_or(1.0)
    }
}

/// `tracks[0]` ist die oberste Zeile und liegt im Bild ganz hinten;
/// untere Spuren verdecken obere.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Project {
    pub assets: Vec<MediaAsset>,
    pub tracks: Vec<Track>,
    pub render_width: i64,
    pub render_height: i64,
    pub fps: i64,
    #[serde(skip_serializing_if = "is_none")]
    pub mark_in: Option<f64>,
    #[serde(skip_serializing_if = "is_none")]
    pub mark_out: Option<f64>,
    #[serde(skip_serializing_if = "is_none")]
    pub markers: Option<Vec<Marker>>,
    /// Projektmarker 1–10 (Schlüssel "1" … "10", Wert Zeit in Sekunden)
    #[serde(skip_serializing_if = "is_none")]
    pub bookmarks: Option<std::collections::BTreeMap<String, f64>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            assets: vec![],
            tracks: (1..=6).map(|i| Track { name: format!("Spur {i}"), ..Default::default() }).collect(),
            render_width: 1920,
            render_height: 1080,
            fps: 30,
            mark_in: None,
            mark_out: None,
            markers: None,
            bookmarks: None,
            extra: Map::new(),
        }
    }
}

impl Project {
    pub fn asset(&self, id: Uuid) -> Option<&MediaAsset> {
        self.assets.iter().find(|a| a.id == id)
    }

    pub fn duration(&self) -> f64 {
        self.tracks.iter().flat_map(|t| &t.clips).map(Clip::end).fold(0.0, f64::max)
    }

    pub fn find_clip(&self, id: Uuid) -> Option<(usize, usize)> {
        for (ti, t) in self.tracks.iter().enumerate() {
            if let Some(ci) = t.clips.iter().position(|c| c.id == id) {
                return Some((ti, ci));
            }
        }
        None
    }

    pub fn clip(&self, id: Uuid) -> Option<&Clip> {
        self.find_clip(id).map(|(t, c)| &self.tracks[t].clips[c])
    }

    pub fn clip_mut(&mut self, id: Uuid) -> Option<&mut Clip> {
        let (t, c) = self.find_clip(id)?;
        Some(&mut self.tracks[t].clips[c])
    }

    /// IDs des Clips und seiner verknüpften Partner.
    pub fn linked_ids(&self, id: Uuid) -> Vec<Uuid> {
        let link = self.clip(id).and_then(|c| c.link);
        match link {
            None => vec![id],
            Some(l) => self
                .tracks
                .iter()
                .flat_map(|t| &t.clips)
                .filter(|c| c.link == Some(l))
                .map(|c| c.id)
                .collect(),
        }
    }

    /// Überlappung des Clips mit Vorgänger (`.0`) und Nachfolger (`.1`) derselben Spur und Art (Sekunden).
    pub fn overlaps(&self, id: Uuid) -> (f64, f64) {
        let Some((ti, ci)) = self.find_clip(id) else { return (0.0, 0.0) };
        let c = &self.tracks[ti].clips[ci];
        let (mut xin, mut xout) = (0.0_f64, 0.0_f64);
        for (i, o) in self.tracks[ti].clips.iter().enumerate() {
            if i == ci || o.is_audio() != c.is_audio() {
                continue;
            }
            if o.start < c.start && o.end() > c.start {
                xin = xin.max(o.end().min(c.end()) - c.start);
            }
            if o.start > c.start && o.start < c.end() {
                xout = xout.max(c.end().min(o.end()) - o.start);
            }
        }
        (xin, xout)
    }

    pub fn remove_clips(&mut self, ids: &[Uuid]) {
        for t in &mut self.tracks {
            t.clips.retain(|c| !ids.contains(&c.id));
        }
    }

    /// Teilt den Clip (samt verknüpften Partnern) an der Zeit `t`; `true`, wenn geteilt wurde.
    pub fn split_at(&mut self, id: Uuid, t: f64) -> bool {
        self.split(id, t).is_some()
    }
}

pub fn load(path: &std::path::Path) -> Result<Project, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let data = data.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&data);
    serde_json::from_slice(data).map_err(|e| e.to_string())
}

pub fn save(p: &Project, path: &std::path::Path) -> Result<(), String> {
    let s = serde_json::to_string_pretty(p).map_err(|e| e.to_string())?;
    std::fs::write(path, s).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_swift_style() {
        let json = r#"{"assets":[],"tracks":[{"id":"6F6E1B0A-0000-4000-8000-000000000001","name":"Spur 1","muted":false,"hidden":false,"locked":false,
          "clips":[{"id":"6F6E1B0A-0000-4000-8000-000000000002","content":{"title":{"_0":{"text":"Hallo","fontSize":0.08,"color":{"r":1,"g":1,"b":1,"a":1},"background":false,"backgroundColor":{"r":0,"g":0,"b":0,"a":1},"leftAligned":false}}},
          "part":"video","start":1.5,"sourceIn":0,"speed":1,"duration":3,"volume":1,"volumePoints":[],"transform":{"x":0,"y":0,"scale":1,"rotation":0},"fx":{},"audioFX":{"windFilter":false,"denoise":0},"transition":"crossfade","keys":["posX",[{"id":"6F6E1B0A-0000-4000-8000-000000000003","t":0,"v":0,"kind":"linear","bz":[0.42,0,0.58,1]}]],"fadeIn":0,"fadeOut":0,"zzz":5}]}],
          "renderWidth":1920,"renderHeight":1080,"fps":30}"#;
        let p: Project = serde_json::from_str(json).unwrap();
        assert_eq!(p.tracks[0].clips[0].start, 1.5);
        let out = serde_json::to_string(&p).unwrap();
        let p2: Project = serde_json::from_str(&out).unwrap();
        assert_eq!(p, p2);
        assert!(out.contains("\"zzz\":5"));
        assert!(out.contains("\"audioFX\""));
    }

    fn media_clip(start: f64, dur: f64) -> Clip {
        Clip { content: ClipContent::Media(Wrap { v: Uuid::new_v4() }), start, duration: dur, ..Default::default() }
    }

    #[test]
    fn split_unlinked_clip() {
        let mut p = Project::default();
        let mut c = media_clip(2.0, 4.0);
        c.source_in = 1.0;
        c.speed = 2.0;
        let id = c.id;
        p.tracks[0].clips.push(c);
        assert!(!p.split_at(id, 2.01), "zu nah am Anfang");
        assert!(p.split_at(id, 4.0));
        let cl = &p.tracks[0].clips;
        assert_eq!(cl.len(), 2);
        assert_eq!((cl[0].start, cl[0].duration), (2.0, 2.0));
        assert_eq!((cl[1].start, cl[1].duration), (4.0, 2.0));
        assert!((cl[1].source_in - 5.0).abs() < 1e-9, "Quellzeit läuft mit Geschwindigkeit weiter");
        assert_ne!(cl[0].id, cl[1].id);
    }

    #[test]
    fn split_linked_clips_together() {
        let mut p = Project::default();
        let link = Uuid::new_v4();
        let mut v = media_clip(0.0, 10.0);
        v.link = Some(link);
        let mut a = media_clip(0.0, 10.0);
        a.part = ClipPart::Audio;
        a.link = Some(link);
        let vid = v.id;
        p.tracks[0].clips.push(v);
        p.tracks[1].clips.push(a);
        assert_eq!(p.linked_ids(vid).len(), 2);
        assert!(p.split_at(vid, 5.0));
        assert_eq!(p.tracks[0].clips.len(), 2);
        assert_eq!(p.tracks[1].clips.len(), 2);
        // neue Hälften sind untereinander verknüpft, aber nicht mit den alten
        let (l0, l1) = (p.tracks[0].clips[0].link, p.tracks[0].clips[1].link);
        assert_ne!(l0, l1);
        assert_eq!(p.tracks[1].clips[1].link, l1);
        // gemeinsames Löschen
        let ids = p.linked_ids(p.tracks[0].clips[1].id);
        p.remove_clips(&ids);
        assert_eq!((p.tracks[0].clips.len(), p.tracks[1].clips.len()), (1, 1));
    }

    #[test]
    fn overlaps_and_duration() {
        let mut p = Project::default();
        let (a, b) = (media_clip(0.0, 3.0), media_clip(2.0, 3.0));
        let (ia, ib) = (a.id, b.id);
        p.tracks[0].clips.extend([a, b]);
        assert_eq!(p.overlaps(ia), (0.0, 1.0));
        assert_eq!(p.overlaps(ib), (1.0, 0.0));
        assert_eq!(p.duration(), 5.0);
        // Ton und Bild derselben Spur beeinflussen sich nicht
        let mut au = media_clip(1.0, 3.0);
        au.part = ClipPart::Audio;
        p.tracks[0].clips.push(au);
        assert_eq!(p.overlaps(ib), (1.0, 0.0));
    }

    #[test]
    fn fade_and_volume_curve() {
        let mut c = media_clip(0.0, 10.0);
        c.fade_in = 2.0;
        c.fade_out = 4.0;
        assert_eq!(c.fade_factor(0.0, 0.0, 0.0), 0.0);
        assert!((c.fade_factor(1.0, 0.0, 0.0) - 0.5).abs() < 1e-9);
        assert_eq!(c.fade_factor(5.0, 0.0, 0.0), 1.0);
        assert!((c.fade_factor(8.0, 0.0, 0.0) - 0.5).abs() < 1e-9);
        // Überlappung dehnt das Einblenden
        assert!((c.fade_factor(1.0, 4.0, 0.0) - 0.25).abs() < 1e-9);
        c.volume_points = vec![VolumePoint { t: 0.0, gain: 0.0 }, VolumePoint { t: 10.0, gain: 2.0 }];
        assert!((c.curve_gain(5.0) - 1.0).abs() < 1e-9);
        assert_eq!(c.curve_gain(-1.0), 0.0);
        assert_eq!(c.curve_gain(99.0), 2.0);
    }

    #[test]
    fn save_and_load_file_incl_bom() {
        let mut p = Project::default();
        p.tracks[2].clips.push(media_clip(1.0, 2.0));
        let dir = std::env::temp_dir().join("video-superbe-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("roundtrip.mvd.json");
        save(&p, &f).unwrap();
        assert_eq!(load(&f).unwrap(), p);
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend(std::fs::read(&f).unwrap());
        std::fs::write(&f, bytes).unwrap();
        assert_eq!(load(&f).unwrap(), p, "UTF-8-BOM wird akzeptiert");
        assert!(load(&dir.join("gibt-es-nicht.json")).is_err());
        std::fs::write(&f, "{kaputt").unwrap();
        assert!(load(&f).is_err());
    }

    #[test]
    fn swift_enum_encoding_matches() {
        let c = media_clip(0.0, 1.0);
        let v = serde_json::to_value(&c).unwrap();
        assert!(v["content"]["media"]["_0"].is_string(), "{v}");
        assert_eq!(v["part"], "video");
        assert!(v.get("audioFX").is_some());
        let t = Clip::default();
        let v = serde_json::to_value(&t).unwrap();
        assert!(v["content"]["title"]["_0"]["text"].is_string());
    }

    /// Alle Felder der neuen Mac-Version überleben Laden und Speichern unverändert; `audioFX.reverse` steht immer drin.
    #[test]
    fn mac_fields_survive_a_roundtrip() {
        let json = r#"{"assets":[],"tracks":[{"name":"Spur 1","clips":[{"id":"6F6E1B0A-0000-4000-8000-0000000000A1","content":{"media":{"_0":"6F6E1B0A-0000-4000-8000-0000000000B1"}},"part":"video","group":"6F6E1B0A-0000-4000-8000-0000000000C1","reversed":true,"start":1,"sourceIn":0,"speed":1,"duration":3,"volume":1,"volumePoints":[],"transform":{"x":0,"y":0,"scale":1,"rotation":0},"fx":{"brightness":0,"contrast":1,"saturation":1,"temperature":0,"sharpness":0,"blur":0,"vignette":0,"fill":false,"cropL":0,"cropT":0,"cropR":0,"cropB":0,"flipH":true,"flipV":false,"quarterTurns":1,"horizon":3.5,"chroma":{"color":{"r":0.1,"g":0.75,"b":0.2,"a":1},"tolerance":0.12,"softness":0.08,"antispill":0.5},"lutPath":"C:/x/a.cube","lutAmount":0.6,"stabilize":0.5,"stabNoScale":true,"cutoutPick":{"x":0.5,"y":0.4,"t0":1.5,"track":[0.0,0.5,0.4]}},"audioFX":{"windFilter":false,"denoise":0,"reverse":true},"transition":"zoom","keys":[],"fadeIn":0,"fadeOut":0},{"id":"6F6E1B0A-0000-4000-8000-0000000000A2","content":{"title":{"_0":{"text":"T","fontSize":0.08,"color":{"r":1,"g":1,"b":1,"a":1},"background":false,"backgroundColor":{"r":0,"g":0,"b":0,"a":0.5},"leftAligned":false,"shadow":true,"animIn":"typewriter","animOut":"flyUp","animDur":0.6,"credits":true,"ticker":false,"bold":true,"italic":true,"underline":true,"rightAligned":true,"lineSpacing":0.2,"kerning":0.1,"shadowAlpha":0.5,"shadowBlur":0.2,"outlineWidth":6,"outlineHollow":true,"zukunft":1}}},"part":"video","start":0,"sourceIn":0,"speed":1,"duration":3,"volume":1,"volumePoints":[],"transform":{"x":0,"y":0,"scale":1,"rotation":0},"fx":{},"audioFX":{"windFilter":false,"denoise":0,"reverse":false},"transition":"crossfade","keys":[],"fadeIn":0,"fadeOut":0}]}],"renderWidth":1920,"renderHeight":1080,"fps":30,"bookmarks":{"1":2.5,"10":9}}"#;
        let p: Project = serde_json::from_str(json).unwrap();
        let c = &p.tracks[0].clips[0];
        assert!(c.group.is_some() && c.reversed == Some(true) && c.audio_fx.reverse);
        assert_eq!(c.fx.horizon, Some(3.5));
        assert_eq!(c.fx.chroma.as_ref().unwrap().tolerance, 0.12);
        assert_eq!(p.bookmarks.as_ref().unwrap()["10"], 9.0);
        let out: Value = serde_json::to_value(&p).unwrap();
        let c0 = &out["tracks"][0]["clips"][0];
        let fx = &c0["fx"];
        assert_eq!((fx["flipH"].as_bool(), fx["flipV"].as_bool(), fx["quarterTurns"].as_i64()), (Some(true), Some(false), Some(1)));
        assert_eq!(fx["horizon"].as_f64(), Some(3.5));
        assert_eq!(fx["lutAmount"].as_f64(), Some(0.6));
        assert_eq!(fx["stabilize"].as_f64(), Some(0.5));
        assert_eq!(fx["stabNoScale"].as_bool(), Some(true));
        assert_eq!(fx["chroma"]["antispill"].as_f64(), Some(0.5), "Greenscreen vollständig");
        assert_eq!(fx["cutoutPick"]["t0"].as_f64(), Some(1.5), "unbekanntes Mac-Feld bleibt erhalten");
        assert_eq!(c0["reversed"].as_bool(), Some(true));
        assert!(c0["group"].is_string());
        assert_eq!(c0["audioFX"]["reverse"].as_bool(), Some(true));
        assert_eq!(c0["transition"], "zoom");
        assert_eq!(out["bookmarks"]["1"].as_f64(), Some(2.5));
        let t = &out["tracks"][0]["clips"][1]["content"]["title"]["_0"];
        for k in ["shadow", "credits", "bold", "italic", "underline", "rightAligned", "outlineHollow"] {
            assert_eq!(t[k].as_bool(), Some(true), "{k}");
        }
        assert_eq!(t["ticker"].as_bool(), Some(false));
        assert_eq!((t["animIn"].as_str(), t["animOut"].as_str()), (Some("typewriter"), Some("flyUp")));
        assert_eq!((t["shadowAlpha"].as_f64(), t["shadowBlur"].as_f64(), t["outlineWidth"].as_f64(), t["lineSpacing"].as_f64(), t["kerning"].as_f64()), (Some(0.5), Some(0.2), Some(6.0), Some(0.2), Some(0.1)));
        assert_eq!(t["zukunft"].as_i64(), Some(1), "unbekannte Titelfelder bleiben erhalten");
        // zweites Laden ergibt dasselbe Modell
        let p2: Project = serde_json::from_value(out).unwrap();
        assert_eq!(p2, p);
        // Ein neu angelegter Clip schreibt `reverse` immer (sonst scheitert die Mac-Version mit keyNotFound)
        let v = serde_json::to_value(&Clip::default()).unwrap();
        assert_eq!(v["audioFX"]["reverse"], false);
    }

    /// Felder der Mac-Version 1.2: Masken, Foto-KI, KI-Verbesserung, Spurfarbe/-lautstärke/-deckkraft.
    #[test]
    fn mac_12_fields_roundtrip() {
        let json = r#"{"assets":[],"tracks":[{"name":"Musik","muted":false,"hidden":false,"locked":false,"color":"green","volume":0.5,"opacity":0.75,"zukunft":true,
          "clips":[{"id":"6F6E1B0A-0000-4000-8000-0000000000A1","content":{"media":{"_0":"6F6E1B0A-0000-4000-8000-0000000000B1"}},"part":"video","start":0,"sourceIn":0,"speed":1,"duration":3,"volume":1,"volumePoints":[],"transform":{"x":0,"y":0,"scale":1,"rotation":0},
          "fx":{"mask":{"kind":"heart","cx":0.1,"cy":-0.1,"w":0.9,"h":0.8,"rot":12,"feather":0.2,"invert":true,"angle":0,"pos":0.5},"photo":{"enhance":0.6,"bg":"blur","bgAmount":0.7,"bgColor":[0.1,0.2,0.3]},"enhance":{"denoise":0.5,"from":0,"to":3},"enhanceOff":true},
          "audioFX":{"windFilter":false,"denoise":0,"reverse":false},"transition":"crossfade","keys":[],"fadeIn":0,"fadeOut":0}]}],"renderWidth":1920,"renderHeight":1080,"fps":30}"#;
        let p: Project = serde_json::from_str(json).unwrap();
        let t = &p.tracks[0];
        assert_eq!((t.color.as_deref(), t.volume, t.opacity, t.custom_name()), (Some("green"), Some(0.5), Some(0.75), Some("Musik")));
        let fx = &t.clips[0].fx;
        let m = fx.mask.as_ref().unwrap();
        assert_eq!((m.kind, m.rot, m.invert), (crate::masks::MaskKind::Heart, 12.0, true));
        let ph = fx.photo.as_ref().unwrap();
        assert_eq!((ph.enhance, ph.denoise, ph.bg.as_str()), (Some(0.6), None, "blur"));
        let out: Value = serde_json::to_value(&p).unwrap();
        let o = &out["tracks"][0];
        assert_eq!((o["color"].as_str(), o["volume"].as_f64(), o["opacity"].as_f64(), o["zukunft"].as_bool()), (Some("green"), Some(0.5), Some(0.75), Some(true)));
        let f = &o["clips"][0]["fx"];
        assert_eq!(f["mask"]["kind"], "heart");
        assert_eq!(f["photo"]["bgAmount"].as_f64(), Some(0.7));
        assert_eq!(f["enhance"]["denoise"].as_f64(), Some(0.5), "KI-Verbesserung (nur Mac) bleibt erhalten");
        assert_eq!(f["enhanceOff"].as_bool(), Some(true));
        let p2: Project = serde_json::from_value(out).unwrap();
        assert_eq!(p2, p);
        // Spuren ohne Angaben schreiben die neuen Felder nicht
        let v = serde_json::to_value(Track::default()).unwrap();
        assert!(v.get("color").is_none() && v.get("volume").is_none() && v.get("opacity").is_none());
        assert_eq!(Track { name: "Spur 3".into(), ..Default::default() }.custom_name(), None);
        // Foto-Effekte schreiben immer bg, bgAmount, bgColor (die Mac-Version verlangt sie)
        let v = serde_json::to_value(PhotoFx { enhance: Some(0.5), ..Default::default() }).unwrap();
        assert_eq!((v["bg"].as_str(), v["bgAmount"].as_f64(), v["bgColor"].as_array().map(|a| a.len())), (Some("none"), Some(0.7), Some(3)));
    }

    #[test]
    fn reset_fx_groups_clear_values_and_keys() {
        use crate::anim::Channel;
        let mut c = media_clip(0.0, 4.0);
        c.fx.brightness = 0.3;
        c.fx.vignette = 0.5;
        c.keys.0.insert(Channel::Brightness, vec![crate::anim::Keyframe::new(0.0, 0.1), crate::anim::Keyframe::new(2.0, 0.4)]);
        c.transform.scale = 1.5;
        c.keys.0.insert(Channel::Scale, vec![crate::anim::Keyframe::new(0.0, 1.0)]);
        c.fx.mask = Some(crate::masks::MaskKind::Star.defaults());
        c.fx.photo = Some(PhotoFx { enhance: Some(0.5), ..Default::default() });
        c.fx.flip_h = Some(true);
        for g in [FxGroup::Color, FxGroup::Sharp, FxGroup::Motion, FxGroup::Mask, FxGroup::PhotoEnhance, FxGroup::Orient] {
            assert!(c.is_modified(g), "{g:?}");
        }
        assert!(!c.is_modified(FxGroup::Lut) && !c.is_modified(FxGroup::Chroma) && !c.is_modified(FxGroup::Crop));
        c.reset_fx(FxGroup::Color);
        assert_eq!(c.fx.brightness, 0.0);
        assert!(!c.keys.0.contains_key(&Channel::Brightness));
        assert_eq!(c.fx.vignette, 0.5, "andere Gruppen bleiben");
        c.reset_fx(FxGroup::Motion);
        assert_eq!(c.transform, Transform2D::default());
        assert!(!c.keys.0.contains_key(&Channel::Scale));
        c.reset_fx(FxGroup::PhotoEnhance);
        assert!(c.fx.photo.is_none(), "leere Foto-Effekte entfallen");
        c.reset_fx(FxGroup::Mask);
        assert!(c.fx.mask.is_none() && c.fx.flip_h == Some(true));
    }
}

