//! Keyframe-Animation (Animation.swift): Kanäle, Stützstellen mit Bézier-Interpolation, Voreinstellungen.

use crate::model::{Clip, Transform2D};
use serde::de::{self, Deserializer, SeqAccess, Visitor};
use serde::ser::{SerializeSeq, Serializer};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Channel {
    PosX,
    PosY,
    Scale,
    Rotation,
    Brightness,
    Contrast,
    Saturation,
    Temperature,
    Sharpness,
    Blur,
    Vignette,
}

impl Channel {
    pub const ALL: [Channel; 11] = [
        Channel::PosX,
        Channel::PosY,
        Channel::Scale,
        Channel::Rotation,
        Channel::Brightness,
        Channel::Contrast,
        Channel::Saturation,
        Channel::Temperature,
        Channel::Sharpness,
        Channel::Blur,
        Channel::Vignette,
    ];
    pub const TRANSFORM: [Channel; 4] = [Channel::PosX, Channel::PosY, Channel::Scale, Channel::Rotation];
    pub const FX: [Channel; 7] = [Channel::Brightness, Channel::Contrast, Channel::Saturation, Channel::Temperature, Channel::Sharpness, Channel::Blur, Channel::Vignette];

    pub fn is_transform(self) -> bool {
        Self::TRANSFORM.contains(&self)
    }

