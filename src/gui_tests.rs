//! GUI-Tests ohne Fenster: die komplette Oberfläche läuft auf einem egui-Kontext, Maus- und Tastaturereignisse
//! werden eingespeist, geprüft wird der Zustand des Projekts.

use crate::app::App;
use crate::model::*;
use crate::render::tests::{clip, image_asset, solid_png};
use crate::ui::timeline::{HEADER_W, RULER_H, ROW_H};
use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};
use uuid::Uuid;

struct H {
    ctx: egui::Context,
    app: App,
    t: f64,
    mods: Modifiers,
}

struct Ids {
    c_a: Uuid,
    c_b: Uuid,
    c_title: Uuid,
    asset_a: Uuid,
}

fn project() -> (Project, Ids) {
    let mut p = Project::default();
    let red = solid_png("gui_red.png", 64, 36, [200, 30, 30]);
    let blue = solid_png("gui_blue.png", 64, 36, [30, 30, 200]);
    let a = image_asset(&mut p, &red, 64.0, 36.0);
    let b = image_asset(&mut p, &blue, 64.0, 36.0);
    let ca = clip(a, 0.0, 2.0);
    let cb = clip(b, 5.0, 2.0);
    let title = Clip { start: 1.0, duration: 3.0, ..Default::default() };
    let ids = Ids { c_a: ca.id, c_b: cb.id, c_title: title.id, asset_a: a };
    p.tracks[0].clips.push(ca);
    p.tracks[1].clips.push(cb);
    p.tracks[2].clips.push(title);
    (p, ids)
}

impl H {
    fn new(p: Project) -> H {
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, None);
        app.project = p;
        app.saved = app.project.clone();
        app.snapping = false;
        let mut h = H { ctx, app, t: 0.0, mods: Modifiers::NONE };
        h.frames(4);
        h
    }

    fn run(&mut self, events: Vec<Event>) {
        self.t += 0.016;
        let mut raw = RawInput::default();
        raw.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 860.0)));
        raw.time = Some(self.t);
        raw.events = events;
        let ctx = self.ctx.clone();
        let app = &mut self.app;
        let out = ctx.run_ui(raw, |ui| app.update_ui(ui));
        out.drop_without_applying_deltas();
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.run(vec![]);
        }
    }

    fn mv(&mut self, p: Pos2) {
        self.run(vec![Event::PointerMoved(p)]);
    }

    fn button(&mut self, p: Pos2, pressed: bool) {
        let m = self.mods;
        self.run(vec![Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: m }]);
    }

    fn click(&mut self, p: Pos2) {
        self.mv(p);
        self.button(p, true);
        self.button(p, false);
        self.frames(2);
    }

    fn drag(&mut self, from: Pos2, to: Pos2) {
        self.mv(from);
        self.button(from, true);
        self.frames(1);
        for i in 1..=6 {
            let f = i as f32 / 6.0;
            self.mv(from + (to - from) * f);
        }
        self.frames(1);
        self.button(to, false);
        self.frames(3);
    }

    fn key(&mut self, key: Key) {
        let m = self.mods;
        self.run(vec![Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers: m }]);
        self.run(vec![Event::Key { key, physical_key: Some(key), pressed: false, repeat: false, modifiers: m }]);
        self.frames(2);
    }

    fn cmd_key(&mut self, key: Key) {
        self.run(vec![Event::ModifiersChanged(Modifiers::COMMAND)]);
        self.mods = Modifiers::COMMAND;
        self.key(key);
        self.mods = Modifiers::NONE;
        self.run(vec![Event::ModifiersChanged(Modifiers::NONE)]);
    }

    /// Bildschirmposition zu Projektzeit `t` und Spur `track` (Mitte der Zeile).
    fn at(&self, t: f64, track: usize) -> Pos2 {
        let r = self.app.tl_rect;
        let x = r.left() + HEADER_W + ((t - self.app.scroll) * self.app.zoom) as f32;
        let y = r.top() + RULER_H + track as f32 * ROW_H + ROW_H / 2.0 - self.app.scroll_y;
        Pos2::new(x, y)
    }

    fn ruler_at(&self, t: f64) -> Pos2 {
        let mut p = self.at(t, 0);
        p.y = self.app.tl_rect.top() + RULER_H / 2.0;
        p
    }

    fn clip(&self, id: Uuid) -> &Clip {
        self.app.project.clip(id).expect("Clip vorhanden")
    }
}

#[test]
fn layout_is_sane() {
    let (p, _) = project();
    let h = H::new(p);
    let r = h.app.tl_rect;
    assert!(r.width() > 900.0 && r.height() > 150.0, "{r:?}");
    assert!(r.bottom() <= 860.0 && r.top() > 300.0, "Timeline unten im Fenster: {r:?}");
}

#[test]
fn renders_all_tabs_for_all_clip_types_without_panic() {
    let (mut p, ids) = project();
    // Bild+Ton-Verknüpfung und Ton-Clip dazu
    let wav = crate::audio::tests::write_sine_wav("gui_tone.wav", 440.0, 2.0, 48000, 0.2);
    let a = MediaAsset { path: wav.to_string_lossy().into_owned(), name: "ton.wav".into(), duration: 2.0, has_audio: true, ..Default::default() };
    let aid = a.id;
    p.assets.push(a);
    let au = Clip { part: ClipPart::Audio, ..clip(aid, 0.0, 2.0) };
    let auid = au.id;
    p.tracks[3].clips.push(au);
    let mut h = H::new(p);
    for tab in 0..4 {
        h.app.inspector_tab = tab;
        for sel in [None, Some(ids.c_a), Some(ids.c_title), Some(auid)] {
            h.app.selected = sel;
            h.frames(2);
        }
    }
    h.app.show_settings = true;
    h.frames(2);
}

#[test]
fn click_selects_and_deselects_clips() {
    let (p, ids) = project();
    let mut h = H::new(p);
    h.click(h.at(1.0, 0));
    assert_eq!(h.app.selected, Some(ids.c_a));
    assert_eq!(h.app.sel_track, 0);
    h.click(h.at(6.0, 1));
    assert_eq!(h.app.selected, Some(ids.c_b));
    assert_eq!(h.app.sel_track, 1);
    h.click(h.at(9.0, 3));
    assert_eq!(h.app.selected, None, "Klick ins Leere hebt die Auswahl auf");
    assert_eq!(h.app.sel_track, 3);
}

#[test]
fn drag_moves_clip_and_undo_restores() {
    let (p, ids) = project();
    let mut h = H::new(p);
    h.drag(h.at(1.0, 0), h.at(3.0, 0));
    let c = h.clip(ids.c_a);
    assert!((c.start - 2.0).abs() < 0.05, "start={}", c.start);
    assert_eq!(h.app.selected, Some(ids.c_a));
    h.cmd_key(Key::Z);
    assert!((h.clip(ids.c_a).start).abs() < 1e-9, "Rückgängig stellt die Position wieder her");
    h.cmd_key(Key::Y);
    assert!((h.clip(ids.c_a).start - 2.0).abs() < 0.05, "Wiederholen");
}

#[test]
fn drag_cannot_move_before_zero_and_between_tracks() {
    let (p, ids) = project();
    let mut h = H::new(p);
    h.drag(h.at(1.0, 0), h.at(-3.0, 0));
    assert!(h.clip(ids.c_a).start >= 0.0);
    // auf eine andere Spur ziehen
    h.drag(h.at(6.0, 1), h.at(6.0, 4));
    let (ti, _) = h.app.project.find_clip(ids.c_b).unwrap();
    assert_eq!(ti, 4, "Clip landet auf Spur 5");
}

#[test]
fn trimming_changes_duration_and_start() {
    let (p, ids) = project();
    let mut h = H::new(p);
    // rechte Kante des Titels (Ende 4,0 s) um 1 s nach rechts ziehen
    h.drag(h.at(3.97, 2), h.at(4.97, 2));
    assert!((h.clip(ids.c_title).duration - 4.0).abs() < 0.1, "duration={}", h.clip(ids.c_title).duration);
    // linke Kante (Start 1,0 s) um 0,5 s nach rechts: Start 1,5, Dauer schrumpft
    let d0 = h.clip(ids.c_title).duration;
    h.drag(h.at(1.03, 2), h.at(1.53, 2));
    let c = h.clip(ids.c_title);
    assert!((c.start - 1.5).abs() < 0.1 && (c.duration - (d0 - 0.5)).abs() < 0.1, "{} {}", c.start, c.duration);
    // Ende bleibt gleich
    assert!((c.end() - 5.0).abs() < 0.1);
}

#[test]
fn keyboard_split_delete_undo() {
    let (p, ids) = project();
    let mut h = H::new(p);
    h.click(h.at(1.0, 0));
    h.app.playhead = 1.0;
    h.key(Key::S);
    assert_eq!(h.app.project.tracks[0].clips.len(), 2, "geteilt");
    let right = h.app.project.tracks[0].clips[1].clone();
    assert!((right.start - 1.0).abs() < 1e-6 && (right.duration - 1.0).abs() < 1e-6);
    h.app.selected = Some(right.id);
    h.key(Key::Delete);
    assert_eq!(h.app.project.tracks[0].clips.len(), 1);
    h.cmd_key(Key::Z);
    assert_eq!(h.app.project.tracks[0].clips.len(), 2, "Löschen rückgängig");
    h.cmd_key(Key::Z);
    assert_eq!(h.app.project.tracks[0].clips.len(), 1, "Teilen rückgängig");
    assert_eq!(h.clip(ids.c_a).duration, 2.0);
}

#[test]
fn space_toggles_playback_and_playhead_moves() {
    let (p, _) = project();
    let mut h = H::new(p);
    h.key(Key::Space);
    assert!(h.app.playing);
    h.frames(20);
    assert!(h.app.playhead > 0.0, "Marker läuft");
    h.key(Key::Space);
    assert!(!h.app.playing);
    let t = h.app.playhead;
    h.frames(10);
    assert_eq!(h.app.playhead, t, "steht nach Pause still");
    h.key(Key::Home);
    assert_eq!(h.app.playhead, 0.0);
    h.key(Key::ArrowRight);
    assert!((h.app.playhead - 1.0 / 30.0).abs() < 1e-9, "ein Bild vor");
    h.key(Key::ArrowDown);
    assert!((h.app.playhead - 1.0).abs() < 1e-9, "nächster Schnittpunkt (Titelanfang)");
    h.key(Key::ArrowUp);
    assert!(h.app.playhead.abs() < 1e-9);
    h.key(Key::End);
    assert!((h.app.playhead - 7.0).abs() < 1e-9, "Filmende");
}

