//! Kleine Fenster der Mac-Version 1.2: Spur umbenennen, Zwischenspeicher (CacheSheet.swift); dazu das Aufräumen
//! hängengebliebener Gesten.

use super::*;
use crate::app::App;
use eframe::egui::{Align2, RichText};

impl App {
    /// Spur umbenennen (Doppelklick auf den Spurkopf oder Spurmenü).
    pub(crate) fn begin_track_rename(&mut self, i: usize) {
        if let Some(t) = self.project.tracks.get(i) {
            self.track_rename = Some((i, t.custom_name().unwrap_or("").to_string()));
        }
    }

    /// Übernimmt den Namen; leer = wieder „Spur N“.
    pub(crate) fn rename_track(&mut self, i: usize, name: &str) {
        if let Some(t) = self.project.tracks.get_mut(i) {
            let n = name.trim();
            t.name = if n.is_empty() { format!("Spur {}", i + 1) } else { n.to_string() };
        }
    }

    pub(crate) fn track_rename_window(&mut self, ctx: &egui::Context) {
        let Some((i, mut text)) = self.track_rename.take() else { return };
        let (mut ok, mut cancel) = (false, false);
        egui::Window::new(format!("Spur {} umbenennen", i + 1)).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut text).hint_text(format!("Spur {}", i + 1)).desired_width(240.0));
            r.request_focus();
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                ok = true;
            }
            ui.horizontal(|ui| {
                if ui.button("Umbenennen").clicked() {
                    ok = true;
                }
                if ui.button("Abbrechen").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    cancel = true;
                }
            });
        });
        if ok {
            self.rename_track(i, &text);
        } else if !cancel {
            self.track_rename = Some((i, text));
        }
    }

    /// Zwischenspeicher: Platzbedarf je Kategorie, Zwischendateien löschen (Originale werden nur angezeigt).
    pub(crate) fn cache_window(&mut self, ctx: &egui::Context) {
        if !self.show_cache {
            return;
        }
        let data = self.feat.data_dir.clone();
        let mut close = false;
        let mut freed: Option<u64> = None;
        egui::Window::new("Zwischenspeicher").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
            ui.set_min_width(460.0);
            ui.label(RichText::new(format!("Datenordner: {}", data.display())).size(11.0).color(SECONDARY));
            ui.add_space(6.0);
            let mut deletable = 0u64;
            egui::Grid::new("cache_grid").num_columns(3).spacing([12.0, 6.0]).show(ui, |ui| {
                for c in &crate::media::CACHE_CATEGORIES {
                    let size = crate::media::dir_size(&data.join(c.folder));
                    if c.deletable {
                        deletable += size;
                    }
                    ui.vertical(|ui| {
                        ui.label(RichText::new(c.title).strong());
                        ui.label(RichText::new(c.detail).size(11.0).color(SECONDARY));
                    });
                    ui.label(RichText::new(crate::media::format_bytes(size)).monospace());
                    if c.deletable {
                        if ui.add_enabled(size > 0, egui::Button::new("Löschen").small()).clicked() {
                            freed = Some(crate::media::clear_category(&data, c));
                        }
                    } else {
                        ui.label(RichText::new("bleibt").size(11.0).color(SECONDARY));
                    }
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(deletable > 0, egui::Button::new(format!("Alle Zwischendateien löschen ({})", crate::media::format_bytes(deletable)))).clicked() {
                    freed = Some(crate::media::CACHE_CATEGORIES.iter().map(|c| crate::media::clear_category(&data, c)).sum());
                }
                if ui.button("Ordner öffnen").clicked() {
                    let _ = std::process::Command::new("explorer.exe").arg(&data).spawn();
                }
                if ui.button("Schließen").clicked() {
                    close = true;
                }
            });
            ui.label(RichText::new("Zwischendateien werden bei Bedarf neu berechnet. Aufnahmen, importierte Standbilder und lokal gesicherte Medien gehören zu deinen Projekten und werden hier nie gelöscht.").size(11.0).color(SECONDARY));
        });
        if let Some(f) = freed {
            self.caches_cleared(f);
        }
        if close {
            self.show_cache = false;
        }
    }

    /// Räumt Zieh-, Kürzen- und Rahmenzustände auf, die nach dem Loslassen der Maustaste übrig geblieben sind.
    pub(crate) fn release_stuck_gestures(&mut self) {
        let tl = &mut self.tl;
        if tl.move_drag.is_some() || tl.trim.is_some() || tl.slip.is_some() || tl.marquee.is_some() || tl.vol.is_some() || tl.scrubbing || tl.range_drag.is_some() {
            tl.move_drag = None;
            tl.trim = None;
            tl.slip = None;
            tl.marquee = None;
            tl.vol = None;
            tl.scrubbing = false;
            tl.range_drag = None;
            tl.snap_guide = None;
            tl.edge = None;
        }
    }
}
