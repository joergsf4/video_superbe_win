use crate::export;
use crate::model::*;
use crate::player::Player;
use crate::video;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use eframe::egui::{
    self, Align2, Color32, FontId, Key, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle, Vec2,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;
use uuid::Uuid;

const IMAGE_SECONDS: f64 = 5.0;

pub(crate) struct ExportJob {
    pub(crate) progress: Arc<AtomicU32>,
    pub(crate) cancel: Arc<AtomicBool>,
    pub(crate) result: Arc<Mutex<Option<Result<(), String>>>>,
    pub(crate) path: PathBuf,
}

/// Einstellungen im Export-Fenster (vor dem Speichern-Dialog).
pub(crate) struct ExportCfg {
    pub(crate) quality: usize,
    pub(crate) use_range: bool,
    pub(crate) normalize: bool,
    pub(crate) target: f64,
    pub(crate) measured: Option<Result<crate::loudness::Loudness, String>>,
    pub(crate) measuring: Option<(std::sync::mpsc::Receiver<Option<crate::loudness::Loudness>>, Arc<AtomicU32>)>,
}

pub struct App {
    pub(crate) feat: crate::features::FeatState,
    pub(crate) export_cfg: Option<ExportCfg>,
    pub(crate) player: Option<Player>,
    pub(crate) export_job: Option<ExportJob>,
    pub(crate) project: Project,
    pub(crate) saved: Project,
    pub(crate) path: Option<PathBuf>,
    pub(crate) undo: Vec<Project>,
    pub(crate) redo: Vec<Project>,
    pub(crate) gesture_open: bool,
    /// Undo/Redo haben das Projekt in diesem Bild selbst verändert: nicht als neue Bearbeitung zählen.
    pub(crate) history_skip: bool,
    pub(crate) play_anchor: Option<(Instant, f64)>,
    pub(crate) play_wait: Instant,
    pub(crate) play_start_pos: f64,
    pub(crate) playhead: f64,
    pub(crate) playing: bool,
    pub(crate) zoom: f64,
    pub(crate) scroll: f64,
    pub(crate) selected: Option<Uuid>,
    /// Zustand der Timeline-Gesten (Verschieben, Kürzen, Auswahlrahmen, Pegellinie …)
    pub(crate) tl: crate::ui::TlState,
    /// Kapitelmarke, die gerade umbenannt wird (Id, Text)
    pub(crate) rename_marker: Option<(Uuid, String)>,
    /// Quellmonitor (Medium ansehen, Bereich wählen) und seine Tonausgabe
    pub(crate) source: Option<crate::source::SourceState>,
    pub(crate) source_player: Option<Player>,
    /// Lage des Vorschaubereichs im Fenster (Klick daneben schließt den Quellmonitor)
    pub(crate) preview_rect: Rect,
    /// Entwurf der Filmeinstellungen (Seitenverhältnis-Index, kurze Seite, Bildrate), erst „OK“ übernimmt ihn
    pub(crate) settings_draft: Option<(usize, i64, i64)>,
    /// Wellenformen (Spitzenwerte je Medium) und laufende Berechnungen
    pub(crate) waveforms: HashMap<Uuid, std::sync::Arc<Vec<f32>>>,
    pub(crate) wave_jobs: HashMap<Uuid, std::sync::mpsc::Receiver<Vec<f32>>>,
    /// weitere ausgewählte Clips (Mehrfachauswahl); die Hauptauswahl steht in `selected`
    pub(crate) extra_sel: std::collections::HashSet<Uuid>,
    pub(crate) clipboard: Vec<(Clip, usize)>,
    pub(crate) sel_track: usize,
    pub(crate) preview: crate::ui::PreviewState,
    /// Wird bei jeder Änderung des Projekts erhöht (die Vorschau rechnet dann neu).
    pub(crate) project_rev: u64,
    /// Animator: gewählte Zeile, aufgeklappte Gruppe, gewählter Keyframe, offene Interpolationsauswahl
    pub(crate) anim_row: Option<String>,
    pub(crate) anim_expanded: bool,
    pub(crate) selected_key: Option<Uuid>,
    pub(crate) interp_popup: Option<String>,
    pub(crate) interp_anchor: Pos2,
    pub(crate) anim_geom: crate::ui::AnimGeom,
    /// Objektrahmen im Monitor: laufende Geste und zuletzt gezeichnete Lage (für Tests)
    pub(crate) overlay_drag: Option<crate::ui::OverlayDrag>,
    pub(crate) overlay_geom: Option<(crate::ui::OverlayBox, Rect)>,
    pub(crate) status: String,
    pub(crate) title_cache: String,
    pub(crate) snapping: bool,
    pub(crate) scroll_y: f32,
    pub(crate) inspector_tab: usize,
    pub(crate) thumbs: HashMap<Uuid, Option<TextureHandle>>,
    pub(crate) thumb_jobs: HashMap<Uuid, std::sync::mpsc::Receiver<Option<(usize, usize, Vec<u8>)>>>,
    pub(crate) show_settings: bool,
    pub(crate) view_w: f32,
    /// Zuletzt gezeichnetes Timeline-Rechteck (für Tests und Zoom).
    pub(crate) tl_rect: Rect,
    /// Kacheln des Medienpools der letzten Darstellung (für Tests).
    pub(crate) pool_tiles: Vec<(Uuid, Rect)>,
    /// Fehlgeschlagene Importe (Dateiname, Erklärung); werden in einem Fenster angezeigt.
    pub(crate) import_errors: Vec<(String, String)>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, arg: Option<PathBuf>) -> Self {
        let mut app = Self::with_ctx(&cc.egui_ctx, arg.clone());
        // Sicherung eines nicht gesicherten Projekts anbieten (nur beim Start ohne Projektdatei)
        app.feat.restore_offer = arg.is_none() && app.autosave_path().exists();
        app
    }

    /// Ohne Fenster nutzbar (Tests): richtet Schriften und Stil auf dem übergebenen Kontext ein.
    pub fn with_ctx(ctx: &egui::Context, arg: Option<PathBuf>) -> Self {
        crate::ui::setup(ctx);
        let p0 = Project::default();
        let mut a = App {
            player: None,
            export_job: None,
            export_cfg: None,
            feat: crate::features::FeatState::new(),
            project: p0.clone(),
            saved: p0,
            path: None,
            undo: vec![],
            redo: vec![],
            gesture_open: false,
            history_skip: false,
            play_anchor: None,
            play_wait: Instant::now(),
            play_start_pos: 0.0,
            playhead: 0.0,
            playing: false,
            zoom: 60.0,
            scroll: 0.0,
            selected: None,
            tl: Default::default(),
            rename_marker: None,
            source: None,
            source_player: None,
            preview_rect: Rect::NOTHING,
            settings_draft: None,
            waveforms: HashMap::new(),
            wave_jobs: HashMap::new(),
            extra_sel: Default::default(),
            clipboard: vec![],
            sel_track: 0,
            preview: crate::ui::PreviewState::new(ctx),
            project_rev: 0,
            anim_row: None,
            anim_expanded: false,
            selected_key: None,
            interp_popup: None,
            interp_anchor: Pos2::ZERO,
            anim_geom: Default::default(),
            overlay_drag: None,
            overlay_geom: None,
            status: "Bereit. Medien per Drag & Drop oder über „Importieren“ hinzufügen.".into(),
            title_cache: String::new(),
            snapping: true,
            scroll_y: 0.0,
            inspector_tab: 0,
            thumbs: HashMap::new(),
            thumb_jobs: HashMap::new(),
            show_settings: false,
            view_w: 800.0,
            tl_rect: Rect::NOTHING,
            pool_tiles: vec![],
            import_errors: vec![],
        };
        if let Some(p) = arg {
            a.open_path(p);
        }
        // Testhilfen (Umgebungsvariablen): Auswahl "Spur:Clip" und Inspektor-Reiter.
        if let Ok(v) = std::env::var("VSB_TEST_SELECT") {
            let mut it = v.split(':').filter_map(|x| x.parse::<usize>().ok());
            if let (Some(t), Some(c)) = (it.next(), it.next()) {
                a.selected = a.project.tracks.get(t).and_then(|tr| tr.clips.get(c)).map(|c| c.id);
                a.sel_track = t;
            }
        }
        if let Some(tab) = std::env::var("VSB_TEST_TAB").ok().and_then(|v| v.parse().ok()) {
            a.inspector_tab = tab;
        }
        // Testhilfen für Bildschirmfotos: Quellmonitor (Medienindex), Export-Fenster, Rückfrage
        if let Some(i) = std::env::var("VSB_TEST_SOURCE").ok().and_then(|v| v.parse::<usize>().ok()) {
            if let Some(id) = a.project.assets.get(i).map(|x| x.id) {
                a.load_source(id);
                if let Some(s) = a.source.as_mut() {
                    s.mark_in = s.mark_out * 0.25;
                    s.mark_out *= 0.6;
                    s.position = s.mark_in + 0.5;
                }
            }
        }
        if std::env::var("VSB_TEST_EXPORT").is_ok() {
            a.export_dialog();
            if let Some(c) = a.export_cfg.as_mut() {
                c.normalize = true;
                c.measured = Some(Ok(crate::loudness::Loudness { lufs: -20.0, peak: 0.2 }));
            }
        }
        if std::env::var("VSB_TEST_CONFIRM").is_ok() {
            a.feat.pending = Some(crate::features::Pending::New);
        }
        if std::env::var("VSB_TEST_METER").is_ok() {
            a.feat.meter = [-14.0, -6.0];
            a.feat.meter_peak = [-9.0, -2.0];
        }
        // Testhilfe: Startposition der Abspielposition in Sekunden.
        if let Some(t) = std::env::args().nth(2).and_then(|s| s.parse::<f64>().ok()) {
            a.playhead = t;
        }
        a
    }

    // ---------- Wiedergabe ----------

    pub(crate) fn set_playing(&mut self, on: bool) {
        if on {
            self.pause_source();
            if self.playhead >= self.project.duration() {
                self.playhead = 0.0;
            }
            self.start_player();
        } else {
            self.player = None;
            self.play_anchor = None;
        }
        self.playing = on;
    }

    /// Startet die Tonausgabe neu und merkt sich den Zeitanker der gleichmäßigen Wiedergabeuhr.
    pub(crate) fn start_player(&mut self) {
        self.player = Some(Player::start(self.project.clone(), self.playhead));
        self.play_anchor = None;
        self.play_wait = Instant::now();
        self.play_start_pos = self.playhead;
    }

    /// Gleichmäßige Wiedergabeuhr: läuft nach der Systemuhr und wird sanft an die Audio-Uhr angeglichen
    /// (die Audio-Position selbst springt in Pufferschritten und würde das Bild ruckeln lassen).
    pub(crate) fn tick_playback(&mut self) {
        let now = Instant::now();
        let audio = self.player.as_ref().and_then(|p| p.clock_running().then(|| p.position()).flatten());
        let has_audio = self.project.tracks.iter().flat_map(|t| &t.clips).any(|c| c.is_audio());
        if self.play_anchor.is_none() {
            match audio {
                Some(a) => self.play_anchor = Some((now, a)),
                None if !has_audio || self.play_wait.elapsed().as_secs_f64() > 0.7 => self.play_anchor = Some((now, self.play_start_pos)),
                None => {} // Ton startet noch: Bild bleibt stehen
            }
        }
        let Some((t0, p0)) = self.play_anchor else { return };
        let mut ph = p0 + (now - t0).as_secs_f64();
        if let Some(a) = audio {
            let drift = a - ph;
            if drift.abs() > 0.08 {
                self.play_anchor = Some((now, a));
                ph = a;
            } else {
                self.play_anchor = Some((t0, p0 + drift * 0.02));
            }
        }
        self.playhead = ph;
    }

    // ---------- Export ----------

    /// Öffnet das Export-Fenster (Qualität, Bereich, Lautheit); erst „Exportieren …“ dort fragt nach der Datei.
    pub(crate) fn export_dialog(&mut self) {
        if self.export_job.is_some() || self.project.duration() <= 0.0 {
            return;
        }
        self.export_cfg = Some(ExportCfg {
            quality: 1,
            use_range: self.export_range().is_some(),
            normalize: false,
            target: crate::loudness::DEFAULT_TARGET,
            measured: None,
            measuring: None,
        });
    }

    /// Bereich, der laut Einstellungen exportiert wird.
    fn cfg_range(&self, cfg: &ExportCfg) -> (f64, f64) {
        if cfg.use_range { self.export_range() } else { None }.unwrap_or((0.0, self.project.duration()))
    }

    fn start_measure(&mut self) {
        let Some(cfg) = &self.export_cfg else { return };
        let range = self.cfg_range(cfg);
        let p = self.project.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let pr = Arc::new(AtomicU32::new(0));
        let pr2 = pr.clone();
        std::thread::spawn(move || {
            let r = crate::loudness::measure(&p, range, &mut |f| {
                pr2.store((f * 1000.0) as u32, Ordering::Relaxed);
                true
            });
            let _ = tx.send(r);
        });
        if let Some(c) = &mut self.export_cfg {
            c.measuring = Some((rx, pr));
            c.measured = None;
        }
    }

    /// Fragt nach der Zieldatei und startet den Export im Hintergrund.
    fn start_export(&mut self) {
        let Some(cfg) = self.export_cfg.take() else { return };
        let Some(path) = rfd::FileDialog::new().add_filter("MP4-Video", &["mp4"]).set_file_name("Film.mp4").save_file() else {
            self.export_cfg = Some(cfg);
            return;
        };
        self.set_playing(false);
        let range = self.cfg_range(&cfg);
        let whole = !cfg.use_range || self.export_range().is_none();
        let job = ExportJob {
            progress: Arc::new(AtomicU32::new(0)),
            cancel: Arc::new(AtomicBool::new(false)),
            result: Arc::new(Mutex::new(None)),
            path: path.clone(),
        };
        let known = cfg.measured.clone().and_then(|m| m.ok());
        let (p, pr, ca, re) = (self.project.clone(), job.progress.clone(), job.cancel.clone(), job.result.clone());
        std::thread::spawn(move || {
            let mut opts = export::ExportOpts::from_project(&p);
            opts.video_kbps = (opts.video_kbps as f64 * [0.5, 1.0, 1.8][cfg.quality.min(2)]) as u32;
            if !whole {
                opts.range = Some(range);
            }
            let mut base = 0.0;
            if cfg.normalize && export::has_audio(&p) {
                // fehlt die Messung, wird sie vor dem Export nachgeholt (erste 10 % des Balkens)
                let m = known.or_else(|| {
                    base = 0.1;
                    crate::loudness::measure(&p, range, &mut |f| {
                        pr.store((f * 100.0) as u32, Ordering::Relaxed);
                        !ca.load(Ordering::Relaxed)
                    })
                });
                if let Some(m) = m {
                    opts.audio_gain = crate::loudness::gain_for(m, cfg.target) as f32;
                }
            }
            let r = export::export(&p, &path, &opts, &mut |f| {
                pr.store(((base + f * (1.0 - base)) * 1000.0) as u32, Ordering::Relaxed);
                !ca.load(Ordering::Relaxed)
            });
            *re.lock().unwrap() = Some(r);
        });
        self.export_job = Some(job);
    }

    fn export_cfg_window(&mut self, ctx: &egui::Context) {
        let Some(mut cfg) = self.export_cfg.take() else { return };
        // Messergebnis abholen
        if let Some((rx, _)) = &cfg.measuring {
            if let Ok(r) = rx.try_recv() {
                cfg.measured = Some(r.ok_or_else(|| "Kein Ton gefunden".to_string()));
                cfg.measuring = None;
            }
        }
        let mark_range = self.export_range();
        let has_audio = export::has_audio(&self.project);
        let (mut go, mut close, mut measure) = (false, false, false);
        egui::Window::new("Exportieren")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_min_width(380.0);
                ui.label(format!("{}×{} · {} fps · MP4 (H.264 + AAC)", self.project.render_width, self.project.render_height, self.project.fps));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("Qualität");
                    ui.selectable_value(&mut cfg.quality, 0, "Klein");
                    ui.selectable_value(&mut cfg.quality, 1, "Normal");
                    ui.selectable_value(&mut cfg.quality, 2, "Hoch");
                });
                if let Some((a, b)) = mark_range {
                    let r = ui.checkbox(&mut cfg.use_range, format!("Nur Bereich zwischen den Marken ({:.1} s – {:.1} s)", a, b));
                    if r.changed() {
                        cfg.measured = None;
                    }
                }
                ui.add_space(4.0);
                ui.add_enabled_ui(has_audio, |ui| {
                    ui.checkbox(&mut cfg.normalize, "Lautheit normalisieren");
                    if cfg.normalize {
                        ui.horizontal(|ui| {
                            ui.label("Ziel");
                            ui.add(egui::DragValue::new(&mut cfg.target).range(-30.0..=-6.0).speed(0.1).suffix(" LUFS").fixed_decimals(1));
                            if cfg.measuring.is_none() && ui.button("Messen").clicked() {
                                measure = true;
                            }
                        });
                        if let Some((_, pr)) = &cfg.measuring {
                            ui.add(egui::ProgressBar::new(pr.load(Ordering::Relaxed) as f32 / 1000.0).desired_width(300.0).text("Messen …"));
                        }
                        match &cfg.measured {
                            Some(Ok(m)) => {
                                let g = crate::loudness::gain_for(*m, cfg.target);
                                ui.label(format!("Aktuell {:.1} LUFS, Spitze {:.1} dBFS → Verstärkung {:+.1} dB", m.lufs, 20.0 * m.peak.max(1e-6).log10(), 20.0 * g.log10()));
                            }
                            Some(Err(e)) => {
                                ui.colored_label(Color32::from_rgb(230, 140, 80), e.as_str());
                            }
                            None => {}
                        }
                    }
                });
                if !has_audio {
                    ui.weak("Das Projekt enthält keinen Ton.");
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Exportieren …").clicked() {
                        go = true;
                    }
                    if ui.button("Abbrechen").clicked() {
                        close = true;
                    }
                });
            });
        self.export_cfg = Some(cfg);
        if measure {
            self.start_measure();
        }
        if close {
            self.export_cfg = None;
        } else if go {
            self.start_export();
        }
        if self.export_cfg.as_ref().is_some_and(|c| c.measuring.is_some()) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    pub(crate) fn export_window(&mut self, ctx: &egui::Context) {
        self.export_cfg_window(ctx);
        let Some(job) = &self.export_job else { return };
        let done = job.result.lock().unwrap().clone();
        if let Some(r) = done {
            self.status = match r {
                Ok(()) => format!("Export fertig: {}", job.path.display()),
                Err(e) => format!("Export: {e}"),
            };
            self.export_job = None;
            return;
        }
        let frac = job.progress.load(Ordering::Relaxed) as f32 / 1000.0;
        let mut cancel = false;
        egui::Window::new("Exportieren")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(job.path.display().to_string());
                ui.add(egui::ProgressBar::new(frac).show_percentage().desired_width(360.0));
                if ui.button("Abbrechen").clicked() {
                    cancel = true;
                }
            });
        if cancel {
            job.cancel.store(true, Ordering::Relaxed);
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }

    // ---------- Datei ----------

    pub(crate) fn new_project_now(&mut self) {
        self.discard_autosave();
        self.project = Project::default();
        self.saved = self.project.clone();
        self.path = None;
        self.undo.clear();
        self.redo.clear();
        self.preview.reset();
        self.project_rev += 1;
        self.selected = None;
        self.playhead = 0.0;
    }

    pub(crate) fn open_path(&mut self, p: PathBuf) {
        match load(&p) {
            Ok(pr) => {
                self.project = pr;
                self.saved = self.project.clone();
                self.path = Some(p.clone());
                self.undo.clear();
                self.redo.clear();
                self.preview.reset();
        self.project_rev += 1;
                self.selected = None;
                self.playhead = 0.0;
                self.status = format!("Geöffnet: {}", p.display());
                let missing = self.missing_media();
                if missing > 0 {
                    self.status = format!("{missing} Medium/Medien fehlen: Bearbeiten ▸ Fehlende Medien suchen …");
                }
            }
            Err(e) => self.status = format!("Öffnen fehlgeschlagen: {e}"),
        }
    }

    pub(crate) fn save_as(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Video-superbe-Projekt", &["json"])
            .set_file_name("Projekt.mvd.json")
            .save_file()
        {
            self.path = Some(p);
            self.save();
        }
    }

    pub(crate) fn save(&mut self) {
        let Some(p) = self.path.clone() else { return self.save_as() };
        match save(&self.project, &p) {
            Ok(()) => {
                self.saved = self.project.clone();
                self.discard_autosave();
                self.status = format!("Gespeichert: {}", p.display());
            }
            Err(e) => self.status = format!("Speichern fehlgeschlagen: {e}"),
        }
    }

    /// Nimmt ein Medium in den Pool auf: Duplikate werden übersprungen, das erste Video bestimmt Format und Bildrate
    /// (iPhone-Hochkant → 9:16), HDR-Material wird gemeldet.
    pub(crate) fn register_asset(&mut self, a: MediaAsset) {
        if self.project.assets.iter().any(|x| x.path == a.path) {
            self.status = "Schon im Medienpool".into();
            return;
        }
        let first = a.has_video && self.project.assets.is_empty() && self.project.tracks.iter().all(|t| t.clips.is_empty());
        if first {
            let h: i64 = if a.width.min(a.height) >= 2160.0 { 2160 } else { 1080 };
            let long = (h * 16 / 9) & !1;
            let (w, hh) = if a.height > a.width { (h, long) } else { (long, h) };
            self.project.render_width = w;
            self.project.render_height = hh;
            if !a.is_image && a.fps > 0.0 {
                if let Some(f) = [24i64, 25, 30, 50, 60].into_iter().min_by(|x, y| ((*x as f64) - a.fps).abs().total_cmp(&((*y as f64) - a.fps).abs())) {
                    self.project.fps = f;
                }
            }
        }
        if a.is_hdr == Some(true) {
            self.status = format!("HDR-Video erkannt: {} wird automatisch nach SDR umgerechnet", a.name);
        }
        self.project.assets.push(a);
    }

    pub(crate) fn import_failed(&mut self, p: &std::path::Path, why: String) {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string());
        self.import_errors.push((name, why));
    }

    pub(crate) fn import_dialog(&mut self) {
        let mut dlg = rfd::FileDialog::new();
        if let Some(dir) = videos_dir() {
            dlg = dlg.set_directory(dir);
        }
        if let Some(files) = dlg
            .add_filter("Medien", &["png", "jpg", "jpeg", "mp4", "mov", "m4v", "wmv", "avi", "mkv", "wav", "mp3", "m4a", "aac", "wma", "flac"])
            .pick_files()
        {
            for f in files {
                self.import_file(f);
            }
        }
    }

    pub(crate) fn import_file(&mut self, p: PathBuf) {
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if p.to_string_lossy().to_lowercase().ends_with(".mvd.json") || ext == "json" {
            self.open_path_checked(p);
            return;
        }
        if matches!(ext.as_str(), "mp4" | "mov" | "m4v" | "wmv" | "avi" | "mkv") {
            match video::probe(&p) {
                Ok(i) => {
                    let a = MediaAsset {
                        path: p.to_string_lossy().into_owned(),
                        name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                        duration: i.duration,
                        has_video: true,
                        has_audio: i.has_audio,
                        width: i.width as f64,
                        height: i.height as f64,
                        fps: i.fps,
                        is_hdr: i.hdr.then_some(true),
                        ..Default::default()
                    };
                    self.status = format!("Importiert: {} ({}×{}, {:.1} s)", a.name, i.width, i.height, i.duration);
                    self.register_asset(a);
                }
                Err(e) => {
                    self.status = format!("Video nicht lesbar: {e}");
                    self.import_failed(&p, explain_video_error(&e));
                }
            }
            return;
        }
        if matches!(ext.as_str(), "wav" | "mp3" | "m4a" | "aac" | "wma" | "flac") {
            match crate::audio::probe(&p) {
                Ok(d) => {
                    let a = MediaAsset {
                        path: p.to_string_lossy().into_owned(),
                        name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                        duration: d,
                        has_audio: true,
                        ..Default::default()
                    };
                    self.status = format!("Importiert: {} ({:.1} s)", a.name, d);
                    self.register_asset(a);
                }
                Err(e) => {
                    self.status = format!("Ton nicht lesbar: {e}");
                    self.import_failed(&p, format!("Die Tondatei lässt sich nicht lesen ({e})."));
                }
            }
            return;
        }
        if !matches!(ext.as_str(), "png" | "jpg" | "jpeg") {
            self.status = format!("Dateityp .{ext} wird nicht unterstützt.");
            self.import_failed(&p, format!("Dateityp „.{ext}“ wird nicht unterstützt. Möglich sind Videos (mp4, mov, m4v, wmv, avi, mkv), Bilder (png, jpg) und Ton (wav, mp3, m4a, aac, wma, flac)."));
            return;
        }
        match image::image_dimensions(&p) {
            Ok((w, h)) => {
                let a = MediaAsset {
                    path: p.to_string_lossy().into_owned(),
                    name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    duration: IMAGE_SECONDS,
                    has_video: true,
                    width: w as f64,
                    height: h as f64,
                    is_image: true,
                    ..Default::default()
                };
                self.status = format!("Importiert: {}", a.name);
                self.register_asset(a);
            }
            Err(e) => {
                self.status = format!("Bild nicht lesbar: {e}");
                self.import_failed(&p, format!("Das Bild lässt sich nicht lesen ({e})."));
            }
        }
    }

    // ---------- Bearbeiten ----------

    pub(crate) fn do_undo(&mut self) {
        if let Some(p) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.project, p));
            self.gesture_open = false;
            self.history_skip = true;
        }
    }

    pub(crate) fn do_redo(&mut self) {
        if let Some(p) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.project, p));
            self.gesture_open = false;
            self.history_skip = true;
        }
    }
}

