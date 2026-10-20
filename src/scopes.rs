//! Messinstrumente (Scopes.swift, ScopesView.swift): Histogramm und RGB-Parade aus dem Bild im Programmmonitor. Sie ersetzen
//! den Medienpool, bis sie ausgeschaltet werden.

use crate::app::App;
use crate::ui::{ACCENT, BAR, PANEL, SECONDARY};
use eframe::egui::{self, Color32, FontId, Pos2, Rect, RichText, Stroke, Ui, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ScopeMode {
    #[default]
    Off,
    Histogram,
    Parade,
    Both,
}

impl ScopeMode {
    pub fn label(self) -> &'static str {
        match self {
            ScopeMode::Off => "Aus",
            ScopeMode::Histogram => "Histogramm",
            ScopeMode::Parade => "RGB-Parade",
            ScopeMode::Both => "Alle nebeneinander",
        }
    }
}

pub struct ScopeData {
    /// 256 Klassen je Kanal, auf 0…1 normiert (Rot, Grün, Blau, Luma)
    pub hist: Vec<Vec<f32>>,
    /// RGB-Parade als RGBA-Bild (Breite 3×cols, Höhe 256)
    pub parade: Vec<u8>,
    pub cols: usize,
    /// Anteil der Pixel in den Lichtern (≥ 250) bzw. Tiefen (≤ 5)
    pub clipped_high: f32,
    pub clipped_low: f32,
}

/// Wertet ein RGBA-Bild aus; es wird auf höchstens `max_w` Spalten verkleinert.
pub fn measure(rgba: &[u8], w: usize, h: usize, intensity: f32, max_w: usize) -> Option<ScopeData> {
    if w == 0 || h == 0 || rgba.len() < w * h * 4 {
        return None;
    }
    let step = (w / max_w.max(1)).max(1);
    let cols = w.div_ceil(step);
    let mut hist = vec![vec![0f32; 256]; 4];
    let mut par = vec![0f32; cols * 3 * 256];
    let (mut total, mut hi, mut lo) = (0f32, 0f32, 0f32);
    for y in (0..h).step_by(step) {
        let mut cx = 0;
        for x in (0..w).step_by(step) {
            let o = (y * w + x) * 4;
            let (r, g, b) = (rgba[o] as usize, rgba[o + 1] as usize, rgba[o + 2] as usize);
            let l = (r * 54 + g * 183 + b * 19) >> 8; // Rec. 709
            hist[0][r] += 1.0;
            hist[1][g] += 1.0;
            hist[2][b] += 1.0;
            hist[3][l] += 1.0;
            par[(255 - r) * cols * 3 + cx] += 1.0;
            par[(255 - g) * cols * 3 + cols + cx] += 1.0;
            par[(255 - b) * cols * 3 + 2 * cols + cx] += 1.0;
            total += 1.0;
            if r >= 250 || g >= 250 || b >= 250 {
                hi += 1.0;
            }
            if r <= 5 && g <= 5 && b <= 5 {
                lo += 1.0;
            }
            cx += 1;
        }
    }
    for hc in &mut hist {
        let m = hc.iter().copied().fold(1.0f32, f32::max);
        for v in hc.iter_mut() {
            *v /= m;
        }
    }
    // Parade: Helligkeit je Punkt aus der Häufigkeit (Wurzel hebt seltene Werte an)
    let rows = h / step + 1;
    let scale = intensity * 6.0 / rows.max(1) as f32 * 40.0;
    let mut img = vec![0u8; cols * 3 * 256 * 4];
    for y in 0..256 {
        for x in 0..cols * 3 {
            let v = (par[y * cols * 3 + x] * scale).min(1.0);
            let a = (v.sqrt() * 255.0).min(255.0) as u8;
            let ch = x / cols;
            let i = (y * cols * 3 + x) * 4;
            img[i] = if ch == 0 { a } else { a / 6 };
            img[i + 1] = if ch == 1 { a } else { a / 6 };
            img[i + 2] = if ch == 2 { a } else { a / 6 };
            img[i + 3] = 255;
            if a == 0 {
                img[i] = 12;
                img[i + 1] = 12;
                img[i + 2] = 12;
            }
        }
    }
    Some(ScopeData { hist, parade: img, cols, clipped_high: hi / total.max(1.0), clipped_low: lo / total.max(1.0) })
}

