use super::*;
use crate::app::App;
use crate::render::{FrameSource, MediaFrame, Renderer};
use crate::video::AsyncDecoder;
use eframe::egui::{Align2, Pos2, Rect, RichText, Stroke, StrokeKind};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// Videobilder für die Vorschau: ein Hintergrund-Decoder je Medium (siehe `AsyncDecoder`).
pub struct PreviewFrames {
    pub decs: HashMap<Uuid, AsyncDecoder>,
    ctx: egui::Context,
}

impl FrameSource for PreviewFrames {
    fn frame(&mut self, asset: &crate::model::MediaAsset, t: f64) -> Option<MediaFrame> {
        if !self.decs.contains_key(&asset.id) {
            let c = self.ctx.clone();
            self.decs.insert(asset.id, AsyncDecoder::spawn(std::path::PathBuf::from(&asset.path), move || c.request_repaint()));
        }
        let d = self.decs.get(&asset.id)?;
        d.request(t);
        d.frame_for(t)
    }
}

impl PreviewFrames {
    fn version(&self) -> u64 {
        self.decs.values().map(|d| d.version()).sum()
    }

    /// Zeit des Videobildes, das für `t` gezeigt würde (`None`, solange der Decoder noch nichts geliefert hat).
    fn frame_time(&mut self, asset: &crate::model::MediaAsset, t: f64) -> Option<f64> {
        self.frame(asset, t).map(|f| f.time)
    }
}

/// Ändert sich das Projektbild bei gleichem Projekt und gleichen Videobildern mit der Zeit? (Keyframes, Ein-/Ausblenden,
/// Überblendungen, bewegte oder zeitabhängige Titel, Stabilisierung, Lautstärke-/Deckkraftkurve)
fn time_dependent(p: &crate::model::Project, t: f64) -> bool {
    p.tracks.iter().filter(|tr| !tr.hidden).flat_map(|tr| tr.clips.iter()).any(|c| {
        if c.is_audio() || t < c.start || t >= c.end() {
            return false;
        }
        let rel = t - c.start;
        let (xin, xout) = p.overlaps(c.id);
        let ramp = c.fade_in.max(xin) > 0.0 && rel < c.fade_in.max(xin) || c.fade_out.max(xout) > 0.0 && c.duration - rel < c.fade_out.max(xout);
        if ramp || !c.keys.is_empty() || !c.volume_points.is_empty() || c.fx.stabilize.is_some() {
            return true;
        }
        match &c.content {
            crate::model::ClipContent::Title(s) => {
                let s = &s.v;
                let d = s.anim_dur.unwrap_or(0.6).max(0.1);
                let moving = |a: &Option<String>| a.as_deref().is_some_and(|a| a != "none");
                s.text.contains('{') || s.credits == Some(true) || s.ticker == Some(true) || moving(&s.anim_in) && rel < d || moving(&s.anim_out) && c.duration - rel < d
            }
            _ => false,
        }
    })
}

/// Zustand der Vorschau: Kompositor, Decoder und das zuletzt gezeichnete Bild.
pub struct PreviewState {
    pub renderer: Renderer,
    pub frames: PreviewFrames,
    pub tex: Option<egui::TextureHandle>,
    pub src_tex: Option<egui::TextureHandle>,
    pub edge_tex: Option<egui::TextureHandle>,
    src_key: Option<(u64, u64, usize, usize, u64)>,
    key: Option<(u64, u64, usize, usize, u64)>,
    pub error: Option<String>,
    /// zuletzt gezeichnetes Projektbild (für die Messinstrumente)
    pub last: Option<(Vec<u8>, usize, usize)>,
    /// Anzahl der Zusammensetzungen (Tests und Leistungsprotokoll)
    pub renders: u64,
}