    pub fn label(self) -> &'static str {
        match self {
            Channel::PosX => "Position X",
            Channel::PosY => "Position Y",
            Channel::Scale => "Skalierung",
            Channel::Rotation => "Drehung",
            Channel::Brightness => "Helligkeit",
            Channel::Contrast => "Kontrast",
            Channel::Saturation => "Sättigung",
            Channel::Temperature => "Farbtemperatur",
            Channel::Sharpness => "Schärfe",
            Channel::Blur => "Weichzeichner",
            Channel::Vignette => "Vignette",
        }
    }

    pub fn range(self) -> (f64, f64) {
        match self {
            Channel::PosX | Channel::PosY => (-1.5, 1.5),
            Channel::Scale => (0.05, 4.0),
            Channel::Rotation => (-360.0, 360.0),
            Channel::Brightness | Channel::Temperature => (-1.0, 1.0),
            Channel::Contrast => (0.5, 2.0),
            Channel::Saturation | Channel::Sharpness => (0.0, 2.0),
            Channel::Blur => (0.0, 30.0),
            Channel::Vignette => (0.0, 1.0),
        }
    }

    pub fn display(self, v: f64) -> String {
        match self {
            Channel::PosX | Channel::PosY => format!("{:.1} %", v * 100.0),
            Channel::Scale => format!("{:.0} %", v * 100.0),
            Channel::Rotation => format!("{v:.1}°"),
            Channel::Blur => format!("{v:.0} px"),
            _ => format!("{:.0} %", v * 100.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InterpKind {
    Linear,
    Hold,
    EaseIn,
    EaseOut,
    EaseInOut,
    Bezier,
}

impl InterpKind {
    pub const ALL: [InterpKind; 6] = [InterpKind::Linear, InterpKind::Hold, InterpKind::EaseIn, InterpKind::EaseOut, InterpKind::EaseInOut, InterpKind::Bezier];
    pub fn label(self) -> &'static str {
        match self {
            InterpKind::Linear => "Linear",
            InterpKind::Hold => "Halten",
            InterpKind::EaseIn => "Ease In",
            InterpKind::EaseOut => "Ease Out",
            InterpKind::EaseInOut => "Ease In/Out",
            InterpKind::Bezier => "Bézier",
        }
    }
}

fn default_bz() -> [f64; 4] {
    [0.42, 0.0, 0.58, 1.0]
}
fn default_kind() -> InterpKind {
    InterpKind::EaseInOut
}
fn new_id() -> Uuid {
    Uuid::new_v4()
}

/// Stützstelle; die Interpolation gilt für das Segment bis zur nächsten Stützstelle.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Keyframe {
    #[serde(default = "new_id")]
    pub id: Uuid,
    /// Sekunden ab Clipanfang
    pub t: f64,
    pub v: f64,
    #[serde(default = "default_kind")]
    pub kind: InterpKind,
    /// Kontrollpunkte (x1, y1, x2, y2) der kubischen Bézierkurve wie bei CSS `cubic-bezier`
    #[serde(default = "default_bz")]
    pub bz: [f64; 4],
}

impl Keyframe {
    pub fn new(t: f64, v: f64) -> Self {
        Keyframe { id: Uuid::new_v4(), t, v, kind: InterpKind::EaseInOut, bz: default_bz() }
    }
    pub fn with_kind(t: f64, v: f64, kind: InterpKind) -> Self {
        Keyframe { kind, ..Keyframe::new(t, v) }
    }
    pub fn controls(&self) -> [f64; 4] {
        match self.kind {
            InterpKind::Linear | InterpKind::Hold => [0.0, 0.0, 1.0, 1.0],
            InterpKind::EaseIn => [0.42, 0.0, 1.0, 1.0],
            InterpKind::EaseOut => [0.0, 0.0, 0.58, 1.0],
            InterpKind::EaseInOut => [0.42, 0.0, 0.58, 1.0],
            InterpKind::Bezier => self.bz,
        }
    }
}

/// Wert der Bézierkurve (0,0)-(x1,y1)-(x2,y2)-(1,1) an der Zeitposition `u` (0…1).
pub fn bezier(u: f64, c: [f64; 4]) -> f64 {
    if u <= 0.0 {
        return 0.0;
    }
    if u >= 1.0 {
        return 1.0;
    }
    let (x1, y1, x2, y2) = (c[0], c[1], c[2], c[3]);
    let (mut lo, mut hi, mut s) = (0.0, 1.0, u);
    for _ in 0..40 {
        let x = 3.0 * (1.0 - s) * (1.0 - s) * s * x1 + 3.0 * (1.0 - s) * s * s * x2 + s * s * s;
        if (x - u).abs() < 1e-7 {
            break;
        }
        if x < u {
            lo = s;
        } else {
            hi = s;
        }
        s = (lo + hi) / 2.0;
    }
    3.0 * (1.0 - s) * (1.0 - s) * s * y1 + 3.0 * (1.0 - s) * s * s * y2 + s * s * s
}

/// Keyframes je Kanal. Swift kodiert `[Channel: [Keyframe]]` als flaches Array `[Kanal, [Keyframes], Kanal, …]`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Keys(pub BTreeMap<Channel, Vec<Keyframe>>);

impl Keys {
    pub fn get(&self, c: Channel) -> Option<&Vec<Keyframe>> {
        self.0.get(&c)
    }
    pub fn is_empty(&self) -> bool {
        self.0.values().all(|v| v.is_empty())
    }
}

impl Serialize for Keys {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let items: Vec<_> = self.0.iter().filter(|(_, v)| !v.is_empty()).collect();
        let mut seq = s.serialize_seq(Some(items.len() * 2))?;
        for (c, v) in items {
            seq.serialize_element(c)?;
            seq.serialize_element(v)?;
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for Keys {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Keys;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("Keyframes als Array [Kanal, [..], …] oder Objekt")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Keys, A::Error> {
                let mut m = BTreeMap::new();
                while let Some(c) = a.next_element::<Channel>()? {
                    let v: Vec<Keyframe> = a.next_element()?.ok_or_else(|| de::Error::custom("Keyframes fehlen"))?;
                    m.insert(c, v);
                }
                Ok(Keys(m))
            }
            fn visit_map<A: de::MapAccess<'de>>(self, mut a: A) -> Result<Keys, A::Error> {
                let mut m = BTreeMap::new();
                while let Some((c, v)) = a.next_entry::<Channel, Vec<Keyframe>>()? {
                    m.insert(c, v);
                }
                Ok(Keys(m))
            }
        }
        d.deserialize_any(V)
    }
}

// ---------------------------------------------------------------------------------------------
// Clip-Methoden
// ---------------------------------------------------------------------------------------------