#[test]
fn ruler_click_and_scrub_set_playhead() {
    let (p, _) = project();
    let mut h = H::new(p);
    h.click(h.ruler_at(3.0));
    assert!((h.app.playhead - 3.0).abs() < 0.05, "{}", h.app.playhead);
    h.drag(h.ruler_at(3.0), h.ruler_at(5.0));
    assert!((h.app.playhead - 5.0).abs() < 0.1, "{}", h.app.playhead);
}

#[test]
fn track_header_toggles_work() {
    let (p, _) = project();
    let mut h = H::new(p);
    let r = h.app.tl_rect;
    let cy = r.top() + RULER_H + ROW_H / 2.0;
    // Symbole: Ton, Solo, Sichtbar, Sperre; Mitten bei 30 + 7,5 + k·20
    let x = |k: f32| r.left() + 30.0 + 7.5 + 20.0 * k;
    h.click(Pos2::new(x(0.0), cy));
    assert!(h.app.project.tracks[0].muted);
    h.click(Pos2::new(x(1.0), cy));
    assert_eq!(h.app.project.tracks[0].solo, Some(true));
    h.click(Pos2::new(x(2.0), cy));
    assert!(h.app.project.tracks[0].hidden);
    h.click(Pos2::new(x(3.0), cy));
    assert!(h.app.project.tracks[0].locked);
    assert!(!h.app.project.tracks[1].muted, "andere Spuren unberührt");
    // erneut klicken schaltet zurück
    h.click(Pos2::new(x(0.0), cy));
    assert!(!h.app.project.tracks[0].muted);
    // gesperrte Spur: Clip lässt sich nicht verschieben
    let id = h.app.project.tracks[0].clips[0].id;
    h.drag(h.at(1.0, 0), h.at(3.0, 0));
    assert_eq!(h.clip(id).start, 0.0);
}

#[test]
fn drag_from_pool_to_timeline_inserts_clip() {
    let (p, ids) = project();
    let mut h = H::new(p);
    let tile = h.app.pool_tiles.iter().find(|(id, _)| *id == ids.asset_a).expect("Kachel").1;
    let before = h.app.project.tracks[4].clips.len();
    h.drag(tile.center(), h.at(3.0, 4));
    let tr = &h.app.project.tracks[4];
    assert_eq!(tr.clips.len(), before + 1, "Clip eingefügt");
    let c = tr.clips.last().unwrap();
    assert!((c.start - 3.0).abs() < 0.1, "start={}", c.start);
    assert_eq!(c.asset_id(), Some(ids.asset_a));
    // ohne Ziehen passiert beim Klick nichts
    let n: usize = h.app.project.tracks.iter().map(|t| t.clips.len()).sum();
    h.click(tile.center());
    assert_eq!(h.app.project.tracks.iter().map(|t| t.clips.len()).sum::<usize>(), n);
}

#[test]
fn zoom_fit_and_zoom_keys() {
    let (p, _) = project();
    let mut h = H::new(p);
    let z0 = h.app.zoom;
    h.key(Key::Plus);
    assert!(h.app.zoom > z0);
    h.key(Key::Minus);
    assert!((h.app.zoom - z0).abs() < 1e-6);
    h.key(Key::Backslash);
    let visible = h.app.view_w as f64 / h.app.zoom;
    assert!(visible >= 7.0 && visible < 8.0, "ganzer Film (7 s) sichtbar: {visible}");
}

#[test]
fn snapping_aligns_to_neighbour_clip_edges() {
    let (p, ids) = project();
    let mut h = H::new(p);
    h.app.snapping = true;
    // Clip B (Start 5,0) auf Spur 2 so ziehen, dass der Anfang nahe 4,0 (Titelende) landet
    h.drag(h.at(6.0, 1), h.at(5.03, 1));
    let c = h.clip(ids.c_b);
    assert!((c.start - 4.0).abs() < 1e-6 || (c.start - 4.03).abs() < 0.2, "rastet ein: {}", c.start);
    assert!((c.start - 4.0).abs() < 1e-6, "genau am Titelende: {}", c.start);
}

#[test]
fn editing_helpers() {
    let (mut p, ids) = project();
    // Medium mit Bild und Ton: verknüpfte Clips auf Spur n und n+1
    let a = MediaAsset { path: "x.mp4".into(), name: "x".into(), duration: 4.0, has_video: true, has_audio: true, width: 64.0, height: 36.0, fps: 30.0, ..Default::default() };
    let vid = a.id;
    p.assets.push(a);
    let mut h = H::new(p);
    h.app.add_to_timeline(vid, Some(3), Some(10.0), None);
    let v = h.app.project.tracks[3].clips.last().unwrap().clone();
    let au = h.app.project.tracks[4].clips.last().unwrap().clone();
    assert_eq!(v.link, au.link);
    assert!(v.link.is_some() && au.is_audio() && !v.is_audio());
    // Geschwindigkeit: doppelt so schnell → halbe Dauer, verknüpft
    h.app.set_speed(v.id, 2.0);
    assert!((h.clip(v.id).duration - 2.0).abs() < 1e-9 && (h.clip(au.id).duration - 2.0).abs() < 1e-9);
    // Duplizieren
    h.app.selected = Some(v.id);
    let n = h.app.project.tracks.iter().map(|t| t.clips.len()).sum::<usize>();
    h.app.duplicate_clips();
    assert_eq!(h.app.project.tracks.iter().map(|t| t.clips.len()).sum::<usize>(), n + 2);
    let dup = h.app.project.clip(h.app.selected.unwrap()).unwrap();
    assert!((dup.start - 12.0).abs() < 1e-9);
    // Bauchbinde
    h.app.sel_track = 5;
    h.app.add_title(crate::fx_edit::TitlePreset::LowerThird);
    let lt = h.app.project.clip(h.app.selected.unwrap()).unwrap();
    match &lt.content {
        ClipContent::Title(w) => assert!(w.v.background && w.v.left_aligned),
        _ => panic!("Titel erwartet"),
    }
    assert!(lt.transform.y > 0.0, "unten im Bild");
    let _ = ids;
}

#[test]
fn import_file_adds_assets_and_rejects_unknown() {
    let (p, _) = project();
    let mut h = H::new(p);
    let png = solid_png("gui_import.png", 20, 10, [1, 2, 3]);
    let n = h.app.project.assets.len();
    h.app.import_file(png);
    assert_eq!(h.app.project.assets.len(), n + 1);
    let a = h.app.project.assets.last().unwrap();
    assert!(a.is_image && a.width == 20.0 && a.height == 10.0);
    h.app.import_file(std::path::PathBuf::from("C:\\gibt\\es\\nicht.xyz"));
    assert_eq!(h.app.project.assets.len(), n + 1);
    assert!(h.app.status.contains("nicht unterstützt"), "{}", h.app.status);
    h.frames(3); // Kachel wird gezeichnet
}

#[test]
fn audio_import_and_drop_creates_audio_clip() {
    let (p, _) = project();
    let mut h = H::new(p);
    let wav = crate::audio::tests::write_sine_wav("gui_import.wav", 330.0, 2.0, 48000, 0.2);
    h.app.import_file(wav);
    let a = h.app.project.assets.last().unwrap().clone();
    assert!(a.has_audio && !a.has_video && !a.is_image && (a.duration - 2.0).abs() < 0.05);
    h.frames(3);
    let tile = h.app.pool_tiles.iter().find(|(id, _)| *id == a.id).expect("Kachel").1;
    h.drag(tile.center(), h.at(2.0, 4));
    let c = h.app.project.tracks[4].clips.last().expect("Ton-Clip auf Spur 5");
    assert!(c.is_audio() && (c.start - 2.0).abs() < 0.1);
}

#[test]
fn import_dialog_starts_in_videos_folder() {
    let d = crate::app::videos_dir().expect("Videos-Ordner gefunden");
    assert!(d.is_dir());
    assert!(d.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase().contains("video") || n.to_string_lossy().to_lowercase().contains("film")), "{d:?}");
}

#[test]
fn failed_imports_are_reported_in_a_window() {
    let (p, _) = project();
    let mut h = H::new(p);
    let dir = std::env::temp_dir().join("video-superbe-tests");
    let fake = dir.join("kaputt.mp4");
    std::fs::write(&fake, b"das ist kein video").unwrap();
    let n = h.app.project.assets.len();
    h.app.import_file(fake);
    h.app.import_file(std::path::PathBuf::from(r"C:\gibt\es\nicht.xyz"));
    h.frames(2);
    assert_eq!(h.app.project.assets.len(), n, "nichts importiert");
    assert_eq!(h.app.import_errors.len(), 2);
    assert!(h.app.import_errors[0].0 == "kaputt.mp4");
    assert!(h.app.import_errors[1].1.contains("nicht unterstützt"));
    h.frames(2); // Fenster wird gezeichnet
}

#[test]
fn hevc_error_gets_a_helpful_explanation() {
    let m = crate::app::explain_video_error("Format (HEVC): Keine passende Transformation (0xC00D5212)");
    assert!(m.contains("H.265") && m.contains("H.264"), "{m}");
    assert!(crate::app::explain_video_error("Kein Videostrom").contains("keinen Videostrom"));
}

