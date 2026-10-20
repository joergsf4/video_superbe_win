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
        let m = ctx.input(|i| i.modifiers);
        let pressed = |key: Key| ctx.input(|i| i.key_pressed(key));
        // genau diese Umschaltkombination (Strg bzw. ⌘ des Mac = `command`)
        let ch = |key: Key, ctrl: bool, shift: bool, alt: bool| pressed(key) && m.command == ctrl && m.shift == shift && m.alt == alt;
        let plain = |key: Key| ch(key, false, false, false);
        let ctrl = |key: Key| ch(key, true, false, false);
        let shift = |key: Key| ch(key, false, true, false);
        let alt = |key: Key| ch(key, false, false, true);
        let ctrl_shift = |key: Key| ch(key, true, true, false);
        let typed: String = ctx.input(|i| i.events.iter().filter_map(|e| if let egui::Event::Text(t) = e { Some(t.clone()) } else { None }).collect());

        // Quellmonitor aktiv: Transporttasten und I/O gelten für die Quelle
        if self.source.is_some() {
            if plain(Key::Space) {
                self.toggle_source_play();
                return;
            }
            if plain(Key::I) {
                self.set_source_in();
                return;
            }
            if plain(Key::O) {
                self.set_source_out();
                return;
            }
            if plain(Key::K) {
                self.pause_source();
                return;
            }
            if plain(Key::ArrowLeft) {
                self.step_source(-1);
                return;
            }
            if plain(Key::ArrowRight) {
                self.step_source(1);
                return;
            }
        }

        // Ziffern: Einfüge- und Mausmodus, Projektmarker, Zoomstufen
        const NUMS: [Key; 10] = [Key::Num0, Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];
        let fps = self.project.fps.max(1) as f64;
        for (n, key) in NUMS.iter().copied().enumerate() {
            if !pressed(key) {
                continue;
            }
            let n = n as u32;
            if shift(key) {
                self.set_bookmark(if n == 0 { 10 } else { n });
            } else if alt(key) {
                match n {
                    1 => self.zoom_to_span(1.0 / fps),
                    2 => self.zoom_to_span(5.0 / fps),
                    3 => self.zoom_to_span(1.0),
                    4 => self.zoom_to_span(10.0),
                    5 => self.zoom_to_span(60.0),
                    _ => {}
                }
            } else if ch(key, true, false, true) && n == 6 {
                self.zoom_to_span(600.0);
            } else if ctrl(key) {
                self.jump_to_bookmark(if n == 0 { 10 } else { n });
            } else if plain(key) {
                use crate::ops::{InsertMode as I, MouseMode as M};
                match n {
                    1 => self.set_insert_mode(I::Auto),
                    2 => self.set_insert_mode(I::Ripple),
                    3 => self.set_insert_mode(I::RippleAll),
                    5 => self.set_insert_mode(I::Overwrite),
                    6 => self.set_mouse_mode(M::Single),
                    7 => self.set_mouse_mode(M::AllTracks),
                    8 => self.set_mouse_mode(M::OneTrack),
                    _ => {}
                }
            }
        }

        // Bild↑/↓: seitenweise scrollen; mit Strg+Umschalt Projektmarker, mit Alt Kapitelmarken
        for (key, fwd) in [(Key::PageDown, true), (Key::PageUp, false)] {
            if plain(key) {
                self.scroll_pages(if fwd { 1.0 } else { -1.0 });
            } else if ctrl_shift(key) {
                self.jump_to_marker(fwd, false);
            } else if alt(key) {
                self.jump_to_marker(fwd, true);
            }
        }
        // Pfeile: Alt = Clips verschieben (Umschalt = 10 Bilder), Umschalt = 5 Bilder springen
        for (key, d) in [(Key::ArrowLeft, -1i64), (Key::ArrowRight, 1)] {
            if ch(key, false, false, true) {
                self.nudge_selection(d);
            } else if ch(key, false, true, true) {
                self.nudge_selection(10 * d);
            } else if shift(key) {
                self.step(5 * d);
            }
        }
        // Eingabetaste: Kapitelmarke, Projektmarker
        if pressed(Key::Enter) {
            if m.shift && !m.command && !m.alt {
                self.add_marker();
            } else if ctrl_shift(Key::Enter) {
                self.remove_chapter_near_playhead();
            } else if ch(Key::Enter, true, false, true) {
                self.remove_all_chapters();
            } else if ctrl(Key::Enter) {
                self.set_next_bookmark();
            }
        }
        // Effekte und Animator (Handbuch: Alt+Umschalt+H, O, G, F, C, B; Animation I, P, R)
        if m.shift && m.alt && !m.command && [Key::H, Key::O, Key::G, Key::F, Key::C, Key::B, Key::I, Key::P, Key::R].iter().any(|k| pressed(*k)) {
            self.inspector_tab = 3;
            self.status = "Effekte (Bild & Look, Animator)".into();
        }

        // Löschen
        if (plain(Key::Delete) || plain(Key::Backspace)) && !ctx.egui_wants_keyboard_input() {
            self.delete_selection();
        }
        if shift(Key::Delete) || shift(Key::Backspace) || ctrl(Key::Delete) || ctrl(Key::Backspace) {
            self.ripple_delete_selection();
        }

        // Buchstaben
        if plain(Key::Space) {
            self.set_playing(!self.playing);
        }
        if plain(Key::S) || plain(Key::T) {
            self.split_at_playhead();
        }
        if plain(Key::Z) {
            self.trim_selection_to_playhead(true);
        }
        if plain(Key::U) {
            self.trim_selection_to_playhead(false);
        }
        if plain(Key::X) {
            self.mark_gap();
        }
        if plain(Key::D) {
            self.duplicate_clips();
        }
        if plain(Key::F) {
            self.zoom_fit();
        }
        if alt(Key::F) {
            self.zoom_to_range();
        }
        if plain(Key::W) {
            self.jump_to_any_marker(true);
        }
        if plain(Key::Q) {
            self.jump_to_any_marker(false);
        }
        if alt(Key::W) {
            self.jump_to_edit_point(true);
        }
        if alt(Key::Q) {
            self.jump_to_edit_point(false);
        }
        if shift(Key::W) {
            self.jump_to_selected(true);
        }
        if shift(Key::Q) {
            self.jump_to_selected(false);
        }
        if ctrl(Key::W) {
            self.select_neighbor(true);
        }
        if ctrl(Key::Q) {
            self.select_neighbor(false);
        }
        if plain(Key::G) {
            self.group_selection();
        }
        if shift(Key::G) {
            self.ungroup_selection();
        }
        if shift(Key::I) {
            if let Some(t) = self.project.mark_in {
                self.seek(t);
            }
        }
        if shift(Key::O) {
            if let Some(t) = self.project.mark_out {
                self.seek(t);
            }
        }
        if alt(Key::L) {
            self.duck_selected();
        }
        if alt(Key::A) {
            self.inspector_tab = 0;
        }
        if typed.contains('<') {
            self.jump_to_gap(true);
        }
        if typed.contains('>') {
            self.jump_to_gap(false);
        }
        if typed.contains('#') {
            self.play_range();
        }
        if plain(Key::N) {
            self.snapping = !self.snapping;
        }
        if plain(Key::E) {
            self.show_settings = true;
        }
        if plain(Key::M) {
            self.add_marker();
        }
        if plain(Key::I) {
            self.set_mark_in();
        }
        if plain(Key::O) {
            self.set_mark_out();
        }
        // J/K/L: zurück (1 s), anhalten, abspielen
        if plain(Key::K) {
            self.set_playing(false);
        }
        if plain(Key::L) && !self.playing {
            self.set_playing(true);
        }
        if plain(Key::J) {
            self.set_playing(false);
            self.playhead = (self.playhead - 1.0).max(0.0);
        }
        if plain(Key::Backslash) {
            self.zoom_fit();
        }
        if plain(Key::Plus) || plain(Key::Equals) {
            self.zoom = (self.zoom * 1.4).min(600.0);
        }
        if plain(Key::Minus) {
            self.zoom = (self.zoom / 1.4).max(4.0);
        }
        if plain(Key::ArrowLeft) {
            self.step(-1);
        }
        if plain(Key::ArrowRight) {
            self.step(1);
        }
        if plain(Key::ArrowUp) {
            self.jump_to_edit_point(false);
        }
        if plain(Key::ArrowDown) {
            self.jump_to_edit_point(true);
        }
        if plain(Key::Home) {
            self.set_playing(false);
            self.playhead = 0.0;
        }
        if plain(Key::End) {
            self.set_playing(false);
            self.playhead = self.project.duration();
        }
        // Strg-Kürzel
        if ctrl(Key::ArrowUp) {
            self.zoom_step(true);
        }
        if ctrl(Key::ArrowDown) {
            self.zoom_step(false);
        }
        if ctrl(Key::Z) {
            self.do_undo();
        }
        if ctrl(Key::Y) || ctrl_shift(Key::Z) {
            self.do_redo();
        }
        if ctrl(Key::S) {
            self.save();
        }
        if ctrl_shift(Key::S) {
            self.save_as();
        }
        if ctrl(Key::O) {
            self.open_dialog();
        }
        if ctrl(Key::N) {
            self.new_project();
        }
        if ctrl(Key::I) {
            self.import_dialog();
        }
        if ctrl(Key::E) {
            self.export_dialog();
        }
        if ctrl(Key::D) {
            self.duplicate_clips();
        }
        if ctrl(Key::A) {
            self.select_all();
        }
        if ctrl(Key::C) {
            self.copy_clips();
        }
        if ctrl(Key::X) {
            self.cut_clips();
        }
        if ctrl(Key::V) {
            self.paste_clips();
        }
        if ctrl_shift(Key::C) {
            self.find_gap();
        }
        if ctrl_shift(Key::M) {
            self.musical_cut_adjustment(1);
        }
        if ch(Key::R, true, false, true) {
            self.toggle_recording();
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
            if self.play_until.is_some_and(|u| self.playhead >= u) {
                self.playhead = self.play_until.unwrap_or(self.playhead);
                self.set_playing(false);
            }
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
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&self.status).size(11.5).color(SECONDARY));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(format!("Einfügen: {} ({}) · Maus: {} ({})", self.insert_mode.label(), self.insert_mode.key(), self.mouse_mode.label(), self.mouse_mode.key())).size(11.0).color(SECONDARY));
                    });
                });
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
        self.timecode_window(&ctx);
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
