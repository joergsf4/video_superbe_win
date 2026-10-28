//! Medienverwaltung (Port von LocalMedia.swift, CacheManager.swift und der Pool-Logik aus AppState.swift):
//! Ansicht, Sortierung, Filter und Auswahl des Medienpools wie im Datei-Explorer, lokale Kopien des Quellmaterials,
//! Übersicht der Zwischenspeicher und Einstellungen, die zwischen den Starten erhalten bleiben.

use crate::app::App;
use crate::model::MediaAsset;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PoolView {
    #[default]
    Large,
    Small,
    List,
    Details,
}

impl PoolView {
    pub const ALL: [PoolView; 4] = [PoolView::Large, PoolView::Small, PoolView::List, PoolView::Details];
    pub fn label(self) -> &'static str {
        match self {
            PoolView::Large => "Große Symbole",
            PoolView::Small => "Kleine Symbole",
            PoolView::List => "Liste",
            PoolView::Details => "Details",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PoolSort {
    #[default]
    Imported,
    Name,
    Type,
    Duration,
    Size,
    Date,
}

impl PoolSort {
    pub const ALL: [PoolSort; 6] = [PoolSort::Imported, PoolSort::Name, PoolSort::Type, PoolSort::Duration, PoolSort::Size, PoolSort::Date];
    pub fn label(self) -> &'static str {
        match self {
            PoolSort::Imported => "Importreihenfolge",
            PoolSort::Name => "Name",
            PoolSort::Type => "Typ",
            PoolSort::Duration => "Dauer",
            PoolSort::Size => "Größe",
            PoolSort::Date => "Änderungsdatum",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PoolFilter {
    #[default]
    All,
    Video,
    Photo,
    Audio,
}

impl PoolFilter {
    pub const ALL: [PoolFilter; 4] = [PoolFilter::All, PoolFilter::Video, PoolFilter::Photo, PoolFilter::Audio];
    pub fn label(self) -> &'static str {
        match self {
            PoolFilter::All => "Alle Medien",
            PoolFilter::Video => "Nur Videos",
            PoolFilter::Photo => "Nur Fotos",
            PoolFilter::Audio => "Nur Audio",
        }
    }
    pub fn accepts(self, a: &MediaAsset) -> bool {
        match self {
            PoolFilter::All => true,
            PoolFilter::Video => a.has_video && !a.is_image,
            PoolFilter::Photo => a.is_image,
            PoolFilter::Audio => !a.has_video && !a.is_image,
        }
    }
}

pub fn kind_label(a: &MediaAsset) -> &'static str {
    if a.is_image {
        "Foto"
    } else if a.has_video {
        "Video"
    } else {
        "Audio"
    }
}

/// Medienpool: Ansicht und Auswahl (wie im Datei-Explorer).
#[derive(Default)]
pub struct PoolState {
    pub view: PoolView,
    pub sort: PoolSort,
    pub ascending: bool,
    pub filter: PoolFilter,
    pub search: String,
    pub selection: HashSet<Uuid>,
    /// Ausgangspunkt für Umschalt-Klick und Umschalt-Pfeile; zuletzt mit der Tastatur erreichtes Medium
    pub anchor: Option<Uuid>,
    pub focus: Option<Uuid>,
    /// Maus über dem Pool: Strg+A, Pfeile, Entf und Eingabe gelten dann dem Pool
    pub hover: bool,
    /// Kacheln der letzten Darstellung (für Pfeil hoch/runter)
    pub frames: Vec<(Uuid, eframe::egui::Rect)>,
    /// beim Ziehen mehrerer Medien: alle in Poolreihenfolge
    pub dragging: Vec<Uuid>,
    /// laufender Auswahlrahmen (Anfang, Ende)
    pub marquee: Option<(eframe::egui::Pos2, eframe::egui::Pos2)>,
    /// Größe und Änderungsdatum der Dateien (Sekunden seit 1970)
    pub file_info: HashMap<Uuid, (u64, u64)>,
}

impl PoolState {
    pub fn file_info(&mut self, a: &MediaAsset) -> (u64, u64) {
        *self.file_info.entry(a.id).or_insert_with(|| {
            let m = std::fs::metadata(&a.path).ok();
            let size = m.as_ref().map_or(0, |m| m.len());
            let date = m.and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
            (size, date)
        })
    }
}

/// Gespeicherte Einstellungen (`Einstellungen.json` im Datenordner).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Prefs {
    pub pool_view: PoolView,
    pub pool_sort: PoolSort,
    pub pool_ascending: bool,
    /// Beim Import alles in den Medienordner kopieren
    pub copy_imports_local: bool,
    /// Eigene Export-Voreinstellungen
    pub export_presets: Vec<crate::export::ExportPresetItem>,
    /// Zuletzt gewählte Voreinstellung im Export-Dialog
    pub last_export_preset: Option<String>,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { pool_view: PoolView::Large, pool_sort: PoolSort::Imported, pool_ascending: true, copy_imports_local: false, export_presets: vec![], last_export_preset: None }
    }
}