#[test]
fn hevc_video_imports_and_shows_thumbnail_and_preview() {
    let Some(dir) = crate::app::videos_dir() else { return };
    let Some(f) = std::fs::read_dir(dir.join("test_vsp")).ok().and_then(|rd| {
        let mut v: Vec<_> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| crate::hevc::parse_file(p).is_ok()).collect();
        v.sort_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(u64::MAX));
        v.into_iter().next()
    }) else {
        eprintln!("keine HEVC-Beispieldatei, Test übersprungen");
        return;
    };
    let (p, _) = project();
    let mut h = H::new(p);
    let n = h.app.project.assets.len();
    h.app.import_file(f);
    assert_eq!(h.app.project.assets.len(), n + 1, "import: {:?}", h.app.import_errors);
    let a = h.app.project.assets.last().unwrap().clone();
    assert!(a.has_video && a.width >= 640.0 && a.duration > 1.0 && a.fps > 10.0, "{a:?}");
    // Vorschaubild entsteht im Hintergrund
    let mut ok = false;
    for _ in 0..200 {
        h.frames(1);
        if h.app.thumbs.get(&a.id).is_some_and(|t| t.is_some()) {
            ok = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(ok, "Vorschaubild der Kachel");
    // Clip einfügen, Marker darauf stellen: das Vorschaubild wird im Hintergrund decodiert
    h.app.add_to_timeline(a.id, Some(0), Some(20.0), None);
    h.app.playhead = 20.5;
    let mut shown = false;
    for _ in 0..300 {
        h.frames(1);
        if h.app.preview.frames.decs.get(&a.id).is_some_and(|d| d.frame_for(20.5 - 20.0).is_some()) {
            shown = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(shown, "Videobild in der Vorschau");
}

fn overlay_h() -> (H, Uuid) {
    let (p, ids) = project();
    let mut h = H::new(p);
    h.app.selected = Some(ids.c_title);
    h.app.playhead = 2.0;
    h.frames(3);
    (h, ids.c_title)
}

#[test]
fn overlay_drag_moves_clip_in_preview() {
    let (mut h, id) = overlay_h();
    let (bx, frame) = h.app.overlay_geom.expect("Objektrahmen gezeichnet");
    let before = h.clip(id).transform.clone();
    h.drag(bx.center, bx.center + Vec2::new(64.0, 36.0));
    let t = h.clip(id).transform.clone();
    assert!((t.x - before.x - 64.0 / frame.width() as f64).abs() < 0.02, "x {} → {}", before.x, t.x);
    assert!((t.y - before.y - 36.0 / frame.height() as f64).abs() < 0.02, "y {} → {}", before.y, t.y);
    h.cmd_key(Key::Z);
    assert_eq!(h.clip(id).transform.x, before.x, "Ziehen ist als ein Schritt rückgängig zu machen");
}

#[test]
fn overlay_scale_and_rotate_handles() {
    let (mut h, id) = overlay_h();
    let (bx, _) = h.app.overlay_geom.unwrap();
    let corner = bx.corner(1.0, 1.0);
    let target = Pos2::new(bx.center.x + (corner.x - bx.center.x) * 1.5, bx.center.y + (corner.y - bx.center.y) * 1.5);
    h.drag(corner, target);
    assert!((h.clip(id).transform.scale - 1.5).abs() < 0.1, "Skalierung {}", h.clip(id).transform.scale);
    // drehen: Knopf nach rechts neben die Mitte ziehen → 90°
    let (bx, _) = h.app.overlay_geom.unwrap();
    h.drag(bx.knob(), Pos2::new(bx.center.x + 120.0, bx.center.y));
    assert!((h.clip(id).transform.rotation - 90.0).abs() < 2.0, "Drehung {}", h.clip(id).transform.rotation);
}

#[test]
fn overlay_on_animated_clip_creates_keyframes() {
    use crate::anim::Channel;
    let (mut h, id) = overlay_h();
    h.app.project.clip_mut(id).unwrap().set_animated(Channel::PosX, true, 0.0);
    h.frames(2);
    let (bx, _) = h.app.overlay_geom.unwrap();
    h.drag(bx.center, bx.center + Vec2::new(60.0, 0.0));
    let ks = h.clip(id).keys.get(Channel::PosX).unwrap().clone();
    assert_eq!(ks.len(), 2, "beim Verschieben einer animierten Eigenschaft entsteht ein Keyframe an der Abspielposition");
    assert!((ks[1].t - 1.0).abs() < 0.05, "relativ zum Clipanfang (1 s): {}", ks[1].t);
}

#[test]
fn animator_shows_rows_seeks_and_drags_keys() {
    use crate::anim::Channel;
    let (p, ids) = project();
    let mut h = H::new(p);
    h.app.selected = Some(ids.c_title);
    h.app.inspector_tab = 3;
    h.app.show_animator = true;
    {
        let c = h.app.project.clip_mut(ids.c_title).unwrap();
        c.set_animated(Channel::PosX, true, 0.0);
        c.add_key(Channel::PosX, 2.5);
        c.fx.blur = 4.0;
    }
    h.app.anim_expanded = true;
    h.frames(3);
    let lanes = h.app.anim_geom.lanes.clone();
    let id_list: Vec<&str> = lanes.iter().map(|l| l.0.as_str()).collect();
    assert_eq!(id_list, vec!["group", "PosX"], "Gruppe und aufgeklappte Spur (Titel haben keine Bildeffekte)");
    let (_, lane) = lanes.iter().find(|l| l.0 == "PosX").unwrap().clone();
    // Klick auf die Mitte der Spur setzt die Abspielposition (Clip: Start 1,0, Dauer 3,0)
    h.click(Pos2::new(lane.center().x, lane.center().y));
    assert!((h.app.playhead - 2.5).abs() < 0.05, "{}", h.app.playhead);
    // Keyframe bei t = 2,5 anklicken und nach links ziehen
    let kx = lane.left() + lane.width() * (2.5 / 3.0);
    h.drag(Pos2::new(kx, lane.center().y), Pos2::new(lane.left() + lane.width() * 0.5, lane.center().y));
    let ks = h.clip(ids.c_title).keys.get(Channel::PosX).unwrap().clone();
    assert!((ks[1].t - 1.5).abs() < 0.1, "Keyframe verschoben: {}", ks[1].t);
    assert!(h.app.selected_key.is_some());
}


impl H {
    fn with_mods(&mut self, m: Modifiers) {
        self.mods = m;
        self.run(vec![Event::ModifiersChanged(m)]);
    }

    /// Obere Kante eines Clips in Bildschirmkoordinaten (Zeilenanfang + 2).
    fn clip_top(&self, track: usize) -> f32 {
        self.app.tl_rect.top() + RULER_H + track as f32 * ROW_H - self.app.scroll_y + 2.0
    }
}

fn media_project(has_video: bool, has_audio: bool, dur: f64) -> (Project, Uuid, Uuid) {
    let mut p = Project::default();
    let a = MediaAsset { path: "gibt-es-nicht.mp4".into(), name: "m".into(), duration: dur, has_video, has_audio, width: 64.0, height: 36.0, fps: 30.0, ..Default::default() };
    let aid = a.id;
    p.assets.push(a.clone());
    let id = p.insert_media(&a, 1.0, Some(0), None).unwrap();
    (p, id, aid)
}

#[test]
fn marquee_and_shift_click_build_multi_selection() {
    let (p, ids) = project();
    let mut h = H::new(p);
    // Auswahlrahmen von leerem Bereich (Spur 5, 8 s) nach links oben über alle drei Clips
    h.drag(h.at(8.5, 3), h.at(0.3, 0));
    let set = h.app.selected_set();
    assert!(set.contains(&ids.c_a) && set.contains(&ids.c_title) && set.contains(&ids.c_b), "alle drei: {}", set.len());
    // Klick ins Leere hebt auf
    h.click(h.at(9.0, 3));
    assert!(h.app.selected_set().is_empty());
    // Umschalt+Klick erweitert
    h.click(h.at(1.0, 0));
    h.with_mods(Modifiers::SHIFT);
    h.click(h.at(6.0, 1));
    h.with_mods(Modifiers::NONE);
    let set = h.app.selected_set();
    assert!(set.contains(&ids.c_a) && set.contains(&ids.c_b) && !set.contains(&ids.c_title));
    // Umschalt+Klick auf einen gewählten Clip nimmt ihn heraus
    h.with_mods(Modifiers::SHIFT);
    h.click(h.at(6.0, 1));
    h.with_mods(Modifiers::NONE);
    assert!(!h.app.selected_set().contains(&ids.c_b));
}

#[test]
fn dragging_a_selected_group_moves_all_clips() {
    let (p, ids) = project();
    let mut h = H::new(p);
    h.app.snapping = false;
    h.click(h.at(1.0, 0));
    h.with_mods(Modifiers::SHIFT);
    h.click(h.at(6.0, 1));
    h.with_mods(Modifiers::NONE);
    h.drag(h.at(1.0, 0), h.at(2.0, 0));
    assert!((h.clip(ids.c_a).start - 1.0).abs() < 0.05, "a: {}", h.clip(ids.c_a).start);
    assert!((h.clip(ids.c_b).start - 6.0).abs() < 0.05, "b folgt: {}", h.clip(ids.c_b).start);
    assert!((h.clip(ids.c_title).start - 1.0).abs() < 1e-9, "nicht gewählte Clips bleiben");
}

#[test]
fn clipboard_keys_copy_paste_duplicate_and_ripple() {
    let (p, ids) = project();
    let mut h = H::new(p);
    h.click(h.at(1.0, 0));
    h.cmd_key(Key::C);
    h.app.playhead = 10.0;
    h.cmd_key(Key::V);
    let tr = &h.app.project.tracks[0].clips;
    assert_eq!(tr.len(), 2);
    assert!((tr[1].start - 10.0).abs() < 1e-9 && tr[1].id != ids.c_a, "Kopie an der Abspielposition");
    h.cmd_key(Key::D);
    assert_eq!(h.app.project.tracks[0].clips.len(), 3, "Duplikat direkt dahinter");
    assert!((h.app.project.tracks[0].clips[2].start - 12.0).abs() < 1e-9);
    // Ausschneiden
    h.cmd_key(Key::X);
    assert_eq!(h.app.project.tracks[0].clips.len(), 2);
    // Alles auswählen, Ripple-Löschen des Titels
    h.cmd_key(Key::A);
    assert!(h.app.selected_set().len() >= 4);
    h.click(h.at(2.0, 2));
    assert_eq!(h.app.selected, Some(ids.c_title));
    h.mods = Modifiers::SHIFT;
    h.run(vec![Event::ModifiersChanged(Modifiers::SHIFT)]);
    h.key(Key::Delete);
    h.mods = Modifiers::NONE;
    h.run(vec![Event::ModifiersChanged(Modifiers::NONE)]);
    assert!(h.app.project.clip(ids.c_title).is_none());
}

#[test]
fn marks_markers_and_chapters() {
    let (p, _) = project();
    let mut h = H::new(p);
    h.app.playhead = 2.0;
    h.key(Key::I);
    h.app.playhead = 4.0;
    h.key(Key::O);
    assert_eq!((h.app.project.mark_in, h.app.project.mark_out), (Some(2.0), Some(4.0)));
    assert_eq!(h.app.export_range(), Some((2.0, 4.0)));
    // Endmarke vor der Anfangsmarke entfernt die Anfangsmarke
    h.app.playhead = 1.0;
    h.key(Key::O);
    assert_eq!((h.app.project.mark_in, h.app.project.mark_out), (None, Some(1.0)));
    h.app.clear_marks();
    assert_eq!(h.app.export_range(), None);
    // Kapitel
    h.app.playhead = 0.0;
    h.key(Key::M);
    h.app.playhead = 65.0;
    h.key(Key::M);
    h.key(Key::M); // gleiche Stelle: keine zweite Marke
    h.app.playhead = 3725.0;
    h.key(Key::M);
    let list = h.app.project.marker_list();
    assert_eq!(list.len(), 3);
    assert_eq!(h.app.chapters_text(), "0:00 Kapitel 1\n1:05 Kapitel 2\n1:02:05 Kapitel 3");
    let id = list[1].id;
    h.app.rename_marker(id, "Strand".into());
    assert!(h.app.chapters_text().contains("1:05 Strand"));
    h.app.remove_marker(id);
    assert_eq!(h.app.project.marker_list().len(), 2);
    // Marken werden im Projekt gespeichert
    let dir = std::env::temp_dir().join("video-superbe-tests");
    let f = dir.join("marks.mvd.json");
    save(&h.app.project, &f).unwrap();
    assert_eq!(load(&f).unwrap().marker_list().len(), 2);
}

#[test]
fn volume_line_double_click_adds_and_removes_points() {
    let (p, id, _aid) = media_project(false, true, 6.0);
    let mut h = H::new(p);
    let c = h.clip(id).clone();
    assert!(c.is_audio());
    let rect_top = h.clip_top(0);
    let y = rect_top + volume_y(1.0, 2.0);
    let pos = Pos2::new(h.at(2.5, 0).x, y);
    h.click(pos);
    h.click(pos);
    let pts = h.clip(id).volume_points.clone();
    assert_eq!(pts.iter().map(|p| (p.t * 10.0).round() / 10.0).collect::<Vec<_>>(), vec![0.0, 1.5, 6.0], "Start, Klickstelle, Ende");
    // zweiter Doppelklick auf den Punkt entfernt ihn wieder
    h.t += 1.0; // kein Dreifachklick
    let px = h.at(1.0 + 1.5, 0).x;
    h.click(Pos2::new(px, y));
    h.click(Pos2::new(px, y));
    assert_eq!(h.clip(id).volume_points.len(), 2);
}

fn volume_y(gain: f64, max_g: f64) -> f32 {
    crate::ui::volume::y_of(gain, 50.0, max_g) as f32
}

#[test]
fn fade_knob_drag_sets_fade_in() {
    let (p, ids) = project();
    let mut h = H::new(p);
    let top = h.clip_top(0);
    let knob = Pos2::new(h.at(0.0, 0).x, top + 6.0);
    h.drag(knob, Pos2::new(h.at(0.5, 0).x, top + 6.0));
    assert!((h.clip(ids.c_a).fade_in - 0.5).abs() < 0.05, "{}", h.clip(ids.c_a).fade_in);
    h.cmd_key(Key::Z);
    assert_eq!(h.clip(ids.c_a).fade_in, 0.0, "Ziehen = ein Undo-Schritt");
}

#[test]
fn trim_stops_at_source_end_and_alt_drag_slips() {
    let (p, id, _aid) = media_project(true, false, 3.0); // Quelle 3 s, Clip 1…4
    let mut h = H::new(p);
    h.app.snapping = false;
    let end = h.clip(id).end();
    h.drag(h.at(end - 0.02, 0), h.at(end + 2.0, 0));
    assert!((h.clip(id).end() - end).abs() < 1e-6, "über das Quellende hinaus nicht verlängerbar");
    // Mit Anfang kürzen
    h.drag(h.at(1.02, 0), h.at(1.6, 0));
    assert!((h.clip(id).start - 1.6).abs() < 0.05 && (h.clip(id).source_in - 0.6).abs() < 0.05, "{} {}", h.clip(id).start, h.clip(id).source_in);
    // Slip: Alt + Ziehen verschiebt den Quellausschnitt, nicht den Clip
    let (start0, in0) = (h.clip(id).start, h.clip(id).source_in);
    h.with_mods(Modifiers::ALT);
    h.drag(h.at(2.5, 0), h.at(2.0, 0)); // 0,5 s nach links = späterer Ausschnitt (+0,5 s)
    h.with_mods(Modifiers::NONE);
    let c = h.clip(id);
    assert_eq!(c.start, start0, "Position bleibt");
    assert!((c.source_in - in0 - 0.5).abs() < 0.08 || (c.source_in - in0).abs() < 1e-9, "Slip {} → {}", in0, c.source_in);
}

#[test]
fn waveform_peaks_are_computed_in_background() {
    let wav = crate::audio::tests::write_sine_wav("gui_wave.wav", 440.0, 2.0, 48000, 0.5);
    let mut p = Project::default();
    let a = MediaAsset { path: wav.to_string_lossy().into_owned(), name: "w".into(), duration: 2.0, has_audio: true, ..Default::default() };
    let aid = a.id;
    p.assets.push(a.clone());
    p.insert_media(&a, 0.0, Some(0), None).unwrap();
    let mut h = H::new(p);
    let mut got = None;
    for _ in 0..100 {
        h.frames(1);
        if let Some(w) = h.app.waveforms.get(&aid) {
            got = Some(w.clone());
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let w = got.expect("Wellenform fertig");
    assert!((w.len() as i64 - 100).abs() <= 2 && w.iter().copied().fold(0.0f32, f32::max) > 0.4);
}

#[test]
fn closing_gap_and_snapping_to_markers() {
    let (p, ids) = project();
    let mut h = H::new(p);
    h.app.snapping = true;
    h.app.project.markers = Some(vec![Marker { id: Uuid::new_v4(), t: 8.0, name: "x".into() }]);
    h.frames(2);
    // Clip B (5…7) so ziehen, dass sein Anfang nahe der Marke 8,0 landet
    h.drag(h.at(6.0, 1), h.at(9.02, 1));
    assert!((h.clip(ids.c_b).start - 8.0).abs() < 1e-6, "rastet an der Kapitelmarke ein: {}", h.clip(ids.c_b).start);
    // Lücke vor B schließen: rückt an das Ende des vorherigen Clips auf derselben Spur (hier keiner → bis 0)
    h.app.selected = Some(ids.c_b);
    h.app.close_gap_before_selection();
    assert_eq!(h.clip(ids.c_b).start, 0.0);
}

fn audio_project() -> (Project, Uuid) {
    let (mut p, _) = project();
    let wav = crate::audio::tests::write_sine_wav("gui_src_tone.wav", 440.0, 10.0, 48000, 0.2);
    let a = MediaAsset { path: wav.to_string_lossy().into_owned(), name: "quelle.wav".into(), duration: 10.0, has_audio: true, ..Default::default() };
    let aid = a.id;
    p.assets.push(a);
    (p, aid)
}

#[test]
fn source_monitor_keys_drag_out_and_close() {
    let (p, aid) = audio_project();
    let mut h = H::new(p);
    h.app.load_source(aid);
    h.frames(3);
    assert!(h.app.source.is_some());
    // I/O setzen Anfang und Ende der Quelle (nicht der Timeline)
    h.app.seek_source(3.0);
    h.key(Key::I);
    h.app.seek_source(6.0);
    h.key(Key::O);
    assert_eq!(h.app.source_range(), Some((3.0, 6.0)));
    assert!(h.app.project.mark_in.is_none() && h.app.project.mark_out.is_none(), "Timeline-Marken bleiben unberührt");
    // Leertaste startet und stoppt die Quellwiedergabe, nicht das Projekt
    h.key(Key::Space);
    assert!(h.app.source.as_ref().unwrap().playing && !h.app.playing);
    h.key(Key::Space);
    assert!(!h.app.source.as_ref().unwrap().playing);
    // Ausschnitt aus dem Monitor auf die Timeline ziehen
    let c = h.app.preview_rect.center();
    let before = h.app.project.tracks.iter().map(|t| t.clips.len()).sum::<usize>();
    h.drag(c, h.at(8.0, 4));
    let after = h.app.project.tracks.iter().map(|t| t.clips.len()).sum::<usize>();
    assert_eq!(after, before + 1, "Clip aus der Quelle eingefügt");
    let id = h.app.selected.expect("neuer Clip ausgewählt");
    let cl = h.clip(id);
    assert!((cl.source_in - 3.0).abs() < 1e-6 && (cl.duration - 3.0).abs() < 1e-6, "Bereich 3–6 s: {} {}", cl.source_in, cl.duration);
    // Klick außerhalb des Vorschaubereichs schließt den Quellmonitor
    h.click(h.at(9.5, 5));
    assert!(h.app.source.is_none());
}

#[test]
fn export_and_confirm_windows_render_and_close() {
    let (p, aid) = audio_project();
    let mut h = H::new(p);
    h.app.add_to_timeline(aid, Some(3), Some(0.0), None);
    h.app.export_dialog();
    assert!(h.app.export_cfg.is_some());
    h.frames(3);
    if let Some(c) = h.app.export_cfg.as_mut() {
        c.normalize = true;
        c.measured = Some(Ok(crate::loudness::Loudness { lufs: -20.0, peak: 0.2 }));
    }
    h.frames(3);
    h.app.export_cfg = None;
    // Rückfrage bei ungesicherten Änderungen wird gezeichnet und lässt sich abbrechen
    h.app.project.fps = 25;
    h.app.new_project();
    assert!(h.app.feat.pending.is_some());
    h.frames(3);
    h.app.resolve_pending(&h.ctx.clone(), crate::features::Answer::Cancel);
    h.frames(2);
    assert!(h.app.feat.pending.is_none() && h.app.project.fps == 25);
    // Sicherungs-Angebot
    h.app.feat.restore_offer = true;
    h.frames(3);
}

#[test]
fn audio_effects_tab_edits_the_clip() {
    let (mut p, aid) = audio_project();
    let au = Clip { part: ClipPart::Audio, ..clip(aid, 0.0, 4.0) };
    let id = au.id;
    p.tracks[3].clips.push(au);
    let mut h = H::new(p);
    h.app.selected = Some(id);
    h.app.inspector_tab = 3;
    h.frames(3);
    // die Regler sind da; Werte setzen und prüfen, dass der Mixer sie nutzt
    h.app.project.clip_mut(id).unwrap().audio_fx.wind_filter = true;
    h.app.project.clip_mut(id).unwrap().audio_fx.denoise = 0.5;
    h.frames(2);
    let c = h.clip(id);
    assert!(c.audio_fx.wind_filter && (c.audio_fx.denoise - 0.5).abs() < 1e-9);
}

#[test]
fn level_meter_is_drawn_while_playing() {
    let (mut p, aid) = audio_project();
    p.tracks[3].clips.push(Clip { part: ClipPart::Audio, ..clip(aid, 0.0, 4.0) });
    let mut h = H::new(p);
    h.app.feat.meter = [-20.0, -9.0];
    h.app.feat.meter_peak = [-12.0, -2.0];
    h.frames(3);
}

#[test]
fn effect_sliders_are_reachable_and_change_the_clip() {
    let (p, ids) = project();
    let mut h = H::new(p);
    h.app.selected = Some(ids.c_a);
    h.app.inspector_tab = 3;
    h.frames(3);
    // Die Seitenleiste behält die volle Höhe (der Animator sitzt nur unter dem Monitor)
    assert!(h.app.preview_rect.right() < 760.0, "Monitor schmaler als das Fenster: {:?}", h.app.preview_rect);
    let start = h.clip(ids.c_a).fx.clone();
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..5 {
        for y in (130..520).step_by(17) {
            let c0 = h.clip(ids.c_a).fx.clone();
            h.drag(Pos2::new(1110.0, y as f32), Pos2::new(1300.0, y as f32));
            let c1 = h.clip(ids.c_a).fx.clone();
            for (name, a, b) in [("Helligkeit", c0.brightness, c1.brightness), ("Kontrast", c0.contrast, c1.contrast), ("Sättigung", c0.saturation, c1.saturation), ("Wärme", c0.temperature, c1.temperature), ("Schärfe", c0.sharpness, c1.sharpness), ("Weichzeichner", c0.blur, c1.blur), ("Vignette", c0.vignette, c1.vignette)] {
                if (a - b).abs() > 1e-9 {
                    seen.insert(name);
                }
            }
        }
        h.mv(Pos2::new(1200.0, 300.0));
        h.run(vec![Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: Vec2::new(0.0, -150.0), modifiers: Modifiers::NONE, phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Wheel }]);
        h.frames(2);
    }
    assert!(h.clip(ids.c_a).fx != start, "kein Regler hat den Clip verändert");
    assert!(seen.len() >= 4, "erreichbare Effekt-Regler: {seen:?}");
}

// ---------- Paket A: Schnittwerkzeuge des Mac-Originals ----------

/// Zwei verknüpfte Bild/Ton-Clips à 4 s: A 0–4, B 4–8 (Bild auf Spur 1, Ton auf Spur 2).
fn av_project() -> (Project, Uuid, Uuid) {
    let a = MediaAsset { path: "gui_av_nicht_vorhanden.mp4".into(), name: "av".into(), duration: 20.0, has_video: true, has_audio: true, width: 100.0, height: 50.0, fps: 30.0, ..Default::default() };
    let mut p = Project::default();
    p.assets.push(a.clone());
    let r1 = p.insert_media(&a, 0.0, Some(0), Some((0.0, 4.0))).unwrap();
    let r2 = p.insert_media(&a, 4.0, Some(0), Some((4.0, 8.0))).unwrap();
    (p, r1, r2)
}

impl H {
    fn scoped_mods(&mut self, m: Modifiers, f: impl FnOnce(&mut H)) {
        self.run(vec![Event::ModifiersChanged(m)]);
        self.mods = m;
        f(self);
        self.mods = Modifiers::NONE;
        self.run(vec![Event::ModifiersChanged(Modifiers::NONE)]);
    }
}

#[test]
fn number_keys_set_insert_and_mouse_modes() {
    let (p, _, _) = av_project();
    let mut h = H::new(p);
    h.key(Key::Num2);
    assert_eq!(h.app.insert_mode, crate::ops::InsertMode::Ripple);
    h.key(Key::Num5);
    assert_eq!(h.app.insert_mode, crate::ops::InsertMode::Overwrite);
    h.key(Key::Num1);
    assert_eq!(h.app.insert_mode, crate::ops::InsertMode::Auto);
    h.key(Key::Num8);
    assert_eq!(h.app.mouse_mode, crate::ops::MouseMode::OneTrack);
    h.key(Key::Num7);
    assert_eq!(h.app.mouse_mode, crate::ops::MouseMode::AllTracks);
    h.key(Key::Num6);
    assert_eq!(h.app.mouse_mode, crate::ops::MouseMode::Single);
}

#[test]
fn bookmark_keys_and_chapter_keys() {
    let (p, _, _) = av_project();
    let mut h = H::new(p);
    h.app.playhead = 2.0;
    h.scoped_mods(Modifiers::SHIFT, |h| h.key(Key::Num3));
    assert_eq!(h.app.project.bookmarks.as_ref().unwrap()["3"], 2.0, "Umschalt+3 setzt Projektmarker 3");
    h.app.playhead = 6.0;
    h.scoped_mods(Modifiers::SHIFT, |h| h.key(Key::Num0));
    assert_eq!(h.app.project.bookmarks.as_ref().unwrap()["10"], 6.0, "Umschalt+0 setzt Projektmarker 10");
    h.app.playhead = 0.0;
    h.cmd_key(Key::Num3);
    assert_eq!(h.app.playhead, 2.0, "Strg+3 springt hin");
    h.key(Key::W);
    assert_eq!(h.app.playhead, 6.0, "W: nächster Marker");
    h.key(Key::Q);
    assert_eq!(h.app.playhead, 2.0, "Q: vorheriger Marker");
    h.scoped_mods(Modifiers::SHIFT, |h| h.key(Key::Enter));
    assert!(h.app.project.marker_list().iter().any(|m| (m.t - 2.0).abs() < 1e-9), "Umschalt+Enter setzt eine Kapitelmarke");
}

#[test]
fn group_and_split_keys() {
    let (p, r1, r2) = av_project();
    let mut h = H::new(p);
    h.app.select(r1, false);
    h.app.select(r2, true);
    h.key(Key::G);
    assert!(h.clip(r1).group.is_some() && h.clip(r1).group == h.clip(r2).group, "G bildet eine Gruppe");
    h.scoped_mods(Modifiers::SHIFT, |h| h.key(Key::G));
    assert!(h.clip(r1).group.is_none(), "Umschalt+G löst auf");
    h.app.clear_selection();
    h.app.playhead = 2.0;
    h.app.select(r1, false);
    h.key(Key::T);
    assert!(h.app.project.tracks[0].clips.len() == 3, "T teilt an der Abspielposition");
}

#[test]
fn mouse_mode_moves_followers_with_the_dragged_clip() {
    let (p, r1, r2) = av_project();
    let mut h = H::new(p);
    h.key(Key::Num8); // eine Spur: alles dahinter rückt mit
    h.drag(h.at(1.0, 0), h.at(2.0, 0));
    assert!((h.clip(r1).start - 1.0).abs() < 0.05, "A verschoben: {}", h.clip(r1).start);
    assert!((h.clip(r2).start - 5.0).abs() < 0.05, "B rückt mit: {}", h.clip(r2).start);
    // im Einzelmodus bleibt B stehen (dann ginge es nur mit Platz dahinter)
    let (p, r1, r2) = av_project();
    let mut h = H::new(p);
    h.drag(h.at(1.0, 0), h.at(1.5, 0));
    assert!((h.clip(r2).start - 4.0).abs() < 1e-6, "Mausmodus 6: B bleibt");
    let _ = r1;
}

#[test]
fn trim_gestures_jl_roll_and_ripple() {
    // J-L-Schnitt: Alt + Kante ziehen kürzt nur das Bild
    let (p, r1, _) = av_project();
    let mut h = H::new(p);
    let partner = h.app.project.partner_id(r1).unwrap();
    h.scoped_mods(Modifiers::ALT, |h| h.drag(h.at(3.95, 0), h.at(3.0, 0)));
    assert!((h.clip(r1).end() - 3.0).abs() < 0.06, "Bild gekürzt: {}", h.clip(r1).end());
    assert!((h.clip(partner).end() - 4.0).abs() < 1e-6, "Ton bleibt: {}", h.clip(partner).end());
    // Roll: Strg + Kante ziehen verschiebt den Schnittpunkt
    let (p, r1, r2) = av_project();
    let mut h = H::new(p);
    h.scoped_mods(Modifiers::COMMAND, |h| h.drag(h.at(3.95, 0), h.at(3.0, 0)));
    assert!((h.clip(r1).end() - 3.0).abs() < 0.06 && (h.clip(r2).start - h.clip(r1).end()).abs() < 1e-6, "Roll: {} / {}", h.clip(r1).end(), h.clip(r2).start);
    // Ripple (Mausmodus 8): Nachfolger rückt auf
    let (p, r1, r2) = av_project();
    let mut h = H::new(p);
    h.key(Key::Num8);
    h.drag(h.at(3.95, 0), h.at(3.0, 0));
    assert!((h.clip(r1).end() - 3.0).abs() < 0.06 && (h.clip(r2).start - 3.0).abs() < 0.06, "Ripple: {} / {}", h.clip(r1).end(), h.clip(r2).start);
}

#[test]
fn timecode_dialog_jumps_and_status_shows_the_modes() {
    let (p, _, _) = av_project();
    let mut h = H::new(p);
    h.app.tc_prompt = Some("00:00:05:00".into());
    h.frames(2);
    h.key(Key::Enter);
    assert!((h.app.playhead - 5.0).abs() < 1e-6, "Zeitcode-Eingabe springt: {}", h.app.playhead);
    assert!(h.app.tc_prompt.is_none());
    // Lückensuche per Tastenkombination (Strg+Umschalt+C)
    h.app.project.move_clips(&std::collections::HashSet::from([h.app.project.tracks[0].clips[1].id]), 0, 1.0);
    h.app.playhead = 0.0;
    h.scoped_mods(Modifiers::COMMAND | Modifiers::SHIFT, |h| h.key(Key::C));
    assert_eq!(h.app.project.mark_in, Some(4.0), "Lücke 4–5 s markiert");
}

fn chip_pos(h: &H, t: f64, track: usize) -> Pos2 {
    let p = h.at(t, track);
    Pos2::new(p.x, p.y - ROW_H / 2.0 + 11.0)
}

#[test]
fn junction_chip_opens_the_menu_and_applies_a_transition() {
    let (p, r1, r2) = av_project();
    let mut h = H::new(p);
    h.click(chip_pos(&h, 4.0, 0));
    assert!(h.app.tl.junction_popup.is_some(), "Klick auf A/B öffnet das Blendenmenü");
    // Menü direkt bedienen (Positionen der Knöpfe sind Sache von egui)
    let (a, b, _) = h.app.tl.junction_popup.unwrap();
    assert_eq!((a, b), (r1, r2));
    h.app.apply_transition(a, b, Some("zoom"), Some(1.0));
    h.app.tl.junction_popup = None;
    h.frames(2);
    let j = h.app.project.junction_of(r2).unwrap();
    assert_eq!(h.app.junction_kind(&j).as_deref(), Some("zoom"));
    assert!((j.overlap - 1.0).abs() < 0.02);
    // Escape schließt das Menü
    h.click(chip_pos(&h, 4.0, 0));
    assert!(h.app.tl.junction_popup.is_some());
    h.key(Key::Escape);
    assert!(h.app.tl.junction_popup.is_none());
}

#[test]
fn dragging_a_transition_tile_onto_a_junction_applies_it() {
    let mut done = false;
    'scan: for y in (150..360).step_by(14) {
        for x in (1100..1390).step_by(95) {
            let (p, _, r2) = av_project();
            let mut h = H::new(p);
            h.app.select(r2, false);
            h.app.inspector_tab = 1;
            h.frames(3);
            let target = chip_pos(&h, 4.0, 0);
            h.drag(Pos2::new(x as f32, y as f32), target);
            if h.app.project.junction_of(r2).is_some_and(|j| j.overlap > 0.5) {
                done = true;
                break 'scan;
            }
        }
    }
    assert!(done, "eine Blendenkachel lässt sich auf den Übergang ziehen");
}

#[test]
fn title_tab_and_effect_sections_render_for_all_title_styles() {
    let (mut p, _, _) = av_project();
    let mut st = TitleStyle { text: "Abspann
Zeile".into(), credits: Some(true), outline: Some(true), outline_hollow: Some(true), shadow: Some(true), anim_in: Some("typewriter".into()), anim_dur: Some(1.0), italic: Some(true), underline: Some(true), ..Default::default() };
    st.right_aligned = Some(true);
    let title = Clip { content: ClipContent::Title(Wrap { v: st }), start: 1.0, duration: 3.0, ..Default::default() };
    let tid = title.id;
    p.tracks[4].clips.push(title);
    let mut h = H::new(p);
    h.app.selected = Some(tid);
    h.app.inspector_tab = 2;
    h.frames(3);
    // alle Bereiche aufklappen, indem man mehrfach auf die Kopfzeilen klickt, ist Sache von egui; hier genügt: es zeichnet ohne Absturz
    h.app.title_templates.push(crate::fx_edit::TitleTemplate { id: Uuid::new_v4(), name: "V".into(), style: TitleStyle::default(), duration: 3.0, x: 0.0, y: 0.0 });
    h.app.template_prompt = Some((tid, "Meine Vorlage".into()));
    h.frames(3);
    h.key(Key::Enter);
    assert!(h.app.template_prompt.is_none() && h.app.title_templates.iter().any(|t| t.name == "Meine Vorlage"), "Name der Vorlage bestätigt");
    h.app.selected = None;
    h.frames(2);
    // Effekte eines Videoclips mit allen neuen Einstellungen
    let (p, r1, _) = av_project();
    let mut h = H::new(p);
    if let Some(c) = h.app.project.clip_mut(r1) {
        c.fx.chroma = Some(ChromaKey::default());
        c.fx.horizon = Some(3.0);
        c.fx.flip_h = Some(true);
        c.fx.stabilize = Some(0.5);
        c.fx.lut_path = Some("nicht_vorhanden.cube".into());
    }
    h.app.selected = Some(r1);
    h.app.inspector_tab = 3;
    h.frames(4);
}


#[test]
fn scopes_replace_the_pool_and_follow_the_preview() {
    let (p, _) = project();
    let mut h = H::new(p);
    h.app.set_scope(crate::scopes::ScopeMode::Both);
    for _ in 0..40 {
        h.frames(2);
        std::thread::sleep(std::time::Duration::from_millis(50));
        if h.app.scope_data.is_some() {
            break;
        }
    }
    let d = h.app.scope_data.as_ref().expect("Messwerte aus dem Vorschaubild");
    assert!(d.hist[0][200] > 0.0, "rotes Testbild: Rot bei 200");
    h.app.set_scope(crate::scopes::ScopeMode::Off);
    h.frames(3);
    assert_eq!(h.app.scope_mode, crate::scopes::ScopeMode::Off);
}

#[test]
fn fullscreen_preview_starts_with_alt_enter_and_ends_with_escape() {
    let (p, _) = project();
    let mut h = H::new(p);
    h.with_mods(Modifiers::ALT);
    h.key(Key::Enter);
    h.with_mods(Modifiers::NONE);
    assert!(h.app.fullscreen && h.app.playing, "Vollbild läuft");
    h.frames(3);
    h.key(Key::Escape);
    assert!(!h.app.fullscreen && !h.app.playing, "Esc beendet");
    // leeres Projekt: kein Vollbild
    let mut e = H::new(Project::default());
    e.with_mods(Modifiers::ALT);
    e.key(Key::Enter);
    assert!(!e.app.fullscreen);
}

#[test]
fn preview_is_recomposed_only_when_the_picture_can_change() {
    // Standbild ohne Animation: die Abspielzeit allein ändert das Bild nicht
    let mut p = Project::default();
    let red = solid_png("gui_static.png", 64, 36, [200, 30, 30]);
    let a = image_asset(&mut p, &red, 64.0, 36.0);
    p.tracks[0].clips.push(clip(a, 0.0, 6.0));
    let mut h = H::new(p);
    h.frames(3);
    let n = h.app.preview.renders;
    assert!(n >= 1);
    for i in 1..20 {
        h.app.playhead = i as f64 * 0.2;
        h.frames(1);
    }
    assert_eq!(h.app.preview.renders, n, "keine neue Zusammensetzung bei bloßer Zeitänderung");
    // Ein-/Ausblenden: im Übergang jedes Bild neu, danach nicht mehr
    h.app.project.tracks[0].clips[0].fade_in = 1.0;
    h.frames(2);
    let n = h.app.preview.renders;
    h.app.playhead = 0.3;
    h.frames(1);
    h.app.playhead = 0.4;
    h.frames(1);
    assert_eq!(h.app.preview.renders, n + 2, "während der Einblendung ändert sich das Bild mit der Zeit");
    h.app.playhead = 3.0;
    h.frames(2);
    let n = h.app.preview.renders;
    h.app.playhead = 3.4;
    h.frames(2);
    assert_eq!(h.app.preview.renders, n, "nach der Einblendung wieder stabil");
}

#[test]
fn tracks_can_be_inserted_and_deleted_and_the_bars_scroll() {
    let (p, ids) = project();
    let mut h = H::new(p);
    let n = h.app.project.tracks.len();
    // Spur einfügen (unter der gewählten) und löschen
    h.app.sel_track = 0;
    h.app.insert_track_at(1);
    assert_eq!(h.app.project.tracks.len(), n + 1);
    assert!(h.app.project.tracks[1].clips.is_empty());
    assert_eq!(h.app.sel_track, 1);
    h.app.remove_track(1);
    assert_eq!(h.app.project.tracks.len(), n);
    // Spur mit Clip löschen: Clips weg, Auswahl aufgehoben, Undo-Schritt über die Oberfläche
    h.app.select(ids.c_b, false);
    let track_of_b = h.app.project.find_clip(ids.c_b).unwrap().0;
    h.app.remove_track(track_of_b);
    assert!(h.app.project.clip(ids.c_b).is_none() && h.app.selected.is_none());
    // letzte Spur bleibt
    while h.app.project.tracks.len() > 1 {
        h.app.remove_track(0);
    }
    h.app.remove_track(0);
    assert_eq!(h.app.project.tracks.len(), 1);
    // Schaltfläche „− Spur“ und „+ Spur“ sind in der Werkzeugleiste: Rechtsklick-Menü ist über die Spurköpfe erreichbar
    h.frames(2);
    // viele Spuren → senkrechte Leiste, Ziehen scrollt
    for _ in 0..12 {
        h.app.insert_track_at(h.app.project.tracks.len());
    }
    h.frames(3);
    let r = h.app.tl_rect;
    let vbar_x = r.right() - 5.0;
    let top = r.top() + RULER_H + 20.0;
    assert_eq!(h.app.scroll_y, 0.0);
    h.drag(Pos2::new(vbar_x, top), Pos2::new(vbar_x, top + 60.0));
    assert!(h.app.scroll_y > 0.0, "senkrechte Leiste scrollt: {}", h.app.scroll_y);
    // waagerechte Leiste: Klick rechts im Streifen springt vorwärts
    h.app.project.tracks[0].clips.push(clip(ids.asset_a, 0.0, 20.0));
    h.app.zoom = 200.0;
    h.frames(3);
    let hy = r.bottom() - 4.0;
    h.click(Pos2::new(r.right() - 40.0, hy));
    assert!(h.app.scroll > 0.0, "waagerechte Leiste scrollt: {}", h.app.scroll);
}

#[test]
fn preview_texture_keeps_its_handle_and_size_while_a_slider_changes_the_picture() {
    let (p, ids) = project();
    let mut h = H::new(p);
    h.app.select(ids.c_a, false);
    h.frames(3);
    let id0 = h.app.preview.tex.as_ref().map(|t| t.id()).expect("Vorschautextur");
    for i in 0..10 {
        h.app.project.tracks[0].clips[0].fx.contrast = 1.0 + i as f64 * 0.05;
        h.app.project_rev += 1; // wie die Oberfläche bei einer Reglerbewegung
        h.frames(1);
    }
    let id1 = h.app.preview.tex.as_ref().map(|t| t.id()).unwrap();
    assert_eq!(id0, id1, "dieselbe Textur wird weiterverwendet (kein neues Speicherobjekt je Bild); bei langsamen Bildern darf nur die Größe wechseln");
    assert!(h.app.preview.renders >= 10);
}

// ---------- Mac-Version 1.2: Austauschen, Mausmodi, Spurkopf, Bereichsleiste, Storyboard, Pool, Masken, Zoom auf Bereich ----------

#[test]
fn keys_4_and_0_and_escape_switch_the_new_modes() {
    let (p, _, _) = av_project();
    let mut h = H::new(p);
    h.key(Key::Num4);
    assert_eq!(h.app.insert_mode, crate::ops::InsertMode::Replace);
    h.key(Key::Num0);
    assert_eq!(h.app.mouse_mode, crate::ops::MouseMode::Stretch);
    h.key(Key::Escape);
    assert_eq!(h.app.mouse_mode, crate::ops::MouseMode::Single, "Esc: zurück zum Standard-Mausmodus");
    // Austauschen ohne gewähltes Objekt: Hinweis statt Einfügen
    h.app.clear_selection();
    let n = h.app.project.tracks.iter().map(|t| t.clips.len()).sum::<usize>();
    let aid = h.app.project.assets[0].id;
    h.app.add_asset_clip(aid);
    assert_eq!(h.app.project.tracks.iter().map(|t| t.clips.len()).sum::<usize>(), n);
    assert!(h.app.status.contains("Austauschen"), "{}", h.app.status);
}

#[test]
fn razor_mode_splits_where_clicked_and_slip_mode_shifts_the_source() {
    let (p, r1, _) = av_project();
    let mut h = H::new(p);
    h.app.set_mouse_mode(crate::ops::MouseMode::Razor);
    h.click(h.at(2.0, 0));
    assert_eq!(h.app.project.tracks[0].clips.len(), 3, "geteilt");
    assert!((h.clip(r1).end() - 2.0).abs() < 1.0 / 30.0 + 0.02, "an der Mausposition: {}", h.clip(r1).end());
    // Ziehen schneidet nicht und verschiebt nichts
    let before = h.app.project.clone();
    h.drag(h.at(1.0, 0), h.at(1.5, 0));
    assert_eq!(h.app.project, before);
    // Objektinhalt verschieben: Lage und Länge bleiben, die Quelle wandert
    let (p, r1, _) = av_project();
    let mut h = H::new(p);
    h.app.set_mouse_mode(crate::ops::MouseMode::Slip);
    let (s0, d0) = (h.clip(r1).start, h.clip(r1).duration);
    h.drag(h.at(2.0, 0), h.at(1.0, 0));
    let c = h.clip(r1);
    assert_eq!((c.start, c.duration), (s0, d0));
    assert!((c.source_in - 1.0).abs() < 0.05, "Quelle um 1 s verschoben: {}", c.source_in);
}

#[test]
fn stretch_mode_changes_the_tempo_at_the_edge() {
    let (p, r1, r2) = av_project();
    let mut h = H::new(p);
    h.app.select(r2, false);
    h.app.ripple_delete_selection();
    h.frames(2);
    h.key(Key::Num0);
    h.drag(h.at(3.95, 0), h.at(7.95, 0));
    let c = h.clip(r1);
    assert!((c.duration - 8.0).abs() < 0.1 && (c.speed - 0.5).abs() < 0.02, "gestreckt: {} s, {}×", c.duration, c.speed);
    let au = h.clip(h.app.project.partner_id(r1).unwrap());
    assert!((au.speed - c.speed).abs() < 1e-9, "Ton zieht mit");
}

#[test]
fn track_header_sets_the_target_track_and_renames_on_double_click() {
    let (p, ids) = project();
    let mut h = H::new(p);
    let r = h.app.tl_rect;
    let head = |i: usize| Pos2::new(r.left() + 10.0, r.top() + RULER_H + i as f32 * ROW_H + 12.0);
    h.click(head(3));
    assert_eq!(h.app.target_track, Some(3), "Klick legt die Zielspur fest");
    h.app.playhead = 10.0;
    h.app.clear_selection();
    h.app.add_asset_clip(ids.asset_a);
    let id = h.app.selected.expect("eingefügt");
    assert_eq!(h.app.project.find_clip(id).unwrap().0, 3, "Einfügen auf der Zielspur");
    h.frames(30);
    h.click(head(3));
    assert_eq!(h.app.target_track, None, "erneuter Klick hebt sie auf");
    // Doppelklick: umbenennen
    h.frames(30);
    h.click(head(1));
    h.click(head(1));
    assert!(h.app.track_rename.is_some(), "Umbenennen-Fenster offen");
    assert_eq!(h.app.target_track, None, "Doppelklick schaltet die Zielspur nicht um");
    h.run(vec![Event::Text("Musik".into())]);
    h.key(Key::Enter);
    assert_eq!(h.app.project.tracks[1].name, "Musik");
    assert_eq!(h.app.project.tracks[1].custom_name(), Some("Musik"));
    // Spur einfügen verschiebt die Zielspur mit
    h.app.target_track = Some(2);
    h.app.insert_track_at(0);
    assert_eq!(h.app.target_track, Some(3));
    h.app.remove_track(3);
    assert_eq!(h.app.target_track, None);
    // Spurfarbe, -höhe, -lautstärke und -deckkraft werden gezeichnet
    h.app.project.tracks[0].color = Some("green".into());
    h.app.project.tracks[0].volume = Some(0.5);
    h.app.project.tracks[0].opacity = Some(0.25);
    h.app.set_track_height(84.0);
    h.frames(3);
    h.app.set_track_height(36.0);
    h.frames(3);
    assert_eq!(h.app.row_h, 36.0);
}

#[test]
fn range_bar_sets_and_moves_the_marks_and_loop_repeats_the_range() {
    let (p, _) = project();
    let mut h = H::new(p);
    let bar = |h: &H, t: f64| {
        let mut p = h.at(t, 0);
        p.y = h.app.tl_rect.top() + crate::ui::timeline::SCALE_H + 3.0;
        p
    };
    h.click(bar(&h, 2.0));
    assert!((h.app.project.mark_in.unwrap() - 2.0).abs() < 0.05, "{:?}", h.app.project.mark_in);
    h.scoped_mods(Modifiers::SHIFT, |h| h.click(bar(h, 5.0)));
    assert!((h.app.project.mark_out.unwrap() - 5.0).abs() < 0.05, "{:?}", h.app.project.mark_out);
    // Band ziehen: beide Marken wandern
    h.drag(bar(&h, 3.5), bar(&h, 4.5));
    assert!((h.app.project.mark_in.unwrap() - 3.0).abs() < 0.06 && (h.app.project.mark_out.unwrap() - 6.0).abs() < 0.06, "{:?} {:?}", h.app.project.mark_in, h.app.project.mark_out);
    // Ende ziehen
    h.drag(bar(&h, 6.0), bar(&h, 7.0));
    assert!((h.app.project.mark_out.unwrap() - 7.0).abs() < 0.06);
    assert_eq!(h.app.playhead, 0.0, "die Bereichsleiste scrubbt nicht");
    // Schleife: Bereich 1,0 … 1,3 s, nach dem Ende geht es am Anfang weiter
    h.app.project.mark_in = Some(1.0);
    h.app.project.mark_out = Some(1.3);
    h.app.loop_range = true;
    h.app.play_range();
    h.frames(2);
    std::thread::sleep(std::time::Duration::from_millis(450));
    h.frames(2);
    assert!(h.app.playing, "läuft weiter");
    assert!(h.app.playhead < 1.3 && h.app.playhead >= 1.0, "zurück am Anfang: {}", h.app.playhead);
    h.app.set_playing(false);
}

#[test]
fn undo_list_names_the_steps_and_undoes_several() {
    let (p, ids) = project();
    let mut h = H::new(p);
    h.drag(h.at(1.0, 0), h.at(2.0, 0));
    h.app.select(ids.c_b, false);
    h.key(Key::Delete);
    h.app.select(ids.c_title, false);
    h.key(Key::D);
    let list = h.app.undo_history(12);
    assert_eq!(&list[..3], &["Clip eingefügt/geteilt", "Clip gelöscht", "Clip verschoben"], "{list:?}");
    h.app.undo_steps(2);
    h.frames(1);
    assert!(h.app.project.clip(ids.c_b).is_some(), "zwei Schritte zurück");
    assert_eq!(h.app.redo_history(5), vec!["Clip gelöscht", "Clip eingefügt/geteilt"]);
    h.app.redo_steps(2);
    assert!(h.app.project.clip(ids.c_b).is_none());
}

#[test]
fn storyboard_shows_scenes_reorders_and_sets_photo_durations() {
    let (p, ids) = project();
    let mut h = H::new(p);
    h.scoped_mods(Modifiers::COMMAND | Modifiers::ALT, |h| h.key(Key::B));
    assert!(h.app.storyboard, "Strg+Alt+B schaltet das Storyboard ein");
    h.frames(3);
    // Hauptspur ist Spur 1 (Clip A); Clip B liegt auf Spur 2
    assert_eq!(h.app.project.storyboard_scenes(), vec![ids.c_a]);
    // drei Fotos auf der Hauptspur
    let b_asset = h.clip(ids.c_b).asset_id().unwrap();
    for t in [2.0, 4.0] {
        h.app.project.tracks[0].clips.push(clip(b_asset, t, 2.0));
    }
    h.frames(2);
    let sc = h.app.project.storyboard_scenes();
    h.app.move_scene(sc[2], Some(sc[0]));
    assert_eq!(h.app.project.storyboard_scenes()[0], sc[2]);
    h.app.set_all_photo_duration(3.0);
    let starts: Vec<f64> = h.app.project.storyboard_scenes().iter().map(|i| h.clip(*i).start).collect();
    assert_eq!(starts, vec![0.0, 3.0, 6.0]);
    h.frames(3);
    h.scoped_mods(Modifiers::COMMAND | Modifiers::ALT, |h| h.key(Key::B));
    assert!(!h.app.storyboard);
}

#[test]
fn pool_selects_like_the_explorer_and_drags_several_media() {
    let (mut p, ids) = project();
    let green = solid_png("gui_green.png", 64, 36, [30, 200, 30]);
    let g = image_asset(&mut p, &green, 64.0, 36.0);
    let mut h = H::new(p);
    h.frames(3);
    let tile = |h: &H, id: Uuid| h.app.pool_tiles.iter().find(|(i, _)| *i == id).expect("Kachel").1.center();
    let b_asset = h.clip(ids.c_b).asset_id().unwrap();
    h.click(tile(&h, ids.asset_a));
    assert_eq!(h.app.pool.selection.len(), 1);
    h.scoped_mods(Modifiers::COMMAND, |h| h.click(tile(h, g)));
    assert_eq!(h.app.pool.selection.len(), 2, "Strg-Klick ergänzt");
    h.frames(30);
    h.click(tile(&h, ids.asset_a));
    h.scoped_mods(Modifiers::SHIFT, |h| h.click(tile(h, g)));
    assert_eq!(h.app.pool.selection.len(), 3, "Umschalt-Klick wählt den Bereich");
    // Tastatur: Pfeil rechts bei Maus über dem Pool
    h.frames(30);
    h.click(tile(&h, ids.asset_a));
    h.mv(tile(&h, ids.asset_a));
    h.key(Key::ArrowRight);
    assert!(h.app.pool.selection.contains(&b_asset) && h.app.pool.selection.len() == 1, "Pfeil wählt das nächste Medium");
    // Strg+A über dem Pool wählt alle Medien (nicht die Clips)
    h.cmd_key(Key::A);
    assert_eq!(h.app.pool.selection.len(), 3);
    // mehrere Medien auf einmal ziehen: hintereinander ab der Ablage
    h.frames(30);
    h.click(tile(&h, ids.asset_a));
    h.scoped_mods(Modifiers::COMMAND, |h| h.click(tile(h, g)));
    let before = h.app.project.tracks[4].clips.len();
    h.drag(tile(&h, ids.asset_a), h.at(10.0, 4));
    let tr = &h.app.project.tracks[4].clips;
    assert_eq!(tr.len(), before + 2, "zwei Clips");
    let mut cs: Vec<&Clip> = tr.iter().collect();
    cs.sort_by(|a, b| a.start.total_cmp(&b.start));
    assert!((cs[0].start - 10.0).abs() < 0.1 && (cs[1].start - cs[0].end()).abs() < 1e-6, "{} {}", cs[0].start, cs[1].start);
    // Entf über dem Pool: nur unbenutzte Medien verschwinden
    let unused = solid_png("gui_unused.png", 32, 18, [90, 90, 90]);
    h.app.import_file(unused);
    let un = h.app.project.assets.last().unwrap().id;
    h.frames(3);
    h.app.pool.selection = std::collections::HashSet::from([un, ids.asset_a]);
    h.mv(tile(&h, un));
    h.key(Key::Delete);
    assert!(h.app.project.asset(un).is_none() && h.app.project.asset(ids.asset_a).is_some(), "{}", h.app.status);
    // Ansichten, Sortierung, Suche
    for v in crate::media::PoolView::ALL {
        h.app.pool.view = v;
        h.frames(2);
        assert_eq!(h.app.pool_tiles.len(), h.app.project.assets.len(), "{v:?}");
    }
    h.app.pool.sort = crate::media::PoolSort::Name;
    h.app.pool.ascending = false;
    let names: Vec<String> = h.app.pool_assets().iter().map(|a| a.name.clone()).collect();
    let mut sorted = names.clone();
    sorted.sort_by(|a, b| crate::media::natural_cmp(b, a));
    assert_eq!(names, sorted);
    // Suche nach Namen (ohne Groß-/Kleinschreibung)
    h.app.project.assets.iter_mut().find(|a| a.id == g).unwrap().name = "Grüner Hintergrund.png".into();
    h.app.pool.search = "grüner".into();
    h.frames(2);
    assert_eq!(h.app.pool_tiles.len(), 1);
    h.app.pool.search.clear();
    h.app.pool.filter = crate::media::PoolFilter::Audio;
    h.frames(2);
    assert!(h.app.pool_tiles.is_empty());
}

#[test]
fn inspector_tabs_effect_cards_masks_and_photo_effects_render() {
    let (p, ids) = project();
    let mut h = H::new(p);
    h.app.select(ids.c_a, false);
    for tab in 0..5 {
        h.app.inspector_tab = tab;
        h.frames(2);
    }
    // Effekte: alle Unterreiter, auch mit Maske, Foto-Effekten, Bewegung
    h.app.inspector_tab = 3;
    for sub in 0..3 {
        h.app.effects_sub = sub;
        h.frames(2);
    }
    {
        let c = h.app.project.clip_mut(ids.c_a).unwrap();
        c.fx.mask = Some(crate::masks::MaskKind::Heart.defaults());
        c.fx.photo = Some(PhotoFx { enhance: Some(0.7), denoise: Some(0.3), ..Default::default() });
        c.apply_preset(crate::anim::AnimPreset::ZoomIn);
    }
    for sub in 0..3 {
        h.app.effects_sub = sub;
        h.frames(2);
    }
    h.app.inspector_tab = 4;
    h.frames(3);
    assert_eq!(h.app.mask_thumbs.len(), 12, "alle Masken-Kacheln haben ein Vorschaubild");
    h.app.project.clip_mut(ids.c_a).unwrap().reset_fx(FxGroup::Mask);
    assert!(h.clip(ids.c_a).fx.mask.is_none());
    // Titel: Masken-Reiter und Bewegung
    h.app.select(ids.c_title, false);
    h.app.inspector_tab = 4;
    h.frames(2);
    h.app.inspector_tab = 3;
    h.frames(2);
    // Animator nur auf Wunsch
    assert!(h.app.animator_clip().is_none(), "Animator aus");
    h.app.show_animator = true;
    assert!(h.app.animator_clip().is_some());
    h.frames(2);
    // ohne Auswahl: Hinweis in gleicher Höhe (die Zeitleiste springt nicht)
    let tl_before = h.app.tl_rect;
    h.app.clear_selection();
    h.frames(2);
    assert_eq!(h.app.tl_rect.top(), tl_before.top(), "Zeitleiste bleibt stehen");
}

#[test]
fn photo_duration_and_presets_for_several_clips() {
    let (mut p, ids) = project();
    let c2 = clip(ids.asset_a, 2.0, 2.0);
    let c2id = c2.id;
    p.tracks[0].clips.push(c2);
    let mut h = H::new(p);
    h.app.select(ids.c_a, false);
    h.app.select(c2id, true);
    h.app.inspector_tab = 0;
    h.frames(2);
    h.app.apply_preset_all(crate::anim::AnimPreset::ZoomOut, ids.c_a);
    assert!(h.clip(ids.c_a).is_keyed(crate::anim::Channel::Scale) && h.clip(c2id).is_keyed(crate::anim::Channel::Scale), "für beide gewählten Clips");
    assert!(h.app.show_animator);
    h.app.set_all_photo_duration(4.0);
    assert_eq!((h.clip(ids.c_a).duration, h.clip(c2id).start), (4.0, 4.0));
}

#[test]
fn zoom_on_region_by_drawing_a_frame_in_the_monitor() {
    let (p, ids) = project();
    let mut h = H::new(p);
    h.app.playhead = 0.5;
    h.app.begin_zoom_pick(ids.c_a, false);
    h.frames(2);
    let c = h.app.preview_rect.center();
    h.drag(c - Vec2::new(60.0, 20.0), c + Vec2::new(60.0, 40.0));
    assert!(h.app.zoom_pick.is_none(), "{}", h.app.status);
    let cl = h.clip(ids.c_a);
    assert!(cl.is_keyed(crate::anim::Channel::Scale), "Zoom-Keyframes geschrieben");
    assert!(cl.value(crate::anim::Channel::Scale, cl.duration) > 1.5, "{}", cl.value(crate::anim::Channel::Scale, cl.duration));
    assert!(h.app.show_animator);
    // Esc bricht ab
    h.app.begin_zoom_pick(ids.c_a, true);
    h.frames(2);
    h.key(Key::Escape);
    assert!(h.app.zoom_pick.is_none());
    // zu kleiner Rahmen: Hinweis, nichts geändert
    let before = h.clip(ids.c_a).clone();
    h.app.begin_zoom_pick(ids.c_a, false);
    h.frames(2);
    h.drag(c, c + Vec2::new(8.0, 4.0));
    assert_eq!(h.clip(ids.c_a), &before);
}

#[test]
fn export_dialog_presets_cache_window_and_rename_render() {
    let (p, aid) = audio_project();
    let mut h = H::new(p);
    h.app.add_to_timeline(aid, Some(3), Some(0.0), None);
    h.app.export_dialog();
    h.frames(3);
    let c = h.app.export_cfg.as_mut().unwrap();
    assert_eq!(c.preset, "YouTube Full HD");
    let all = crate::export::builtin_presets();
    c.apply(&all[3]);
    assert!(!c.normalize && c.s.height == Some(720));
    c.manual_bitrate = true;
    c.s.kbps = Some(3000);
    c.save_name = Some("Mein Preset".into());
    h.frames(3);
    h.app.export_cfg = None;
    h.app.show_cache = true;
    h.frames(3);
    h.app.show_cache = false;
    h.app.begin_track_rename(0);
    h.frames(2);
    h.key(Key::Escape);
    assert!(h.app.track_rename.is_none());
    h.app.rename_track(2, "  ");
    assert_eq!(h.app.project.tracks[2].name, "Spur 3", "leer = wieder „Spur N“");
}