impl Transform2D {
    pub fn get(&self, c: Channel) -> f64 {
        match c {
            Channel::PosX => self.x,
            Channel::PosY => self.y,
            Channel::Scale => self.scale,
            Channel::Rotation => self.rotation,
            _ => 0.0,
        }
    }
    pub fn set(&mut self, c: Channel, v: f64) {
        match c {
            Channel::PosX => self.x = v,
            Channel::PosY => self.y = v,
            Channel::Scale => self.scale = v,
            Channel::Rotation => self.rotation = v,
            _ => {}
        }
    }
}

impl crate::model::Effects {
    pub fn get(&self, c: Channel) -> Option<f64> {
        Some(match c {
            Channel::Brightness => self.brightness,
            Channel::Contrast => self.contrast,
            Channel::Saturation => self.saturation,
            Channel::Temperature => self.temperature,
            Channel::Sharpness => self.sharpness,
            Channel::Blur => self.blur,
            Channel::Vignette => self.vignette,
            _ => return None,
        })
    }
    pub fn set(&mut self, c: Channel, v: f64) -> bool {
        match c {
            Channel::Brightness => self.brightness = v,
            Channel::Contrast => self.contrast = v,
            Channel::Saturation => self.saturation = v,
            Channel::Temperature => self.temperature = v,
            Channel::Sharpness => self.sharpness = v,
            Channel::Blur => self.blur = v,
            Channel::Vignette => self.vignette = v,
            _ => return false,
        }
        true
    }
}

const SAME_KEY: f64 = 0.02;

impl Clip {
    pub fn is_animated(&self) -> bool {
        self.keys.0.values().any(|v| !v.is_empty())
    }

    pub fn is_keyed(&self, c: Channel) -> bool {
        self.keys.get(c).is_some_and(|v| !v.is_empty())
    }

    /// Fester Wert eines Kanals (ohne Keyframes).
    pub fn static_value(&self, c: Channel) -> f64 {
        self.fx.get(c).unwrap_or_else(|| self.transform.get(c))
    }

    pub fn set_static(&mut self, c: Channel, v: f64) {
        if !self.fx.set(c, v) {
            self.transform.set(c, v);
        }
    }

    /// Wert des Kanals zur Zeit `t` (Sekunden ab Clipanfang).
    pub fn value(&self, c: Channel, t: f64) -> f64 {
        let Some(ks) = self.keys.get(c).filter(|k| !k.is_empty()) else { return self.static_value(c) };
        let mut ks: Vec<&Keyframe> = ks.iter().collect();
        ks.sort_by(|a, b| a.t.total_cmp(&b.t));
        let (first, last) = (ks[0], ks[ks.len() - 1]);
        if t <= first.t {
            return first.v;
        }
        if t >= last.t {
            return last.v;
        }
        for w in ks.windows(2) {
            let (a, b) = (w[0], w[1]);
            if t <= b.t {
                if a.kind == InterpKind::Hold {
                    return a.v;
                }
                let u = if b.t > a.t { (t - a.t) / (b.t - a.t) } else { 1.0 };
                return a.v + (b.v - a.v) * bezier(u, a.controls());
            }
        }
        last.v
    }

    /// Bildeffekte zur Zeit `t` (animierte Kanäle ausgewertet).
    pub fn fx_at(&self, t: f64) -> crate::model::Effects {
        let mut f = self.fx.clone();
        for c in Channel::FX {
            if self.is_keyed(c) {
                f.set(c, self.value(c, t));
            }
        }
        f
    }

    pub fn transform_at(&self, t: f64) -> Transform2D {
        let mut r = Transform2D::default();
        for c in Channel::TRANSFORM {
            r.set(c, self.value(c, t));
        }
        r
    }

    fn new_key(&self, c: Channel, t: f64, v: f64) -> Keyframe {
        let mut k = Keyframe::new(t, v);
        if let Some(ks) = self.keys.get(c) {
            if let Some(r) = ks.iter().rev().find(|k| k.t <= t).or(ks.first()) {
                k.kind = r.kind;
                k.bz = r.bz;
            }
        }
        k
    }