impl PreviewState {
    pub fn new(ctx: &egui::Context) -> Self {
        PreviewState { renderer: Renderer::new(), frames: PreviewFrames { decs: HashMap::new(), ctx: ctx.clone() }, tex: None, src_tex: None, edge_tex: None, src_key: None, key: None, error: None, last: None, renders: 0 }
    }

    /// Nach Projektwechsel: Decoder und Bild verwerfen.
    pub fn dbg_key(&self) -> Option<(u64, u64, usize, usize, u64)> {
        self.key
    }

    pub fn reset(&mut self) {
        self.frames.decs.clear();
        self.key = None;
        self.tex = None;
    }
}

impl App {
    /// Rechnet das Projektbild bei Bedarf neu (Zeit, Projekt, Größe oder neue Videobilder) und gibt es zurück.
    pub(crate) fn preview_image(&mut self, ctx: &egui::Context, w: usize, h: usize) -> Option<egui::TextureHandle> {
        // Schlüssel: Projektstand, Größe und die gewählten Videobilder; die Abspielzeit nur, wenn sie das Bild verändert.
        // So wird ein 30-fps-Video auf einem 60-Hz-Bildschirm nur je Videobild einmal zusammengesetzt.
        let t = self.playhead;
        let mut sig = 0xcbf29ce484222325u64;
        let mut missing = false;
        for tr in self.project.tracks.iter().filter(|tr| !tr.hidden) {
            for c in tr.clips.iter().filter(|c| !c.is_audio() && t >= c.start && t < c.end()) {
                let Some(a) = c.asset_id().and_then(|id| self.project.asset(id)) else { continue };
                if a.is_image {
                    continue;
                }
                match self.preview.frames.frame_time(a, c.source_in + (t - c.start) * c.speed) {
                    Some(ft) => sig = (sig ^ ft.to_bits()).wrapping_mul(0x100000001b3),
                    None => missing = true,
                }
            }
        }
        let tkey = if time_dependent(&self.project, t) { t.to_bits() } else { 0 };
        let ver = if missing { self.preview.frames.version() } else { 0 };
        let key = (tkey ^ sig.rotate_left(17), self.project_rev, w, h, ver);
        if self.preview.key != Some(key) || self.preview.tex.is_none() {
            let t0 = std::time::Instant::now();
            self.preview.renders += 1;
            let (p, t) = (&self.project, self.playhead);
            let pv = &mut self.preview;
            let rgba = pv.renderer.render_with(p, t, w, h, &mut pv.frames);
            let t1 = std::time::Instant::now();
            // Das Bild ist deckend: ohne Vormultiplizieren direkt in Color32 übernehmen
            let ci = egui::ColorImage { size: [w, h], source_size: egui::vec2(w as f32, h as f32), pixels: rgba.chunks_exact(4).map(|p| Color32::from_rgb(p[0], p[1], p[2])).collect() };
            pv.last = Some((rgba, w, h));
            match &mut pv.tex {
                Some(tex) => tex.set(ci, egui::TextureOptions::LINEAR),
                None => pv.tex = Some(ctx.load_texture("preview", ci, egui::TextureOptions::LINEAR)),
            }
            pv.key = Some(key);
            crate::perf::record(t1 - t0, t1.elapsed(), w, h);
            // Fehler beim Öffnen von Videos in der Statuszeile melden
            for d in pv.frames.decs.values() {
                if let Some(e) = d.error() {
                    pv.error = Some(e);
                }
            }
            if let Some(e) = pv.error.take() {
                self.status = format!("Video konnte nicht geöffnet werden: {e}");
            }
        }
        self.preview.tex.clone()
    }

    /// Zeichnet das Projektbild in `avail`; das Format wird eingepasst.
    pub(crate) fn draw_project(&mut self, ui: &mut egui::Ui, avail: Rect) {
        self.draw_project_opts(ui, avail, true);
    }

