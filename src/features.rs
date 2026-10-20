//! Weitere Funktionen der Mac-Version: Taktraster, Ducking, Standbild, Untertitel-Export (SRT), Sprecheraufnahme,
//! Autosave, fehlende Medien neu verknüpfen, Rückfrage bei ungesicherten Änderungen, Pegelanzeige.

use crate::app::App;
use crate::model::*;
use crate::record::Recorder;
use eframe::egui::{self, Align2, Vec2};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Ergebnisse aus Hintergrundthreads.
pub(crate) enum Bg {
    Beats(Uuid, Option<BeatGrid>),
    /// (Musikclip, dB, Sprachintervalle je Tonquelle)
    Voices(Uuid, f64, Vec<(Uuid, Vec<(f64, f64)>)>),
}

/// Was nach der Rückfrage „Änderungen sichern?“ geschehen soll.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Pending {
    New,
    Open(Option<PathBuf>),
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Answer {
    Save,
    Discard,
    Cancel,
}

pub(crate) struct FeatState {
    tx: Sender<Bg>,
    rx: Receiver<Bg>,
    pub(crate) beat_busy: HashSet<Uuid>,
    beat_cache: HashMap<Uuid, BeatGrid>,
    voice_cache: HashMap<Uuid, Vec<(f64, f64)>>,
    pub(crate) duck_busy: bool,
    /// Ordner für Autosave, Aufnahmen und Standbilder (neben der EXE, sonst im Temp-Ordner)
    pub(crate) data_dir: PathBuf,
    last_autosave_check: Instant,
    last_autosaved: Option<Project>,
    pub(crate) pending: Option<Pending>,
    pub(crate) restore_offer: bool,
    pub(crate) allow_close: bool,
    pub(crate) recorder: Option<Recorder>,
    pub(crate) rec_start: f64,
    /// Pegelanzeige: aktuelle Werte und Spitzenwerte in dBFS, Haltezähler
    pub(crate) meter: [f32; 2],
    pub(crate) meter_peak: [f32; 2],
    meter_hold: [u32; 2],
}

/// Datenordner: `VSB_DATA_DIR`, sonst „VideoSuperbe-Daten“ neben der EXE (portabel), sonst Temp.
pub(crate) fn default_data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("VSB_DATA_DIR") {
        return PathBuf::from(d);
    }
    if cfg!(test) {
        return std::env::temp_dir().join("video-superbe-tests").join("data");
    }
    let beside = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.join("VideoSuperbe-Daten")));
    if let Some(d) = beside {
        if std::fs::create_dir_all(&d).is_ok() && std::fs::write(d.join(".w"), b"").is_ok() {
            let _ = std::fs::remove_file(d.join(".w"));
            return d;
        }
    }
    std::env::temp_dir().join("VideoSuperbe-Daten")
}

impl FeatState {
    pub(crate) fn new() -> Self {
        let (tx, rx) = channel();
        FeatState {
            tx,
            rx,
            beat_busy: HashSet::new(),
            beat_cache: HashMap::new(),
            voice_cache: HashMap::new(),
            duck_busy: false,
            data_dir: default_data_dir(),
            last_autosave_check: Instant::now(),
            last_autosaved: None,
            pending: None,
            restore_offer: false,
            allow_close: false,
            recorder: None,
            rec_start: 0.0,
            meter: [-90.0; 2],
            meter_peak: [-90.0; 2],
            meter_hold: [0; 2],
        }
    }
}

/// Untertitel als SRT: alle Titelclips, die als Untertitel markiert sind, nach Startzeit.
pub fn srt_text(p: &Project) -> String {
    let mut subs: Vec<(f64, f64, String)> = p
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .filter_map(|c| match &c.content {
            ClipContent::Title(w) if w.v.subtitle == Some(true) => Some((c.start, c.end(), w.v.text.clone())),
            _ => None,
        })
        .collect();
    subs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let ts = |t: f64| {
        let ms = (t * 1000.0).round() as i64;
        format!("{:02}:{:02}:{:02},{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
    };
    subs.iter().enumerate().map(|(i, (a, b, t))| format!("{}\n{} --> {}\n{}\n", i + 1, ts(*a), ts(*b), t.trim())).collect::<Vec<_>>().join("\n")
}

/// Alle Dateien unterhalb von `dir` nach (kleingeschriebenem) Dateinamen (Tiefe ≤ 10).
pub fn scan_dir(dir: &Path) -> HashMap<String, PathBuf> {
    let mut out = HashMap::new();
    let mut stack = vec![(dir.to_path_buf(), 0)];
    let mut seen = 0usize;
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            seen += 1;
            if seen > 300_000 {
                return out;
            }
            let p = e.path();
            if p.is_dir() {
                if depth < 10 {
                    stack.push((p, depth + 1));
                }
            } else if let Some(n) = p.file_name() {
                out.entry(n.to_string_lossy().to_lowercase()).or_insert(p);
            }
        }
    }
    out
}

/// Setzt fehlende Medienpfade anhand der Dateinamen neu; gibt die Zahl der neu verknüpften Medien zurück.
pub fn relink_in(p: &mut Project, found: &HashMap<String, PathBuf>) -> usize {
    let mut n = 0;
    for a in &mut p.assets {
        if Path::new(&a.path).exists() {
            continue;
        }
        let name = Path::new(&a.path).file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        if let Some(u) = found.get(&name) {
            a.path = u.to_string_lossy().into_owned();
            n += 1;
        }
    }
    n
}

impl App {
    // ---------- Taktraster ----------

    /// Erkennt das Taktraster der Tonquelle des Clips und legt es an allen Tonclips dieser Quelle an.
    pub(crate) fn detect_beats(&mut self, clip_id: Uuid) {
        let Some(c) = self.project.clip(clip_id) else { return };
        let Some(asset) = c.asset_id().and_then(|a| self.project.asset(a)).cloned() else { return };
        if !asset.has_audio || self.feat.beat_busy.contains(&asset.id) {
            return;
        }
        if let Some(g) = self.feat.beat_cache.get(&asset.id).copied() {
            self.apply_beat_grid(asset.id, g);
            return;
        }
        self.feat.beat_busy.insert(asset.id);
        self.status = "Taktraster wird berechnet …".into();
        let (tx, id, path) = (self.feat.tx.clone(), asset.id, PathBuf::from(&asset.path));
        std::thread::spawn(move || {
            crate::video::init();
            let mut x: Vec<f32> = vec![];
            let _ = crate::audio::stream_mono(&path, crate::analysis::BEAT_RATE, 150.0, &mut |b| x.extend_from_slice(b));
            let _ = tx.send(Bg::Beats(id, crate::analysis::detect_grid(&x)));
        });
    }