impl Prefs {
    fn path(dir: &Path) -> PathBuf {
        dir.join("Einstellungen.json")
    }
    pub fn load(dir: &Path) -> Prefs {
        std::fs::read(Self::path(dir)).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default()
    }
    pub fn save(&self, dir: &Path) {
        let _ = std::fs::create_dir_all(dir);
        if let Ok(s) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(Self::path(dir), s);
        }
    }
}

// ---------- Quellmaterial lokal sichern ----------

/// Medienordner im Datenordner.
pub fn media_folder(data: &Path) -> PathBuf {
    data.join("Medien")
}

/// Liegt die Datei schon im Medienordner oder einem eigenen Ordner der App (Aufnahmen, importierte Bilder)?
pub fn is_managed(data: &Path, p: &Path) -> bool {
    let norm = |x: &Path| x.to_string_lossy().replace('/', "\\").to_lowercase();
    let f = norm(p);
    ["Medien", "Aufnahmen", "Imported"].iter().any(|d| f.starts_with(&(norm(&data.join(d)) + "\\")))
}

/// Zwischenspeicher, der verschwinden kann (Temp-Ordner, Anhänge aus Outlook und Browser-Caches, entpackte ZIP-Dateien).
pub fn is_temporary(p: &Path) -> bool {
    let f = p.to_string_lossy().replace('/', "\\").to_lowercase();
    let tmp = std::env::temp_dir().to_string_lossy().replace('/', "\\").to_lowercase();
    let tmp = tmp.trim_end_matches('\\').to_string();
    (!tmp.is_empty() && f.starts_with(&(tmp + "\\")))
        || f.contains("\\content.outlook\\")
        || f.contains("\\inetcache\\")
        || f.contains("\\temporary internet files\\")
        || f.contains("\\windows\\temp\\")
}

/// Ziel im Medienordner: gleicher Name; ist er belegt, wird bei gleicher Größe die vorhandene Kopie genutzt, sonst nummeriert.
fn destination(folder: &Path, src: &Path, size: u64) -> (PathBuf, bool) {
    let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Medium".into());
    let ext = src.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    for n in 1..1000 {
        let name = if n == 1 { format!("{stem}{ext}") } else { format!("{stem} {n}{ext}") };
        let d = folder.join(name);
        match std::fs::metadata(&d) {
            Err(_) => return (d, false),
            Ok(m) if m.len() == size => return (d, true),
            Ok(_) => {}
        }
    }
    (folder.join(format!("{}_{}", Uuid::new_v4(), src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())), false)
}

/// Liefert die lokale Kopie der Datei (kopiert bei Bedarf). Ohne `forced` und ohne die Einstellung wird nur kopiert, was
/// sonst verschwinden würde (temporäre Dateien).
pub fn local_copy(data: &Path, src: &Path, enabled: bool, forced: bool) -> Result<PathBuf, String> {
    if is_managed(data, src) || !(forced || enabled || is_temporary(src)) {
        return Ok(src.to_path_buf());
    }
    let folder = media_folder(data);
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let size = std::fs::metadata(src).map_err(|e| e.to_string())?.len();
    let (dest, exists) = destination(&folder, src, size);
    if !exists {
        std::fs::copy(src, &dest).map_err(|e| e.to_string())?;
    }
    Ok(dest)
}

