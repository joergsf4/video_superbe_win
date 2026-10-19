//! Hauptschleife der Oberfläche (Panel-Anordnung wie in ContentView.swift), Tastenkürzel und Bearbeitungshelfer.

use crate::app::App;
use crate::ui::{BAR, BG, PANEL, SECONDARY};
use eframe::egui::{self, Frame, Key, Margin, RichText};
use std::path::PathBuf;
use uuid::Uuid;

impl App {
    pub(crate) fn step(&mut self, frames: i64) {
        let dt = 1.0 / self.project.fps.max(1) as f64;
        self.set_playing(false);
        self.playhead = (self.playhead + frames as f64 * dt).max(0.0);
    }

    pub(crate) fn zoom_fit(&mut self) {
        let d = self.project.duration().max(1.0);
        self.zoom = (self.view_w as f64 / (d * 1.04)).clamp(4.0, 512.0);
        self.scroll = 0.0;
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (shift, alt) = ctx.input(|i| (i.modifiers.shift, i.modifiers.alt));
        let k = |key: Key| ctx.input(|i| i.key_pressed(key) && !i.modifiers.command);
        let cmd = |key: Key| ctx.input(|i| i.modifiers.command && !i.modifiers.shift && i.key_pressed(key));
        // Quellmonitor aktiv: Transporttasten und I/O gelten für die Quelle
        if self.source.is_some() {
            if k(Key::Space) {
                self.toggle_source_play();
                return;
            }
            if k(Key::I) {
                self.set_source_in();
                return;
            }
            if k(Key::O) {
                self.set_source_out();
                return;
            }
            if k(Key::K) {
                self.pause_source();
                return;
            }
            if k(Key::ArrowLeft) {
                self.step_source(-1);
                return;
            }
            if k(Key::ArrowRight) {
                self.step_source(1);
                return;
            }
        }
        if k(Key::Space) {
            self.set_playing(!self.playing);
        }
        if (k(Key::Delete) || k(Key::Backspace)) && !shift {
            self.delete_selection();
        }
        if (k(Key::Delete) || k(Key::Backspace)) && shift {
            self.ripple_delete_selection();
        }
        if k(Key::S) && !alt {
            self.split_at_playhead();
        }
        if k(Key::N) {
            self.snapping = !self.snapping;
        }
        if k(Key::E) {
            self.show_settings = true;
        }
        if k(Key::M) {
            self.add_marker();
        }
        if k(Key::I) {
            self.set_mark_in();
        }
        if k(Key::O) {
            self.set_mark_out();
        }
        // J/K/L: zurück (1 s), anhalten, abspielen
        if k(Key::K) {
            self.set_playing(false);
        }
        if k(Key::L) && !self.playing {
            self.set_playing(true);
        }
        if k(Key::J) {
            self.set_playing(false);
            self.playhead = (self.playhead - 1.0).max(0.0);
        }
        if k(Key::Backslash) {
            self.zoom_fit();
        }
        if k(Key::Plus) || k(Key::Equals) {
            self.zoom = (self.zoom * 1.4).min(600.0);
        }
        if k(Key::Minus) {
            self.zoom = (self.zoom / 1.4).max(4.0);
        }
        if k(Key::ArrowLeft) {
            self.step(-1);
        }
        if k(Key::ArrowRight) {
            self.step(1);
        }
        if k(Key::ArrowUp) {
            self.jump_to_edit_point(false);
        }
        if k(Key::ArrowDown) {
            self.jump_to_edit_point(true);
        }
        if k(Key::Home) {
            self.set_playing(false);
            self.playhead = 0.0;
        }
        if k(Key::End) {
            self.set_playing(false);
            self.playhead = self.project.duration();
        }
        if cmd(Key::Z) {
            self.do_undo();
        }
        if cmd(Key::Y) || ctx.input(|i| i.modifiers.command && i.modifiers.shift && i.key_pressed(Key::Z)) {
            self.do_redo();
        }
        if cmd(Key::S) {
            self.save();
        }
        if ctx.input(|i| i.modifiers.command && i.modifiers.shift && i.key_pressed(Key::S)) {
            self.save_as();
        }
        if cmd(Key::O) {
            self.open_dialog();
        }
        if cmd(Key::N) {
            self.new_project();
        }
        if cmd(Key::I) {
            self.import_dialog();
        }
        if cmd(Key::E) {
            self.export_dialog();
        }
        if cmd(Key::D) {
            self.duplicate_clips();
        }
        if cmd(Key::A) {
            self.select_all();
        }
        if cmd(Key::C) {
            self.copy_clips();
        }
        if cmd(Key::X) {
            self.cut_clips();
        }
        if cmd(Key::V) {
            self.paste_clips();
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.update_ui(ui);
    }
}

impl App {
    /// Ein Bild der gesamten Oberfläche (von der Hauptschleife und den GUI-Tests aufgerufen).
    pub(crate) fn update_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let before = self.project.clone();

