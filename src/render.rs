//! Software-Kompositor: rendert ein Projektbild zu einer Zeit als RGBA. Gleicher Code für vorschau und Export.
//! Bilder, Videos, Titel, Position/Skalierung/Drehung (auch animiert), Beschnitt, Deckkraft, Überblendung,
//! Übergänge (Überblenden, Schwarzblende, Wischen, Schieben) und die Effekte Helligkeit, Kontrast, Sättigung,
//! Farbtemperatur, Vignette, Schärfe und Weichzeichner.

use crate::fxlib::{self, Lut, Orient};
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

/// Quellbild eines Clips: Bild/Titel oder Videobild (ohne Kopie des Decoder-Bildes).
enum Src {
    Img(Arc<Img>),
    Frame(MediaFrame),
}

impl Src {
    fn parts(&self) -> (usize, usize, &[u8]) {
        match self {
            Src::Img(i) => (i.w, i.h, &i.px),
            Src::Frame(f) => (f.width, f.height, &f.rgba),
        }
    }
}

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
    /// geladene 3D-LUTs (`.cube`) nach Pfad
    luts: HashMap<String, Option<Arc<Lut>>>,
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
    src: &'a Src,
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
    /// Verschiebung nach unten in Zielpixeln (Titelbewegung, Abspann)
    dy: f32,
    /// Sichtbar nur oberhalb dieser y-Position (Übergang „Wischen von oben“)
    wipe_y: Option<f32>,
    /// Vergrößerung um die Bildmitte (Übergang „Zoom“, Titel-Zoom)
    zoom: f32,
    /// Kreisförmige Blende um die Bildmitte mit diesem Radius (Irisblende)
    iris: Option<f32>,
    /// Spiegeln/Drehen/Horizont
    orient: Option<Orient>,
    /// 3D-LUT mit Stärke
    lut: Option<(Arc<Lut>, f32)>,
    /// Stabilisierung: Verschiebung (Anteile von Breite/Höhe) und Skalierung um die Bildmitte
    stab: Option<(f32, f32, f32)>,
}