    /// Setzt den Wert: hat der Kanal Keyframes, wird bei `t` einer angelegt/aktualisiert, sonst der feste Wert.
    pub fn set_channel(&mut self, c: Channel, v: f64, t: f64) {
        if !self.is_keyed(c) {
            self.set_static(c, v);
            return;
        }
        let time = t.clamp(0.0, self.duration);
        let mut ks = self.keys.0.remove(&c).unwrap_or_default();
        match ks.iter().position(|k| (k.t - time).abs() < SAME_KEY) {
            Some(i) => ks[i].v = v,
            None => {
                self.keys.0.insert(c, ks.clone());
                ks.push(self.new_key(c, time, v));
            }
        }
        ks.sort_by(|a, b| a.t.total_cmp(&b.t));
        self.keys.0.insert(c, ks);
    }

    pub fn toggle_key(&mut self, c: Channel, t: f64) {
        let time = t.clamp(0.0, self.duration);
        let v = self.value(c, time);
        let mut ks = self.keys.0.get(&c).cloned().unwrap_or_default();
        if let Some(i) = ks.iter().position(|k| (k.t - time).abs() < SAME_KEY) {
            ks.remove(i);
            if ks.is_empty() {
                self.set_static(c, v); // Wert behalten
            }
        } else {
            ks.push(self.new_key(c, time, v));
        }
        ks.sort_by(|a, b| a.t.total_cmp(&b.t));
        if ks.is_empty() {
            self.keys.0.remove(&c);
        } else {
            self.keys.0.insert(c, ks);
        }
    }

    /// Setzt die Interpolation aller Keyframes der Kanäle (Spur-Symbol im Animator).
    pub fn set_interpolation(&mut self, channels: &[Channel], kind: InterpKind) {
        for c in channels {
            if let Some(ks) = self.keys.0.get_mut(c) {
                for k in ks {
                    if kind == InterpKind::Bezier && k.kind != InterpKind::Bezier {
                        k.bz = k.controls();
                    }
                    k.kind = kind;
                }
            }
        }
    }

    /// Legt (falls nicht vorhanden) einen Keyframe mit dem aktuellen Wert bei `t` an.
    pub fn add_key(&mut self, c: Channel, t: f64) {
        let time = t.clamp(0.0, self.duration);
        if self.keys.get(c).is_some_and(|ks| ks.iter().any(|k| (k.t - time).abs() < SAME_KEY)) {
            return;
        }
        let k = self.new_key(c, time, self.value(c, time));
        let ks = self.keys.0.entry(c).or_default();
        ks.push(k);
        ks.sort_by(|a, b| a.t.total_cmp(&b.t));
    }

    /// Entfernt Keyframes bei `t` (der feste Wert bleibt der aktuelle).
    pub fn remove_key(&mut self, c: Channel, t: f64) {
        let v = self.value(c, t);
        let Some(ks) = self.keys.0.get_mut(&c) else { return };
        let Some(i) = ks.iter().position(|k| (k.t - t).abs() < SAME_KEY) else { return };
        ks.remove(i);
        if ks.is_empty() {
            self.keys.0.remove(&c);
            self.set_static(c, v);
        }
    }

    /// Animation ein-/ausschalten (Stoppuhr): an = Keyframe bei `t`, aus = alle Keyframes weg, Wert bleibt.
    pub fn set_animated(&mut self, c: Channel, on: bool, t: f64) {
        let time = t.clamp(0.0, self.duration);
        if on {
            if !self.is_keyed(c) {
                self.keys.0.insert(c, vec![Keyframe::new(time, self.static_value(c))]);
            }
        } else {
            let v = self.value(c, time);
            self.set_static(c, v);
            self.keys.0.remove(&c);
        }
    }