    fn apply_beat_grid(&mut self, asset: Uuid, g: BeatGrid) {
        for t in &mut self.project.tracks {
            for c in &mut t.clips {
                if c.asset_id() == Some(asset) && c.is_audio() {
                    c.beat_grid = Some(g);
                }
            }
        }
        self.status = format!("Taktraster: {:.1} BPM – Clips rasten jetzt an den Beat-Linien ein", g.bpm());
    }

    pub(crate) fn remove_beats(&mut self, clip_id: Uuid) {
        let Some(a) = self.project.clip(clip_id).and_then(|c| c.asset_id()) else { return };
        for t in &mut self.project.tracks {
            for c in &mut t.clips {
                if c.asset_id() == Some(a) {
                    c.beat_grid = None;
                }
            }
        }
        self.feat.beat_cache.remove(&a);
    }

    fn set_grid_for_asset(&mut self, a: Uuid, g: BeatGrid) {
        self.feat.beat_cache.insert(a, g);
        for t in &mut self.project.tracks {
            for c in &mut t.clips {
                if c.asset_id() == Some(a) && c.beat_grid.is_some() {
                    c.beat_grid = Some(g);
                }
            }
        }
    }

    /// Tempo verdoppeln/halbieren (falls die Erkennung eine Oktave daneben liegt).
    pub(crate) fn scale_beats(&mut self, clip_id: Uuid, factor: f64) {
        let Some(c) = self.project.clip(clip_id) else { return };
        let (Some(mut g), Some(a)) = (c.beat_grid, c.asset_id()) else { return };
        g.period /= factor;
        self.set_grid_for_asset(a, g);
    }

    /// Richtet den Beat so aus, dass er genau an der Abspielposition liegt.
    pub(crate) fn align_beats(&mut self, clip_id: Uuid) {
        let Some(c) = self.project.clip(clip_id) else { return };
        let (Some(mut g), Some(a)) = (c.beat_grid, c.asset_id()) else { return };
        let src = c.source_in + (self.playhead - c.start) * c.speed;
        if src < 0.0 {
            return;
        }
        g.phase = src % g.period;
        self.set_grid_for_asset(a, g);
    }

    // ---------- Ducking ----------

    /// Tonclips (außer der Musik), die sich mit der Musik zeitlich überschneiden und hörbar sind.
    fn voice_clips(&self, music: &Clip) -> Vec<Clip> {
        let music_id = music.id;
        self.project
            .tracks
            .iter()
            .filter(|t| !t.muted)
            .flat_map(|t| &t.clips)
            .filter(|c| c.is_audio() && c.id != music_id && c.end() > music.start && c.start < music.end() && c.asset_id().and_then(|a| self.project.asset(a)).is_some())
            .cloned()
            .collect()
    }

    /// Setzt auf dem Musikclip eine Lautstärkekurve, die bei Sprache anderer Tonclips um `db` absenkt.
    pub(crate) fn apply_ducking(&mut self, music_id: Uuid, db: f64) {
        let Some(music) = self.project.clip(music_id).cloned() else { return };
        if !music.is_audio() || self.feat.duck_busy {
            return;
        }
        let voices = self.voice_clips(&music);
        if voices.is_empty() {
            self.status = "Keine andere Tonspur (Sprache) im Bereich der Musik".into();
            return;
        }
        let todo: Vec<(Uuid, PathBuf)> = voices
            .iter()
            .filter_map(|v| v.asset_id())
            .collect::<HashSet<_>>()
            .into_iter()
            .filter(|a| !self.feat.voice_cache.contains_key(a))
            .filter_map(|a| self.project.asset(a).map(|x| (a, PathBuf::from(&x.path))))
            .collect();
        if todo.is_empty() {
            self.finish_ducking(music_id, db);
            return;
        }
        self.feat.duck_busy = true;
        self.status = "Sprache wird analysiert …".into();
        let tx = self.feat.tx.clone();
        std::thread::spawn(move || {
            crate::video::init();
            let res = todo
                .into_iter()
                .map(|(id, path)| {
                    let mut b = crate::analysis::EnvelopeBuilder::default();
                    let _ = crate::audio::stream_mono(&path, crate::analysis::VOICE_RATE, 36000.0, &mut |x| b.push(x));
                    (id, crate::analysis::voice_intervals(&b.env))
                })
                .collect();
            let _ = tx.send(Bg::Voices(music_id, db, res));
        });
    }

    fn finish_ducking(&mut self, music_id: Uuid, db: f64) {
        let Some(m) = self.project.clip(music_id).cloned() else { return };
        let voices: Vec<(Clip, Vec<(f64, f64)>)> = self
            .voice_clips(&m)
            .into_iter()
            .filter_map(|v| {
                let ivs = self.feat.voice_cache.get(&v.asset_id()?)?.clone();
                Some((v, ivs))
            })
            .collect();
        let speech = crate::analysis::speech_on_timeline(&m, &voices);
        if speech.is_empty() {
            self.status = "Keine Sprache erkannt".into();
            return;
        }
        let pts = crate::analysis::ducking_points(&m, &speech, db);
        if let Some(c) = self.project.clip_mut(music_id) {
            c.volume_points = pts;
        }
        self.status = format!("Abdunkeln eingerichtet: {} Sprachabschnitte, {:.0} dB (Kurve im Clip änderbar)", speech.len(), db);
    }

    /// Alt+L: Musik unter Sprache abdunkeln (−12 dB) für den gewählten Tonclip.
    pub(crate) fn duck_selected(&mut self) {
        let Some(id) = self.selected else {
            self.status = "Einen Tonclip (Musik) wählen".into();
            return;
        };
        let target = match self.project.clip(id) {
            Some(c) if c.is_audio() => Some(id),
            Some(_) => self.project.partner_id(id),
            None => None,
        };
        match target {
            Some(t) => self.apply_ducking(t, -12.0),
            None => self.status = "Einen Tonclip (Musik) wählen".into(),
        }
    }