    pub(crate) fn draw_project_opts(&mut self, ui: &mut egui::Ui, avail: Rect, overlay: bool) {
        let aspect = self.project.render_width as f32 / self.project.render_height.max(1) as f32;
        let mut size = Vec2::new(avail.width(), avail.width() / aspect);
        if size.y > avail.height() {
            size = Vec2::new(avail.height() * aspect, avail.height());
        }
        let frame = Rect::from_center_size(avail.center(), size.max(Vec2::splat(10.0)));
        let ppp = ui.ctx().pixels_per_point();
        let w = ((frame.width() * ppp).round() as usize).clamp(16, 1920) & !1;
        let h = ((frame.height() * ppp).round() as usize).clamp(16, 1080) & !1;
        let ctx = ui.ctx().clone();
        let painter = ui.painter_at(avail);
        painter.rect_filled(frame, 0.0, Color32::BLACK);
        if let Some(tex) = self.preview_image(&ctx, w, h) {
            painter.image(tex.id(), frame, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        painter.rect_stroke(frame, 0.0, Stroke::new(1.0, Color32::from_gray(70)), StrokeKind::Outside);
        // Hilfslinien beim Begradigen des Horizonts (Gitter über dem Bild)
        let guides = self.show_guides && self.selected.and_then(|id| self.project.clip(id)).is_some_and(|c| c.fx.horizon.is_some() && self.playhead >= c.start && self.playhead <= c.end());
        if guides {
            let line = Stroke::new(0.5, Color32::from_white_alpha(90));
            for k in 1..8 {
                let f = k as f32 / 8.0;
                painter.line_segment([egui::pos2(frame.left() + frame.width() * f, frame.top()), egui::pos2(frame.left() + frame.width() * f, frame.bottom())], line);
                painter.line_segment([egui::pos2(frame.left(), frame.top() + frame.height() * f), egui::pos2(frame.right(), frame.top() + frame.height() * f)], line);
            }
        }
        if overlay {
            self.draw_overlay(ui, frame);
        }
    }

    /// Vollbildvorschau: Film ab der Abspielposition auf dem ganzen Bildschirm; Esc, Klick oder das Filmende schließen.
    pub(crate) fn toggle_fullscreen(&mut self, ctx: &egui::Context) {
        if self.fullscreen {
            self.fullscreen = false;
            self.set_playing(false);
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            return;
        }
        if self.project.duration() <= 0.0 {
            return;
        }
        self.fullscreen = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
        if self.playhead >= self.project.duration() - 0.05 {
            self.playhead = 0.0;
        }
        self.set_playing(true);
    }

    pub(crate) fn fullscreen_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let (esc, space, click) = ctx.input(|i| (i.key_pressed(egui::Key::Escape), i.key_pressed(egui::Key::Space), i.pointer.primary_clicked()));
        if esc || click {
            self.toggle_fullscreen(&ctx);
            return;
        }
        if space {
            self.set_playing(!self.playing);
        }
        if !self.playing && self.playhead >= self.project.duration() - 0.05 {
            self.toggle_fullscreen(&ctx);
            return;
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::BLACK)).show_inside(ui, |ui| {
            let r = ui.available_rect_before_wrap();
            ui.allocate_rect(r, Sense::hover());
            self.draw_project_opts(ui, r, false);
        });
        ctx.set_cursor_icon(egui::CursorIcon::None);
    }
}

impl App {
    /// Projekt für die Quellansicht: das Medium als einziger Clip, Format = Medienformat.
    fn source_project(&self, a: &crate::model::MediaAsset) -> crate::model::Project {
        use crate::model::*;
        let mut p = Project::default();
        p.render_width = (a.width.max(16.0)) as i64;
        p.render_height = (a.height.max(16.0)) as i64;
        let pic = MediaAsset { has_audio: false, ..a.clone() };
        p.assets.push(pic.clone());
        p.tracks[0].clips.push(Clip { content: ClipContent::Media(Wrap { v: pic.id }), start: 0.0, duration: a.duration.max(0.1), ..Default::default() });
        p
    }

