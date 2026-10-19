//! Oberfläche: Farben und Maße folgen der macOS-Version (Theme.swift), Symbole kommen aus der in
//! Windows 11 eingebauten Schrift „Segoe Fluent Icons“.

mod animator;
mod inspector;
mod monitor;
mod overlay;
pub use animator::AnimGeom;
pub use overlay::{OverlayBox, OverlayDrag};
pub use timeline::{EdgePreview, TlState};
pub use monitor::PreviewState;
mod pool;
pub(crate) mod timeline;
pub(crate) mod volume;
mod toolbar;

use eframe::egui::{self, Color32, FontFamily, FontId, Response, Sense, Ui, Vec2};

pub const BG: Color32 = Color32::from_rgb(33, 36, 41);
pub const PANEL: Color32 = Color32::from_rgb(43, 46, 51);
pub const BAR: Color32 = Color32::from_rgb(61, 66, 71);
pub const TRACK: Color32 = Color32::from_rgb(28, 31, 36);
pub const TRACK_ALT: Color32 = Color32::from_rgb(36, 38, 43);
pub const VIDEO: Color32 = Color32::from_rgb(64, 107, 158);
pub const TITLE: Color32 = Color32::from_rgb(158, 64, 158);
pub const AUDIO: Color32 = Color32::from_rgb(56, 128, 71);
pub const HEADER_ALT: Color32 = Color32::from_rgb(54, 58, 63);
pub const ACCENT: Color32 = Color32::from_rgb(255, 97, 64);
pub const SECONDARY: Color32 = Color32::from_gray(150);

// Segoe Fluent Icons
pub const I_SAVE: &str = "\u{E74E}";
pub const I_OPEN: &str = "\u{E8E5}";
pub const I_UNDO: &str = "\u{E7A7}";
pub const I_REDO: &str = "\u{E7A6}";
pub const I_DELETE: &str = "\u{E74D}";
pub const I_CUT: &str = "\u{E8C6}";
pub const I_TEXT: &str = "\u{E8D2}";
pub const I_FIT: &str = "\u{E740}";
pub const I_ADD: &str = "\u{E710}";
pub const I_MIC: &str = "\u{E720}";
pub const I_ZOOM_IN: &str = "\u{E8A3}";
pub const I_ZOOM_OUT: &str = "\u{E71F}";
pub const I_PLAY: &str = "\u{E768}";
pub const I_PAUSE: &str = "\u{E769}";
pub const I_PREV: &str = "\u{E892}";
pub const I_NEXT: &str = "\u{E893}";
pub const I_STEP_BACK: &str = "\u{E76B}";
pub const I_STEP_FWD: &str = "\u{E76C}";
pub const I_VOLUME: &str = "\u{E767}";
pub const I_MUTE: &str = "\u{E74F}";
pub const I_VIEW: &str = "\u{E7B3}";
pub const I_HIDE: &str = "\u{E7B4}";
pub const I_LOCK: &str = "\u{E72E}";
pub const I_UNLOCK: &str = "\u{E785}";
pub const I_EXPORT: &str = "\u{E72D}";
pub const I_ASPECT: &str = "\u{E799}";
pub const I_PIN: &str = "\u{E718}";

pub fn icons() -> FontFamily {
    FontFamily::Name("icons".into())
}

/// Schriften: Segoe UI (Text), Segoe UI Symbol/Emoji (Fallback), Segoe Fluent Icons (eigene Familie „icons“).
pub fn setup(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    let read = |f: &str| std::fs::read(format!("{windir}\\Fonts\\{f}")).ok();
    let mut fonts = egui::FontDefinitions::default();
    for (name, file) in [("segoeui", "segoeui.ttf"), ("seguisym", "seguisym.ttf"), ("seguiemj", "seguiemj.ttf")] {
        if let Some(data) = read(file) {
            fonts.font_data.insert(name.into(), std::sync::Arc::new(egui::FontData::from_owned(data)));
            for fam in [FontFamily::Proportional, FontFamily::Monospace] {
                fonts.families.entry(fam).or_default().push(name.into());
            }
        }
    }
    // Segoe UI als Hauptschrift vor die eingebaute stellen
    if fonts.font_data.contains_key("segoeui") {
        let p = fonts.families.entry(FontFamily::Proportional).or_default();
        p.retain(|n| n != "segoeui");
        p.insert(0, "segoeui".into());
    }
    let icon_data = read("SegoeIcons.ttf").or_else(|| read("segmdl2.ttf"));
    if let Some(d) = icon_data {
        fonts.font_data.insert("icons".into(), std::sync::Arc::new(egui::FontData::from_owned(d)));
        fonts.families.insert(icons(), vec!["icons".into()]);
    } else {
        fonts.families.insert(icons(), vec!["seguisym".into()]);
    }
    ctx.set_fonts(fonts);

    ctx.global_style_mut(|st| {
        let v = &mut st.visuals;
        v.panel_fill = PANEL;
        v.window_fill = PANEL;
        v.extreme_bg_color = TRACK;
        v.faint_bg_color = TRACK_ALT;
        v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, Color32::from_black_alpha(90));
        v.widgets.inactive.weak_bg_fill = BAR;
        v.widgets.inactive.bg_fill = Color32::from_rgb(22, 24, 28);
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(78, 84, 90);
        v.selection.bg_fill = ACCENT.gamma_multiply(0.7);
        st.spacing.item_spacing = Vec2::new(8.0, 6.0);
        st.spacing.button_padding = Vec2::new(8.0, 3.0);
        st.interaction.tooltip_delay = 0.4;
    });
}

