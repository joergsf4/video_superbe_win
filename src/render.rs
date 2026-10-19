//! Software-Kompositor: rendert ein Projektbild zu einer Zeit als RGBA. Gleicher Code für vorschau und Export.
//! Bilder, Videos, Titel, Position/Skalierung/Drehung (auch animiert), Beschnitt, Deckkraft, Überblendung,
//! Übergänge (Überblenden, Schwarzblende, Wischen, Schieben) und die Effekte Helligkeit, Kontrast, Sättigung,
//! Farbtemperatur, Vignette, Schärfe und Weichzeichner.

use crate::model::*;
use crate::video::{Decoder, Frame};
use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

pub struct Img {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl Img {
    /// Halbe Größe (2×2-Mittelung) für die Verkleinerung großer Bilder ohne Treppeneffekte.
    fn half(&self) -> Img {
        let (w, h) = ((self.w / 2).max(1), (self.h / 2).max(1));
        let mut px = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let (x0, y0) = ((x * 2).min(self.w - 1), (y * 2).min(self.h - 1));
                let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
                for c in 0..4 {
                    let s = self.px[(y0 * self.w + x0) * 4 + c] as u32
                        + self.px[(y0 * self.w + x1) * 4 + c] as u32
                        + self.px[(y1 * self.w + x0) * 4 + c] as u32
                        + self.px[(y1 * self.w + x1) * 4 + c] as u32;
                    px[(y * w + x) * 4 + c] = ((s + 2) / 4) as u8;
                }
            }
        }
        Img { w, h, px }
    }
}

/// Ein decodiertes Videobild (RGBA, bereits gedreht)
pub type MediaFrame = Arc<Frame>;

/// Quelle der Videobilder (Vorschau: Hintergrund-Decoder, Export und Tests: blockierend).
pub trait FrameSource {
    /// Videobild des Assets zur Quellzeit `t` (Sekunden).
    fn frame(&mut self, asset: &MediaAsset, t: f64) -> Option<MediaFrame>;
}

/// Blockierendes Decodieren in voller Auflösung (Export, Tests).
#[derive(Default)]
pub struct BlockingFrames {
    decoders: HashMap<Uuid, Option<Decoder>>,
}

impl FrameSource for BlockingFrames {
    fn frame(&mut self, asset: &MediaAsset, t: f64) -> Option<Arc<Frame>> {
        let dec = self.decoders.entry(asset.id).or_insert_with(|| {
            let mut d = Decoder::open_checked(Path::new(&asset.path)).ok()?;
            d.step = 1;
            d.last = None;
            Some(d)
        });
        let f = dec.as_mut()?.frame_at(t)?;
        Some(Arc::new(Frame { width: f.width, height: f.height, rgba: f.rgba.clone(), time: f.time }))
    }
}

pub struct Renderer {
    /// Bilder in Verkleinerungsstufen (Stufe 0 = Original)
    images: HashMap<Uuid, Option<Vec<Arc<Img>>>>,
    titles: HashMap<String, Arc<Img>>,
    regular: Option<FontVec>,
    bold: Option<FontVec>,
    /// weitere Schriftfamilien (normal, fett), bei Bedarf geladen
    faces: HashMap<String, (FontVec, FontVec)>,
    blocking: BlockingFrames,
}

/// Auswahl im Titel-Inspektor: Familienname, Datei normal, Datei fett (Windows-Schriften im Ordner Fonts).
const FONT_FAMILIES: &[(&str, &str, &str)] = &[
    ("Segoe UI", "segoeui.ttf", "segoeuib.ttf"),
    ("Arial", "arial.ttf", "arialbd.ttf"),
    ("Arial Black", "ariblk.ttf", "ariblk.ttf"),
    ("Calibri", "calibri.ttf", "calibrib.ttf"),
    ("Georgia", "georgia.ttf", "georgiab.ttf"),
    ("Impact", "impact.ttf", "impact.ttf"),
    ("Verdana", "verdana.ttf", "verdanab.ttf"),
    ("Tahoma", "tahoma.ttf", "tahomabd.ttf"),
    ("Trebuchet MS", "trebuc.ttf", "trebucbd.ttf"),
    ("Times New Roman", "times.ttf", "timesbd.ttf"),
    ("Courier New", "cour.ttf", "courbd.ttf"),
    ("Consolas", "consola.ttf", "consolab.ttf"),
    ("Comic Sans MS", "comic.ttf", "comicbd.ttf"),
    ("Segoe Script", "segoesc.ttf", "segoescb.ttf"),
];

/// Schriftnamen aus Mac-Projekten auf passende Windows-Schriften abbilden; `None` = Standardschrift.
fn resolve_family(name: &str) -> Option<&'static str> {
    let n = match name {
        "System" | "Segoe UI" | "" => return None,
        "Helvetica Neue" | "Helvetica" => "Arial",
        "Menlo" | "Monaco" => "Consolas",
        "Avenir Next" | "Gill Sans" | "Optima" | "Futura" => "Segoe UI",
        "Marker Felt" | "Chalkboard SE" | "Bradley Hand" | "Zapfino" => "Segoe Script",
        other => other,
    };
    FONT_FAMILIES.iter().find(|f| f.0 == n).map(|f| f.0).filter(|f| *f != "Segoe UI")
}

/// Schriftfamilien, die auf diesem Rechner installiert sind (für die Auswahl); „System“ steht immer vorn.
pub fn installed_fonts() -> Vec<&'static str> {
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    let mut v = vec!["System"];
    v.extend(FONT_FAMILIES.iter().filter(|f| f.0 != "Segoe UI" && std::path::Path::new(&format!("{windir}\\Fonts\\{}", f.1)).exists()).map(|f| f.0));
    v
}

fn load_font(file: &str) -> Option<FontVec> {
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    FontVec::try_from_vec(std::fs::read(format!("{windir}\\Fonts\\{file}")).ok()?).ok()
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

/// Beschreibung eines zu zeichnenden Quellbildes mit allen Parametern.
struct Draw<'a> {
    src: &'a Img,
    /// Titel: Originalgröße statt Einpassen
    native: bool,
    tr: Transform2D,
    fx: Effects,
    alpha: f32,
    /// Verschiebung in Zielpixeln (Übergang „Schieben“)
    dx: f32,
    /// Sichtbar nur links von dieser x-Position (Übergang „Wischen“)
    wipe_x: Option<f32>,
    /// Ziel ist deckend (Hauptbild) statt durchsichtig (Ebene für Weichzeichner)
    opaque: bool,
}