// ---------- Zwischenspeicher ----------

/// Kategorie im Datenordner. Zwischendateien lassen sich gefahrlos löschen (die App rechnet sie bei Bedarf neu);
/// Aufnahmen, importierte Standbilder und lokal gesicherte Medien sind Originale der Projekte und werden nur angezeigt.
pub struct CacheCategory {
    pub title: &'static str,
    pub detail: &'static str,
    pub folder: &'static str,
    pub deletable: bool,
}

pub const CACHE_CATEGORIES: [CacheCategory; 4] = [
    CacheCategory { title: "Stabilisierung", detail: "gespeicherte Bewegungsdaten; neu berechnen dauert etwa so lange wie das Video", folder: "Stabilisierung", deletable: true },
    CacheCategory { title: "Aufnahmen", detail: "Sprecher – Originale deiner Projekte, werden nicht gelöscht", folder: "Aufnahmen", deletable: false },
    CacheCategory { title: "Importierte Bilder und Standbilder", detail: "gehören zu Projekten, werden nicht gelöscht", folder: "Imported", deletable: false },
    CacheCategory { title: "Lokal gesicherte Medien", detail: "Kopien des Quellmaterials – werden nicht gelöscht", folder: "Medien", deletable: false },
];

/// Größe eines Ordners in Bytes (rekursiv).
pub fn dir_size(p: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else { return 0 };
    rd.filter_map(|e| e.ok())
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => dir_size(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

/// Löscht den Inhalt der Kategorie (nicht den Ordner). Rückgabe: freigegebene Bytes.
pub fn clear_category(data: &Path, c: &CacheCategory) -> u64 {
    if !c.deletable {
        return 0;
    }
    let dir = data.join(c.folder);
    let before = dir_size(&dir);
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.filter_map(|e| e.ok()) {
            let p = e.path();
            let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
        }
    }
    before.saturating_sub(dir_size(&dir))
}

pub fn format_bytes(b: u64) -> String {
    let b = b as f64;
    if b <= 0.0 {
        "0 KB".into()
    } else if b < 1e6 {
        format!("{:.0} KB", (b / 1e3).max(1.0))
    } else if b < 1e9 {
        format!("{:.1} MB", b / 1e6)
    } else {
        format!("{:.2} GB", b / 1e9)
    }
}

impl App {
    /// Medien in der angezeigten Reihenfolge (Filter, Suche, Sortierung).
    pub(crate) fn pool_assets(&mut self) -> Vec<MediaAsset> {
        let q = self.pool.search.trim().to_lowercase();
        let mut l: Vec<MediaAsset> = self.project.assets.iter().filter(|a| self.pool.filter.accepts(a) && (q.is_empty() || a.name.to_lowercase().contains(&q))).cloned().collect();
        let sort = self.pool.sort;
        if sort != PoolSort::Imported {
            let info: HashMap<Uuid, (u64, u64)> = l.iter().map(|a| (a.id, self.pool.file_info(a))).collect();
            // stabil: gleiche Werte behalten die Importreihenfolge
            l.sort_by(|x, y| {
                let o = match sort {
                    PoolSort::Name => natural_cmp(&x.name, &y.name),
                    PoolSort::Type => kind_label(x).cmp(kind_label(y)).then_with(|| natural_cmp(&x.name, &y.name)),
                    PoolSort::Duration => x.duration.total_cmp(&y.duration),
                    PoolSort::Size => info[&x.id].0.cmp(&info[&y.id].0),
                    PoolSort::Date => info[&x.id].1.cmp(&info[&y.id].1),
                    PoolSort::Imported => std::cmp::Ordering::Equal,
                };
                o
            });
        }
        if !self.pool.ascending {
            l.reverse();
        }
        l
    }

    /// Klick: einzeln wählen; Strg schaltet um; Umschalt wählt den Bereich vom Ankerpunkt.
    pub(crate) fn pool_click(&mut self, id: Uuid, ctrl: bool, shift: bool) {
        let order: Vec<Uuid> = self.pool_assets().iter().map(|a| a.id).collect();
        let pos = |x: Uuid| order.iter().position(|o| *o == x);
        if let (true, Some(i), Some(j)) = (shift, self.pool.anchor.and_then(pos), pos(id)) {
            let r: HashSet<Uuid> = order[i.min(j)..=i.max(j)].iter().copied().collect();
            self.pool.selection = if ctrl { self.pool.selection.union(&r).copied().collect() } else { r };
        } else if ctrl {
            if !self.pool.selection.remove(&id) {
                self.pool.selection.insert(id);
            }
            self.pool.anchor = Some(id);
        } else {
            self.pool.selection = HashSet::from([id]);
            self.pool.anchor = Some(id);
        }
        self.pool.focus = Some(id);
    }

    pub(crate) fn select_all_assets(&mut self) {
        let order: Vec<Uuid> = self.pool_assets().iter().map(|a| a.id).collect();
        self.pool.anchor = order.first().copied();
        self.pool.selection = order.into_iter().collect();
    }

    /// Gewählte Medien in Poolreihenfolge.
    pub(crate) fn pool_selected_assets(&mut self) -> Vec<MediaAsset> {
        let sel = self.pool.selection.clone();
        self.pool_assets().into_iter().filter(|a| sel.contains(&a.id)).collect()
    }

    /// Pfeiltasten (Umschalt erweitert die Auswahl), Eingabe (an die Zeitleiste), Entf (aus dem Pool), Esc (Auswahl aufheben).
    /// Wahr, wenn die Taste verbraucht ist.
    pub(crate) fn pool_key(&mut self, key: eframe::egui::Key, shift: bool) -> bool {
        use eframe::egui::Key;
        let order: Vec<Uuid> = self.pool_assets().iter().map(|a| a.id).collect();
        if order.is_empty() {
            return false;
        }
        match key {
            Key::Delete => {
                if self.pool.selection.is_empty() {
                    return false;
                }
                let s = self.pool.selection.clone();
                self.remove_assets(&s);
                return true;
            }
            Key::Enter => {
                if self.pool.selection.is_empty() {
                    return false;
                }
                for a in self.pool_selected_assets() {
                    self.add_from_pool(a.id);
                }
                return true;
            }
            Key::Escape => {
                if self.pool.selection.is_empty() {
                    return false;
                }
                self.pool.selection.clear();
                return true;
            }
            Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown => {}
            _ => return false,
        }
        let pos = |x: Uuid| order.iter().position(|o| *o == x);
        let Some(fi) = self.pool.focus.or(self.pool.anchor).and_then(pos) else {
            self.pool_click(order[0], false, false);
            return true;
        };
        let mut ni = fi;
        match key {
            Key::ArrowLeft => ni = fi.saturating_sub(1),
            Key::ArrowRight => ni = (fi + 1).min(order.len() - 1),
            _ => {
                // Zeile darüber/darunter: Kachel mit dem ähnlichsten x-Wert in der nächsten Zeile
                let up = key == Key::ArrowUp;
                if let Some(f) = self.pool.frames.iter().find(|(i, _)| *i == order[fi]).map(|(_, r)| *r) {
                    let rows: Vec<&(Uuid, eframe::egui::Rect)> = self.pool.frames.iter().filter(|(_, r)| if up { r.center().y < f.top() - 1.0 } else { r.top() > f.bottom() + 1.0 }).collect();
                    let row_y = if up { rows.iter().map(|(_, r)| r.center().y).fold(f32::NEG_INFINITY, f32::max) } else { rows.iter().map(|(_, r)| r.center().y).fold(f32::INFINITY, f32::min) };
                    if let Some(best) = rows.iter().filter(|(_, r)| (r.center().y - row_y).abs() < 2.0).min_by(|a, b| (a.1.center().x - f.center().x).abs().total_cmp(&(b.1.center().x - f.center().x).abs())) {
                        ni = pos(best.0).unwrap_or(fi);
                    }
                }
            }
        }
        let id = order[ni];
        match (shift, self.pool.anchor.and_then(pos)) {
            (true, Some(a)) => self.pool.selection = order[a.min(ni)..=a.max(ni)].iter().copied().collect(),
            _ => {
                self.pool.selection = HashSet::from([id]);
                self.pool.anchor = Some(id);
            }
        }
        self.pool.focus = Some(id);
        true
    }

    /// Entfernt Medien aus dem Pool (nur nicht verwendete; rückgängig möglich).
    pub(crate) fn remove_assets(&mut self, ids: &HashSet<Uuid>) {
        let used: HashSet<Uuid> = self.project.tracks.iter().flat_map(|t| &t.clips).filter_map(|c| c.asset_id()).collect();
        let free: HashSet<Uuid> = ids.difference(&used).copied().collect();
        if free.is_empty() {
            self.status = "Diese Medien werden im Film verwendet und lassen sich nicht aus dem Pool entfernen.".into();
            return;
        }
        self.project.assets.retain(|a| !free.contains(&a.id));
        self.pool.selection.retain(|i| !free.contains(i));
        self.status = if free.len() == ids.len() { format!("{} Medium/Medien aus dem Pool entfernt", free.len()) } else { format!("{} entfernt, {} werden im Film verwendet", free.len(), ids.len() - free.len()) };
    }

    /// Kopiert alle Medien des Projekts, die noch außerhalb des Medienordners liegen, dorthin und verknüpft sie neu.
    pub(crate) fn collect_media_locally(&mut self) {
        let data = self.feat.data_dir.clone();
        let todo: Vec<(Uuid, String)> = self.project.assets.iter().filter(|a| Path::new(&a.path).exists() && !is_managed(&data, Path::new(&a.path))).map(|a| (a.id, a.path.clone())).collect();
        let gone = self.project.assets.iter().filter(|a| !Path::new(&a.path).exists()).count();
        if todo.is_empty() {
            self.status = if gone > 0 { format!("Alle vorhandenen Medien liegen schon lokal; {gone} fehlen (Bearbeiten ▸ Fehlende Medien suchen)") } else { "Alle Medien liegen schon im Medienordner".into() };
            return;
        }
        let (mut done, mut failed) = (0, 0);
        for (id, path) in todo {
            match local_copy(&data, Path::new(&path), true, true) {
                Ok(np) => {
                    if let Some(a) = self.project.assets.iter_mut().find(|a| a.id == id) {
                        a.path = np.to_string_lossy().into_owned();
                    }
                    done += 1;
                }
                Err(_) => failed += 1,
            }
        }
        self.status = format!(
            "{done} Medien lokal gespeichert{}{} (Ordner: {})",
            if failed > 0 { format!(", {failed} fehlgeschlagen") } else { String::new() },
            if gone > 0 { format!(", {gone} fehlen weiterhin") } else { String::new() },
            media_folder(&data).display()
        );
    }

    /// Nach dem Löschen von Zwischendateien: gemerkte Daten vergessen und die Vorschau neu rechnen.
    pub(crate) fn caches_cleared(&mut self, freed: u64) {
        crate::stabilize::forget_all();
        self.preview.reset();
        self.project_rev += 1;
        self.status = format!("Zwischenspeicher aufgeräumt: {} frei.", format_bytes(freed));
    }

    pub(crate) fn save_prefs(&mut self) {
        self.prefs.pool_view = self.pool.view;
        self.prefs.pool_sort = self.pool.sort;
        self.prefs.pool_ascending = self.pool.ascending;
        self.prefs.save(&self.feat.data_dir);
    }
}

/// Vergleich mit Zahlen in natürlicher Reihenfolge („Clip 2“ vor „Clip 10“), ohne Groß-/Kleinschreibung.
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(ch) = it.peek().copied().filter(|c| c.is_ascii_digit()) {
                        s.push(ch);
                        it.next();
                    }
                    s
                };
                let (n, m) = (take(&mut x), take(&mut y));
                let (nt, mt) = (n.trim_start_matches('0'), m.trim_start_matches('0'));
                let o = nt.len().cmp(&mt.len()).then_with(|| nt.cmp(mt));
                if o != std::cmp::Ordering::Equal {
                    return o;
                }
            }
            (Some(c), Some(d)) => {
                if c != d {
                    return c.cmp(&d);
                }
                x.next();
                y.next();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_and_kinds() {
        let mut v = vec!["Clip 10", "clip 2", "Clip 1", "Abend"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, vec!["Abend", "Clip 1", "clip 2", "Clip 10"]);
        let img = MediaAsset { is_image: true, has_video: true, ..Default::default() };
        let vid = MediaAsset { has_video: true, has_audio: true, ..Default::default() };
        let snd = MediaAsset { has_audio: true, ..Default::default() };
        assert_eq!((kind_label(&img), kind_label(&vid), kind_label(&snd)), ("Foto", "Video", "Audio"));
        assert!(PoolFilter::Photo.accepts(&img) && !PoolFilter::Video.accepts(&img) && PoolFilter::Audio.accepts(&snd) && !PoolFilter::Audio.accepts(&vid));
    }

    #[test]
    fn local_copies_reuse_same_size_and_number_others() {
        let base = std::env::temp_dir().join("video-superbe-tests").join("localmedia");
        let _ = std::fs::remove_dir_all(&base);
        let data = base.join("data");
        let src_dir = base.join("quelle");
        std::fs::create_dir_all(&src_dir).unwrap();
        let a = src_dir.join("Urlaub.mp4");
        std::fs::write(&a, b"12345").unwrap();
        // ohne Einstellung bleibt eine normale Datei, wo sie ist … (der Testordner liegt im Temp-Ordner, daher erzwungen prüfen)
        let c1 = local_copy(&data, &a, true, false).unwrap();
        assert_eq!(c1, media_folder(&data).join("Urlaub.mp4"));
        assert!(is_managed(&data, &c1));
        // gleiche Datei noch einmal: vorhandene Kopie
        assert_eq!(local_copy(&data, &a, true, false).unwrap(), c1);
        // gleicher Name, andere Größe: nummeriert
        let other = base.join("anders").join("Urlaub.mp4");
        std::fs::create_dir_all(other.parent().unwrap()).unwrap();
        std::fs::write(&other, b"123456789").unwrap();
        assert_eq!(local_copy(&data, &other, true, false).unwrap(), media_folder(&data).join("Urlaub 2.mp4"));
        // verwaltete Dateien werden nicht noch einmal kopiert
        assert_eq!(local_copy(&data, &c1, true, true).unwrap(), c1);
        assert!(is_temporary(&std::env::temp_dir().join("x.mp4")));
        assert!(!is_temporary(Path::new("D:\\Filme\\x.mp4")));
        assert!(is_temporary(Path::new("C:\\Users\\x\\AppData\\Local\\Microsoft\\Windows\\INetCache\\Content.Outlook\\AB\\v.mp4")));
    }

    #[test]
    fn cache_sizes_and_clearing_only_deletable() {
        let data = std::env::temp_dir().join("video-superbe-tests").join("cachemgr");
        let _ = std::fs::remove_dir_all(&data);
        std::fs::create_dir_all(data.join("Stabilisierung")).unwrap();
        std::fs::create_dir_all(data.join("Aufnahmen")).unwrap();
        std::fs::write(data.join("Stabilisierung").join("a.stab.json"), vec![0u8; 3000]).unwrap();
        std::fs::write(data.join("Aufnahmen").join("r.wav"), vec![0u8; 5000]).unwrap();
        assert_eq!(dir_size(&data), 8000);
        let freed: u64 = CACHE_CATEGORIES.iter().map(|c| clear_category(&data, c)).sum();
        assert_eq!(freed, 3000);
        assert!(data.join("Stabilisierung").is_dir(), "der Ordner bleibt");
        assert!(data.join("Aufnahmen").join("r.wav").exists(), "Aufnahmen werden nie gelöscht");
        assert_eq!((format_bytes(0), format_bytes(3000), format_bytes(2_500_000)), ("0 KB".to_string(), "3 KB".to_string(), "2.5 MB".to_string()));
    }

    #[test]
    fn prefs_roundtrip() {
        let dir = std::env::temp_dir().join("video-superbe-tests").join("prefs");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(Prefs::load(&dir), Prefs::default());
        let p = Prefs { pool_view: PoolView::Details, pool_sort: PoolSort::Size, pool_ascending: false, copy_imports_local: true, ..Default::default() };
        p.save(&dir);
        assert_eq!(Prefs::load(&dir), p);
    }
}