impl App {
    pub(crate) fn set_scope(&mut self, m: ScopeMode) {
        self.scope_mode = m;
        self.scope_data = None;
        self.scope_tex = None;
        if m != ScopeMode::Off {
            self.refresh_scopes(true);
        }
    }

    /// Misst das zuletzt gezeichnete Monitorbild (höchstens fünfmal pro Sekunde).
    pub(crate) fn refresh_scopes(&mut self, force: bool) {
        if self.scope_mode == ScopeMode::Off || self.source.is_some() {
            return;
        }
        if !force && self.scope_last.elapsed().as_millis() < 200 {
            return;
        }
        self.scope_last = std::time::Instant::now();
        let Some((rgba, w, h)) = self.preview.last.as_ref() else { return };
        self.scope_data = measure(rgba, *w, *h, 0.5, 200);
        self.scope_tex = None;
    }

    pub(crate) fn scopes_panel(&mut self, ui: &mut Ui) {
        let full = ui.available_rect_before_wrap();
        ui.allocate_rect(full, egui::Sense::hover());
        ui.painter().rect_filled(full, 0.0, PANEL);
        let bar = Rect::from_min_size(full.min, Vec2::new(full.width(), 30.0));
        ui.painter().rect_filled(bar, 0.0, BAR);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(Vec2::new(12.0, 3.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
        for (m, n) in [(ScopeMode::Histogram, "Histogramm"), (ScopeMode::Parade, "RGB-Parade"), (ScopeMode::Both, "Beide")] {
            if child.selectable_label(self.scope_mode == m, n).clicked() {
                self.set_scope(m);
            }
        }
        child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("✕").on_hover_text("Schließen – der Medienpool kommt zurück").clicked() {
                self.set_scope(ScopeMode::Off);
            }
        });
        let body = Rect::from_min_max(Pos2::new(full.left() + 14.0, bar.bottom() + 14.0), Pos2::new(full.right() - 14.0, full.bottom() - 10.0));
        let w = body.width().max(160.0);
        let Some(d) = &self.scope_data else {
            ui.painter().text(body.left_top(), egui::Align2::LEFT_TOP, "Kein Bild – Film in die Zeitleiste legen.", FontId::proportional(12.0), SECONDARY);
            return;
        };
        let both = self.scope_mode == ScopeMode::Both;
        let mut y = body.top();
        let p = ui.painter();
        if self.scope_mode != ScopeMode::Parade {
            let h = if both { 130.0 } else { 220.0 };
            p.text(Pos2::new(body.left(), y), egui::Align2::LEFT_TOP, "Histogramm", FontId::proportional(10.5), SECONDARY);
            let r = Rect::from_min_size(Pos2::new(body.left(), y + 16.0), Vec2::new(w, h));
            p.rect_filled(r, 4.0, Color32::from_black_alpha(100));
            for (c, col) in [(0usize, Color32::RED), (1, Color32::GREEN), (2, Color32::from_rgb(70, 110, 255)), (3, Color32::WHITE)] {
                let a = if c == 3 { 0.22 } else { 0.4 };
                let fill = col.gamma_multiply(a);
                for i in 0..256 {
                    let (x0, x1) = (r.left() + i as f32 / 256.0 * w, r.left() + (i + 1) as f32 / 256.0 * w);
                    let hh = d.hist[c][i] * h;
                    if hh > 0.3 {
                        p.rect_filled(Rect::from_min_max(Pos2::new(x0, r.bottom() - hh), Pos2::new(x1 + 0.5, r.bottom())), 0.0, fill);
                    }
                }
            }
            for q in [0.25, 0.5, 0.75] {
                p.line_segment([Pos2::new(r.left() + w * q, r.top()), Pos2::new(r.left() + w * q, r.bottom())], Stroke::new(0.5, Color32::from_white_alpha(30)));
            }
            y = r.bottom() + 4.0;
            let mut warn = vec![];
            if d.clipped_high > 0.01 {
                warn.push(format!("☀ Lichter {:.0} % abgeschnitten", d.clipped_high * 100.0));
            }
            if d.clipped_low > 0.05 {
                warn.push(format!("☾ Tiefen {:.0} % schwarz", d.clipped_low * 100.0));
            }
            if !warn.is_empty() {
                p.text(Pos2::new(body.left(), y), egui::Align2::LEFT_TOP, warn.join("    "), FontId::proportional(10.5), Color32::from_rgb(255, 214, 10));
                y += 16.0;
            }
            y += 10.0;
        }
        if self.scope_mode != ScopeMode::Histogram {
            let h = if both { 150.0 } else { 260.0 };
            ui.painter().text(Pos2::new(body.left(), y), egui::Align2::LEFT_TOP, "RGB-Parade", FontId::proportional(10.5), SECONDARY);
            let r = Rect::from_min_size(Pos2::new(body.left(), y + 16.0), Vec2::new(w, h));
            if self.scope_tex.is_none() {
                let ci = egui::ColorImage::from_rgba_unmultiplied([d.cols * 3, 256], &d.parade);
                self.scope_tex = Some(ui.ctx().load_texture("scope_parade", ci, egui::TextureOptions::NEAREST));
            }
            if let Some(t) = &self.scope_tex {
                ui.painter().image(t.id(), r, Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)), Color32::WHITE);
            }
            for q in [0.0, 0.25, 0.5, 0.75, 1.0] {
                let yy = r.top() + h * q;
                ui.painter().line_segment([Pos2::new(r.left(), yy), Pos2::new(r.right(), yy)], Stroke::new(0.5, Color32::from_white_alpha(38)));
            }
        }
        let _ = ACCENT;
        let _ = RichText::new("");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(r: u8, g: u8, b: u8, w: usize, h: usize) -> Vec<u8> {
        (0..w * h).flat_map(|_| [r, g, b, 255]).collect()
    }

    #[test]
    fn histogram_of_solid_colours() {
        let d = measure(&solid(255, 0, 0, 64, 36), 64, 36, 0.5, 200).unwrap();
        assert_eq!((d.hist[0][255], d.hist[1][0], d.hist[2][0]), (1.0, 1.0, 1.0), "Rot ganz oben, Grün und Blau ganz unten");
        assert!(d.hist[0][254] == 0.0 && d.hist[3][53] == 1.0, "Luma von Rot (54·255/256 = 53)");
        assert!(d.clipped_high > 0.99 && d.clipped_low < 0.01);
        let black = measure(&solid(0, 0, 0, 64, 36), 64, 36, 0.5, 200).unwrap();
        assert!(black.clipped_low > 0.99 && black.clipped_high < 0.01, "Tiefen");
        let grey = measure(&solid(128, 128, 128, 64, 36), 64, 36, 0.5, 200).unwrap();
        assert!(grey.clipped_low == 0.0 && grey.clipped_high == 0.0);
        assert!(measure(&[], 0, 0, 0.5, 200).is_none() && measure(&[0; 8], 4, 4, 0.5, 200).is_none());
    }

    #[test]
    fn parade_shows_the_levels_in_the_right_channel_band() {
        // Hälfte links reines Rot, Hälfte rechts reines Blau
        let (w, h) = (100, 40);
        let img: Vec<u8> = (0..w * h).flat_map(|i| if i % w < 50 { [255, 0, 0, 255] } else { [0, 0, 255, 255] }).collect();
        let d = measure(&img, w, h, 0.5, 200).unwrap();
        assert_eq!(d.cols, 100);
        let px = |x: usize, y: usize| { let i = (y * d.cols * 3 + x) * 4; [d.parade[i], d.parade[i + 1], d.parade[i + 2]] };
        assert!(px(10, 0)[0] > 150, "Rot-Band links: Rot 255 liegt in der obersten Zeile: {:?}", px(10, 0));
        assert!(px(d.cols * 2 + 80, 0)[2] > 150, "Blau-Band rechts oben");
        assert!(px(d.cols + 10, 255)[1] > 150, "Grün 0 liegt ganz unten im Grün-Band");
        assert!(px(d.cols + 10, 0)[1] < 60, "Grün hat oben nichts");
    }

    #[test]
    fn wide_images_are_subsampled() {
        let d = measure(&solid(10, 20, 30, 1000, 20), 1000, 20, 0.5, 200).unwrap();
        assert!(d.cols <= 200 && d.cols >= 100, "{}", d.cols);
    }
}