fn transition_kind(clip: &Clip) -> &str {
    clip.transition.as_str()
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            images: HashMap::new(),
            titles: HashMap::new(),
            regular: load_font("segoeui.ttf"),
            bold: load_font("segoeuib.ttf"),
            faces: HashMap::new(),
            blocking: BlockingFrames::default(),
        }
    }

    /// Größe der Titelgrafik (Pixel) bei der Bildhöhe `canvas_h` (zwischengespeichert).
    pub fn title_size(&mut self, s: &TitleStyle, canvas_h: usize) -> Option<(usize, usize)> {
        let key = format!("size|{canvas_h}|{s:?}");
        if let Some(i) = self.titles.get(&key) {
            return Some((i.w, i.h));
        }
        let img = self.render_title(s, canvas_h)?;
        let r = (img.w, img.h);
        self.titles.insert(key, Arc::new(Img { w: img.w, h: img.h, px: vec![] }));
        Some(r)
    }

    pub fn has_fonts(&self) -> bool {
        self.regular.is_some()
    }

    /// RGBA, deckend (schwarzer Hintergrund); Videobilder werden blockierend decodiert.
    pub fn render(&mut self, p: &Project, t: f64, w: usize, h: usize) -> Vec<u8> {
        let mut frames = std::mem::take(&mut self.blocking);
        let r = self.render_with(p, t, w, h, &mut frames);
        self.blocking = frames;
        r
    }

    pub fn render_with(&mut self, p: &Project, t: f64, w: usize, h: usize, frames: &mut dyn FrameSource) -> Vec<u8> {
        let mut dst = vec![0u8; w * h * 4];
        for px in dst.chunks_exact_mut(4) {
            px[3] = 255;
        }
        let px_scale = w as f32 / p.render_width.max(1) as f32;
        for tr in &p.tracks {
            if tr.hidden {
                continue;
            }
            for c in &tr.clips {
                if c.is_audio() || t < c.start || t >= c.end() {
                    continue;
                }
                let rel = t - c.start;
                let (xin, xout) = p.overlaps(c.id);
                let in_kind = transition_kind(c).to_string();
                // Übergangsart des Nachfolgers (für den auslaufenden Clip)
                let out_kind = if xout > 0.0 {
                    tr.clips.iter().filter(|o| o.id != c.id && o.is_audio() == c.is_audio() && o.start > c.start && o.start < c.end()).min_by(|a, b| a.start.total_cmp(&b.start)).map(|o| transition_kind(o).to_string())
                } else {
                    None
                };
                // Deckkraft = Pegelkurve × Ein-/Ausblenden × Überblendung; der auslaufende Clip einer Überblendung bleibt deckend
                let fi = c.fade_in.max(if in_kind == "crossfade" { xin } else { 0.0 });
                let fo = if xout > 0.0 { 0.0 } else { c.fade_out };
                let mut fade: f64 = 1.0;
                if fi > 0.0 {
                    fade = fade.min(rel / fi);
                }
                if fo > 0.0 {
                    fade = fade.min((c.duration - rel) / fo);
                }
                let mut alpha = (c.curve_gain(rel) * fade.clamp(0.0, 1.0)).clamp(0.0, 1.0) as f32;
                if alpha <= 0.0 {
                    continue;
                }
                // Übergänge
                let (mut dx, mut wipe_x) = (0.0f32, None);
                if xin > 0.0 && in_kind != "crossfade" {
                    let p_in = (rel / xin).clamp(0.0, 1.0) as f32;
                    match in_kind.as_str() {
                        "dip" => alpha *= ((p_in - 0.5) * 2.0).max(0.0),
                        "wipe" => wipe_x = Some(w as f32 * p_in),
                        "slide" => dx += (1.0 - p_in) * w as f32,
                        _ => {}
                    }
                }
                if let Some(k) = out_kind.as_deref().filter(|k| *k != "crossfade") {
                    let p_out = ((rel - (c.duration - xout)) / xout).clamp(0.0, 1.0) as f32;
                    match k {
                        "dip" => alpha *= (1.0 - 2.0 * p_out).max(0.0),
                        "slide" => dx -= p_out * w as f32,
                        _ => {}
                    }
                }
                if alpha <= 0.0 {
                    continue;
                }
                let fx = if c.is_title() { Effects::default() } else { c.fx_at(rel) };
                let tr_now = c.transform_at(rel);
                let (src, native): (Arc<Img>, bool) = match &c.content {
                    ClipContent::Title(s) => {
                        let key = format!("{w}x{h}|{:?}", s.v);
                        if !self.titles.contains_key(&key) {
                            if let Some(img) = self.render_title(&s.v, h) {
                                self.titles.insert(key.clone(), Arc::new(img));
                            }
                        }
                        match self.titles.get(&key) {
                            Some(i) => (i.clone(), true),
                            None => continue,
                        }
                    }
                    ClipContent::Media(m) => {
                        let Some(asset) = p.asset(m.v) else { continue };
                        if asset.is_image {
                            let levels = self.images.entry(m.v).or_insert_with(|| load_image(&asset.path).map(|i| vec![Arc::new(i)]));
                            let Some(levels) = levels else { continue };
                            let k = fit_scale(levels[0].w, levels[0].h, &fx, w, h, false, tr_now.scale as f32);
                            (pick_level(levels, k), false)
                        } else {
                            let Some(f) = frames.frame(asset, c.source_in + rel * c.speed) else { continue };
                            let img = Img { w: f.width, h: f.height, px: f.rgba.clone() };
                            (Arc::new(img), false)
                        }
                    }
                };
                let needs_layer = fx.blur > 0.0 || fx.sharpness > 0.0;
                let mk = |opaque: bool| Draw { src: &src, native, tr: tr_now.clone(), fx: fx.clone(), alpha: if opaque { alpha } else { 1.0 }, dx, wipe_x, opaque };
                if needs_layer {
                    let mut layer = vec![0u8; w * h * 4];
                    blit(&mut layer, w, h, &mk(false));
                    if fx.sharpness > 0.0 {
                        sharpen_layer(&mut layer, w, h, fx.sharpness as f32);
                    }
                    if fx.blur > 0.0 {
                        blur_layer(&mut layer, w, h, fx.blur as f32 * px_scale);
                    }
                    for (d, s) in dst.chunks_exact_mut(4).zip(layer.chunks_exact(4)) {
                        if s[3] > 0 {
                            over(d, [s[0], s[1], s[2], s[3]], alpha);
                            d[3] = 255;
                        }
                    }
                } else {
                    blit(&mut dst, w, h, &mk(true));
                }
            }
        }
        dst
    }

    fn render_title(&mut self, s: &TitleStyle, canvas_h: usize) -> Option<Img> {
        let family = s.font.as_deref().and_then(resolve_family);
        if let Some(f) = family {
            if !self.faces.contains_key(f) {
                if let Some((_, a, b)) = FONT_FAMILIES.iter().find(|x| x.0 == f) {
                    if let (Some(r), Some(bd)) = (load_font(a), load_font(b)) {
                        self.faces.insert(f.to_string(), (r, bd));
                    }
                }
            }
        }
        let (reg, bold) = match family.and_then(|f| self.faces.get(f)) {
            Some((r, b)) => (r, b),
            None => {
                let reg = self.regular.as_ref()?;
                (reg, self.bold.as_ref().unwrap_or(reg))
            }
        };
        let fs = (canvas_h as f64 * s.font_size).max(4.0) as f32;
        let lines: Vec<&str> = s.text.split('\n').collect();
        struct L<'a> {
            text: &'a str,
            size: f32,
            font: &'a FontVec,
            w: f32,
            asc: f32,
            lh: f32,
        }
        let ls: Vec<L> = lines
            .iter()
            .enumerate()
            .map(|(i, text)| {
                let size = if i == 0 { fs } else { fs * 0.6 };
                let font = if i == 0 { bold } else { reg };
                let sf = font.as_scaled(PxScale::from(size));
                let mut w = 0.0;
                let mut prev = None;
                for ch in text.chars() {
                    let g = sf.glyph_id(ch);
                    if let Some(pg) = prev {
                        w += sf.kern(pg, g);
                    }
                    w += sf.h_advance(g);
                    prev = Some(g);
                }
                L { text, size, font, w, asc: sf.ascent(), lh: sf.height() + sf.line_gap() }
            })
            .collect();
        let pad = if s.background { fs * 0.5 } else if s.outline == Some(true) { fs * 0.25 } else { fs * 0.2 };
        let tw = ls.iter().map(|l| l.w).fold(0.0, f32::max);
        let th: f32 = ls.iter().map(|l| l.lh).sum();
        let (iw, ih) = ((tw + 2.0 * pad).ceil().max(1.0) as usize, (th + 2.0 * pad).ceil().max(1.0) as usize);
        let mut img = Img { w: iw, h: ih, px: vec![0; iw * ih * 4] };
        if s.background {
            let b = s.background_color;
            let col = [(b.r * 255.0) as u8, (b.g * 255.0) as u8, (b.b * 255.0) as u8, (b.a * 255.0) as u8];
            let rad = fs * 0.2;
            for y in 0..ih {
                for x in 0..iw {
                    let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                    let dx = (rad - fx).max(fx - (iw as f32 - rad)).max(0.0);
                    let dy = (rad - fy).max(fy - (ih as f32 - rad)).max(0.0);
                    if dx * dx + dy * dy <= rad * rad {
                        let o = (y * iw + x) * 4;
                        img.px[o..o + 4].copy_from_slice(&col);
                    }
                }
            }
        }
        let c = s.color;
        let main = [(c.r * 255.0) as u8, (c.g * 255.0) as u8, (c.b * 255.0) as u8, (c.a * 255.0) as u8];
        // Kontur: schwarze Kopien rund um die Buchstaben; sonst ein weicher Schatten (nur ohne Hintergrund)
        let passes: Vec<([u8; 4], f32, f32)> = if s.outline == Some(true) {
            let r = (fs * 0.05).max(1.0);
            let mut v: Vec<([u8; 4], f32, f32)> = (0..12).map(|k| { let a = k as f32 * std::f32::consts::TAU / 12.0; ([0, 0, 0, 255], r * a.cos(), r * a.sin()) }).collect();
            v.push((main, 0.0, 0.0));
            v
        } else if s.background {
            vec![(main, 0.0, 0.0)]
        } else {
            vec![([0, 0, 0, 150], fs * 0.04, fs * 0.04), (main, 0.0, 0.0)]
        };
        for (col, offx, offy) in passes {
            let mut y = pad;
            for l in &ls {
                let x0 = if s.left_aligned { pad } else { (iw as f32 - l.w) / 2.0 };
                let sf = l.font.as_scaled(PxScale::from(l.size));
                let mut x = x0;
                let mut prev = None;
                for ch in l.text.chars() {
                    let gid = sf.glyph_id(ch);
                    if let Some(pg) = prev {
                        x += sf.kern(pg, gid);
                    }
                    let glyph = gid.with_scale_and_position(PxScale::from(l.size), ab_glyph::point(x + offx, y + l.asc + offy));
                    if let Some(og) = l.font.outline_glyph(glyph) {
                        let b = og.px_bounds();
                        og.draw(|gx, gy, cov| {
                            let (px, py) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
                            if px >= 0 && py >= 0 && (px as usize) < iw && (py as usize) < ih {
                                let o = (py as usize * iw + px as usize) * 4;
                                over(&mut img.px[o..o + 4], col, cov);
                            }
                        });
                    }
                    x += sf.h_advance(gid);
                    prev = Some(gid);
                }
                y += l.lh;
            }
        }
        Some(img)
    }
}