        // Wiedergabe
        if self.playing {
            self.tick_playback();
            if self.playhead >= self.project.duration() {
                self.playhead = self.project.duration();
                self.set_playing(false);
            }
            ctx.request_repaint();
        }

        // Hintergrundergebnisse (Taktraster, Ducking), Autosave, Fenster schließen, Pegelanzeige
        self.poll_features(&ctx);
        self.autosave_tick();
        self.guard_close(&ctx);
        let rec = self.feat.recorder.is_some();
        if rec || self.playing || self.source.as_ref().is_some_and(|s| s.playing) || self.feat.meter != [-90.0; 2] || self.feat.meter_peak != [-90.0; 2] {
            self.update_meter();
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }
        if rec {
            let secs = self.feat.recorder.as_ref().map_or(0.0, |r| r.elapsed());
            self.status = format!("● Aufnahme {:02}:{:04.1} – zum Beenden das Mikrofon erneut anklicken", (secs / 60.0) as i64, secs % 60.0);
        }

        // Quellmonitor: Wiedergabe weiterlaufen lassen; Klick außerhalb des Vorschaubereichs schließt ihn
        self.tick_source();
        if self.source.as_ref().is_some_and(|s| s.playing) {
            ctx.request_repaint();
        }
        if self.source.is_some() && ctx.input(|i| i.pointer.primary_pressed()) {
            if let Some(p) = ctx.input(|i| i.pointer.press_origin()) {
                if self.preview_rect.is_positive() && !self.preview_rect.contains(p) {
                    self.exit_source();
                }
            }
        }

        // Dateien per Drag & Drop (aus dem Explorer)
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        for p in dropped {
            self.import_file(p);
        }
        self.handle_keys(&ctx);

        egui::Panel::top("menu").frame(Frame::new().fill(PANEL).inner_margin(Margin::symmetric(4, 2))).show_inside(ui, |ui| self.menu_bar(ui));
        egui::Panel::top("toolbar").frame(Frame::new().fill(BAR)).exact_size(40.0).resizable(false).show_inside(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status")
            .frame(Frame::new().fill(PANEL).inner_margin(Margin::symmetric(10, 2)))
            .exact_size(22.0)
            .resizable(false)
            .show_inside(ui, |ui| {
                ui.label(RichText::new(&self.status).size(11.5).color(SECONDARY));
            });
        egui::Panel::bottom("timeline")
            .frame(Frame::new().fill(BG))
            .resizable(true)
            .min_size(200.0)
            .default_size(310.0)
            .show_inside(ui, |ui| self.timeline_panel(ui));
        egui::Panel::right("inspector")
            .frame(Frame::new().fill(PANEL))
            .resizable(true)
            .default_size(340.0)
            .size_range(300.0..=460.0)
            .show_inside(ui, |ui| self.inspector_panel(ui));
        egui::Panel::right("pool")
            .frame(Frame::new().fill(PANEL))
            .resizable(true)
            .default_size(340.0)
            .size_range(260.0..=900.0)
            .show_inside(ui, |ui| self.pool_panel(ui));
        // Keyframe-Animator nur unter dem Monitor (nach den Seitenleisten, damit diese die volle Höhe behalten), solange der Reiter „Effekte“ offen ist
        if let Some(clip) = self.animator_clip() {
            egui::Panel::bottom("animator").frame(Frame::new().fill(PANEL)).resizable(false).exact_size(170.0).show_inside(ui, |ui| self.animator_panel(ui, clip));
        }
        egui::CentralPanel::default().frame(Frame::new().fill(PANEL)).show_inside(ui, |ui| self.preview_pane(ui));

        self.settings_window(&ctx);
        self.marker_window(&ctx);
        self.import_error_window(&ctx);
        self.export_window(&ctx);
        self.feature_windows(&ctx);

        // Rückgängig: Eine zusammenhängende Bedienung (Ziehen mit gedrückter Maustaste, Tippen in einem Textfeld)
        // ergibt einen Schritt, jede andere Änderung (Taste, Klick) einen eigenen.
        if self.history_skip {
            self.history_skip = false;
            self.gesture_open = false;
            self.project_rev += 1;
        } else if self.project != before {
            self.project_rev += 1;
            if !self.gesture_open {
                self.undo.push(before);
                if self.undo.len() > 200 {
                    self.undo.remove(0);
                }
            }
            self.redo.clear();
            self.gesture_open = ctx.input(|i| i.pointer.any_down()) || ctx.egui_wants_keyboard_input();
            if self.playing {
                self.start_player();
            }
            ctx.request_repaint();
        } else if !ctx.input(|i| i.pointer.any_down()) && !ctx.egui_wants_keyboard_input() {
            self.gesture_open = false;
        }

        let title = format!(
            "{}{} – Video superbe",
            self.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or("Unbenannt".into()),
            if self.project != self.saved { " •" } else { "" }
        );
        if title != self.title_cache {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title_cache = title;
        }
    }
}