    pub(crate) fn clear_ducking(&mut self, id: Uuid) {
        if let Some(c) = self.project.clip_mut(id) {
            c.volume_points.clear();
        }
    }

    /// Holt Ergebnisse der Hintergrundanalysen ab.
    pub(crate) fn poll_features(&mut self, ctx: &egui::Context) {
        while let Ok(m) = self.feat.rx.try_recv() {
            match m {
                Bg::Beats(asset, g) => {
                    self.feat.beat_busy.remove(&asset);
                    match g {
                        Some(g) => {
                            self.feat.beat_cache.insert(asset, g);
                            self.apply_beat_grid(asset, g);
                        }
                        None => self.status = "Kein gleichmäßiger Takt gefunden".into(),
                    }
                }
                Bg::Voices(music, db, res) => {
                    self.feat.duck_busy = false;
                    for (a, iv) in res {
                        self.feat.voice_cache.insert(a, iv);
                    }
                    self.finish_ducking(music, db);
                }
            }
        }
        if !self.feat.beat_busy.is_empty() || self.feat.duck_busy {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }

    // ---------- Standbild ----------

    /// Das Quellbild des gewählten Videoclips an der Abspielposition wird als Bildclip (3 s) darüber eingefügt.
    pub(crate) fn freeze_frame(&mut self) {
        let clip = self.selected.and_then(|id| self.project.clip(id)).cloned();
        let Some(clip) = clip.filter(|c| c.part == ClipPart::Video && !self.project.is_still(c) && self.playhead >= c.start && self.playhead <= c.end()) else {
            self.status = "Videoclip wählen und Abspielmarker darüber setzen".into();
            return;
        };
        let Some(asset) = clip.asset_id().and_then(|a| self.project.asset(a)).cloned().filter(|a| a.has_video && !a.is_image) else { return };
        let Some(track) = self.project.tracks.iter().position(|t| t.clips.iter().any(|c| c.id == clip.id)) else { return };
        let src = (clip.source_in + (self.playhead - clip.start) * clip.speed).min((asset.duration - 0.05).max(0.0)).max(0.0);
        let frame = crate::video::Decoder::open(Path::new(&asset.path)).ok().and_then(|mut d| d.frame_at(src).map(|f| (f.width as u32, f.height as u32, f.rgba.clone())));
        let Some((w, h, rgba)) = frame else {
            self.status = "Standbild konnte nicht erzeugt werden".into();
            return;
        };
        let dir = self.feat.data_dir.join("Imported");
        let stem = Path::new(&asset.name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let png = dir.join(format!("Standbild_{}_{}.png", stem, (src * 1000.0) as i64));
        let saved = std::fs::create_dir_all(&dir).is_ok() && image::RgbaImage::from_raw(w, h, rgba).is_some_and(|i| i.save(&png).is_ok());
        if !saved {
            self.status = "Standbild konnte nicht gespeichert werden".into();
            return;
        }
        self.import_file(png.clone());
        let Some(img) = self.project.assets.iter().find(|a| a.path == png.to_string_lossy()).cloned() else { return };
        let start = self.playhead;
        let n = self.project.tracks.len();
        let target = (track + 1..(track + 2).max(n + 1)).find(|&t| t >= n || !self.project.strict_overlaps(start, start + 3.0, t, None)).unwrap_or(n);
        if let Some(id) = self.project.insert_media(&img, start, Some(target), None) {
            if let Some(c) = self.project.clip_mut(id) {
                c.duration = 3.0;
            }
            self.select(id, false);
        }
    }

    /// Aktuelles Vorschaubild in voller Projektauflösung als PNG/JPG speichern.
    pub(crate) fn export_still(&mut self) {
        if self.project.duration() <= 0.0 {
            return;
        }
        let Some(path) = rfd::FileDialog::new().add_filter("PNG", &["png"]).add_filter("JPEG", &["jpg", "jpeg"]).set_file_name("Standbild.png").save_file() else { return };
        self.status = match self.write_still(&path) {
            Ok(()) => format!("Standbild gespeichert: {}", path.display()),
            Err(e) => format!("Standbild: {e}"),
        };
    }

    pub(crate) fn write_still(&self, path: &Path) -> Result<(), String> {
        let (w, h) = ((self.project.render_width.max(16) as usize) & !1, (self.project.render_height.max(16) as usize) & !1);
        let rgba = crate::render::Renderer::new().render(&self.project, self.playhead.min((self.project.duration() - 1e-3).max(0.0)), w, h);
        let img = image::RgbaImage::from_raw(w as u32, h as u32, rgba).ok_or("Bild")?;
        let jpg = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg"));
        if jpg {
            image::DynamicImage::ImageRgba8(img).to_rgb8().save(path).map_err(|e| e.to_string())
        } else {
            img.save(path).map_err(|e| e.to_string())
        }
    }

    // ---------- Untertitel ----------

    pub(crate) fn export_srt(&mut self) {
        let text = srt_text(&self.project);
        if text.is_empty() {
            self.status = "Keine Untertitel im Projekt (Titelclips mit Untertitel-Stil)".into();
            return;
        }
        let Some(path) = rfd::FileDialog::new().add_filter("SubRip-Untertitel", &["srt"]).set_file_name("Untertitel.srt").save_file() else { return };
        self.status = match std::fs::write(&path, text) {
            Ok(()) => format!("Untertitel gespeichert: {}", path.display()),
            Err(e) => format!("Untertitel: {e}"),
        };
    }

    // ---------- Sprecher aufnehmen ----------

    pub(crate) fn toggle_recording(&mut self) {
        if self.feat.recorder.is_some() {
            self.stop_recording();
            return;
        }
        match Recorder::start() {
            Ok(r) => {
                self.feat.recorder = Some(r);
                self.feat.rec_start = self.playhead;
                self.status = "Aufnahme läuft … (Kopfhörer empfohlen)".into();
                if self.project.duration() > 0.0 {
                    self.set_playing(true);
                }
            }
            Err(e) => self.status = e,
        }
    }

    pub(crate) fn stop_recording(&mut self) {
        let Some(r) = self.feat.recorder.take() else { return };
        self.set_playing(false);
        let samples = r.finish();
        if samples.len() < 4800 {
            self.status = "Aufnahme zu kurz".into();
            return;
        }
        let name = format!("Aufnahme_{}.wav", chrono_stamp());
        let path = self.feat.data_dir.join("Aufnahmen").join(name);
        if let Err(e) = crate::record::write_wav(&path, &samples, crate::record::REC_RATE) {
            self.status = format!("Aufnahme konnte nicht gespeichert werden: {e}");
            return;
        }
        self.insert_recording(path, self.feat.rec_start);
    }

    /// Importiert die Aufnahme und legt sie auf die erste Spur, auf der der Platz frei ist.
    pub(crate) fn insert_recording(&mut self, path: PathBuf, start: f64) {
        self.import_file(path.clone());
        let Some(a) = self.project.assets.iter().find(|a| a.path == path.to_string_lossy()).cloned() else {
            self.status = "Aufnahme konnte nicht importiert werden".into();
            return;
        };
        let mut t = 0;
        while t < self.project.tracks.len() && self.project.strict_overlaps(start, start + a.duration, t, None) {
            t += 1;
        }
        if let Some(id) = self.project.insert_media(&a, start, Some(t), None) {
            self.select(id, false);
            self.status = format!("Aufnahme eingefügt ({:.1} s)", a.duration);
        }
    }

    // ---------- Autosave ----------

    pub(crate) fn autosave_path(&self) -> PathBuf {
        self.feat.data_dir.join("Autosave.mvd.json")
    }

    pub(crate) fn is_dirty(&self) -> bool {
        self.project != self.saved
    }

    /// Schreibt eine Sicherung, wenn es ungesicherte Änderungen gibt, die noch nicht gesichert wurden.
    pub(crate) fn autosave_now(&mut self) {
        if !self.is_dirty() || self.feat.last_autosaved.as_ref() == Some(&self.project) {
            return;
        }
        let _ = std::fs::create_dir_all(&self.feat.data_dir);
        if save(&self.project, &self.autosave_path()).is_ok() {
            self.feat.last_autosaved = Some(self.project.clone());
        }
    }

    /// Alle 15 Sekunden aufrufen lassen.
    pub(crate) fn autosave_tick(&mut self) {
        if self.feat.last_autosave_check.elapsed() >= Duration::from_secs(15) {
            self.feat.last_autosave_check = Instant::now();
            self.autosave_now();
        }
    }

    pub(crate) fn discard_autosave(&mut self) {
        let _ = std::fs::remove_file(self.autosave_path());
        self.feat.last_autosaved = None;
    }

    /// Lädt die Sicherung; das Projekt gilt danach als ungesichert.
    pub(crate) fn restore_autosave(&mut self) {
        match load(&self.autosave_path()) {
            Ok(p) => {
                self.new_project_now();
                self.project = p;
                self.saved = Project::default();
                self.status = "Sicherung wiederhergestellt – bitte sichern".into();
            }
            Err(e) => self.status = format!("Sicherung nicht lesbar: {e}"),
        }
    }

    // ---------- Rückfrage bei ungesicherten Änderungen ----------

    pub(crate) fn new_project(&mut self) {
        if self.is_dirty() {
            self.feat.pending = Some(Pending::New);
        } else {
            self.new_project_now();
        }
    }

    pub(crate) fn open_dialog(&mut self) {
        if self.is_dirty() {
            self.feat.pending = Some(Pending::Open(None));
        } else {
            self.open_dialog_now(None);
        }
    }

    /// Öffnet eine Projektdatei (mit Rückfrage bei ungesicherten Änderungen).
    pub(crate) fn open_path_checked(&mut self, p: PathBuf) {
        if self.is_dirty() {
            self.feat.pending = Some(Pending::Open(Some(p)));
        } else {
            self.open_path(p);
        }
    }

    pub(crate) fn open_dialog_now(&mut self, picked: Option<PathBuf>) {
        let p = picked.or_else(|| rfd::FileDialog::new().add_filter("Video-superbe-Projekt", &["json"]).pick_file());
        if let Some(p) = p {
            self.open_path(p);
        }
    }

    /// Antwort auf die Rückfrage; `true`, wenn die angefragte Aktion ausgeführt wurde.
    pub(crate) fn resolve_pending(&mut self, ctx: &egui::Context, a: Answer) -> bool {
        let Some(p) = self.feat.pending.clone() else { return false };
        match a {
            Answer::Cancel => {
                self.feat.pending = None;
                return false;
            }
            Answer::Save => {
                self.save();
                if self.is_dirty() {
                    // Speichern abgebrochen oder fehlgeschlagen: nichts verwerfen
                    self.feat.pending = None;
                    return false;
                }
            }
            Answer::Discard => {}
        }
        self.feat.pending = None;
        match p {
            Pending::New => self.new_project_now(),
            Pending::Open(path) => {
                // Änderungen sind jetzt gesichert oder bewusst verworfen
                self.saved = self.project.clone();
                self.open_dialog_now(path)
            }
            Pending::Quit => {
                self.feat.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        true
    }

    /// Fenster zum Schließen mit ungesicherten Änderungen abfangen.
    pub(crate) fn guard_close(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && self.is_dirty() && !self.feat.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.feat.pending = Some(Pending::Quit);
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.is_dirty() {
            self.discard_autosave();
        }
    }

    pub(crate) fn feature_windows(&mut self, ctx: &egui::Context) {
        if self.feat.pending.is_some() {
            let title = self.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Unbenannt".into());
            let mut ans = None;
            egui::Window::new("Änderungen sichern?").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
                ui.label(format!("Das Projekt „{title}“ hat ungesicherte Änderungen."));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Sichern").clicked() {
                        ans = Some(Answer::Save);
                    }
                    if ui.button("Nicht sichern").clicked() {
                        ans = Some(Answer::Discard);
                    }
                    if ui.button("Abbrechen").clicked() {
                        ans = Some(Answer::Cancel);
                    }
                });
            });
            if let Some(a) = ans {
                self.resolve_pending(ctx, a);
            }
        } else if self.feat.restore_offer {
            let mut ans = None;
            egui::Window::new("Sicherung gefunden").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
                ui.label("Beim letzten Mal wurde das Programm beendet, ohne das Projekt zu sichern.");
                ui.label("Soll die automatische Sicherung wiederhergestellt werden?");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Wiederherstellen").clicked() {
                        ans = Some(true);
                    }
                    if ui.button("Verwerfen").clicked() {
                        ans = Some(false);
                    }
                });
            });
            if let Some(restore) = ans {
                self.feat.restore_offer = false;
                if restore {
                    self.restore_autosave();
                } else {
                    self.discard_autosave();
                }
            }
        }
    }

    // ---------- Fehlende Medien ----------

    pub(crate) fn missing_media(&self) -> usize {
        self.project.assets.iter().filter(|a| !Path::new(&a.path).exists()).count()
    }

    /// Sucht fehlende Medien (nach Dateiname) in einem gewählten Ordner und verknüpft sie neu.
    pub(crate) fn relink_missing(&mut self) {
        let missing = self.missing_media();
        if missing == 0 {
            self.status = "Alle Medien sind vorhanden".into();
            return;
        }
        let Some(dir) = rfd::FileDialog::new().set_title("Ordner wählen, in dem die fehlenden Medien liegen").pick_folder() else { return };
        self.relink_from(&dir);
    }

    pub(crate) fn relink_from(&mut self, dir: &Path) {
        let missing = self.missing_media();
        let found = scan_dir(dir);
        let n = relink_in(&mut self.project, &found);
        self.thumbs.clear();
        self.thumb_jobs.clear();
        self.waveforms.clear();
        self.preview.reset();
        self.status = format!("{n} von {missing} Medien neu verknüpft");
    }

    // ---------- Pegelanzeige ----------

    /// Nimmt die Spitzenpegel des Players (oder der Aufnahme) auf; Rücklauf ca. 36 dB/s bei 30 Hz.
    pub(crate) fn update_meter(&mut self) {
        let raw: [f32; 2] = if let Some(r) = &self.feat.recorder {
            let l = r.level();
            [l, l]
        } else if self.playing {
            self.player.as_ref().map(|p| p.levels()).unwrap_or([0.0; 2])
        } else if let Some(p) = &self.source_player {
            p.levels()
        } else {
            [0.0; 2]
        };
        for ch in 0..2 {
            let db = if raw[ch] > 1e-4 { 20.0 * raw[ch].log10() } else { -90.0 };
            let m = &mut self.feat.meter[ch];
            *m = db.max(*m - 1.2);
            if *m >= self.feat.meter_peak[ch] {
                self.feat.meter_peak[ch] = *m;
                self.feat.meter_hold[ch] = 45;
            } else if self.feat.meter_hold[ch] > 0 {
                self.feat.meter_hold[ch] -= 1;
            } else {
                self.feat.meter_peak[ch] = (self.feat.meter_peak[ch] - 0.5).max(-90.0);
            }
        }
    }
}

