//! Medienpool („Import“): Kacheln mit Vorschaubild, Dauer und Namen; Doppelklick legt in die Timeline,
//! Ziehen auf die Timeline legt an der Zielposition ab.

use super::*;
use crate::app::App;
use crate::model::MediaAsset;
use crate::video;
use eframe::egui::{Align2, Rect, RichText, Stroke, StrokeKind};
use uuid::Uuid;

const TILE_W: f32 = 130.0;
const TILE_H: f32 = 80.0;

impl App {
    /// Vorschaubild eines Mediums. Wird im Hintergrund erzeugt (HEVC-Videos brauchen dafür einen Moment);
    /// bis es fertig ist, liefert die Funktion `None`.
    pub(crate) fn thumb(&mut self, ctx: &egui::Context, a: &MediaAsset) -> Option<egui::TextureHandle> {
        if let Some(t) = self.thumbs.get(&a.id) {
            return t.clone();
        }
        if let Some(rx) = self.thumb_jobs.get(&a.id) {
            use std::sync::mpsc::TryRecvError;
            return match rx.try_recv() {
                Ok(res) => {
                    self.thumb_jobs.remove(&a.id);
                    let tex = res.map(|(w, h, px)| {
                        let ci = egui::ColorImage::from_rgba_unmultiplied([w, h], &px);
                        ctx.load_texture(format!("thumb-{}", a.id), ci, egui::TextureOptions::LINEAR)
                    });
                    self.thumbs.insert(a.id, tex.clone());
                    tex
                }
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    self.thumb_jobs.remove(&a.id);
                    self.thumbs.insert(a.id, None);
                    None
                }
            };
        }
        if self.thumb_jobs.len() >= 2 {
            return None; // höchstens zwei gleichzeitig; die Kachel fragt im nächsten Bild erneut
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let asset = a.clone();
        let wake = ctx.clone();
        std::thread::spawn(move || {
            let res = (|| {
                let rgba = if asset.is_image {
                    image::open(&asset.path).ok()?.thumbnail(260, 160).to_rgba8()
                } else if asset.has_video {
                    let d = video::Decoder::open(std::path::Path::new(&asset.path)).ok()?;
                    let f = d.last.as_ref()?;
                    let img = image::RgbaImage::from_raw(f.width as u32, f.height as u32, f.rgba.clone())?;
                    image::imageops::thumbnail(&img, 260, 160)
                } else {
                    return None;
                };
                Some((rgba.width() as usize, rgba.height() as usize, rgba.into_raw()))
            })();
            let _ = tx.send(res);
            wake.request_repaint();
        });
        self.thumb_jobs.insert(a.id, rx);
        None
    }

    pub(crate) fn pool_panel(&mut self, ui: &mut Ui) {
        // Kopfzeile
        let bar = egui::Frame::new().fill(BAR).inner_margin(egui::Margin::symmetric(18, 0));
        bar.show(ui, |ui| {
            ui.set_height(30.0);
            ui.horizontal_centered(|ui| {
                ui.label(RichText::new("Import").strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if tool_button(ui, I_ADD, "Importieren", "Videos, Bilder oder Ton importieren (Strg+I)", true).clicked() {
                        self.import_dialog();
                    }
                });
            });
        });