    /// Bild der Quelle an der aktuellen Position (zwischengespeichert, solange sich nichts ändert).
    fn source_image(&mut self, ctx: &egui::Context, w: usize, h: usize) -> Option<egui::TextureHandle> {
        let a = self.source_asset()?;
        if !(a.has_video || a.is_image) {
            return None;
        }
        let pos = self.source.as_ref()?.position;
        let ver = self.preview.frames.version();
        let key = (pos.to_bits(), a.id.as_u128() as u64, w, h, ver);
        if self.preview.src_key != Some(key) || self.preview.src_tex.is_none() {
            let sp = self.source_project(&a);
            let pv = &mut self.preview;
            let rgba = pv.renderer.render_with(&sp, pos.min(a.duration - 1e-3).max(0.0), w, h, &mut pv.frames);
            let ci = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
            match &mut pv.src_tex {
                Some(t) => t.set(ci, egui::TextureOptions::LINEAR),
                None => pv.src_tex = Some(ctx.load_texture("source", ci, egui::TextureOptions::LINEAR)),
            }
            pv.src_key = Some(key);
        }
        self.preview.src_tex.clone()
    }

    /// Kantenbild beim Kürzen/Slip: das Quellbild an `EdgePreview.t` über dem Monitor.
    fn draw_edge_preview(&mut self, ui: &mut Ui, monitor: Rect) {
        let Some(e) = self.tl.edge.clone() else { return };
        let Some(a) = self.project.asset(e.asset).cloned() else { return };
        if let Some(f) = crate::render::FrameSource::frame(&mut self.preview.frames, &a, e.t) {
            let ci = egui::ColorImage::from_rgba_unmultiplied([f.width, f.height], &f.rgba);
            match &mut self.preview.edge_tex {
                Some(t) => t.set(ci, egui::TextureOptions::LINEAR),
                None => self.preview.edge_tex = Some(ui.ctx().load_texture("edge", ci, egui::TextureOptions::LINEAR)),
            }
            if let Some(tex) = &self.preview.edge_tex {
                let area = monitor.shrink(30.0);
                let aspect = f.width as f32 / f.height.max(1) as f32;
                let mut size = Vec2::new(area.width(), area.width() / aspect);
                if size.y > area.height() {
                    size = Vec2::new(area.height() * aspect, area.height());
                }
                let r = Rect::from_center_size(area.center(), size);
                ui.painter().image(tex.id(), r, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                let g = ui.painter().layout_no_wrap(e.label.clone(), FontId::proportional(11.0), Color32::WHITE);
                let chip = Rect::from_center_size(egui::pos2(r.center().x, r.bottom() - 18.0), g.size() + Vec2::new(12.0, 10.0));
                ui.painter().rect_filled(chip, 4.0, Color32::from_black_alpha(180));
                ui.painter().galley(chip.min + Vec2::new(6.0, 5.0), g, Color32::WHITE);
            }
        }
    }

    /// Vorschau-Bereich: Kopfzeile mit Zeitcode, Monitor (Projekt oder Quelle), Quellleiste, Transportleiste.
    pub(crate) fn preview_pane(&mut self, ui: &mut Ui) {
        let full = ui.available_rect_before_wrap();
        self.preview_rect = full;
        let pane = ui.allocate_rect(full, Sense::hover());
        let in_source = self.source.is_some();
        let bar_h = if in_source { 56.0 } else { 0.0 };
        let header = Rect::from_min_size(full.min, Vec2::new(full.width(), 26.0));
        let transport = Rect::from_min_max(egui::pos2(full.left(), full.bottom() - 38.0), full.max);
        let source_bar = Rect::from_min_max(egui::pos2(full.left(), transport.top() - bar_h), egui::pos2(full.right(), transport.top()));
        let monitor = Rect::from_min_max(egui::pos2(full.left(), header.bottom()), egui::pos2(full.right(), source_bar.top()));

        // Medium aus dem Pool auf den Monitor ziehen: im Quellmonitor ansehen
        if let Some(d) = pane.dnd_release_payload::<crate::source::DragAsset>() {
            self.load_source(d.id);
        }
        let hovering_asset = pane.dnd_hover_payload::<crate::source::DragAsset>().is_some();

        let fps = self.project.fps;
        let p = ui.painter();
        p.rect_filled(full, 0.0, PANEL);
        p.rect_filled(monitor, 0.0, Color32::BLACK);
        let src = self.source.clone().zip(self.source_asset());
        if let Some((s, a)) = &src {
            let sfps = a.fps.round().max(1.0) as i64;
            p.text(egui::pos2(header.left() + 10.0, header.center().y), Align2::LEFT_CENTER, format!("▣ Quelle: {}", a.name), FontId::proportional(13.0), ACCENT);
            let dur = format!("/ {}", timecode(a.duration, sfps));
            let gd = p.layout_no_wrap(dur, FontId::monospace(12.5), SECONDARY);
            let gt = p.layout_no_wrap(timecode(s.position, sfps), FontId::monospace(12.5), Color32::WHITE);
            let btn_w = 80.0;
            let x = header.right() - 10.0 - btn_w - gd.size().x;
            p.galley(egui::pos2(x, header.center().y - gd.size().y / 2.0), gd, SECONDARY);
            p.galley(egui::pos2(x - 8.0 - gt.size().x, header.center().y - gt.size().y / 2.0), gt, Color32::WHITE);
            let br = Rect::from_min_size(egui::pos2(header.right() - btn_w - 6.0, header.top() + 2.0), Vec2::new(btn_w, 22.0));
            let r = ui.interact(br, ui.id().with("exit_source"), Sense::click()).on_hover_text("Quellmonitor schließen und zum Projekt zurück");
            ui.painter().text(br.center(), Align2::CENTER_CENTER, "✕ Projekt", FontId::proportional(12.0), if r.hovered() { Color32::WHITE } else { Color32::from_gray(210) });
            if r.clicked() {
                self.exit_source();
            }
        } else {
            p.text(egui::pos2(header.left() + 10.0, header.center().y), Align2::LEFT_CENTER, "Vorschau", FontId::proportional(13.0), SECONDARY);
            let dur = format!("/ {}", timecode(self.project.duration(), fps));
            let gd = p.layout_no_wrap(dur, FontId::monospace(12.5), SECONDARY);
            let gt = p.layout_no_wrap(timecode(self.playhead, fps), FontId::monospace(12.5), Color32::WHITE);
            let x = header.right() - 10.0 - gd.size().x;
            let (tw, th) = (gt.size().x, gt.size().y);
            p.galley(egui::pos2(x, header.center().y - gd.size().y / 2.0), gd, SECONDARY);
            p.galley(egui::pos2(x - 8.0 - tw, header.center().y - th / 2.0), gt, Color32::WHITE);
            let tc_rect = Rect::from_min_size(egui::pos2(x - 8.0 - tw, header.center().y - th / 2.0), Vec2::new(tw, th)).expand(3.0);
            if ui.interact(tc_rect, ui.id().with("tc_click"), Sense::click()).on_hover_text("Klick: Zeitcode eintippen und hinspringen").clicked() {
                self.tc_prompt = Some(timecode(self.playhead, fps));
            }
        }

        let area = monitor.shrink(30.0);
        if let Some((_, a)) = &src {
            // Quelle: Bild einpassen; Ziehen aus dem Monitor legt den gewählten Bereich auf die Timeline
            let ctx = ui.ctx().clone();
            let aspect = (a.width.max(1.0) / a.height.max(1.0)) as f32;
            let full_area = monitor.shrink(8.0);
            let mut size = Vec2::new(full_area.width(), full_area.width() / aspect);
            if size.y > full_area.height() {
                size = Vec2::new(full_area.height() * aspect, full_area.height());
            }
            let frame = Rect::from_center_size(full_area.center(), size.max(Vec2::splat(10.0)));
            let ppp = ctx.pixels_per_point();
            let (w, h) = ((((frame.width() * ppp).round() as usize).clamp(16, 1920)) & !1, (((frame.height() * ppp).round() as usize).clamp(16, 1080)) & !1);
            if let Some(tex) = self.source_image(&ctx, w, h) {
                ui.painter().image(tex.id(), frame, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            } else {
                ui.painter().text(monitor.center(), Align2::CENTER_CENTER, I_VOLUME, FontId::new(60.0, icons()), SECONDARY);
            }
            let drag = ui.interact(monitor, ui.id().with("source_drag"), Sense::drag());
            if drag.drag_started() {
                drag.dnd_set_drag_payload(crate::source::DragAsset { id: a.id, range: self.source_range() });
            }
            let note = match self.source_range() {
                Some(r) => format!("Auswahl {:.1} s – ziehen in die Timeline", r.1 - r.0),
                None => "Ganzes Medium – ziehen in die Timeline".to_string(),
            };
            let g = ui.painter().layout_no_wrap(note, FontId::proportional(10.5), Color32::WHITE);
            let chip = Rect::from_min_size(monitor.left_top() + Vec2::new(6.0, 6.0), g.size() + Vec2::new(10.0, 8.0));
            ui.painter().rect_filled(chip, 3.0, Color32::from_black_alpha(140));
            ui.painter().galley(chip.min + Vec2::new(5.0, 4.0), g, Color32::WHITE);
        } else if area.width() > 40.0 && area.height() > 40.0 {
            if ui.interact(monitor, ui.id().with("monitor_dbl"), Sense::click()).on_hover_text("Doppelklick: Vollbild").double_clicked() {
                let ctx = ui.ctx().clone();
                self.toggle_fullscreen(&ctx);
            }
            self.draw_project(ui, area);
            self.draw_edge_preview(ui, monitor);
        }
        if hovering_asset {
            ui.painter().rect_filled(monitor, 0.0, ACCENT.gamma_multiply(0.12));
            ui.painter().rect_stroke(monitor, 0.0, Stroke::new(3.0, ACCENT), StrokeKind::Inside);
            ui.painter().text(monitor.center(), Align2::CENTER_CENTER, "Loslassen: im Monitor ansehen und Bereich wählen", FontId::proportional(12.0), Color32::WHITE);
        }

        if in_source {
            self.source_bar(ui, source_bar);
        }

        self.draw_meter(ui, Rect::from_min_max(egui::pos2(transport.right() - 150.0, transport.center().y - 9.0), egui::pos2(transport.right() - 12.0, transport.center().y + 9.0)));

        // Transport
        let btn = Vec2::new(34.0, 30.0);
        let total = 5.0 * btn.x + 4.0 * 14.0;
        let rect = Rect::from_min_size(egui::pos2(transport.center().x - total / 2.0, transport.center().y - btn.y / 2.0), Vec2::new(total, btn.y));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
        child.spacing_mut().item_spacing.x = 14.0;
        let ui2 = &mut child;
        let playing = if in_source { self.source.as_ref().is_some_and(|s| s.playing) } else { self.playing };
        if icon_button_sized(ui2, I_PREV, "Zum Anfang (Pos1)", false, 17.0, btn, true).clicked() {
            if in_source {
                let t = self.source.as_ref().map(|s| s.mark_in).unwrap_or(0.0);
                self.pause_source();
                self.seek_source(t);
            } else {
                self.set_playing(false);
                self.playhead = 0.0;
            }
        }
        if icon_button_sized(ui2, I_STEP_BACK, "Ein Bild zurück (←)", false, 17.0, btn, true).clicked() {
            if in_source { self.step_source(-1) } else { self.step(-1) }
        }
        let g = if playing { I_PAUSE } else { I_PLAY };
        if icon_button_sized(ui2, g, "Abspielen / Pause (Leertaste)", false, 19.0, btn, true).clicked() {
            if in_source { self.toggle_source_play() } else { self.set_playing(!self.playing) }
        }
        if icon_button_sized(ui2, I_STEP_FWD, "Ein Bild vor (→)", false, 17.0, btn, true).clicked() {
            if in_source { self.step_source(1) } else { self.step(1) }
        }
        if icon_button_sized(ui2, I_NEXT, "Zum Ende (Ende)", false, 17.0, btn, true).clicked() {
            if in_source {
                let t = self.source.as_ref().map(|s| s.mark_out).unwrap_or(0.0);
                self.pause_source();
                self.seek_source(t);
            } else {
                self.set_playing(false);
                self.playhead = self.project.duration();
            }
        }
    }

    /// Stereo-Pegelanzeige (−60 … 0 dBFS) mit Spitzenwert-Haltestrich; grün, ab −12 dB gelb, ab −3 dB rot.
    pub(crate) fn draw_meter(&self, ui: &mut Ui, r: Rect) {
        let x0 = r.left() + 14.0;
        let w = (r.right() - x0).max(10.0);
        let db_x = |db: f32| x0 + (((db + 60.0) / 60.0).clamp(0.0, 1.0)) * w;
        let p = ui.painter();
        for ch in 0..2 {
            let y = r.top() + 2.0 + ch as f32 * 8.0;
            let bar = Rect::from_min_size(egui::pos2(x0, y), Vec2::new(w, 6.0));
            p.rect_filled(bar, 1.5, Color32::from_gray(38));
            let lvl = self.feat.meter[ch];
            if lvl > -60.0 {
                for (from, to, col) in [(-60.0f32, -12.0f32, Color32::from_rgb(64, 190, 90)), (-12.0, -3.0, Color32::from_rgb(230, 190, 50)), (-3.0, 0.0, Color32::from_rgb(230, 70, 60))] {
                    let (a, b) = (db_x(from), db_x(lvl.min(to)));
                    if lvl > from && b > a {
                        p.rect_filled(Rect::from_min_max(egui::pos2(a, y), egui::pos2(b, y + 6.0)), 0.0, col);
                    }
                }
            }
            let pk = self.feat.meter_peak[ch];
            if pk > -60.0 {
                let x = db_x(pk);
                p.rect_filled(Rect::from_min_size(egui::pos2(x - 1.0, y), Vec2::new(2.0, 6.0)), 0.0, if pk > -3.0 { Color32::from_rgb(230, 70, 60) } else { Color32::from_gray(225) });
            }
        }
        p.text(egui::pos2(r.left() + 6.0, r.center().y), Align2::CENTER_CENTER, I_VOLUME, FontId::new(11.0, icons()), SECONDARY);
    }

    /// Kleine Zeitleiste des Quellclips über der Transportsteuerung: Abspielposition, Anfangs- und Endpunkt.
    fn source_bar(&mut self, ui: &mut Ui, rect: Rect) {
        let (Some(s), Some(a)) = (self.source.clone(), self.source_asset()) else { return };
        ui.painter().rect_filled(rect, 0.0, BAR.gamma_multiply(0.6));
        let dur = a.duration.max(0.1);
        let track = Rect::from_min_size(rect.min + Vec2::new(10.0, 6.0), Vec2::new(rect.width() - 20.0, 22.0));
        let x_of = |t: f64| track.left() + (t / dur) as f32 * track.width();
        let t_of = |x: f32| (((x - track.left()) / track.width()) as f64 * dur).clamp(0.0, dur);
        let p = ui.painter();
        p.rect_filled(track, 3.0, Color32::from_black_alpha(115));
        let (xi, xo) = (x_of(s.mark_in), x_of(s.mark_out));
        p.rect_filled(Rect::from_min_max(Pos2::new(xi, track.top()), Pos2::new(xo.max(xi + 2.0), track.bottom())), 0.0, ACCENT.gamma_multiply(0.45));
        // Leiste anklicken/ziehen: Position setzen
        let bar = ui.interact(track, ui.id().with("sbar"), Sense::click_and_drag());
        if bar.is_pointer_button_down_on() {
            if let Some(pos) = bar.interact_pointer_pos() {
                self.pause_source();
                self.seek_source(t_of(pos.x));
            }
        }
        // Anfasser für Anfang und Ende
        for is_in in [true, false] {
            let x = if is_in { xi - 7.0 } else { xo };
            let hr = Rect::from_min_size(Pos2::new(x, track.top()), Vec2::new(8.0, 22.0));
            let r = ui.interact(hr.expand2(Vec2::new(3.0, 0.0)), ui.id().with(("shandle", is_in)), Sense::drag());
            ui.painter().rect_filled(hr, if is_in { egui::CornerRadius { nw: 4, sw: 4, ne: 0, se: 0 } } else { egui::CornerRadius { nw: 0, sw: 0, ne: 4, se: 4 } }, ACCENT);
            ui.painter().text(hr.center(), Align2::CENTER_CENTER, if is_in { "[" } else { "]" }, FontId::proportional(11.0), Color32::WHITE);
            if r.dragged() {
                if let Some(pos) = r.interact_pointer_pos() {
                    let t = t_of(pos.x + if is_in { 7.0 } else { 0.0 });
                    let cur = self.source.clone().unwrap();
                    let (min_gap, nt) = (crate::ops::MIN_CLIP, t);
                    let v = if is_in { nt.min(cur.mark_out - min_gap).max(0.0) } else { nt.max(cur.mark_in + min_gap).min(dur) };
                    if let Some(m) = self.source.as_mut() {
                        if is_in {
                            m.mark_in = v;
                        } else {
                            m.mark_out = v;
                        }
                    }
                    self.pause_source();
                    self.seek_source(v);
                }
            }
        }
        let px = x_of(s.position);
        ui.painter().rect_filled(Rect::from_min_size(Pos2::new(px - 1.0, track.top()), Vec2::new(2.0, 22.0)), 0.0, Color32::WHITE);
        // Knöpfe
        let row = Rect::from_min_max(Pos2::new(rect.left() + 10.0, track.bottom() + 4.0), Pos2::new(rect.right() - 10.0, rect.bottom() - 2.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(row).layout(egui::Layout::left_to_right(egui::Align::Center)));
        child.spacing_mut().item_spacing.x = 10.0;
        let u = &mut child;
        let fps = a.fps.round().max(1.0) as i64;
        if u.add(egui::Button::new(RichText::new("[ Anfang").size(11.5)).frame(false)).on_hover_text("Anfangspunkt an der Abspielposition (I)").clicked() {
            self.set_source_in();
        }
        if u.add(egui::Button::new(RichText::new("Ende ]").size(11.5)).frame(false)).on_hover_text("Endpunkt an der Abspielposition (O)").clicked() {
            self.set_source_out();
        }
        let txt = format!("{} – {}  ({:.1} s)", &timecode(s.mark_in, fps)[3..], &timecode(s.mark_out, fps)[3..], s.mark_out - s.mark_in);
        u.label(RichText::new(txt).size(10.5).monospace().color(SECONDARY));
        u.with_layout(egui::Layout::right_to_left(egui::Align::Center), |u| {
            if u.add(egui::Button::new(RichText::new("+ Einfügen").size(11.5)).frame(false)).on_hover_text("Auswahl an der Abspielposition in die Timeline einfügen").clicked() {
                self.insert_source();
            }
            if u.add(egui::Button::new(RichText::new("Zurücksetzen").size(11.5)).frame(false)).clicked() {
                if let Some(m) = self.source.as_mut() {
                    m.mark_in = 0.0;
                    m.mark_out = a.duration;
                }
            }
        });
    }
}