pub fn timecode(t: f64, fps: i64) -> String {
    let fps = fps.max(1);
    let f = (t.max(0.0) * fps as f64).round() as i64;
    format!("{:02}:{:02}:{:02}:{:02}", f / (fps * 3600), f / (fps * 60) % 60, f / fps % 60, f % fps)
}

/// Zeitangabe ohne Bildanteil (für Lineal und Dauer).
pub fn clock(t: f64) -> String {
    let s = t.max(0.0).round() as i64;
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// Flache Symbolschaltfläche; `active` färbt sie in der Akzentfarbe.
pub fn icon_button(ui: &mut Ui, glyph: &str, tip: &str, active: bool) -> Response {
    icon_button_sized(ui, glyph, tip, active, 15.0, Vec2::new(28.0, 24.0), true)
}

pub fn icon_button_sized(ui: &mut Ui, glyph: &str, tip: &str, active: bool, size: f32, dim: Vec2, enabled: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(dim, if enabled { Sense::click() } else { Sense::hover() });
    let resp = resp.on_hover_text(tip);
    let hot = enabled && resp.hovered();
    if hot || active {
        ui.painter().rect_filled(rect, 5.0, if active { ACCENT.gamma_multiply(0.22) } else { Color32::from_white_alpha(18) });
    }
    let col = if !enabled {
        Color32::from_gray(90)
    } else if active {
        ACCENT
    } else if hot {
        Color32::WHITE
    } else {
        Color32::from_gray(205)
    };
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, glyph, FontId::new(size, icons()), col);
    resp
}

/// Textschaltfläche mit Symbol davor (Werkzeugleiste oben).
pub fn tool_button(ui: &mut Ui, glyph: &str, label: &str, tip: &str, enabled: bool) -> Response {
    let font = FontId::proportional(13.0);
    let gal = ui.painter().layout_no_wrap(label.to_string(), font.clone(), Color32::WHITE);
    let w = 12.0 + 16.0 + 6.0 + gal.size().x + 12.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 28.0), if enabled { Sense::click() } else { Sense::hover() });
    let resp = resp.on_hover_text(tip);
    if enabled && resp.hovered() {
        ui.painter().rect_filled(rect, 6.0, Color32::from_white_alpha(20));
    }
    let col = if enabled { Color32::from_gray(225) } else { Color32::from_gray(100) };
    ui.painter().text(egui::pos2(rect.left() + 12.0 + 8.0, rect.center().y), egui::Align2::CENTER_CENTER, glyph, FontId::new(15.0, icons()), col);
    ui.painter().text(egui::pos2(rect.left() + 12.0 + 22.0, rect.center().y), egui::Align2::LEFT_CENTER, label, font, col);
    resp
}

pub fn pct(x: f64) -> String {
    format!("{:.0} %", x * 100.0)
}
pub fn pct1(x: f64) -> String {
    format!("{:.1} %", x * 100.0)
}
pub fn secs(x: f64) -> String {
    format!("{x:.1} s")
}
pub fn px(x: f64) -> String {
    format!("{x:.0} px")
}
pub fn deg(x: f64) -> String {
    format!("{x:.1}°")
}

/// Beschriftung links, Wert rechts, darunter der Regler (wie `slider` im Inspektor der Mac-Version).
pub fn slider_row(ui: &mut Ui, title: &str, value: &mut f64, range: std::ops::RangeInclusive<f64>, fmt: fn(f64) -> String) -> bool {
    let (label_rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 16.0), Sense::hover());
    let p = ui.painter();
    p.text(label_rect.left_center(), egui::Align2::LEFT_CENTER, title, FontId::proportional(12.0), Color32::from_gray(215));
    let mut changed = false;
    ui.spacing_mut().slider_width = (ui.available_width() - 4.0).max(60.0);
    changed |= ui.add(egui::Slider::new(value, range).show_value(false)).changed();
    ui.painter().text(label_rect.right_center(), egui::Align2::RIGHT_CENTER, fmt(*value), FontId::monospace(11.5), SECONDARY);
    changed
}