impl App {
    /// Wellenform (Spitzenwerte) eines Mediums; wird beim ersten Zugriff im Hintergrund berechnet.
    pub(crate) fn wave(&mut self, ctx: &egui::Context, aid: Uuid) -> Option<std::sync::Arc<Vec<f32>>> {
        if let Some(w) = self.waveforms.get(&aid) {
            return Some(w.clone());
        }
        if let Some(rx) = self.wave_jobs.get(&aid) {
            return match rx.try_recv() {
                Ok(p) => {
                    self.wave_jobs.remove(&aid);
                    let arc = std::sync::Arc::new(p);
                    self.waveforms.insert(aid, arc.clone());
                    Some(arc)
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(_) => {
                    self.wave_jobs.remove(&aid);
                    self.waveforms.insert(aid, std::sync::Arc::new(vec![]));
                    None
                }
            };
        }
        let a = self.project.asset(aid)?;
        if !a.has_audio || self.wave_jobs.len() >= 2 {
            return None;
        }
        let path = PathBuf::from(&a.path);
        let (tx, rx) = std::sync::mpsc::channel();
        let wake = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(crate::audio::peaks(&path));
            wake.request_repaint();
        });
        self.wave_jobs.insert(aid, rx);
        None
    }
}

/// Der Windows-Ordner „Videos“ des Benutzers (auch wenn er umgeleitet ist, z. B. nach OneDrive).
pub(crate) fn videos_dir() -> Option<PathBuf> {
    use windows::Win32::UI::Shell::{FOLDERID_Videos, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    let from_shell = unsafe {
        SHGetKnownFolderPath(&FOLDERID_Videos, KF_FLAG_DEFAULT, None).ok().and_then(|p| {
            let s = p.to_string().ok();
            windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
            s.map(PathBuf::from)
        })
    };
    from_shell
        .or_else(|| std::env::var_os("USERPROFILE").map(|u| PathBuf::from(u).join("Videos")))
        .filter(|p| p.is_dir())
}

/// Verständliche Erklärung für einen Video-Importfehler.
pub(crate) fn explain_video_error(e: &str) -> String {
    if e.contains("(HEVC)") || e.contains("(hvc1)") || e.contains("(hev1)") {
        format!("Dieses H.265/HEVC-Video ließ sich nicht decodieren ({e}). Als Ausweg das Video vorher nach H.264 umwandeln (z. B. mit HandBrake).")
    } else if e.contains("Kein Videostrom") {
        "Die Datei enthält keinen Videostrom.".to_string()
    } else if e.contains("Datei nicht lesbar") {
        format!("Die Datei lässt sich nicht öffnen ({e}).")
    } else {
        format!("Das Videoformat wird nicht unterstützt ({e}).")
    }
}