impl Draw<'_> {
    fn new<'a>(src: &'a Src, native: bool, tr: Transform2D, fx: Effects, alpha: f32, opaque: bool) -> Draw<'a> {
        Draw { src, native, tr, fx, alpha, dx: 0.0, wipe_x: None, opaque, dy: 0.0, wipe_y: None, zoom: 1.0, iris: None, orient: None, lut: None, stab: None }
    }
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
            luts: HashMap::new(),
            blocking: BlockingFrames::default(),
        }
    }

    /// Größe der Titelgrafik (Pixel) bei Bildbreite `canvas_w` und -höhe `canvas_h` (zwischengespeichert).
    pub fn title_size(&mut self, s: &TitleStyle, canvas_w: usize, canvas_h: usize) -> Option<(usize, usize)> {
        let key = format!("size|{canvas_w}x{canvas_h}|{s:?}");
        if let Some(i) = self.titles.get(&key) {
            return Some((i.w, i.h));
        }
        let img = self.render_title(s, canvas_w, canvas_h, None)?;
        let r = (img.w, img.h);
        self.titles.insert(key, Arc::new(Img { w: img.w, h: img.h, px: vec![] }));
        Some(r)
    }

    fn lut(&mut self, path: &str) -> Option<Arc<Lut>> {
        self.luts.entry(path.to_string()).or_insert_with(|| fxlib::load_cube(path)).clone()
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
        par_rows(&mut dst, w * 4, |_, band| {
            for px in band.chunks_exact_mut(4) {
                px[3] = 255;
            }
        });
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
                let (mut wipe_y, mut zoom, mut iris, mut flash): (Option<f32>, f32, Option<f32>, Option<f32>) = (None, 1.0, None, None);
                if xin > 0.0 && in_kind != "crossfade" {
                    let p_in = (rel / xin).clamp(0.0, 1.0) as f32;
                    match in_kind.as_str() {
                        "dip" | "flash" => alpha *= ((p_in - 0.5) * 2.0).max(0.0),
                        "wipe" => wipe_x = Some(w as f32 * p_in),
                        "wipeDown" => wipe_y = Some(h as f32 * p_in),
                        "slide" => dx += (1.0 - p_in) * w as f32,
                        "zoom" => {
                            alpha *= p_in;
                            zoom *= 1.0 + 0.35 * (1.0 - p_in);
                        }
                        "iris" => {
                            if p_in <= 0.001 {
                                alpha = 0.0;
                            } else if p_in < 0.999 {
                                iris = Some(p_in * (w as f32).hypot(h as f32) / 2.0 * 1.05);
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(k) = out_kind.as_deref().filter(|k| *k != "crossfade") {
                    let p_out = ((rel - (c.duration - xout)) / xout).clamp(0.0, 1.0) as f32;
                    match k {
                        "dip" => alpha *= (1.0 - 2.0 * p_out).max(0.0),
                        "flash" => {
                            alpha *= (1.0 - 2.0 * p_out).max(0.0);
                            flash = Some(1.0 - (2.0 * p_out - 1.0).abs());
                        }
                        "slide" => dx -= p_out * w as f32,
                        "zoom" => zoom *= 1.0 + 0.35 * p_out,
                        _ => {}
                    }
                }
                if let Some(wa) = flash {
                    // Weißblende: weiße Fläche über dem bisherigen Ergebnis
                    for px in dst.chunks_exact_mut(4) {
                        over(px, [255, 255, 255, 255], wa);
                        px[3] = 255;
                    }
                }
                if alpha <= 0.0 {
                    continue;
                }
                let fx = if c.is_title() { Effects::default() } else { c.fx_at(rel) };
                let tr_now = c.transform_at(rel);
                let (mut title_dx, mut title_dy, mut title_zoom) = (0.0f32, 0.0f32, 1.0f32);
                let (src, native): (Src, bool) = match &c.content {
                    ClipContent::Title(s) => {
                        // Platzhalter, Schreibmaschine, Ein-/Auslaufen, Abspann und Laufschrift (alles zeitabhängig)
                        let mut style = s.v.clone();
                        style.text = fxlib::expand_tokens(&style.text, t, p.fps, Some(rel));
                        let dur = style.anim_dur.unwrap_or(0.6).max(0.1);
                        let mut visible: Option<usize> = None;
                        let mut motion = (0.0, 0.0, 1.0, 1.0);
                        if let Some(a) = style.anim_in.as_deref().filter(|a| *a != "none") {
                            if rel < dur {
                                if a == "typewriter" {
                                    visible = Some((style.text.chars().count() as f64 * (rel / dur).clamp(0.0, 1.0)) as usize);
                                } else {
                                    motion = fxlib::title_motion(a, 1.0 - fxlib::ease(rel / dur), w as f64, h as f64);
                                }
                            }
                        }
                        if let Some(a) = style.anim_out.as_deref().filter(|a| *a != "none" && *a != "typewriter") {
                            if c.duration - rel < dur {
                                motion = fxlib::title_motion(a, 1.0 - fxlib::ease((c.duration - rel) / dur), w as f64, h as f64);
                            }
                        }
                        let key = format!("{w}x{h}|{visible:?}|{style:?}");
                        if !self.titles.contains_key(&key) {
                            if let Some(img) = self.render_title(&style, w, h, visible) {
                                self.titles.insert(key.clone(), Arc::new(img));
                            }
                        }
                        let Some(img) = self.titles.get(&key).cloned() else { continue };
                        let p01 = (rel / c.duration.max(0.1)).clamp(0.0, 1.0) as f32;
                        if style.credits == Some(true) {
                            // Abspann: von unten nach oben über die ganze Clip-Dauer
                            let (hh, ih) = (h as f32, img.h as f32);
                            title_dy = (hh - p01 * (hh + ih) + ih / 2.0) - hh / 2.0;
                        }
                        if style.ticker == Some(true) {
                            // Laufschrift: von rechts nach links über die ganze Clip-Dauer
                            let (ww, iw) = (w as f32, img.w as f32);
                            title_dx = (ww - p01 * (ww + iw) + iw / 2.0) - ww / 2.0;
                        }
                        title_dx += motion.0 as f32;
                        title_dy += motion.1 as f32;
                        title_zoom = motion.2 as f32;
                        alpha *= motion.3 as f32;
                        (Src::Img(img), true)
                    }
                    ClipContent::Media(m) => {
                        let Some(asset) = p.asset(m.v) else { continue };
                        if asset.is_image {
                            let levels = self.images.entry(m.v).or_insert_with(|| load_image(&asset.path).map(|i| vec![Arc::new(i)]));
                            let Some(levels) = levels else { continue };
                            let (ow, oh) = Orient::from_fx(&fx).map(|o| o.dims(levels[0].w as f32, levels[0].h as f32)).map(|d| (d.0 as usize, d.1 as usize)).unwrap_or((levels[0].w, levels[0].h));
                            let k = fit_scale(ow, oh, &fx, w, h, false, tr_now.scale as f32);
                            (Src::Img(pick_level(levels, k)), false)
                        } else {
                            let Some(f) = frames.frame(asset, c.source_in + rel * c.speed) else { continue };
                            (Src::Frame(f), false)
                        }
                    }
                };
                let needs_layer = fx.blur > 0.0 || fx.sharpness > 0.0;
                let lut = fx.lut_path.clone().and_then(|pth| self.lut(&pth)).map(|l| (l, fx.lut_amount.unwrap_or(1.0) as f32));
                let orient = Orient::from_fx(&fx);
                // Stabilisierung: Verschiebung und Skalierung aus den gemessenen Bewegungsdaten der Quelle
                let stab = match (&c.content, fx.stabilize) {
                    (ClipContent::Media(m), Some(strength)) if strength > 0.001 => crate::stabilize::get(m.v).and_then(|path| path.transform_at(c.source_in + rel * c.speed, strength, fx.stab_no_scale != Some(true))),
                    _ => None,
                };
                let mk = |opaque: bool| {
                    let mut d = Draw::new(&src, native, tr_now.clone(), fx.clone(), if opaque { alpha } else { 1.0 }, opaque);
                    d.dx = dx + title_dx;
                    d.dy = title_dy;
                    d.wipe_x = wipe_x;
                    d.wipe_y = wipe_y;
                    d.zoom = zoom * title_zoom;
                    d.iris = iris;
                    d.orient = orient;
                    d.lut = lut.clone();
                    d.stab = stab;
                    d
                };
                if needs_layer {
                    let mut layer = vec![0u8; w * h * 4];
                    blit(&mut layer, w, h, &mk(false));
                    if fx.sharpness > 0.0 {
                        sharpen_layer(&mut layer, w, h, fx.sharpness as f32, px_scale);
                    }
                    if fx.blur > 0.0 {
                        blur_layer(&mut layer, w, h, fx.blur as f32 * px_scale);
                    }
                    par_rows(&mut dst, w * 4, |first, band| {
                        let lay = &layer[first * w * 4..first * w * 4 + band.len()];
                        for (d, s) in band.chunks_exact_mut(4).zip(lay.chunks_exact(4)) {
                            if s[3] > 0 {
                                over(d, [s[0], s[1], s[2], s[3]], alpha);
                                d[3] = 255;
                            }
                        }
                    });
                } else {
                    blit(&mut dst, w, h, &mk(true));
                }
            }
        }
        dst
    }

    /// Rendert Titel/Bauchbinden als enges Bild (Text samt Hintergrundbox): Auszeichnung (fett, kursiv, unterstrichen),
    /// Ausrichtung, Zeilenabstand, Laufweite, Umbruch bei 90 % der Bildbreite, Schatten mit Unschärfe, Umrandung (auch nur
    /// Rahmen). `visible`: Schreibmaschine, nur die ersten Zeichen sind sichtbar (das Layout bleibt gleich).
    fn render_title(&mut self, s: &TitleStyle, canvas_w: usize, canvas_h: usize, visible: Option<usize>) -> Option<Img> {
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
        let kern = s.kerning.unwrap_or(0.0) as f32 * fs;
        let line_gap_extra = s.line_spacing.unwrap_or(0.0) as f32 * fs;
        let max_w = if s.ticker == Some(true) { 1.0e6 } else { canvas_w as f32 * 0.9 };
        let width_of = |font: &FontVec, size: f32, text: &str| -> f32 {
            let sf = font.as_scaled(PxScale::from(size));
            let (mut w, mut prev) = (0.0, None);
            for ch in text.chars() {
                let g = sf.glyph_id(ch);
                if let Some(p) = prev {
                    w += sf.kern(p, g);
                }
                w += sf.h_advance(g) + kern;
                prev = Some(g);
            }
            w
        };
        // Zeilen mit Umbruch
        struct Row<'a> {
            text: String,
            size: f32,
            font: &'a FontVec,
            start: usize,
            w: f32,
            asc: f32,
            lh: f32,
        }
        let mut rows: Vec<Row> = vec![];
        let mut idx = 0usize;
        for (li, line) in s.text.split('\n').enumerate() {
            let size = if li == 0 { fs } else { fs * 0.6 };
            let font = if s.bold.unwrap_or(li == 0) { bold } else { reg };
            let sf = font.as_scaled(PxScale::from(size));
            let (asc, lh) = (sf.ascent(), sf.height() + sf.line_gap());
            let mut cur = String::new();
            let mut cur_start = idx;
            let mut pos = idx;
            for word in line.split(' ') {
                let cand = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
                if !cur.is_empty() && width_of(font, size, &cand) > max_w {
                    let w = width_of(font, size, &cur);
                    rows.push(Row { text: std::mem::take(&mut cur), size, font, start: cur_start, w, asc, lh });
                    cur = word.to_string();
                    cur_start = pos;
                } else {
                    cur = cand;
                }
                pos += word.chars().count() + 1;
            }
            let w = width_of(font, size, &cur);
            rows.push(Row { text: cur, size, font, start: cur_start, w, asc, lh });
            idx += line.chars().count() + 1;
        }
        let outline = s.outline == Some(true);
        let hollow = outline && s.outline_hollow == Some(true);
        let ow = s.outline_width.unwrap_or(4.0) as f32;
        let ring_r = (ow / 200.0 * fs).max(0.75);
        let pad = (if s.background { fs * 0.5 } else { fs * 0.2 }) + if outline { ring_r } else { 0.0 };
        let tw = rows.iter().map(|r| r.w).fold(0.0, f32::max);
        let th: f32 = rows.iter().map(|r| r.lh).sum::<f32>() + line_gap_extra * (rows.len().saturating_sub(1)) as f32;
        let (iw, ih) = ((tw + 2.0 * pad).ceil().max(1.0) as usize, (th + 2.0 * pad).ceil().max(1.0) as usize);
        if iw > 16000 || ih > 16000 {
            return None;
        }
        let align = if s.left_aligned { 0 } else if s.right_aligned == Some(true) { 2 } else { 1 };
        let italic = s.italic == Some(true);
        let underline = s.underline == Some(true);

        // Abdeckung (0…1) des sichtbaren Textes, um (ox, oy) verschoben
        let coverage = |ox: f32, oy: f32| -> Vec<f32> {
            let mut cov = vec![0f32; iw * ih];
            let mut y = pad;
            for row in &rows {
                let x0 = match align {
                    0 => pad,
                    2 => pad + tw - row.w,
                    _ => pad + (tw - row.w) / 2.0,
                };
                let sf = row.font.as_scaled(PxScale::from(row.size));
                let base = y + row.asc;
                let (mut x, mut prev) = (x0, None);
                let mut drawn_end = x0;
                for (ci, ch) in row.text.chars().enumerate() {
                    let gid = sf.glyph_id(ch);
                    if let Some(p) = prev {
                        x += sf.kern(p, gid);
                    }
                    let shown = visible.is_none_or(|v| row.start + ci < v);
                    if shown {
                        let glyph = gid.with_scale_and_position(PxScale::from(row.size), ab_glyph::point(x + ox, base + oy));
                        if let Some(og) = row.font.outline_glyph(glyph) {
                            let b = og.px_bounds();
                            og.draw(|gx, gy, c| {
                                let py = b.min.y as i32 + gy as i32;
                                let mut px = b.min.x as i32 + gx as i32;
                                if italic {
                                    px += ((base + oy - py as f32) * 0.21) as i32;
                                }
                                if px >= 0 && py >= 0 && (px as usize) < iw && (py as usize) < ih {
                                    let o = py as usize * iw + px as usize;
                                    cov[o] = cov[o].max(c);
                                }
                            });
                        }
                        drawn_end = x + sf.h_advance(gid) + kern;
                    }
                    x += sf.h_advance(gid) + kern;
                    prev = Some(gid);
                }
                if underline && drawn_end > x0 {
                    let th_u = (row.size / 14.0).max(1.0);
                    let yu = base + oy + row.size * 0.12;
                    for yy in (yu.floor() as i32)..((yu + th_u).ceil() as i32) {
                        for xx in ((x0 + ox).floor() as i32)..((drawn_end + ox).ceil() as i32) {
                            if xx >= 0 && yy >= 0 && (xx as usize) < iw && (yy as usize) < ih {
                                cov[yy as usize * iw + xx as usize] = 1.0;
                            }
                        }
                    }
                }
                y += row.lh + line_gap_extra;
            }
            cov
        };

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
        let paint = |img: &mut Img, cov: &[f32], color: [u8; 4]| {
            for (i, c) in cov.iter().enumerate() {
                if *c > 0.0 {
                    over(&mut img.px[i * 4..i * 4 + 4], color, *c);
                }
            }
        };
        let c = s.color;
        let main = [(c.r * 255.0) as u8, (c.g * 255.0) as u8, (c.b * 255.0) as u8, (c.a * 255.0) as u8];
        let fill = coverage(0.0, 0.0);
        // Schatten (Standard: nur ohne Hintergrund)
        if s.shadow.unwrap_or(!s.background) {
            let off = fs * 0.04;
            let mut sh = coverage(0.0, off);
            let sigma = fs * s.shadow_blur.unwrap_or(0.1) as f32 / 2.0;
            if sigma > 0.3 {
                blur_cov(&mut sh, iw, ih, sigma);
            }
            let alpha = s.shadow_alpha.unwrap_or(0.7).clamp(0.0, 1.0) as f32;
            paint(&mut img, &sh, [0, 0, 0, (alpha * 255.0) as u8]);
        }
        if outline {
            // Umrandung als Ring um die Buchstaben: aufgeweitete minus (bei „nur Rahmen“) verkleinerte Abdeckung
            let offs: Vec<(f32, f32)> = (0..12).map(|k| { let a = k as f32 * std::f32::consts::TAU / 12.0; (ring_r * a.cos(), ring_r * a.sin()) }).collect();
            let mut grown = fill.clone();
            for (ox, oy) in &offs {
                for (g, v) in grown.iter_mut().zip(coverage(*ox, *oy)) {
                    *g = g.max(v);
                }
            }
            if hollow {
                let mut shrunk = fill.clone();
                for (ox, oy) in &offs {
                    for (g, v) in shrunk.iter_mut().zip(coverage(*ox, *oy)) {
                        *g = g.min(v);
                    }
                }
                let ring: Vec<f32> = grown.iter().zip(&shrunk).map(|(g, s)| (g - s).clamp(0.0, 1.0)).collect();
                paint(&mut img, &ring, main);
            } else {
                paint(&mut img, &grown, [0, 0, 0, 255]);
                paint(&mut img, &fill, main);
            }
        } else {
            paint(&mut img, &fill, main);
        }
        Some(img)
    }
}