fn load_image(path: &str) -> Option<Img> {
    let i = image::open(path).ok()?.to_rgba8();
    Some(Img { w: i.width() as usize, h: i.height() as usize, px: i.into_raw() })
}

/// Wählt die Verkleinerungsstufe, deren Auflösung zur Darstellungsgröße passt (k = Zielpixel je Quellpixel).
fn pick_level(levels: &mut Vec<Arc<Img>>, k: f32) -> Arc<Img> {
    let (mut l, mut kk) = (0usize, k);
    while kk < 0.5 && l < 6 && levels[0].w >> (l + 1) >= 2 && levels[0].h >> (l + 1) >= 2 {
        kk *= 2.0;
        l += 1;
    }
    while levels.len() <= l {
        let n = levels.last().unwrap().half();
        levels.push(Arc::new(n));
    }
    levels[l].clone()
}

/// Zielpixel je Quellpixel (Einpassen oder Füllen, mal Skalierung).
fn fit_scale(sw: usize, sh: usize, fx: &Effects, dw: usize, dh: usize, native: bool, scale: f32) -> f32 {
    if native {
        return scale;
    }
    let cw = (sw as f32 * (1.0 - fx.crop_l as f32 - fx.crop_r as f32)).max(1.0);
    let ch = (sh as f32 * (1.0 - fx.crop_t as f32 - fx.crop_b as f32)).max(1.0);
    let (a, b) = (dw as f32 / cw, dh as f32 / ch);
    (if fx.fill { a.max(b) } else { a.min(b) }) * scale
}