    /// Ausschnitt der Keyframes für [a, b] (Clipzeit), verschoben auf 0; an den Rändern werden Randwerte ergänzt.
    pub fn keys_slice(&self, a: f64, b: f64) -> Keys {
        let mut out = BTreeMap::new();
        for (&c, ks) in &self.keys.0 {
            if ks.is_empty() {
                continue;
            }
            let mut inner: Vec<Keyframe> = ks.iter().filter(|k| k.t >= a - 1e-9 && k.t <= b + 1e-9).cloned().collect();
            let before = ks.iter().any(|k| k.t < a - 1e-9);
            let after = ks.iter().any(|k| k.t > b + 1e-9);
            if before && !inner.first().is_some_and(|k| (k.t - a).abs() < 1e-9) {
                let mut k = Keyframe::new(a, self.value(c, a));
                if let Some(r) = ks.iter().rev().find(|k| k.t < a) {
                    k.kind = r.kind;
                    k.bz = r.bz;
                }
                inner.insert(0, k);
            }
            if after && !inner.last().is_some_and(|k| (k.t - b).abs() < 1e-9) {
                inner.push(Keyframe::new(b, self.value(c, b)));
            }
            for k in &mut inner {
                k.t -= a;
            }
            if !inner.is_empty() {
                out.insert(c, inner);
            }
        }
        Keys(out)
    }
}

/// Fertige Bewegungen (Keyframes), die auf den gewählten Clip angewendet werden.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimPreset {
    SlideInLeft,
    SlideInBottom,
    PopIn,
    ZoomIn,
    ZoomOut,
    PanRight,
    PanLeft,
}

impl AnimPreset {
    pub const ALL: [AnimPreset; 7] = [AnimPreset::SlideInLeft, AnimPreset::SlideInBottom, AnimPreset::PopIn, AnimPreset::ZoomIn, AnimPreset::ZoomOut, AnimPreset::PanRight, AnimPreset::PanLeft];
    pub fn label(self) -> &'static str {
        match self {
            AnimPreset::SlideInLeft => "Einschieben von links",
            AnimPreset::SlideInBottom => "Einschieben von unten",
            AnimPreset::PopIn => "Aufpoppen",
            AnimPreset::ZoomIn => "Langsam hineinzoomen (Ken Burns)",
            AnimPreset::ZoomOut => "Langsam herauszoomen",
            AnimPreset::PanRight => "Schwenk nach rechts",
            AnimPreset::PanLeft => "Schwenk nach links",
        }
    }
}