        let ctx = ui.ctx().clone();
        let mut add: Option<Uuid> = None;
        let mut remove: Option<Uuid> = None;
        let mut view: Option<Uuid> = None;
        self.pool_tiles.clear();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if self.project.assets.is_empty() {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("Videos hierher ziehen oder „Importieren“ wählen").color(SECONDARY));
                });
            }
            let avail = ui.available_width() - 24.0;
            let cols = ((avail + 10.0) / (TILE_W + 10.0)).floor().max(1.0) as usize;
            let assets: Vec<MediaAsset> = self.project.assets.clone();
            ui.add_space(12.0);
            for chunk in assets.chunks(cols) {
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.spacing_mut().item_spacing.x = 10.0;
                    for a in chunk {
                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(TILE_W, TILE_H + 20.0), Sense::click_and_drag());
                        let img = Rect::from_min_size(rect.min, Vec2::new(TILE_W, TILE_H));
                        self.pool_tiles.push((a.id, img));
                        let p = ui.painter();
                        p.rect_filled(img, 4.0, Color32::BLACK);
                        let thumb = self.thumb(&ctx, a);
                        if thumb.is_none() && !self.thumbs.contains_key(&a.id) {
                            ctx.request_repaint_after(std::time::Duration::from_millis(150));
                        }
                        if let Some(t) = thumb {
                            let s = t.size_vec2();
                            let k = (TILE_W / s.x).max(TILE_H / s.y);
                            let uv_w = TILE_W / (s.x * k);
                            let uv_h = TILE_H / (s.y * k);
                            let uv = Rect::from_min_max(
                                egui::pos2((1.0 - uv_w) / 2.0, (1.0 - uv_h) / 2.0),
                                egui::pos2((1.0 + uv_w) / 2.0, (1.0 + uv_h) / 2.0),
                            );
                            ui.painter().image(t.id(), img, uv, Color32::WHITE);
                        } else {
                            let g = if a.has_video || a.is_image { I_VIEW } else { I_VOLUME };
                            ui.painter().text(img.center(), Align2::CENTER_CENTER, g, FontId::new(22.0, icons()), Color32::from_gray(150));
                        }
                        let dur = clock(a.duration);
                        let gal = ui.painter().layout_no_wrap(dur, FontId::proportional(10.0), Color32::WHITE);
                        let tag = Rect::from_min_size(
                            egui::pos2(img.right() - gal.size().x - 9.0, img.bottom() - gal.size().y - 7.0),
                            gal.size() + Vec2::new(6.0, 4.0),
                        );
                        ui.painter().rect_filled(tag, 2.0, Color32::from_black_alpha(160));
                        ui.painter().galley(tag.min + Vec2::new(3.0, 2.0), gal, Color32::WHITE);
                        if resp.hovered() || resp.dragged() {
                            ui.painter().rect_stroke(img, 4.0, Stroke::new(1.5, ACCENT), StrokeKind::Inside);
                        }
                        ui.painter().with_clip_rect(Rect::from_min_size(rect.min + Vec2::new(0.0, TILE_H), Vec2::new(TILE_W, 20.0))).text(
                            egui::pos2(rect.left() + 1.0, rect.top() + TILE_H + 4.0),
                            Align2::LEFT_TOP,
                            &a.name,
                            FontId::proportional(11.5),
                            Color32::from_gray(215),
                        );
                        if resp.drag_started() {
                            resp.dnd_set_drag_payload(crate::source::DragAsset { id: a.id, range: None });
                        }
                        if resp.dragged() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                        }
                        if resp.double_clicked() {
                            add = Some(a.id);
                        }
                        resp.on_hover_text(format!(
                            "{}\n{}\nDoppelklick: an Abspielposition einfügen · Ziehen: auf die Timeline ablegen",
                            a.name,
                            if a.is_image { format!("Bild {}×{}", a.width as i64, a.height as i64) } else if a.has_video { format!("Video {}×{}, {:.0} fps", a.width as i64, a.height as i64, a.fps) } else { "Ton".to_string() }
                        ))
                        .context_menu(|ui| {
                            if ui.button("Im Monitor ansehen (Bereich wählen)").clicked() {
                                view = Some(a.id);
                                ui.close();
                            }
                            if ui.button("An Abspielposition einfügen").clicked() {
                                add = Some(a.id);
                                ui.close();
                            }
                            if ui.button("Aus dem Pool entfernen").clicked() {
                                remove = Some(a.id);
                                ui.close();
                            }
                        });
                    }
                });
                ui.add_space(12.0);
            }
        });
        if let Some(id) = add {
            self.add_asset_clip(id);
        }
        if let Some(id) = view {
            self.load_source(id);
        }
        if let Some(id) = remove {
            self.project.assets.retain(|a| a.id != id);
            self.project.remove_clips(&self.project.tracks.iter().flat_map(|t| &t.clips).filter(|c| c.asset_id() == Some(id)).map(|c| c.id).collect::<Vec<_>>());
        }
    }
}