/// Weichzeichnen einer Abdeckungskarte (3 Kastenfilter ≈ Gauß).
fn blur_cov(buf: &mut [f32], w: usize, h: usize, sigma: f32) {
    for r in box_radii(sigma) {
        if r == 0 {
            continue;
        }
        let mut tmp = vec![0f32; buf.len()];
        // waagerecht
        for y in 0..h {
            let row = &buf[y * w..(y + 1) * w];
            let n = (2 * r + 1) as f32;
            let mut sum: f32 = (0..=r.min(w - 1)).map(|i| row[i]).sum::<f32>();
            for x in 0..w {
                tmp[y * w + x] = sum / n;
                let add = x + r + 1;
                if add < w {
                    sum += row[add];
                }
                if x >= r {
                    sum -= row[x - r];
                }
            }
        }
        // senkrecht
        for x in 0..w {
            let n = (2 * r + 1) as f32;
            let mut sum: f32 = (0..=r.min(h - 1)).map(|i| tmp[i * w + x]).sum::<f32>();
            for y in 0..h {
                buf[y * w + x] = sum / n;
                let add = y + r + 1;
                if add < h {
                    sum += tmp[add * w + x];
                }
                if y >= r {
                    sum -= tmp[(y - r) * w + x];
                }
            }
        }
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

/// Anzahl Rechenfäden für Bildbänder.
fn threads() -> usize {
    static N: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *N.get_or_init(|| {
        if std::env::var("VSB_THREADS").is_ok_and(|v| v == "1") {
            1
        } else {
            std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).clamp(1, 8)
        }
    })
}

/// Teilt `buf` (Zeilen à `row_len`) in waagerechte Bänder und ruft `f(erste Zeile, Band)` je Band auf (parallel).
fn par_rows<T: Send>(buf: &mut [T], row_len: usize, f: impl Fn(usize, &mut [T]) + Sync) {
    let rows = if row_len == 0 { 0 } else { buf.len() / row_len };
    #[cfg(test)]
    let serial = tests::SERIAL.with(|s| s.get());
    #[cfg(not(test))]
    let serial = false;
    let n = if serial { 1 } else { threads().min(rows / 24).max(1) };
    if n <= 1 {
        f(0, buf);
        return;
    }
    let per = rows.div_ceil(n);
    std::thread::scope(|sc| {
        let f = &f;
        for (i, band) in buf.chunks_mut(per * row_len).enumerate() {
            sc.spawn(move || f(i * per, band));
        }
    });
}

/// "Over" für Straight-Alpha-Pixel (dst RGBA, src RGBA, zusätzlicher Faktor).
fn over(dst: &mut [u8], src: [u8; 4], k: f32) {
    let sa = src[3] as f32 / 255.0 * k;
    if sa <= 0.0 {
        return;
    }
    if dst[3] == 255 {
        // deckendes Ziel: ohne Division, Alpha bleibt 255
        if sa >= 0.9999 {
            dst[..3].copy_from_slice(&src[..3]);
        } else {
            let ia = 1.0 - sa;
            for i in 0..3 {
                dst[i] = (src[i] as f32 * sa + dst[i] as f32 * ia) as u8;
            }
        }
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
    let (sw, sh, spx) = d.src.parts();
    if sw == 0 || sh == 0 || spx.len() < sw * sh * 4 {
        return;
    }
    let fx = &d.fx;
    // Größe nach Spiegeln/Drehen; Beschnitt und Einpassen beziehen sich darauf
    let (ow, oh) = d.orient.map(|o| o.dims(sw as f32, sh as f32)).map(|q| (q.0 as usize, q.1 as usize)).unwrap_or((sw, sh));
    let (cl, ct, cr, cb) = (fx.crop_l as f32, fx.crop_t as f32, fx.crop_r as f32, fx.crop_b as f32);
    let cw = ow as f32 * (1.0 - cl - cr);
    let ch = oh as f32 * (1.0 - ct - cb);
    if cw < 1.0 || ch < 1.0 {
        return;
    }
    let k0 = fit_scale(ow, oh, fx, dw, dh, d.native, d.tr.scale as f32);
    if k0 <= 0.0 {
        return;
    }
    let k = k0 * d.zoom;
    let c0 = (dw as f32 / 2.0, dh as f32 / 2.0);
    let mut center = (c0.0 + d.tr.x as f32 * dw as f32 + d.dx, c0.1 + d.tr.y as f32 * dh as f32 + d.dy);
    if d.zoom != 1.0 {
        center = (c0.0 + (center.0 - c0.0) * d.zoom, c0.1 + (center.1 - c0.1) * d.zoom);
    }
    let ang = (d.tr.rotation as f32).to_radians();
    let (sn, cs) = ang.sin_cos();
    let (hw, hh) = (cw * k / 2.0, ch * k / 2.0);
    let ex = hw * cs.abs() + hh * sn.abs();
    let ey = hw * sn.abs() + hh * cs.abs();
    let x0 = ((center.0 - ex).floor().max(0.0)) as usize;
    let x1 = ((center.0 + ex).ceil().min(dw as f32)) as usize;
    let y0 = ((center.1 - ey).floor().max(0.0)) as usize;
    let y1 = ((center.1 + ey).ceil().min(dh as f32)) as usize;
    let fxn = !fx.is_neutral() || d.lut.is_some();
    let diag = (hw * hw + hh * hh).sqrt().max(1.0);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    par_rows(&mut dst[y0 * dw * 4..y1 * dw * 4], dw * 4, |first, band| {
        for (i, row) in band.chunks_exact_mut(dw * 4).enumerate() {
            let y = y0 + first + i;
            for x in x0..x1 {
                if let Some(wx) = d.wipe_x {
                    if x as f32 + 0.5 > wx {
                        continue;
                    }
                }
                if let Some(wy) = d.wipe_y {
                    if y as f32 + 0.5 > wy {
                        continue;
                    }
                }
                // Irisblende: voll sichtbar bis 97 % des Radius, dann linear auf 0
                let mut mask = 1.0f32;
                if let Some(r) = d.iris {
                    let dist = ((x as f32 + 0.5 - c0.0).powi(2) + (y as f32 + 0.5 - c0.1).powi(2)).sqrt();
                    mask = if dist <= r * 0.97 { 1.0 } else if dist >= r { 0.0 } else { (r - dist) / (r * 0.03) };
                    if mask <= 0.0 {
                        continue;
                    }
                }
                let (rx, ry) = (x as f32 + 0.5 - center.0, y as f32 + 0.5 - center.1);
                let (ux, uy) = (rx * cs + ry * sn, -rx * sn + ry * cs);
                if ux.abs() > hw || uy.abs() > hh {
                    continue;
                }
                let px = cl * ow as f32 + (ux + hw) / k;
                let py = ct * oh as f32 + (uy + hh) / k;
                let (mut sx, mut sy) = match d.orient {
                    Some(o) => o.to_source(px, py, sw as f32, sh as f32),
                    None => (px, py),
                };
                if let Some((sdx, sdy, sc)) = d.stab {
                    let (cx, cy) = (sw as f32 / 2.0, sh as f32 / 2.0);
                    sx = cx + (sx - cx - sdx * sw as f32) / sc;
                    sy = cy + (sy - cy - sdy * sh as f32) / sc;
                }
                let mut s = sample(spx, sw, sh, sx - 0.5, sy - 0.5);
                if let Some(ck) = &fx.chroma {
                    let (a, col) = fxlib::chroma_key(s[0] as f32 / 255.0, s[1] as f32 / 255.0, s[2] as f32 / 255.0, ck);
                    s[3] = (s[3] as f32 * a).round() as u8;
                    for i in 0..3 {
                        s[i] = (col[i].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                    }
                    if s[3] == 0 {
                        continue;
                    }
                }
                if fxn {
                    apply_fx(&mut s, fx, ((ux * ux + uy * uy).sqrt()) / diag, d.lut.as_ref());
                }
                let o = x * 4;
                over(&mut row[o..o + 4], s, d.alpha * mask);
                if d.opaque {
                    row[o + 3] = 255;
                }
            }
        }
    });
}

fn sample(src: &[u8], w: usize, h: usize, x: f32, y: f32) -> [u8; 4] {
    // floor ohne Bibliotheksaufruf (auf dem GNU-Ziel ohne SSE4.1 ein Funktionsaufruf je Pixel)
    let (xi, yi) = (x as i32 - (x < 0.0) as i32, y as i32 - (y < 0.0) as i32);
    let (xf, yf) = (xi as f32, yi as f32);
    let (tx, ty) = (((x - xf) * 256.0) as u32, ((y - yf) * 256.0) as u32);
    let cl = |v: f32, m: usize| (v.max(0.0) as usize).min(m - 1);
    let (x0, x1, y0, y1) = (cl(xf, w) * 4, cl(xf + 1.0, w) * 4, cl(yf, h) * w * 4, cl(yf + 1.0, h) * w * 4);
    let (a, b, c, d) = (&src[y0 + x0..y0 + x0 + 4], &src[y0 + x1..y0 + x1 + 4], &src[y1 + x0..y1 + x0 + 4], &src[y1 + x1..y1 + x1 + 4]);
    let mut o = [0u8; 4];
    for i in 0..4 {
        let top = a[i] as u32 * (256 - tx) + b[i] as u32 * tx;
        let bot = c[i] as u32 * (256 - tx) + d[i] as u32 * tx;
        o[i] = ((top * (256 - ty) + bot * ty + 32768) >> 16) as u8;
    }
    o
}

fn apply_fx(p: &mut [u8; 4], fx: &Effects, vign_d: f32, lut: Option<&(Arc<Lut>, f32)>) {
    const K: f32 = 1.0 / 255.0;
    let (sat, contrast, bright, temp) = (fx.saturation as f32, fx.contrast as f32, fx.brightness as f32 * 0.5 + 0.5, 0.1 * fx.temperature as f32);
    let mut c = [p[0] as f32 * K, p[1] as f32 * K, p[2] as f32 * K];
    let gray = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    for v in &mut c {
        *v = gray + (*v - gray) * sat;
        // wie CIColorControls: Helligkeit wirkt mit dem halben Wert, Kontrast um die Mitte
        *v = (*v - 0.5) * contrast + bright;
    }
    c[0] += temp;
    c[2] -= temp;
    if let Some((l, amount)) = lut {
        c = l.apply([c[0].clamp(0.0, 1.0), c[1].clamp(0.0, 1.0), c[2].clamp(0.0, 1.0)], *amount);
    }
    if fx.vignette > 0.0 {
        let f = (1.0 - fx.vignette as f32 * vign_d * vign_d).max(0.0);
        for v in &mut c {
            *v *= f;
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

fn box_row(s: &[f32], d: &mut [f32], w: usize, r: usize) {
    let norm = 1.0 / (2 * r + 1) as f32;
    let at = |j: isize| -> &[f32] {
        let j = j.clamp(0, w as isize - 1) as usize * 4;
        &s[j..j + 4]
    };
    let mut acc = [0f32; 4];
    for k in -(r as isize)..=(r as isize) {
        let p = at(k);
        for c in 0..4 {
            acc[c] += p[c];
        }
    }
    for i in 0..w {
        for c in 0..4 {
            d[i * 4 + c] = acc[c] * norm;
        }
        let (add, sub) = (at(i as isize + r as isize + 1), at(i as isize - r as isize));
        for c in 0..4 {
            acc[c] += add[c] - sub[c];
        }
    }
}

/// Kastenfilter waagerecht (Zeilen parallel) oder senkrecht (zeilenweise mit laufender Summe je Spalte).
fn box_pass(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: usize, horizontal: bool) {
    if horizontal {
        par_rows(dst, w * 4, |first, band| {
            for (i, row) in band.chunks_exact_mut(w * 4).enumerate() {
                let y = first + i;
                box_row(&src[y * w * 4..(y + 1) * w * 4], row, w, r);
            }
        });
        return;
    }
    let norm = 1.0 / (2 * r + 1) as f32;
    let row = |y: isize| -> &[f32] {
        let y = y.clamp(0, h as isize - 1) as usize;
        &src[y * w * 4..(y + 1) * w * 4]
    };
    let mut acc = vec![0f32; w * 4];
    for k in -(r as isize)..=(r as isize) {
        for (a, v) in acc.iter_mut().zip(row(k)) {
            *a += *v;
        }
    }
    for y in 0..h {
        for (o, a) in dst[y * w * 4..(y + 1) * w * 4].iter_mut().zip(&acc) {
            *o = *a * norm;
        }
        let (add, sub) = (row(y as isize + r as isize + 1), row(y as isize - r as isize));
        for ((a, p), q) in acc.iter_mut().zip(add).zip(sub) {
            *a += *p - *q;
        }
    }
}

/// RGBA8 (gerade) → vormultipliziertes f32 (parallel).
fn premultiplied(buf: &[u8], w: usize) -> Vec<f32> {
    let mut a = vec![0f32; buf.len()];
    par_rows(&mut a, w * 4, |first, band| {
        let src = &buf[first * w * 4..first * w * 4 + band.len()];
        for (q, p) in band.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            let al = p[3] as f32 / 255.0;
            q[0] = p[0] as f32 * al;
            q[1] = p[1] as f32 * al;
            q[2] = p[2] as f32 * al;
            q[3] = p[3] as f32;
        }
    });
    a
}

/// Weichzeichner (Gauß, durch dreifachen Kastenfilter angenähert) auf einer Ebene mit Alpha.
fn blur_layer(buf: &mut [u8], w: usize, h: usize, sigma: f32) {
    if sigma < 0.3 || w == 0 || h == 0 {
        return;
    }
    // vormultipliziert rechnen, damit Ränder nicht dunkel auslaufen
    let mut a = premultiplied(buf, w);
    let mut b = vec![0f32; a.len()];
    for r in box_radii(sigma) {
        if r == 0 {
            continue;
        }
        box_pass(&a, &mut b, w, h, r, true);
        box_pass(&b, &mut a, w, h, r, false);
    }
    par_rows(buf, w * 4, |first, band| {
        let q = &a[first * w * 4..first * w * 4 + band.len()];
        for (p, q) in band.chunks_exact_mut(4).zip(q.chunks_exact(4)) {
            let al = q[3] / 255.0;
            if al > 0.0 {
                p[0] = (q[0] / al).clamp(0.0, 255.0) as u8;
                p[1] = (q[1] / al).clamp(0.0, 255.0) as u8;
                p[2] = (q[2] / al).clamp(0.0, 255.0) as u8;
            }
            p[3] = q[3].clamp(0.0, 255.0) as u8;
        }
    });
}

/// Schärfen (Unscharfmaske auf der Leuchtdichte) einer Ebene. `px_scale`: Vorschaugröße zur Projektgröße, damit der
/// Radius im Bild (nicht in Pixeln) gleich bleibt.
fn sharpen_layer(buf: &mut [u8], w: usize, h: usize, amount: f32, px_scale: f32) {
    let r = ((2.0 * px_scale).round() as usize).clamp(1, 4);
    let a = premultiplied(buf, w);
    let mut b = vec![0f32; a.len()];
    let mut c = vec![0f32; a.len()];
    box_pass(&a, &mut b, w, h, r, true);
    box_pass(&b, &mut c, w, h, r, false);
    drop(b);
    par_rows(buf, w * 4, |first, band| {
        let off = first * w * 4;
        for (i, p) in band.chunks_exact_mut(4).enumerate() {
            if p[3] == 0 {
                continue;
            }
            let j = off + i * 4;
            let al = p[3] as f32 / 255.0;
            let lum_now = 0.2126 * a[j] + 0.7152 * a[j + 1] + 0.0722 * a[j + 2];
            let lum_blur = 0.2126 * c[j] + 0.7152 * c[j + 1] + 0.0722 * c[j + 2];
            let d = (lum_now - lum_blur) * amount * 0.8;
            for ch in 0..3 {
                let v = (a[j + ch] + d) / al.max(1e-3);
                p[ch] = v.clamp(0.0, 255.0) as u8;
            }
        }
    });
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
        let (pa, pb) = (px(&plain, 64, 16, 32)[0] as i32, px(&sharpened, 64, 16, 32)[0] as i32);
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
        let (w0, _) = r.title_size(&base, 640, 360).unwrap();
        if crate::render::installed_fonts().contains(&"Courier New") {
            let mono = TitleStyle { font: Some("Courier New".into()), ..base.clone() };
            let (w1, _) = r.title_size(&mono, 640, 360).unwrap();
            assert!(w1 > w0 + 20, "Festbreitenschrift breiter: {w1} vs {w0}");
        }
        // unbekannte oder Mac-Namen fallen auf eine vorhandene Schrift zurück
        let mac = TitleStyle { font: Some("Zapfino".into()), ..base.clone() };
        assert!(r.title_size(&mac, 640, 360).is_some());
        let unknown = TitleStyle { font: Some("Gibt es nicht".into()), ..base.clone() };
        assert_eq!(r.title_size(&unknown, 640, 360), Some((w0, r.title_size(&base, 640, 360).unwrap().1)));
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
            t.v.outline_width = Some(10.0);
        }
        let outlined = dark(&r.render(&p, 0.5, 640, 360));
        assert!(outlined > plain + 100, "Kontur sichtbar: {outlined} vs {plain}");
    }

    // ---------- Neuerungen des Mac-Originals: Blenden, Spiegeln, Greenscreen, LUT, Titel ----------

    fn lit(buf: &[u8], x0: usize, x1: usize, y0: usize, y1: usize, w: usize) -> usize {
        (y0..y1).flat_map(|y| (x0..x1).map(move |x| (x, y))).filter(|&(x, y)| px(buf, w, x, y).iter().any(|c| *c > 40)).count()
    }

    #[test]
    fn transitions_wipe_down_zoom_iris_flash() {
        let mut r = Renderer::new();
        // Wischen von oben: oben schon das neue Bild
        let p = two_clip_project("wipeDown");
        let b = r.render(&p, 1.5, 32, 32);
        assert_eq!(px(&b, 32, 16, 4), [0, 0, 255], "oben neu");
        assert_eq!(px(&b, 32, 16, 28), [255, 0, 0], "unten noch alt");
        // Zoom: das neue Bild blendet ein (Deckkraft p)
        let p = two_clip_project("zoom");
        let b = r.render(&p, 1.5, 32, 32);
        assert!(near(px(&b, 32, 16, 16), [128, 0, 127], 4), "Zoom: halbe Deckkraft {:?}", px(&b, 32, 16, 16));
        assert_eq!(px(&r.render(&p, 2.5, 32, 32), 32, 16, 16), [0, 0, 255]);
        // Irisblende: in der Mitte schon neu, in der Ecke noch alt
        let p = two_clip_project("iris");
        let b = r.render(&p, 1.5, 32, 32);
        assert_eq!(px(&b, 32, 16, 16), [0, 0, 255], "Iris Mitte neu");
        assert_eq!(px(&b, 32, 1, 1), [255, 0, 0], "Iris Ecke alt");
        assert_eq!(px(&r.render(&p, 1.0, 32, 32), 32, 16, 16), [255, 0, 0], "Iris am Anfang: nur das alte Bild");
        // Weißblende: in der Mitte ganz weiß
        let p = two_clip_project("flash");
        assert_eq!(px(&r.render(&p, 1.5, 32, 32), 32, 16, 16), [255, 255, 255], "Weißblende Mitte");
        assert_eq!(px(&r.render(&p, 2.5, 32, 32), 32, 16, 16), [0, 0, 255]);
        assert!(px(&r.render(&p, 1.25, 32, 32), 32, 16, 16)[0] > 100, "Weißblende steigt an");
    }

    #[test]
    fn flip_turn_and_horizon_remap_the_picture() {
        let dir = std::env::temp_dir().join("video-superbe-tests");
        let path = dir.join("two_px.png");
        let mut im = image::RgbaImage::new(2, 1);
        im.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        im.put_pixel(1, 0, image::Rgba([0, 0, 255, 255]));
        im.save(&path).unwrap();
        let build = |fx: Effects| {
            let mut p = Project::default();
            let a = image_asset(&mut p, &path, 2.0, 1.0);
            let mut c = clip(a, 0.0, 2.0);
            c.fx = fx;
            p.tracks[0].clips.push(c);
            p
        };
        let mut r = Renderer::new();
        let b = r.render(&build(Effects::default()), 0.5, 32, 16);
        assert!(px(&b, 32, 4, 8)[0] > 200 && px(&b, 32, 28, 8)[2] > 200, "ohne Spiegeln: links rot, rechts blau");
        let b = r.render(&build(Effects { flip_h: Some(true), ..Default::default() }), 0.5, 32, 16);
        assert!(px(&b, 32, 4, 8)[2] > 200 && px(&b, 32, 28, 8)[0] > 200, "gespiegelt: links blau");
        let b = r.render(&build(Effects { quarter_turns: Some(1), ..Default::default() }), 0.5, 16, 32);
        assert!(px(&b, 16, 8, 4)[0] > 200 && px(&b, 16, 8, 28)[2] > 200, "90° im Uhrzeigersinn: rot oben, blau unten");
        // Horizont: keine schwarzen Ränder, obwohl gedreht wird
        let mut p = Project::default();
        let red = solid_png("horizon_red.png", 64, 36, [220, 30, 30]);
        let a = image_asset(&mut p, &red, 64.0, 36.0);
        let mut c = clip(a, 0.0, 2.0);
        c.fx.horizon = Some(5.0);
        p.tracks[0].clips.push(c);
        let b = r.render(&p, 0.5, 64, 36);
        for (x, y) in [(0, 0), (63, 0), (0, 35), (63, 35), (32, 18)] {
            assert!(px(&b, 64, x, y)[0] > 150, "Rand ({x},{y}) bleibt gefüllt: {:?}", px(&b, 64, x, y));
        }
    }

    #[test]
    fn greenscreen_and_lut_change_pixels() {
        let mut r = Renderer::new();
        let mut p = Project::default();
        let bg = solid_png("gs_blue.png", 16, 16, [20, 20, 220]);
        let green = solid_png("gs_green.png", 16, 16, [25, 190, 50]);
        let (ba, ga) = (image_asset(&mut p, &bg, 16.0, 16.0), image_asset(&mut p, &green, 16.0, 16.0));
        p.tracks[0].clips.push(clip(ba, 0.0, 2.0));
        let mut top = clip(ga, 0.0, 2.0);
        top.fx.chroma = Some(ChromaKey::default());
        p.tracks[1].clips.push(top);
        let b = r.render(&p, 0.5, 16, 16);
        assert!(near(px(&b, 16, 8, 8), [20, 20, 220], 6), "Grün ist ausgestanzt, der Hintergrund scheint durch: {:?}", px(&b, 16, 8, 8));
        // ohne Schlüssel deckt das Grün
        p.tracks[1].clips[0].fx.chroma = None;
        assert!(near(px(&r.render(&p, 0.5, 16, 16), 16, 8, 8), [25, 190, 50], 3));
        // LUT: Umkehrung
        let mut inv = String::from("LUT_3D_SIZE 2\n");
        for b in 0..2 {
            for g in 0..2 {
                for rr in 0..2 {
                    inv += &format!("{} {} {}\n", 1 - rr, 1 - g, 1 - b);
                }
            }
        }
        let cube = std::env::temp_dir().join("video-superbe-tests").join("inv.cube");
        std::fs::write(&cube, inv).unwrap();
        let mut q = Project::default();
        let orange = solid_png("lut_orange.png", 16, 16, [204, 102, 26]);
        let oa = image_asset(&mut q, &orange, 16.0, 16.0);
        let mut c = clip(oa, 0.0, 2.0);
        c.fx.lut_path = Some(cube.to_string_lossy().into_owned());
        q.tracks[0].clips.push(c);
        assert!(near(px(&r.render(&q, 0.5, 16, 16), 16, 8, 8), [51, 153, 229], 4), "Umkehr-LUT: {:?}", px(&r.render(&q, 0.5, 16, 16), 16, 8, 8));
        q.tracks[0].clips[0].fx.lut_amount = Some(0.5);
        assert!(near(px(&r.render(&q, 0.5, 16, 16), 16, 8, 8), [128, 128, 128], 6), "Stärke 50 % mischt");
    }

    fn title_style(text: &str) -> TitleStyle {
        TitleStyle { text: text.into(), font_size: 0.12, ..Default::default() }
    }

    #[test]
    fn title_typeface_features_change_the_image() {
        let mut r = Renderer::new();
        if !r.has_fonts() {
            return;
        }
        let sum = |i: &Img| i.px.chunks(4).map(|p| p[3] as u64).sum::<u64>();
        let base = r.render_title(&title_style("Hallo Welt"), 640, 360, None).unwrap();
        // Schatten: Standard ohne Hintergrund an
        let no_shadow = r.render_title(&TitleStyle { shadow: Some(false), ..title_style("Hallo Welt") }, 640, 360, None).unwrap();
        assert!(sum(&base) > sum(&no_shadow), "Schatten ist da");
        // Schatten mit Hintergrund nur auf Wunsch
        let bg = TitleStyle { background: true, ..title_style("Hallo Welt") };
        let bg_no = r.render_title(&bg, 640, 360, None).unwrap();
        let bg_sh = r.render_title(&TitleStyle { shadow: Some(true), ..bg.clone() }, 640, 360, None).unwrap();
        let dark = |i: &Img| i.px.chunks(4).filter(|p| p[0] < 10 && p[1] < 20 && p[3] > 200).count();
        assert!(dark(&bg_sh) > dark(&bg_no), "ausdrücklicher Schatten auch mit Hintergrund");
        // Kursiv, unterstrichen, fett aus: verändern das Bild
        let diff = |a: &Img, b: &Img| a.w != b.w || a.h != b.h || a.px != b.px;
        for st in [TitleStyle { italic: Some(true), ..title_style("Hallo Welt") }, TitleStyle { underline: Some(true), ..title_style("Hallo Welt") }, TitleStyle { bold: Some(false), ..title_style("Hallo Welt") }] {
            let img = r.render_title(&st, 640, 360, None).unwrap();
            assert!(diff(&img, &base), "{st:?} ändert das Bild");
        }
        let underlined = r.render_title(&TitleStyle { underline: Some(true), shadow: Some(false), ..title_style("Hallo Welt") }, 640, 360, None).unwrap();
        assert!(sum(&underlined) > sum(&no_shadow), "Unterstreichung fügt Pixel hinzu");
        // Laufweite weitet, Zeilenabstand streckt
        let wide = r.render_title(&TitleStyle { kerning: Some(0.3), ..title_style("Hallo Welt") }, 640, 360, None).unwrap();
        assert!(wide.w > base.w + 20, "Laufweite: {} vs {}", wide.w, base.w);
        let two = r.render_title(&title_style("Eins\nZwei"), 640, 360, None).unwrap();
        let spaced = r.render_title(&TitleStyle { line_spacing: Some(1.0), ..title_style("Eins\nZwei") }, 640, 360, None).unwrap();
        assert!(spaced.h > two.h + 20, "Zeilenabstand: {} vs {}", spaced.h, two.h);
        // Umbruch bei 90 % der Bildbreite; Laufschrift bricht nicht um
        let long = "Das ist ein ziemlich langer Titel der nicht in eine Zeile passt";
        let wrapped = r.render_title(&title_style(long), 640, 360, None).unwrap();
        assert!(wrapped.w <= (640.0 * 0.9) as usize + 40 && wrapped.h > base.h * 3 / 2, "umgebrochen: {}×{}", wrapped.w, wrapped.h);
        let ticker = r.render_title(&TitleStyle { ticker: Some(true), ..title_style(long) }, 640, 360, None).unwrap();
        assert!(ticker.w > 640 && ticker.h < wrapped.h, "Laufschrift in einer Zeile: {}×{}", ticker.w, ticker.h);
    }

    #[test]
    fn title_alignment_outline_and_typewriter() {
        let mut r = Renderer::new();
        if !r.has_fonts() {
            return;
        }
        // Ausrichtung: erste Zeile kurz, zweite lang
        let range = |img: &Img, y0: usize, y1: usize| {
            let xs: Vec<usize> = (y0..y1).flat_map(|y| (0..img.w).map(move |x| (x, y))).filter(|&(x, y)| img.px[(y * img.w + x) * 4 + 3] > 120).map(|p| p.0).collect();
            (*xs.iter().min().unwrap_or(&0), *xs.iter().max().unwrap_or(&0))
        };
        let st = |l: bool, rt: bool| TitleStyle { left_aligned: l, right_aligned: rt.then_some(true), shadow: Some(false), bold: Some(false), ..title_style("i\nWWWWWWWW") };
        let left = r.render_title(&st(true, false), 640, 360, None).unwrap();
        let right = r.render_title(&st(false, true), 640, 360, None).unwrap();
        let (h2, hr) = (left.h / 3, right.h / 3);
        let (lmin, _) = range(&left, 0, h2);
        let (rmin, rmax) = range(&right, 0, hr);
        assert!(lmin < left.w / 6, "linksbündig: erste Zeile am linken Rand ({lmin})");
        assert!(rmax > right.w * 5 / 6 && rmin > right.w / 2, "rechtsbündig: erste Zeile am rechten Rand ({rmin}..{rmax} von {})", right.w);
        // Umrandung: Standard schwarz um weißen Text; „nur Rahmen“ lässt das Innere leer
        let big = |o: Option<bool>, h: Option<bool>, w: f64| TitleStyle { outline: o, outline_hollow: h, outline_width: Some(w), shadow: Some(false), font_size: 0.2, ..title_style("HHH") };
        let plain = r.render_title(&big(None, None, 6.0), 640, 360, None).unwrap();
        let outlined = r.render_title(&big(Some(true), None, 6.0), 640, 360, None).unwrap();
        let hollow = r.render_title(&big(Some(true), Some(true), 6.0), 640, 360, None).unwrap();
        let black = |i: &Img| i.px.chunks(4).filter(|p| p[0] < 30 && p[3] > 200).count();
        assert!(black(&plain) == 0 && black(&outlined) > 300, "schwarze Umrandung: {}", black(&outlined));
        // „nur Rahmen“: dort, wo die ausgefüllte Variante weiße Füllung hat, ist der Rahmen innen leer
        assert_eq!((outlined.w, outlined.h), (hollow.w, hollow.h));
        let holes = outlined.px.chunks(4).zip(hollow.px.chunks(4)).filter(|(o, h)| o[0] > 225 && o[3] > 225 && h[3] < 40).count();
        assert!(holes > 100, "nur Rahmen: Inneres der Buchstaben ist leer ({holes} Pixel)");
        let thin = r.render_title(&big(Some(true), None, 2.0), 640, 360, None).unwrap();
        assert!(black(&outlined) > black(&thin), "breitere Umrandung hat mehr schwarze Pixel");
        // Schreibmaschine: nichts sichtbar bei 0, alles bei „alle“, dazwischen weniger; die Größe bleibt gleich
        let full = r.render_title(&title_style("ABCDEFGH"), 640, 360, None).unwrap();
        let none = r.render_title(&title_style("ABCDEFGH"), 640, 360, Some(0)).unwrap();
        let half = r.render_title(&title_style("ABCDEFGH"), 640, 360, Some(4)).unwrap();
        let alpha = |i: &Img| i.px.chunks(4).map(|p| p[3] as u64).sum::<u64>();
        assert!(alpha(&none) == 0 && alpha(&half) > 0 && alpha(&half) < alpha(&full), "Schreibmaschine: {} < {} < {}", alpha(&none), alpha(&half), alpha(&full));
        assert!(none.w == full.w && half.h == full.h, "das Layout bleibt gleich");
    }

    #[test]
    fn title_motion_credits_ticker_and_animation_in_the_compositor() {
        let mut r = Renderer::new();
        if !r.has_fonts() {
            return;
        }
        let mk = |st: TitleStyle| {
            let mut p = Project::default();
            p.render_width = 320;
            p.render_height = 180;
            p.tracks[0].clips.push(Clip { content: ClipContent::Title(Wrap { v: TitleStyle { shadow: Some(false), ..st } }), start: 0.0, duration: 4.0, ..Default::default() });
            p
        };
        let (w, h) = (320usize, 180usize);
        // Abspann: am Anfang unterhalb des Bildes, in der Mitte im Bild, am Ende oberhalb
        let p = mk(TitleStyle { credits: Some(true), ..title_style("Abspann\nDanke") });
        assert_eq!(lit(&r.render(&p, 0.0, w, h), 0, w, 0, h, w), 0, "Abspann beginnt außerhalb");
        assert!(lit(&r.render(&p, 2.0, w, h), 0, w, 0, h, w) > 50, "Abspann in der Mitte sichtbar");
        let early = r.render(&p, 1.0, w, h);
        let late = r.render(&p, 3.0, w, h);
        let rows = |b: &[u8]| (0..h).filter(|&y| lit(b, 0, w, y, y + 1, w) > 0).map(|y| y as i32).sum::<i32>() / (0..h).filter(|&y| lit(b, 0, w, y, y + 1, w) > 0).count().max(1) as i32;
        assert!(rows(&early) > rows(&late), "Abspann läuft nach oben ({} > {})", rows(&early), rows(&late));
        // Laufschrift: von rechts nach links
        let p = mk(TitleStyle { ticker: Some(true), ..title_style("Laufschrift") });
        assert_eq!(lit(&r.render(&p, 0.0, w, h), 0, w, 0, h, w), 0, "Laufschrift beginnt rechts außerhalb");
        let t1 = r.render(&p, 1.0, w, h);
        let t3 = r.render(&p, 3.0, w, h);
        let cols = |b: &[u8]| { let xs: Vec<usize> = (0..w).filter(|&x| lit(b, x, x + 1, 0, h, w) > 0).collect(); xs.iter().sum::<usize>() / xs.len().max(1) };
        assert!(cols(&t1) > cols(&t3), "Laufschrift wandert nach links ({} > {})", cols(&t1), cols(&t3));
        // Einlaufen von unten: am Anfang unsichtbar, danach an Ort und Stelle; Auslaufen am Ende
        let p = mk(TitleStyle { anim_in: Some("flyUp".into()), anim_out: Some("zoom".into()), anim_dur: Some(1.0), ..title_style("Hallo") });
        assert_eq!(lit(&r.render(&p, 0.0, w, h), 0, w, 0, h, w), 0, "Einlaufen: Anfang unsichtbar");
        let mid = lit(&r.render(&p, 2.0, w, h), 0, w, 0, h, w);
        assert!(mid > 40, "Titel steht");
        let fading = lit(&r.render(&p, 3.9, w, h), 0, w, 0, h, w);
        assert!(fading < mid / 2, "Auslaufen: schrumpft und blendet aus ({fading} < {mid})");
        // Schreibmaschine im Compositor: der Anfang zeigt noch keine Zeichen, später alle
        let p = mk(TitleStyle { anim_in: Some("typewriter".into()), anim_dur: Some(1.0), ..title_style("ABCDEFGH") });
        let t0 = lit(&r.render(&p, 0.0, w, h), 0, w, 0, h, w);
        let t_half = lit(&r.render(&p, 0.5, w, h), 0, w, 0, h, w);
        let t_full = lit(&r.render(&p, 2.0, w, h), 0, w, 0, h, w);
        assert!(t0 == 0 && t_half > 0 && t_half < t_full, "Schreibmaschine {t0} < {t_half} < {t_full}");
        // Platzhalter: Timecode ändert sich mit der Zeit
        let p = mk(TitleStyle { credits: None, ..title_style("{timecode}") });
        assert_ne!(r.render(&p, 0.0, w, h), r.render(&p, 1.0, w, h), "Timecode zählt");
    }

    thread_local! {
        pub(super) static SERIAL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    /// Das parallele Rechnen in Bändern liefert Pixel für Pixel dasselbe wie der einfädige Durchlauf.
    #[test]
    fn parallel_rendering_equals_serial_rendering() {
        let mut p = Project::default();
        p.render_width = 200;
        p.render_height = 120;
        let png = solid_png("par_a.png", 40, 24, [200, 90, 30]);
        let img = std::env::temp_dir().join("video-superbe-tests").join("par_grad.png");
        image::RgbaImage::from_fn(97, 61, |x, y| image::Rgba([(x * 2) as u8, (y * 4) as u8, ((x * y) & 255) as u8, 255])).save(&img).unwrap();
        let a = image_asset(&mut p, &img, 97.0, 61.0);
        let b = image_asset(&mut p, &png, 40.0, 24.0);
        p.tracks[0].clips.push(clip(a, 0.0, 4.0));
        let mut c2 = clip(b, 2.0, 4.0);
        c2.transition = "crossfade".into();
        p.tracks[0].clips.push(c2);
        let mut r = Renderer::new();
        let variants: Vec<(&str, Box<dyn Fn(&mut Project)>)> = vec![
            ("plain", Box::new(|_| {})),
            ("colour+vignette", Box::new(|p| {
                let f = &mut p.tracks[0].clips[0].fx;
                f.saturation = 1.5;
                f.contrast = 1.2;
                f.vignette = 0.6;
            })),
            ("blur", Box::new(|p| p.tracks[0].clips[0].fx.blur = 9.0)),
            ("sharpen", Box::new(|p| p.tracks[0].clips[0].fx.sharpness = 1.5)),
            ("rotated", Box::new(|p| p.tracks[0].clips[0].transform.rotation = 17.0)),
        ];
        for (name, f) in &variants {
            let mut q = p.clone();
            f(&mut q);
            for t in [1.0, 2.5] {
                SERIAL.with(|s| s.set(true));
                let one = r.render(&q, t, 200, 120);
                SERIAL.with(|s| s.set(false));
                let many = r.render(&q, t, 200, 120);
                assert!(one == many, "{name} bei {t} s: Bänder liefern andere Pixel");
            }
        }
    }

    #[test]
    fn sharpen_radius_follows_the_preview_scale() {
        // gleiche Kante, einmal in voller, einmal in halber Größe: die Überschwinger bleiben im Bild gleich breit
        let mk = |w: usize, h: usize| {
            let mut buf = vec![0u8; w * h * 4];
            for y in 0..h {
                for x in 0..w {
                    let v = if x < w / 2 { 80 } else { 160 };
                    buf[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&[v, v, v, 255]);
                }
            }
            buf
        };
        let width_of_overshoot = |w: usize, h: usize, scale: f32| {
            let mut b = mk(w, h);
            sharpen_layer(&mut b, w, h, 2.0, scale);
            let row = h / 2;
            (0..w).filter(|x| (b[(row * w + x) * 4] as i32 - if *x < w / 2 { 80 } else { 160 }).abs() > 2).count() as f32 / w as f32
        };
        let (full, half) = (width_of_overshoot(200, 40, 1.0), width_of_overshoot(100, 20, 0.5));
        assert!((full - half).abs() < 0.01, "Breite der Kantenüberhöhung: voll {full}, halb {half}");
    }

    /// Messung (cargo test bench_render -- --ignored --nocapture): ms je 1080p-Bild für typische Fälle.
    #[test]
    #[ignore]
    fn bench_render() {
        let dir = std::env::temp_dir().join("video-superbe-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("bench_grad.png");
        image::RgbaImage::from_fn(1920, 1080, |x, y| image::Rgba([(x / 8) as u8, (y / 5) as u8, ((x ^ y) & 255) as u8, 255])).save(&png).unwrap();
        let cube = dir.join("bench.cube");
        let n = 17;
        let mut t = format!("LUT_3D_SIZE {n}
");
        for b in 0..n {
            for g in 0..n {
                for r in 0..n {
                    t += &format!("{} {} {}
", r as f32 / 16.0, (g as f32 / 16.0).powf(0.9), b as f32 / 16.0);
                }
            }
        }
        std::fs::write(&cube, t).unwrap();
        let mk = |f: &dyn Fn(&mut Project)| {
            let mut p = Project::default();
            p.render_width = 1920;
            p.render_height = 1080;
            let a = image_asset(&mut p, &png, 1920.0, 1080.0);
            let b = image_asset(&mut p, &png, 1920.0, 1080.0);
            p.tracks[0].clips.push(clip(a, 0.0, 4.0));
            let mut c2 = clip(b, 3.0, 4.0);
            c2.transition = "crossfade".into();
            p.tracks[0].clips.push(c2);
            f(&mut p);
            p
        };
        let cases: Vec<(&str, f64, Project)> = vec![
            ("einfach", 1.0, mk(&|_| {})),
            ("Farbe", 1.0, mk(&|p| p.tracks[0].clips[0].fx.saturation = 1.4)),
            ("LUT", 1.0, mk(&|p| p.tracks[0].clips[0].fx.lut_path = Some(cube.to_string_lossy().into_owned()))),
            ("Schärfe", 1.0, mk(&|p| p.tracks[0].clips[0].fx.sharpness = 1.0)),
            ("Weichzeichner 8", 1.0, mk(&|p| p.tracks[0].clips[0].fx.blur = 8.0)),
            ("Vignette", 1.0, mk(&|p| p.tracks[0].clips[0].fx.vignette = 0.5)),
            ("Überblendung", 3.5, mk(&|_| {})),
        ];
        let mut r = Renderer::new();
        for (name, t, p) in &cases {
            for (w, h) in [(1920usize, 1080usize), (1280, 720)] {
                r.render(p, *t, w, h);
                let t0 = std::time::Instant::now();
                for _ in 0..5 {
                    r.render(p, *t, w, h);
                }
                eprintln!("BENCH {name:<16} {w}x{h}: {:6.1} ms", t0.elapsed().as_secs_f64() * 200.0);
            }
        }
    }
}