/// "Over" für Straight-Alpha-Pixel (dst RGBA, src RGBA, zusätzlicher Faktor).
fn over(dst: &mut [u8], src: [u8; 4], k: f32) {
    let sa = src[3] as f32 / 255.0 * k;
    if sa <= 0.0 {
        return;
    }
    let da = dst[3] as f32 / 255.0;
    let oa = sa + da * (1.0 - sa);
    for i in 0..3 {
        let c = (src[i] as f32 * sa + dst[i] as f32 * da * (1.0 - sa)) / oa;
        dst[i] = c.clamp(0.0, 255.0) as u8;
    }
    dst[3] = (oa * 255.0).round().clamp(0.0, 255.0) as u8;
}

/// Zeichnet ein Quellbild mit Transformation, Effekten und Übergangsparametern auf `dst`.
fn blit(dst: &mut [u8], dw: usize, dh: usize, d: &Draw) {
    let (sw, sh) = (d.src.w, d.src.h);
    if sw == 0 || sh == 0 || d.src.px.len() < sw * sh * 4 {
        return;
    }
    let fx = &d.fx;
    let (cl, ct, cr, cb) = (fx.crop_l as f32, fx.crop_t as f32, fx.crop_r as f32, fx.crop_b as f32);
    let cw = sw as f32 * (1.0 - cl - cr);
    let ch = sh as f32 * (1.0 - ct - cb);
    if cw < 1.0 || ch < 1.0 {
        return;
    }
    let k = fit_scale(sw, sh, fx, dw, dh, d.native, d.tr.scale as f32);
    if k <= 0.0 {
        return;
    }
    let center = (dw as f32 / 2.0 + d.tr.x as f32 * dw as f32 + d.dx, dh as f32 / 2.0 + d.tr.y as f32 * dh as f32);
    let ang = (d.tr.rotation as f32).to_radians();
    let (sn, cs) = ang.sin_cos();
    let (hw, hh) = (cw * k / 2.0, ch * k / 2.0);
    let ex = hw * cs.abs() + hh * sn.abs();
    let ey = hw * sn.abs() + hh * cs.abs();
    let x0 = ((center.0 - ex).floor().max(0.0)) as usize;
    let x1 = ((center.0 + ex).ceil().min(dw as f32)) as usize;
    let y0 = ((center.1 - ey).floor().max(0.0)) as usize;
    let y1 = ((center.1 + ey).ceil().min(dh as f32)) as usize;
    let fxn = !fx.is_neutral();
    let diag = (hw * hw + hh * hh).sqrt().max(1.0);
    for y in y0..y1 {
        for x in x0..x1 {
            if let Some(wx) = d.wipe_x {
                if x as f32 + 0.5 > wx {
                    continue;
                }
            }
            let (rx, ry) = (x as f32 + 0.5 - center.0, y as f32 + 0.5 - center.1);
            let (ux, uy) = (rx * cs + ry * sn, -rx * sn + ry * cs);
            if ux.abs() > hw || uy.abs() > hh {
                continue;
            }
            let sx = cl * sw as f32 + (ux + hw) / k;
            let sy = ct * sh as f32 + (uy + hh) / k;
            let mut s = sample(&d.src.px, sw, sh, sx - 0.5, sy - 0.5);
            if fxn {
                apply_fx(&mut s, fx, ((ux * ux + uy * uy).sqrt()) / diag);
            }
            let o = (y * dw + x) * 4;
            over(&mut dst[o..o + 4], s, d.alpha);
            if d.opaque {
                dst[o + 3] = 255;
            }
        }
    }
}

fn sample(src: &[u8], w: usize, h: usize, x: f32, y: f32) -> [u8; 4] {
    let (xf, yf) = (x.floor(), y.floor());
    let (tx, ty) = (x - xf, y - yf);
    let cl = |v: f32, m: usize| (v.max(0.0) as usize).min(m - 1);
    let (x0, x1, y0, y1) = (cl(xf, w), cl(xf + 1.0, w), cl(yf, h), cl(yf + 1.0, h));
    let px = |xx: usize, yy: usize| &src[(yy * w + xx) * 4..(yy * w + xx) * 4 + 4];
    let (a, b, c, d) = (px(x0, y0), px(x1, y0), px(x0, y1), px(x1, y1));
    let mut o = [0u8; 4];
    for i in 0..4 {
        let top = a[i] as f32 * (1.0 - tx) + b[i] as f32 * tx;
        let bot = c[i] as f32 * (1.0 - tx) + d[i] as f32 * tx;
        o[i] = (top * (1.0 - ty) + bot * ty + 0.5) as u8;
    }
    o
}