fn chrono_stamp() -> String {
    use windows::Win32::System::SystemInformation::GetLocalTime;
    let t = unsafe { GetLocalTime() };
    format!("{:04}{:02}{:02}_{:02}{:02}{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::write_wav;

    fn app() -> App {
        let ctx = egui::Context::default();
        let mut a = App::with_ctx(&ctx, None);
        let dir = std::env::temp_dir().join("video-superbe-tests").join(format!("data-{}", Uuid::new_v4()));
        a.feat.data_dir = dir;
        a
    }

    fn wait_for(app: &mut App, what: impl Fn(&App) -> bool) {
        let ctx = egui::Context::default();
        for _ in 0..400 {
            app.poll_features(&ctx);
            if what(app) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("Zeitüberschreitung: {}", app.status);
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join("video-superbe-tests");
        std::fs::create_dir_all(&d).unwrap();
        d.join(name)
    }

    fn add_audio(app: &mut App, path: &Path, track: usize, start: f64, dur: f64) -> Uuid {
        let a = MediaAsset { path: path.to_string_lossy().into_owned(), name: path.file_name().unwrap().to_string_lossy().into_owned(), duration: dur, has_audio: true, ..Default::default() };
        let aid = a.id;
        app.project.assets.push(a);
        while app.project.tracks.len() <= track {
            app.project.tracks.push(Track::default());
        }
        let c = Clip { content: ClipContent::Media(Wrap { v: aid }), part: ClipPart::Audio, start, duration: dur, ..Default::default() };
        let id = c.id;
        app.project.tracks[track].clips.push(c);
        id
    }

    #[test]
    fn srt_has_numbered_blocks_with_comma_milliseconds() {
        let mut p = Project::default();
        let mk = |s: f64, d: f64, t: &str, sub: bool| Clip { content: ClipContent::Title(Wrap { v: TitleStyle { text: t.into(), subtitle: sub.then_some(true), ..Default::default() } }), start: s, duration: d, ..Default::default() };
        p.tracks[0].clips.push(mk(3723.5, 2.0, "Später", true));
        p.tracks[0].clips.push(mk(1.0, 1.25, " Erster ", true));
        p.tracks[0].clips.push(mk(0.0, 5.0, "Kein Untertitel", false));
        let s = srt_text(&p);
        assert_eq!(s, "1\n00:00:01,000 --> 00:00:02,250\nErster\n\n2\n01:02:03,500 --> 01:02:05,500\nSpäter\n");
        assert_eq!(srt_text(&Project::default()), "");
    }

    #[test]
    fn relink_finds_files_by_name_in_subfolders() {
        let root = tmp("relink_root");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::write(root.join("a/b/Clip1.MP4"), b"x").unwrap();
        std::fs::write(root.join("note.txt"), b"x").unwrap();
        let mut p = Project::default();
        p.assets.push(MediaAsset { path: "C:/weg/clip1.mp4".into(), name: "clip1.mp4".into(), ..Default::default() });
        p.assets.push(MediaAsset { path: "C:/weg/anders.mp4".into(), ..Default::default() });
        p.assets.push(MediaAsset { path: root.join("note.txt").to_string_lossy().into_owned(), ..Default::default() });
        let n = relink_in(&mut p, &scan_dir(&root));
        assert_eq!(n, 1);
        assert!(p.assets[0].path.ends_with("Clip1.MP4") && Path::new(&p.assets[0].path).exists());
        assert!(!Path::new(&p.assets[1].path).exists(), "nicht gefunden bleibt unverändert");
        // über die App
        let mut a = app();
        a.project.assets.push(MediaAsset { path: "C:/weg/note.txt".into(), ..Default::default() });
        assert_eq!(a.missing_media(), 1);
        a.relink_from(&root);
        assert_eq!(a.missing_media(), 0);
        assert!(a.status.starts_with("1 von 1"), "{}", a.status);
    }

    #[test]
    fn autosave_is_written_only_when_dirty_and_restored_on_request() {
        let mut a = app();
        a.autosave_now();
        assert!(!a.autosave_path().exists(), "sauberes Projekt: keine Sicherung");
        a.project.markers = Some(vec![Marker { id: Uuid::new_v4(), t: 3.0, name: "Eins".into() }]);
        assert!(a.is_dirty());
        a.autosave_now();
        assert!(a.autosave_path().exists());
        let first = std::fs::metadata(a.autosave_path()).unwrap().modified().unwrap();
        std::thread::sleep(Duration::from_millis(30));
        a.autosave_now();
        assert_eq!(std::fs::metadata(a.autosave_path()).unwrap().modified().unwrap(), first, "unverändert: nicht neu schreiben");
        // Neustart: Sicherung wiederherstellen
        let dir = a.feat.data_dir.clone();
        let mut b = app();
        b.feat.data_dir = dir;
        b.restore_autosave();
        assert_eq!(b.project.markers.as_ref().unwrap()[0].name, "Eins");
        assert!(b.is_dirty(), "gilt als ungesichert");
        b.discard_autosave();
        assert!(!b.autosave_path().exists());
    }

    #[test]
    fn unsaved_changes_ask_before_new_open_and_quit() {
        let ctx = egui::Context::default();
        let mut a = app();
        a.new_project();
        assert!(a.feat.pending.is_none(), "sauber: sofort neu");
        a.project.fps = 25;
        a.new_project();
        assert_eq!(a.feat.pending, Some(Pending::New), "geändert: Rückfrage");
        assert_eq!(a.project.fps, 25);
        assert!(!a.resolve_pending(&ctx, Answer::Cancel));
        assert_eq!(a.project.fps, 25, "Abbrechen verwirft nichts");
        a.new_project();
        assert!(a.resolve_pending(&ctx, Answer::Discard));
        assert_eq!(a.project.fps, Project::default().fps);
        // Sichern in eine Datei, dann geht es weiter
        a.project.fps = 50;
        a.path = Some(tmp("pending_save.mvd.json"));
        a.new_project();
        assert!(a.resolve_pending(&ctx, Answer::Save));
        assert!(load(&tmp("pending_save.mvd.json")).unwrap().fps == 50, "wurde gesichert");
        // Beenden
        a.project.fps = 24;
        a.open_path_checked(tmp("pending_save.mvd.json"));
        assert!(matches!(a.feat.pending, Some(Pending::Open(Some(_)))));
        assert!(a.resolve_pending(&ctx, Answer::Discard));
        assert_eq!(a.project.fps, 50, "Datei geöffnet");
        a.project.fps = 30;
        a.feat.pending = Some(Pending::Quit);
        a.resolve_pending(&ctx, Answer::Discard);
        assert!(a.feat.allow_close);
    }

    #[test]
    fn beat_grid_is_detected_in_the_background_and_edited() {
        let mut a = app();
        // 24 s Klickspur mit 100 BPM als WAV
        let rate = 48000usize;
        let mut s = vec![0i16; rate * 24];
        let mut t = 0.2;
        while t < 23.5 {
            let st = (t * rate as f64) as usize;
            for i in 0..4000 {
                let v = (-(i as f32) / 700.0).exp() * (2.0 * std::f32::consts::PI * 70.0 * i as f32 / rate as f32).sin();
                s[st + i] = s[st + i].saturating_add((v * 22000.0) as i16);
            }
            t += 0.6;
        }
        let wav = tmp("beats_click.wav");
        write_wav(&wav, &s, rate as u32).unwrap();
        let cid = add_audio(&mut a, &wav, 0, 0.0, 24.0);
        a.detect_beats(cid);
        assert!(a.feat.beat_busy.len() == 1);
        wait_for(&mut a, |x| x.project.clip(cid).unwrap().beat_grid.is_some());
        let g = a.project.clip(cid).unwrap().beat_grid.unwrap();
        assert!((g.bpm() - 100.0).abs() < 1.5, "{} BPM", g.bpm());
        assert!(a.feat.beat_busy.is_empty());
        // Tempo verdoppeln / halbieren
        a.scale_beats(cid, 2.0);
        assert!((a.project.clip(cid).unwrap().beat_grid.unwrap().bpm() - 200.0).abs() < 3.0);
        a.scale_beats(cid, 0.5);
        // Ausrichten an der Abspielposition
        a.playhead = 5.0;
        a.align_beats(cid);
        let g2 = a.project.clip(cid).unwrap().beat_grid.unwrap();
        assert!((g2.phase - 5.0 % g2.period).abs() < 1e-9);
        // zweiter Clip derselben Quelle bekommt das Raster beim erneuten Erkennen sofort (Zwischenspeicher)
        let c2 = a.project.clip(cid).unwrap().clone();
        a.project.tracks[1].clips.push(Clip { id: Uuid::new_v4(), start: 30.0, ..c2 });
        let c2id = a.project.tracks[1].clips[0].id;
        a.project.tracks[1].clips[0].beat_grid = None;
        a.detect_beats(c2id);
        assert!(a.project.clip(c2id).unwrap().beat_grid.is_some(), "aus dem Zwischenspeicher");
        a.remove_beats(cid);
        assert!(a.project.tracks.iter().flat_map(|t| &t.clips).all(|c| c.beat_grid.is_none()), "an allen Clips der Quelle entfernt");
    }

    #[test]
    fn ducking_lowers_music_during_speech_only() {
        let mut a = app();
        let rate = 48000usize;
        // Sprache: 3–6 s und 12–14 s Ton mit Pegel, dazwischen leises Rauschen
        let mut v = vec![0i16; rate * 16];
        let mut seed = 7u32;
        for (i, x) in v.iter_mut().enumerate() {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let noise = ((seed >> 16) as f32 / 65535.0 - 0.5) * 60.0;
            let t = i as f64 / rate as f64;
            let speech = (3.0..6.0).contains(&t) || (12.0..14.0).contains(&t);
            *x = (noise + if speech { (2.0 * std::f32::consts::PI * 300.0 * i as f32 / rate as f32).sin() * 9000.0 } else { 0.0 }) as i16;
        }
        let voice = tmp("duck_voice.wav");
        write_wav(&voice, &v, rate as u32).unwrap();
        let music = crate::audio::tests::write_sine_wav("duck_music.wav", 440.0, 16.0, 48000, 0.3);
        let mid = add_audio(&mut a, &music, 0, 0.0, 16.0);
        add_audio(&mut a, &voice, 1, 0.0, 16.0);
        a.apply_ducking(mid, -12.0);
        assert!(a.feat.duck_busy);
        wait_for(&mut a, |x| !x.feat.duck_busy);
        let m = a.project.clip(mid).unwrap();
        assert!(!m.volume_points.is_empty(), "{}", a.status);
        let low = 10f64.powf(-12.0 / 20.0);
        assert!((m.curve_gain(4.5) - low).abs() < 0.03, "bei Sprache leiser: {}", m.curve_gain(4.5));
        assert!((m.curve_gain(9.0) - 1.0).abs() < 0.03, "in der Pause volle Lautstärke: {}", m.curve_gain(9.0));
        assert!((m.curve_gain(13.0) - low).abs() < 0.03, "zweiter Abschnitt");
        assert!(a.status.contains("2 Sprachabschnitte"), "{}", a.status);
        a.clear_ducking(mid);
        assert!(a.project.clip(mid).unwrap().volume_points.is_empty());
        // ohne andere Tonspur: Hinweis
        let mut b = app();
        let only = add_audio(&mut b, &music, 0, 0.0, 16.0);
        b.apply_ducking(only, -8.0);
        assert!(b.status.starts_with("Keine andere Tonspur") && !b.feat.duck_busy);
    }

    #[test]
    fn freeze_frame_inserts_a_three_second_still_above() {
        let mut a = app();
        // kurzes Video aus zwei Farbbildern erzeugen
        let red = crate::render::tests::solid_png("freeze_red.png", 320, 180, [220, 20, 20]);
        let blue = crate::render::tests::solid_png("freeze_blue.png", 320, 180, [20, 20, 220]);
        let mut p = Project::default();
        let ra = crate::render::tests::image_asset(&mut p, &red, 320.0, 180.0);
        let ba = crate::render::tests::image_asset(&mut p, &blue, 320.0, 180.0);
        p.tracks[0].clips.push(crate::render::tests::clip(ra, 0.0, 1.0));
        p.tracks[0].clips.push(crate::render::tests::clip(ba, 1.0, 1.0));
        p.render_width = 320;
        p.render_height = 180;
        p.fps = 15;
        let mp4 = tmp("freeze_src.mp4");
        crate::export::export(&p, &mp4, &crate::export::ExportOpts::from_project(&p), &mut |_| true).unwrap();
        a.import_file(mp4.clone());
        let vid = a.project.assets.iter().find(|x| !x.is_image).unwrap().id;
        a.add_to_timeline(vid, None, Some(0.0), None);
        let cid = a.selected.unwrap();
        a.playhead = 1.5; // blaue Hälfte
        a.freeze_frame();
        let still = a.project.clip(a.selected.unwrap()).unwrap().clone();
        assert_ne!(still.id, cid);
        assert!((still.start - 1.5).abs() < 1e-9 && (still.duration - 3.0).abs() < 1e-9);
        let asset = a.project.asset(still.asset_id().unwrap()).unwrap();
        assert!(asset.is_image && asset.path.contains("Standbild_"), "{}", asset.path);
        let img = image::open(&asset.path).unwrap().to_rgba8();
        let px = img.get_pixel(160, 90);
        assert!(px[2] > 150 && px[0] < 90, "blaues Bild: {px:?}");
        let (st, _) = a.project.find_clip(still.id).unwrap();
        let (vt, _) = a.project.find_clip(cid).unwrap();
        assert!(st > vt, "Standbild liegt über dem Video");
        // ohne Auswahl / außerhalb: Hinweis
        a.selected = Some(cid);
        a.playhead = 50.0;
        a.freeze_frame();
        assert!(a.status.starts_with("Videoclip wählen"));
    }

    #[test]
    fn recording_is_inserted_on_the_first_free_track() {
        let mut a = app();
        let wav = tmp("rec_insert.wav");
        write_wav(&wav, &vec![1000i16; 48000 * 2], 48000).unwrap();
        let tone = crate::audio::tests::write_sine_wav("rec_other.wav", 300.0, 3.0, 48000, 0.2);
        add_audio(&mut a, &tone, 0, 0.0, 3.0);
        a.insert_recording(wav.clone(), 1.0);
        let c = a.project.clip(a.selected.unwrap()).unwrap();
        let (t, _) = a.project.find_clip(c.id).unwrap();
        assert_eq!(t, 1, "Spur 1 ist belegt, die Aufnahme kommt auf Spur 2");
        assert!((c.start - 1.0).abs() < 1e-9 && (c.duration - 2.0).abs() < 0.1, "{} {}", c.start, c.duration);
        assert!(a.status.starts_with("Aufnahme eingefügt"));
    }

    #[test]
    fn still_image_export_writes_png_and_jpg() {
        let mut a = app();
        let red = crate::render::tests::solid_png("still_red.png", 64, 36, [200, 30, 30]);
        a.import_file(red);
        let id = a.project.assets[0].id;
        a.add_to_timeline(id, None, Some(0.0), None);
        a.playhead = 1.0;
        a.project.render_width = 320;
        a.project.render_height = 180;
        let png = tmp("still_out.png");
        a.write_still(&png).unwrap();
        let i = image::open(&png).unwrap().to_rgba8();
        assert_eq!((i.width(), i.height()), (320, 180));
        let p = i.get_pixel(160, 90);
        assert!(p[0] > 150 && p[1] < 80, "{p:?}");
        let jpg = tmp("still_out.jpg");
        a.write_still(&jpg).unwrap();
        assert_eq!(image::open(&jpg).unwrap().width(), 320);
    }

    #[test]
    fn meter_follows_levels_and_decays() {
        let mut a = app();
        a.feat.meter = [-6.0, -6.0];
        a.feat.meter_peak = [-6.0, -6.0];
        a.feat.meter_hold = [45; 2];
        for _ in 0..10 {
            a.update_meter();
        }
        assert!(a.feat.meter[0] < -17.0 && a.feat.meter[0] > -19.0, "Rücklauf 1,2 dB je Schritt: {}", a.feat.meter[0]);
        assert_eq!(a.feat.meter_peak[0], -6.0, "Spitze wird gehalten");
        for _ in 0..200 {
            a.update_meter();
        }
        assert!(a.feat.meter_peak[0] < -30.0 && a.feat.meter[0] == -90.0);
    }

    #[test]
    fn subtitle_preset_sits_at_the_bottom_and_feeds_the_srt() {
        let mut a = app();
        a.playhead = 2.0;
        a.add_title(crate::editing::TitlePreset::Subtitle);
        let c = a.project.clip(a.selected.unwrap()).unwrap().clone();
        let ClipContent::Title(t) = &c.content else { panic!() };
        assert_eq!(t.v.subtitle, Some(true));
        assert!(t.v.background && (c.transform.y - 0.38).abs() < 1e-9 && (c.duration - 3.0).abs() < 1e-9);
        assert!(srt_text(&a.project).contains("00:00:02,000 --> 00:00:05,000
Untertitel"));
        // Hochformat: kleinere Schrift, höher im Bild
        let mut b = app();
        b.project.render_width = 1080;
        b.project.render_height = 1920;
        b.add_title(crate::editing::TitlePreset::Subtitle);
        let c = b.project.clip(b.selected.unwrap()).unwrap();
        assert!((c.transform.y - 0.30).abs() < 1e-9);
    }

    #[test]
    fn first_video_sets_the_format_and_duplicates_are_skipped() {
        let mut a = app();
        let tall = crate::render::tests::solid_png("fmt_tall.png", 90, 160, [10, 10, 10]);
        a.import_file(tall.clone());
        assert_eq!((a.project.render_width, a.project.render_height), (1080, 1920), "Hochformat → 9:16");
        a.import_file(tall);
        assert_eq!(a.project.assets.len(), 1, "gleiche Datei nicht doppelt");
        assert_eq!(a.status, "Schon im Medienpool");
        // zweites Medium ändert das Format nicht mehr
        let wide = crate::render::tests::solid_png("fmt_wide.png", 160, 90, [10, 10, 10]);
        a.import_file(wide);
        assert_eq!((a.project.render_width, a.project.render_height), (1080, 1920));
        // Bildrate: nächstliegende Standardrate
        let mut b = app();
        b.register_asset(MediaAsset { path: "x.mp4".into(), name: "x.mp4".into(), has_video: true, width: 3840.0, height: 2160.0, fps: 59.94, duration: 3.0, ..Default::default() });
        assert_eq!((b.project.render_width, b.project.render_height, b.project.fps), (3840, 2160, 60));
        let mut c = app();
        c.register_asset(MediaAsset { path: "y.mp4".into(), name: "y.mp4".into(), has_video: true, width: 1920.0, height: 1080.0, fps: 23.976, duration: 3.0, is_hdr: Some(true), ..Default::default() });
        assert_eq!(c.project.fps, 24);
        assert!(c.status.contains("HDR"), "{}", c.status);
    }

    /// Bild-Effekte müssen auch bei Videoclips wirken, in der Vorschau (asynchroner Decoder) wie im Export.
    #[test]
    fn effects_apply_to_video_clips_in_renderer_and_preview_path() {
        let mut a = app();
        let red = crate::render::tests::solid_png("vfx_red.png", 320, 180, [220, 20, 20]);
        let mut p = Project::default();
        let ra = crate::render::tests::image_asset(&mut p, &red, 320.0, 180.0);
        p.tracks[0].clips.push(crate::render::tests::clip(ra, 0.0, 2.0));
        p.render_width = 320;
        p.render_height = 180;
        p.fps = 15;
        let mp4 = tmp("vfx_src.mp4");
        crate::export::export(&p, &mp4, &crate::export::ExportOpts::from_project(&p), &mut |_| true).unwrap();
        a.import_file(mp4);
        let vid = a.project.assets.iter().find(|x| !x.is_image).unwrap().id;
        a.add_to_timeline(vid, None, Some(0.0), None);
        let cid = a.selected.unwrap();
        let ctx = egui::Context::default();
        let center = |rgba: &[u8], w: usize| { let o = (90 * w + 160) * 4; [rgba[o] as i32, rgba[o + 1] as i32, rgba[o + 2] as i32] };
        // Vorschau-Weg: Renderer mit asynchronem Decoder (wie im Monitor), Bild erst nach kurzer Wartezeit
        let mut grab = |a: &mut App| {
            for _ in 0..60 {
                let tex_key_before = a.preview.dbg_key();
                let _ = a.preview_image(&ctx, 320, 180);
                if a.preview.dbg_key() != tex_key_before { /* neu gerechnet */ }
                std::thread::sleep(Duration::from_millis(40));
                a.project_rev += 0;
            }
            let pv = &mut a.preview;
            pv.renderer.render_with(&a.project, 0.5, 320, 180, &mut pv.frames)
        };
        let plain = center(&grab(&mut a), 320);
        assert!(plain[0] > 150 && plain[1] < 80, "rot ohne Effekt: {plain:?}");
        a.project.clip_mut(cid).unwrap().set_channel(crate::anim::Channel::Saturation, 0.0, 0.5);
        a.project_rev += 1;
        let gray = center(&grab(&mut a), 320);
        assert!((gray[0] - gray[1]).abs() < 12 && (gray[1] - gray[2]).abs() < 12, "Sättigung 0 = grau: {gray:?}");
    }
}