impl Clip {
    /// Ersetzt die Keyframes der betroffenen Kanäle; Ausgangswerte sind die festen Werte des Clips.
    pub fn apply_preset(&mut self, p: AnimPreset) {
        use InterpKind::*;
        let (x0, y0, s0) = (self.transform.x, self.transform.y, self.transform.scale);
        let d = self.duration;
        let intro = (0.6f64).min(d / 2.0);
        let k = |t: f64, v: f64, kind: InterpKind| Keyframe::with_kind(t, v, kind);
        match p {
            AnimPreset::SlideInLeft => {
                self.keys.0.insert(Channel::PosX, vec![k(0.0, x0 - 0.25, EaseOut), k(intro, x0, EaseInOut)]);
            }
            AnimPreset::SlideInBottom => {
                self.keys.0.insert(Channel::PosY, vec![k(0.0, y0 + 0.2, EaseOut), k(intro, y0, EaseInOut)]);
            }
            AnimPreset::PopIn => {
                self.keys.0.insert(Channel::Scale, vec![k(0.0, s0 * 0.78, EaseOut), k(intro * 0.6, s0 * 1.06, EaseInOut), k(intro, s0, EaseInOut)]);
            }
            AnimPreset::ZoomIn => {
                self.keys.0.insert(Channel::Scale, vec![k(0.0, s0, Linear), k(d, s0 * 1.2, EaseInOut)]);
            }
            AnimPreset::ZoomOut => {
                self.keys.0.insert(Channel::Scale, vec![k(0.0, s0 * 1.2, Linear), k(d, s0, EaseInOut)]);
            }
            AnimPreset::PanRight => {
                self.transform.scale = s0 * 1.12;
                self.keys.0.remove(&Channel::Scale);
                self.keys.0.insert(Channel::PosX, vec![k(0.0, x0 - 0.04, Linear), k(d, x0 + 0.04, EaseInOut)]);
            }
            AnimPreset::PanLeft => {
                self.transform.scale = s0 * 1.12;
                self.keys.0.remove(&Channel::Scale);
                self.keys.0.insert(Channel::PosX, vec![k(0.0, x0 + 0.04, Linear), k(d, x0 - 0.04, EaseInOut)]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn clip_with(c: Channel, ks: Vec<Keyframe>) -> Clip {
        let mut cl = Clip { duration: 10.0, ..Default::default() };
        cl.keys.0.insert(c, ks);
        cl
    }

    #[test]
    fn bezier_endpoints_and_symmetry() {
        let lin = [0.0, 0.0, 1.0, 1.0];
        for u in [0.0, 0.25, 0.5, 0.9, 1.0] {
            assert!((bezier(u, lin) - u).abs() < 1e-4, "linear u={u}");
        }
        let ease = [0.42, 0.0, 0.58, 1.0];
        assert!((bezier(0.5, ease) - 0.5).abs() < 1e-4, "ease-in-out ist punktsymmetrisch");
        assert!(bezier(0.2, ease) < 0.2 && bezier(0.8, ease) > 0.8);
        assert_eq!((bezier(-1.0, ease), bezier(2.0, ease)), (0.0, 1.0));
    }

    #[test]
    fn value_interpolates_clamps_and_holds() {
        let c = clip_with(Channel::PosX, vec![Keyframe::with_kind(1.0, 0.0, InterpKind::Linear), Keyframe::with_kind(3.0, 1.0, InterpKind::Linear)]);
        assert_eq!(c.value(Channel::PosX, 0.0), 0.0);
        assert!((c.value(Channel::PosX, 2.0) - 0.5).abs() < 1e-6);
        assert_eq!(c.value(Channel::PosX, 9.0), 1.0);
        let h = clip_with(Channel::Scale, vec![Keyframe::with_kind(0.0, 1.0, InterpKind::Hold), Keyframe::with_kind(2.0, 2.0, InterpKind::Hold)]);
        assert_eq!(h.value(Channel::Scale, 1.9), 1.0, "Halten behält den Wert bis zur nächsten Stützstelle");
        assert_eq!(h.value(Channel::Scale, 2.0), 2.0);
        // ohne Keyframes gilt der feste Wert
        let mut plain = Clip::default();
        plain.transform.y = 0.3;
        assert_eq!(plain.value(Channel::PosY, 5.0), 0.3);
        assert!(!plain.is_animated());
    }

    #[test]
    fn transform_and_fx_use_keys() {
        let mut c = clip_with(Channel::Brightness, vec![Keyframe::with_kind(0.0, 0.0, InterpKind::Linear), Keyframe::with_kind(10.0, 1.0, InterpKind::Linear)]);
        c.keys.0.insert(Channel::Rotation, vec![Keyframe::with_kind(0.0, 0.0, InterpKind::Linear), Keyframe::with_kind(10.0, 90.0, InterpKind::Linear)]);
        assert!((c.fx_at(5.0).brightness - 0.5).abs() < 1e-6);
        assert_eq!(c.fx_at(5.0).contrast, 1.0);
        assert!((c.transform_at(5.0).rotation - 45.0).abs() < 1e-6);
        assert_eq!(c.transform_at(5.0).scale, 1.0);
    }

    #[test]
    fn toggle_set_channel_and_remove() {
        let mut c = Clip { duration: 10.0, ..Default::default() };
        c.set_channel(Channel::PosX, 0.4, 3.0);
        assert_eq!(c.transform.x, 0.4, "ohne Keyframes: fester Wert");
        c.toggle_key(Channel::PosX, 2.0);
        assert!(c.is_keyed(Channel::PosX));
        assert_eq!(c.keys.get(Channel::PosX).unwrap()[0].v, 0.4);
        c.set_channel(Channel::PosX, 0.9, 2.0);
        assert_eq!(c.keys.get(Channel::PosX).unwrap().len(), 1);
        assert_eq!(c.value(Channel::PosX, 5.0), 0.9);
        c.set_channel(Channel::PosX, 0.1, 6.0);
        assert_eq!(c.keys.get(Channel::PosX).unwrap().len(), 2);
        assert!((c.value(Channel::PosX, 4.0) - 0.5).abs() < 0.5);
        c.remove_key(Channel::PosX, 2.0);
        c.toggle_key(Channel::PosX, 6.0);
        assert!(!c.is_keyed(Channel::PosX), "letzter Keyframe weg → wieder fester Wert");
        assert_eq!(c.transform.x, 0.1, "der Wert bleibt erhalten");
    }

    #[test]
    fn animated_switch_and_interpolation() {
        let mut c = Clip { duration: 4.0, ..Default::default() };
        c.transform.scale = 1.5;
        c.set_animated(Channel::Scale, true, 1.0);
        assert_eq!(c.keys.get(Channel::Scale).unwrap(), &vec![Keyframe { id: c.keys.get(Channel::Scale).unwrap()[0].id, ..Keyframe::new(1.0, 1.5) }]);
        c.add_key(Channel::Scale, 3.0);
        c.set_interpolation(&[Channel::Scale], InterpKind::Bezier);
        assert!(c.keys.get(Channel::Scale).unwrap().iter().all(|k| k.kind == InterpKind::Bezier && k.bz == [0.42, 0.0, 0.58, 1.0]));
        c.set_animated(Channel::Scale, false, 2.0);
        assert!(!c.is_keyed(Channel::Scale));
        assert_eq!(c.transform.scale, 1.5);
    }

    #[test]
    fn keys_slice_adds_boundary_values() {
        let c = clip_with(Channel::PosX, vec![Keyframe::with_kind(0.0, 0.0, InterpKind::Linear), Keyframe::with_kind(10.0, 1.0, InterpKind::Linear)]);
        let left = c.keys_slice(0.0, 4.0);
        let ks = left.get(Channel::PosX).unwrap();
        assert_eq!((ks.len(), ks[0].t, ks[1].t), (2, 0.0, 4.0));
        assert!((ks[1].v - 0.4).abs() < 1e-6);
        let right = c.keys_slice(4.0, 10.0);
        let ks = right.get(Channel::PosX).unwrap();
        assert!((ks[0].v - 0.4).abs() < 1e-6 && ks[0].t == 0.0 && (ks[1].t - 6.0).abs() < 1e-9 && ks[1].v == 1.0);
    }

    #[test]
    fn presets_create_expected_keys() {
        let mut c = Clip { duration: 5.0, ..Default::default() };
        c.apply_preset(AnimPreset::ZoomIn);
        assert_eq!(c.value(Channel::Scale, 0.0), 1.0);
        assert!((c.value(Channel::Scale, 5.0) - 1.2).abs() < 1e-9);
        c.apply_preset(AnimPreset::PanRight);
        assert!((c.transform.scale - 1.12).abs() < 1e-9 && !c.is_keyed(Channel::Scale));
        assert!(c.value(Channel::PosX, 0.0) < 0.0 && c.value(Channel::PosX, 5.0) > 0.0);
    }

    #[test]
    fn keys_serialize_like_swift_and_accept_objects() {
        let c = clip_with(Channel::PosX, vec![Keyframe::new(1.0, 0.5)]);
        let v = serde_json::to_value(&c.keys).unwrap();
        assert_eq!(v[0], "posX");
        assert_eq!(v[1][0]["t"], 1.0);
        let back: Keys = serde_json::from_value(v).unwrap();
        assert_eq!(back, c.keys);
        let obj: Keys = serde_json::from_str(r#"{"scale":[{"t":0,"v":1,"kind":"linear"}]}"#).unwrap();
        assert_eq!(obj.get(Channel::Scale).unwrap()[0].kind, InterpKind::Linear);
        assert_eq!(obj.get(Channel::Scale).unwrap()[0].bz, [0.42, 0.0, 0.58, 1.0]);
        // ganzes Projekt behält die Keyframes
        let mut p = Project::default();
        p.tracks[0].clips.push(c);
        let s = serde_json::to_string(&p).unwrap();
        let q: Project = serde_json::from_str(&s).unwrap();
        assert_eq!(p, q);
    }
}