fn apply_fx(p: &mut [u8; 4], fx: &Effects, vign_d: f32) {
    let mut c = [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0];
    let gray = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    for v in &mut c {
        *v = gray + (*v - gray) * fx.saturation as f32;
        // wie CIColorControls: Helligkeit wirkt mit dem halben Wert, Kontrast um die Mitte
        *v = (*v - 0.5) * fx.contrast as f32 + 0.5 + fx.brightness as f32 * 0.5;
    }
    c[0] += 0.1 * fx.temperature as f32;
    c[2] -= 0.1 * fx.temperature as f32;
    if fx.vignette > 0.0 {
        let f = 1.0 - fx.vignette as f32 * vign_d * vign_d;
        for v in &mut c {
            *v *= f.max(0.0);
        }
    }
    for i in 0..3 {
        p[i] = (c[i].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    }
}

impl Effects {
    /// true, wenn keine der pixelweisen Effekte (ohne Schärfe/Weichzeichner) aktiv ist.
    pub fn is_neutral(&self) -> bool {
        self.brightness == 0.0 && self.contrast == 1.0 && self.saturation == 1.0 && self.temperature == 0.0 && self.vignette == 0.0
    }
}

/// Radien dreier Kastenfilter, die zusammen einen Gaußfilter mit Sigma `sigma` annähern.
fn box_radii(sigma: f32) -> [usize; 3] {
    let n = 3.0f32;
    let ideal = ((12.0 * sigma * sigma / n) + 1.0).sqrt();
    let mut wl = ideal.floor() as i32;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wu = wl + 2;
    let m = ((12.0 * sigma * sigma - n * (wl * wl) as f32 - 4.0 * n * wl as f32 - 3.0 * n) / (-4.0 * wl as f32 - 4.0)).round() as i32;
    let mut r = [0usize; 3];
    for (i, v) in r.iter_mut().enumerate() {
        let s = if (i as i32) < m { wl } else { wu };
        *v = ((s - 1) / 2).max(0) as usize;
    }
    r
}

fn box_pass(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: usize, horizontal: bool) {
    let (len, lines, stride_i, stride_l) = if horizontal { (w, h, 4, w * 4) } else { (h, w, w * 4, 4) };
    let norm = 1.0 / (2 * r + 1) as f32;
    for l in 0..lines {
        let base = l * stride_l;
        for c in 0..4 {
            let at = |i: isize| -> f32 {
                let j = i.clamp(0, len as isize - 1) as usize;
                src[base + j * stride_i + c]
            };
            let mut acc = 0.0;
            for k in -(r as isize)..=(r as isize) {
                acc += at(k);
            }
            for i in 0..len {
                dst[base + i * stride_i + c] = acc * norm;
                acc += at(i as isize + r as isize + 1) - at(i as isize - r as isize);
            }
        }
    }
}

/// Weichzeichner (Gauß, durch dreifachen Kastenfilter angenähert) auf einer Ebene mit Alpha.
fn blur_layer(buf: &mut [u8], w: usize, h: usize, sigma: f32) {
    if sigma < 0.3 || w == 0 || h == 0 {
        return;
    }
    // vormultipliziert rechnen, damit Ränder nicht dunkel auslaufen
    let mut a: Vec<f32> = Vec::with_capacity(buf.len());
    for p in buf.chunks_exact(4) {
        let al = p[3] as f32 / 255.0;
        a.extend([p[0] as f32 * al, p[1] as f32 * al, p[2] as f32 * al, p[3] as f32]);
    }
    let mut b = vec![0f32; a.len()];
    for r in box_radii(sigma) {
        if r == 0 {
            continue;
        }
        box_pass(&a, &mut b, w, h, r, true);
        box_pass(&b, &mut a, w, h, r, false);
    }
    for (p, q) in buf.chunks_exact_mut(4).zip(a.chunks_exact(4)) {
        let al = q[3] / 255.0;
        if al > 0.0 {
            p[0] = (q[0] / al).clamp(0.0, 255.0) as u8;
            p[1] = (q[1] / al).clamp(0.0, 255.0) as u8;
            p[2] = (q[2] / al).clamp(0.0, 255.0) as u8;
        }
        p[3] = q[3].clamp(0.0, 255.0) as u8;
    }
}

/// Schärfen (Unscharfmaske auf der Leuchtdichte) einer Ebene.
fn sharpen_layer(buf: &mut [u8], w: usize, h: usize, amount: f32) {
    let mut a: Vec<f32> = Vec::with_capacity(buf.len());
    for p in buf.chunks_exact(4) {
        let al = p[3] as f32 / 255.0;
        a.extend([p[0] as f32 * al, p[1] as f32 * al, p[2] as f32 * al, p[3] as f32]);
    }
    let mut b = vec![0f32; a.len()];
    let mut c = vec![0f32; a.len()];
    box_pass(&a, &mut b, w, h, 2, true);
    box_pass(&b, &mut c, w, h, 2, false);
    for (i, p) in buf.chunks_exact_mut(4).enumerate() {
        if p[3] == 0 {
            continue;
        }
        let al = p[3] as f32 / 255.0;
        let lum_now = 0.2126 * a[i * 4] + 0.7152 * a[i * 4 + 1] + 0.0722 * a[i * 4 + 2];
        let lum_blur = 0.2126 * c[i * 4] + 0.7152 * c[i * 4 + 1] + 0.0722 * c[i * 4 + 2];
        let d = (lum_now - lum_blur) * amount * 0.8;
        for ch in 0..3 {
            let v = (a[i * 4 + ch] + d) / al.max(1e-3);
            p[ch] = v.clamp(0.0, 255.0) as u8;
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn solid_png(name: &str, w: u32, h: u32, rgb: [u8; 3]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("video-superbe-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        image::RgbaImage::from_pixel(w, h, image::Rgba([rgb[0], rgb[1], rgb[2], 255])).save(&path).unwrap();
        path
    }

    pub fn image_asset(p: &mut Project, path: &std::path::Path, w: f64, h: f64) -> Uuid {
        let a = MediaAsset {
            path: path.to_string_lossy().into_owned(),
            name: "img".into(),
            duration: 5.0,
            has_video: true,
            is_image: true,
            width: w,
            height: h,
            ..Default::default()
        };
        let id = a.id;
        p.assets.push(a);
        id
    }

    pub fn clip(aid: Uuid, start: f64, dur: f64) -> Clip {
        Clip { content: ClipContent::Media(Wrap { v: aid }), start, duration: dur, ..Default::default() }
    }

    fn px(buf: &[u8], w: usize, x: usize, y: usize) -> [u8; 3] {
        let o = (y * w + x) * 4;
        [buf[o], buf[o + 1], buf[o + 2]]
    }

    fn near(a: [u8; 3], b: [u8; 3], tol: i32) -> bool {
        (0..3).all(|i| (a[i] as i32 - b[i] as i32).abs() <= tol)
    }

    #[test]
    fn empty_project_is_black() {
        let mut r = Renderer::new();
        let buf = r.render(&Project::default(), 0.0, 8, 8);
        assert!(buf.chunks(4).all(|p| p == [0, 0, 0, 255]));
    }

    #[test]
    fn image_fills_canvas_and_letterboxes() {
        let mut p = Project::default();
        let red = solid_png("red_16x9.png", 32, 18, [255, 0, 0]);
        let aid = image_asset(&mut p, &red, 32.0, 18.0);
        p.tracks[0].clips.push(clip(aid, 0.0, 2.0));
        let mut r = Renderer::new();
        let buf = r.render(&p, 1.0, 64, 36);
        assert_eq!(px(&buf, 64, 32, 18), [255, 0, 0]);
        assert_eq!(px(&buf, 64, 1, 1), [255, 0, 0]);
        // außerhalb der Clipzeit: schwarz
        let buf = r.render(&p, 2.5, 64, 36);
        assert_eq!(px(&buf, 64, 32, 18), [0, 0, 0]);
        // Hochformatbild in Querformat-Canvas: Seitenränder schwarz
        let mut p2 = Project::default();
        let blue = solid_png("blue_tall.png", 18, 32, [0, 0, 255]);
        let aid = image_asset(&mut p2, &blue, 18.0, 32.0);
        p2.tracks[0].clips.push(clip(aid, 0.0, 2.0));
        let buf = r.render(&p2, 0.5, 64, 36);
        assert_eq!(px(&buf, 64, 32, 18), [0, 0, 255]);
        assert_eq!(px(&buf, 64, 2, 18), [0, 0, 0]);
        // „fill“ füllt das ganze Bild
        p2.tracks[0].clips[0].fx.fill = true;
        let buf = r.render(&p2, 0.5, 64, 36);
        assert_eq!(px(&buf, 64, 2, 18), [0, 0, 255]);
    }

    #[test]
    fn lower_tracks_cover_upper_tracks() {
        let mut p = Project::default();
        let red = solid_png("red_s.png", 16, 16, [255, 0, 0]);
        let green = solid_png("green_s.png", 16, 16, [0, 255, 0]);
        let ra = image_asset(&mut p, &red, 16.0, 16.0);
        let ga = image_asset(&mut p, &green, 16.0, 16.0);
        p.tracks[0].clips.push(clip(ra, 0.0, 2.0));
        p.tracks[1].clips.push(clip(ga, 0.0, 2.0));
        let mut r = Renderer::new();
        let buf = r.render(&p, 1.0, 16, 16);
        assert_eq!(px(&buf, 16, 8, 8), [0, 255, 0], "untere Spur (Index 1) verdeckt obere");
        // verdeckte Spur ausblenden → obere sichtbar
        p.tracks[1].hidden = true;
        let buf = r.render(&p, 1.0, 16, 16);
        assert_eq!(px(&buf, 16, 8, 8), [255, 0, 0]);
    }

    #[test]
    fn crossfade_by_overlap_blends() {
        let mut p = Project::default();
        let red = solid_png("red_x.png", 16, 16, [255, 0, 0]);
        let blue = solid_png("blue_x.png", 16, 16, [0, 0, 255]);
        let ra = image_asset(&mut p, &red, 16.0, 16.0);
        let ba = image_asset(&mut p, &blue, 16.0, 16.0);
        p.tracks[0].clips.push(clip(ra, 0.0, 2.0));
        p.tracks[0].clips.push(clip(ba, 1.0, 2.0)); // Überlappung 1…2 s
        let mut r = Renderer::new();
        assert_eq!(px(&r.render(&p, 0.5, 16, 16), 16, 8, 8), [255, 0, 0]);
        assert!(near(px(&r.render(&p, 1.5, 16, 16), 16, 8, 8), [128, 0, 127], 3));
        assert!(near(px(&r.render(&p, 1.99, 16, 16), 16, 8, 8), [0, 0, 255], 5));
        assert_eq!(px(&r.render(&p, 2.5, 16, 16), 16, 8, 8), [0, 0, 255]);
    }

    #[test]
    fn opacity_and_fade_in() {
        let mut p = Project::default();
        let white = solid_png("white.png", 16, 16, [255, 255, 255]);
        let a = image_asset(&mut p, &white, 16.0, 16.0);
        let mut c = clip(a, 0.0, 4.0);
        c.volume = 0.5;
        p.tracks[0].clips.push(c);
        let mut r = Renderer::new();
        assert!(near(px(&r.render(&p, 1.0, 16, 16), 16, 8, 8), [128, 128, 128], 2));
        p.tracks[0].clips[0].volume = 1.0;
        p.tracks[0].clips[0].fade_in = 2.0;
        assert!(near(px(&r.render(&p, 1.0, 16, 16), 16, 8, 8), [128, 128, 128], 2));
        assert_eq!(px(&r.render(&p, 3.0, 16, 16), 16, 8, 8), [255, 255, 255]);
    }

    #[test]
    fn transform_scale_position_rotation_crop() {
        let mut p = Project::default();
        let white = solid_png("white_t.png", 32, 32, [255, 255, 255]);
        let a = image_asset(&mut p, &white, 32.0, 32.0);
        let mut c = clip(a, 0.0, 2.0);
        c.transform.scale = 0.5;
        p.tracks[0].clips.push(c);
        let mut r = Renderer::new();
        let buf = r.render(&p, 0.5, 64, 64);
        assert_eq!(px(&buf, 64, 32, 32), [255, 255, 255]);
        assert_eq!(px(&buf, 64, 10, 10), [0, 0, 0], "bei 50 % bleibt der Rand schwarz");
        assert_eq!(px(&buf, 64, 20, 32), [255, 255, 255]);
        // Position: nach rechts verschoben um 25 % der Breite
        p.tracks[0].clips[0].transform.x = 0.25;
        let buf = r.render(&p, 0.5, 64, 64);
        assert_eq!(px(&buf, 64, 48, 32), [255, 255, 255]);
        assert_eq!(px(&buf, 64, 16, 32), [0, 0, 0]);
        // Drehung um 45° bei 100 %: Ecke des Bildes bleibt leer
        p.tracks[0].clips[0].transform = Transform2D { rotation: 45.0, ..Default::default() };
        let buf = r.render(&p, 0.5, 64, 64);
        assert_eq!(px(&buf, 64, 1, 1), [0, 0, 0]);
        assert_eq!(px(&buf, 64, 32, 32), [255, 255, 255]);
        // Beschnitt: Bild links rot / rechts blau, linke Hälfte abschneiden → Ausschnitt (nur blau) wird eingepasst
        let dir = std::env::temp_dir().join("video-superbe-tests");
        let two = dir.join("two_halves.png");
        image::RgbaImage::from_fn(32, 32, |x, _| if x < 16 { image::Rgba([255, 0, 0, 255]) } else { image::Rgba([0, 0, 255, 255]) })
            .save(&two)
            .unwrap();
        let mut p2 = Project::default();
        let a2 = image_asset(&mut p2, &two, 32.0, 32.0);
        p2.tracks[0].clips.push(clip(a2, 0.0, 2.0));
        let buf = r.render(&p2, 0.5, 64, 64);
        assert_eq!(px(&buf, 64, 20, 32), [255, 0, 0]);
        assert_eq!(px(&buf, 64, 44, 32), [0, 0, 255]);
        p2.tracks[0].clips[0].fx.crop_l = 0.5;
        let buf = r.render(&p2, 0.5, 64, 64);
        assert_eq!(px(&buf, 64, 32, 32), [0, 0, 255]);
        assert_eq!(px(&buf, 64, 20, 32), [0, 0, 255], "Ausschnitt füllt die Höhe, enthält kein Rot mehr");
        assert_eq!(px(&buf, 64, 4, 32), [0, 0, 0], "Seitenrand des eingepassten Ausschnitts");
    }

    #[test]
    fn colour_effects() {
        let mut p = Project::default();
        let grey = solid_png("grey.png", 16, 16, [100, 100, 100]);
        let a = image_asset(&mut p, &grey, 16.0, 16.0);
        p.tracks[0].clips.push(clip(a, 0.0, 2.0));
        let mut r = Renderer::new();
        p.tracks[0].clips[0].fx.brightness = 0.2;
        // Helligkeit wirkt wie bei CIColorControls mit dem halben Wert: 100/255 + 0.1 → 126
        assert!(near(px(&r.render(&p, 0.5, 16, 16), 16, 8, 8), [126, 126, 126], 2));
        p.tracks[0].clips[0].fx.brightness = 0.0;
        p.tracks[0].clips[0].fx.saturation = 0.0;
        assert!(near(px(&r.render(&p, 0.5, 16, 16), 16, 8, 8), [100, 100, 100], 2));
        p.tracks[0].clips[0].fx.saturation = 1.0;
        p.tracks[0].clips[0].fx.temperature = 1.0;
        let w = px(&r.render(&p, 0.5, 16, 16), 16, 8, 8);
        assert!(w[0] > w[2], "warm: mehr Rot als Blau {w:?}");
    }

    #[test]
    fn title_renders_text_and_background() {
        let mut r = Renderer::new();
        if !r.has_fonts() {
            eprintln!("keine Systemschrift, Titeltest übersprungen");
            return;
        }
        let mut p = Project::default();
        p.tracks[0].clips.push(Clip {
            content: ClipContent::Title(Wrap { v: TitleStyle { text: "Hallo\nWelt".into(), ..Default::default() } }),
            start: 0.0,
            duration: 2.0,
            ..Default::default()
        });
        let buf = r.render(&p, 0.5, 640, 360);
        let lit = buf.chunks(4).filter(|p| p[0] > 200 && p[1] > 200 && p[2] > 200).count();
        assert!(lit > 150, "Text sichtbar: {lit}");
        // Text ist zentriert: linke Randspalte bleibt schwarz
        assert!((0..360).all(|y| px(&buf, 640, 2, y) == [0, 0, 0]));
        // mit Hintergrundbox: Boxfarbe in der Mitte-Nähe vorhanden
        if let ClipContent::Title(t) = &mut p.tracks[0].clips[0].content {
            t.v.background = true;
        }
        let buf = r.render(&p, 0.5, 640, 360);
        let boxed = buf.chunks(4).filter(|p| p[2] > p[0] && p[2] > 30 && p[0] < 40).count();
        assert!(boxed > 500, "Hintergrundbox sichtbar: {boxed}");
    }

    fn two_clip_project(transition: &str) -> Project {
        let mut p = Project::default();
        let red = solid_png("tr_red.png", 16, 16, [255, 0, 0]);
        let blue = solid_png("tr_blue.png", 16, 16, [0, 0, 255]);
        let ra = image_asset(&mut p, &red, 16.0, 16.0);
        let ba = image_asset(&mut p, &blue, 16.0, 16.0);
        p.tracks[0].clips.push(clip(ra, 0.0, 2.0));
        let mut b = clip(ba, 1.0, 2.0); // Überlappung 1…2 s
        b.transition = transition.into();
        p.tracks[0].clips.push(b);
        p
    }

    #[test]
    fn transition_dip_to_black() {
        let p = two_clip_project("dip");
        let mut r = Renderer::new();
        assert!(near(px(&r.render(&p, 1.25, 16, 16), 16, 8, 8), [128, 0, 0], 3), "rot blendet zuerst aus");
        assert_eq!(px(&r.render(&p, 1.5, 16, 16), 16, 8, 8), [0, 0, 0], "Mitte der Schwarzblende");
        assert!(near(px(&r.render(&p, 1.75, 16, 16), 16, 8, 8), [0, 0, 128], 3), "blau blendet danach ein");
        assert_eq!(px(&r.render(&p, 2.5, 16, 16), 16, 8, 8), [0, 0, 255]);
    }

    #[test]
    fn transition_wipe_and_slide() {
        let mut r = Renderer::new();
        let p = two_clip_project("wipe");
        let b = r.render(&p, 1.5, 32, 32);
        assert_eq!(px(&b, 32, 4, 16), [0, 0, 255], "links schon das neue Bild");
        assert_eq!(px(&b, 32, 28, 16), [255, 0, 0], "rechts noch das alte Bild");
        let p = two_clip_project("slide");
        let b = r.render(&p, 1.5, 32, 32);
        assert_eq!(px(&b, 32, 28, 16), [0, 0, 255], "neues Bild schiebt von rechts herein");
        assert_eq!(px(&b, 32, 4, 16), [255, 0, 0], "altes Bild schiebt nach links hinaus");
    }

    #[test]
    fn outgoing_clip_of_crossfade_stays_opaque() {
        let mut p = two_clip_project("crossfade");
        p.tracks[0].clips[0].fade_out = 1.0; // würde den auslaufenden Clip sonst ausblenden
        let mut r = Renderer::new();
        assert!(near(px(&r.render(&p, 1.5, 16, 16), 16, 8, 8), [128, 0, 127], 3));
    }

    #[test]
    fn keyframe_animation_moves_clip() {
        use crate::anim::{Channel, InterpKind, Keyframe};
        let mut p = Project::default();
        let white = solid_png("anim_white.png", 32, 32, [255, 255, 255]);
        let a = image_asset(&mut p, &white, 32.0, 32.0);
        let mut c = clip(a, 0.0, 2.0);
        c.transform.scale = 0.25;
        c.keys.0.insert(Channel::PosX, vec![Keyframe::with_kind(0.0, -0.25, InterpKind::Linear), Keyframe::with_kind(2.0, 0.25, InterpKind::Linear)]);
        p.tracks[0].clips.push(c);
        let mut r = Renderer::new();
        let b0 = r.render(&p, 0.0, 64, 64);
        assert_eq!(px(&b0, 64, 16, 32), [255, 255, 255]);
        assert_eq!(px(&b0, 64, 48, 32), [0, 0, 0]);
        let b1 = r.render(&p, 1.0, 64, 64);
        assert_eq!(px(&b1, 64, 32, 32), [255, 255, 255], "Mitte nach der Hälfte");
        assert_eq!(px(&b1, 64, 16, 32), [0, 0, 0]);
        let b2 = r.render(&p, 1.99, 64, 64);
        assert_eq!(px(&b2, 64, 48, 32), [255, 255, 255]);
    }

    #[test]
    fn opacity_curve_from_volume_points() {
        let mut p = Project::default();
        let white = solid_png("curve_white.png", 16, 16, [255, 255, 255]);
        let a = image_asset(&mut p, &white, 16.0, 16.0);
        let mut c = clip(a, 0.0, 4.0);
        c.volume_points = vec![VolumePoint { t: 0.0, gain: 0.0 }, VolumePoint { t: 4.0, gain: 1.0 }];
        p.tracks[0].clips.push(c);
        let mut r = Renderer::new();
        assert!(near(px(&r.render(&p, 2.0, 16, 16), 16, 8, 8), [128, 128, 128], 3));
        assert!(near(px(&r.render(&p, 1.0, 16, 16), 16, 8, 8), [64, 64, 64], 3));
    }

    #[test]
    fn blur_softens_edges_and_sharpen_enhances_them() {
        let mut p = Project::default();
        p.render_width = 64; // Weichzeichner-Pixel beziehen sich auf die Projektgröße
        p.render_height = 64;
        let white = solid_png("blur_white.png", 16, 16, [255, 255, 255]);
        let a = image_asset(&mut p, &white, 16.0, 16.0);
        let mut c = clip(a, 0.0, 2.0);
        c.transform.scale = 0.5; // weißes Quadrat 32×32 in 64×64
        p.tracks[0].clips.push(c);
        let mut r = Renderer::new();
        let sharp = r.render(&p, 0.5, 64, 64);
        assert_eq!(px(&sharp, 64, 14, 32), [0, 0, 0], "ohne Weichzeichner scharfe Kante");
        p.tracks[0].clips[0].fx.blur = 6.0;
        let blurred = r.render(&p, 0.5, 64, 64);
        let outside = px(&blurred, 64, 14, 32)[0];
        let inside = px(&blurred, 64, 20, 32)[0];
        assert!(outside > 5, "Licht läuft über die Kante hinaus: {outside}");
        assert!(inside < 250, "Kante innen wird dunkler: {inside}");
        assert!(px(&blurred, 64, 32, 32)[0] > 200, "Mitte bleibt hell");
        assert!(px(&blurred, 64, 2, 2)[0] < 3, "Ecke bleibt dunkel");
        // Schärfen: zwei Grautöne, an der Kante entsteht Überschwingen
        let grey_l = solid_png("sh_l.png", 16, 16, [100, 100, 100]);
        let mut q = Project::default();
        let a1 = image_asset(&mut q, &grey_l, 16.0, 16.0);
        let mut left = clip(a1, 0.0, 2.0);
        left.transform.scale = 1.0;
        q.tracks[0].clips.push(left);
        let grey_r = solid_png("sh_r.png", 16, 16, [150, 150, 150]);
        let a2 = image_asset(&mut q, &grey_r, 16.0, 16.0);
        let mut right = clip(a2, 0.0, 2.0);
        right.transform.scale = 0.5; // helleres Quadrat mittig auf dunklem Grund
        q.tracks[1].clips.push(right);
        let plain = r.render(&q, 0.5, 64, 64);
        q.tracks[1].clips[0].fx.sharpness = 2.0;
        let sharpened = r.render(&q, 0.5, 64, 64);
        let (pa, pb) = (px(&plain, 64, 17, 32)[0] as i32, px(&sharpened, 64, 17, 32)[0] as i32);
        assert!(pb > pa + 3, "innen an der Kante heller: {pa} → {pb}");
        let _ = grey_r;
    }

    #[test]
    fn title_font_and_outline_change_the_picture() {
        let mut r = Renderer::new();
        if !r.has_fonts() {
            return;
        }
        // Schriftart: Courier New (Festbreite) ist deutlich anders breit als die Standardschrift
        let base = TitleStyle { text: "Wiiiii".into(), ..Default::default() };
        let (w0, _) = r.title_size(&base, 360).unwrap();
        if crate::render::installed_fonts().contains(&"Courier New") {
            let mono = TitleStyle { font: Some("Courier New".into()), ..base.clone() };
            let (w1, _) = r.title_size(&mono, 360).unwrap();
            assert!(w1 > w0 + 20, "Festbreitenschrift breiter: {w1} vs {w0}");
        }
        // unbekannte oder Mac-Namen fallen auf eine vorhandene Schrift zurück
        let mac = TitleStyle { font: Some("Zapfino".into()), ..base.clone() };
        assert!(r.title_size(&mac, 360).is_some());
        let unknown = TitleStyle { font: Some("Gibt es nicht".into()), ..base.clone() };
        assert_eq!(r.title_size(&unknown, 360), Some((w0, r.title_size(&base, 360).unwrap().1)));
        assert_eq!(installed_fonts()[0], "System");

        // Kontur: auf weißem Grund erscheinen schwarze Randpixel um rote Schrift
        let mut p = Project::default();
        let white = solid_png("outline_white.png", 16, 16, [255, 255, 255]);
        let wa = image_asset(&mut p, &white, 16.0, 16.0);
        p.tracks[0].clips.push(clip(wa, 0.0, 2.0));
        let style = TitleStyle { text: "Hallo".into(), color: Rgba { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }, ..Default::default() };
        p.tracks.push(Track::default());
        p.tracks[1].clips.push(Clip { content: ClipContent::Title(Wrap { v: style.clone() }), start: 0.0, duration: 2.0, ..Default::default() });
        // nur im quadratischen Bildbereich (links und rechts liegen schwarze Balken)
        let dark = |buf: &[u8]| buf.chunks(4).enumerate().filter(|(i, q)| (150..490).contains(&(i % 640)) && q[0] < 60 && q[1] < 60 && q[2] < 60).count();
        let plain = dark(&r.render(&p, 0.5, 640, 360));
        if let ClipContent::Title(t) = &mut p.tracks[1].clips[0].content {
            t.v.outline = Some(true);
        }
        let outlined = dark(&r.render(&p, 0.5, 640, 360));
        assert!(outlined > plain + 100, "Kontur sichtbar: {outlined} vs {plain}");
    }
}
