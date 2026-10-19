//! H.265/HEVC (iPhone, GoPro) aus MP4/MOV: eigener Container-Leser und der eingebettete Decoder libde265
//! (LGPL, `vendor/libde265`, statisch gelinkt). Windows bringt für Programme ohne Installation keinen
//! HEVC-Decoder mit, deshalb wird hier selbst decodiert.

use crate::video::{Frame, Info};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::raw::{c_int, c_void};
use std::path::Path;

mod ffi {
    use std::os::raw::{c_int, c_void};
    unsafe extern "C" {
        pub fn de265_new_decoder() -> *mut c_void;
        pub fn de265_free_decoder(ctx: *mut c_void) -> c_int;
        pub fn de265_start_worker_threads(ctx: *mut c_void, n: c_int) -> c_int;
        pub fn de265_push_NAL(ctx: *mut c_void, data: *const c_void, len: c_int, pts: i64, user: *mut c_void) -> c_int;
        pub fn de265_flush_data(ctx: *mut c_void) -> c_int;
        pub fn de265_decode(ctx: *mut c_void, more: *mut c_int) -> c_int;
        pub fn de265_get_next_picture(ctx: *mut c_void) -> *const c_void;
        pub fn de265_release_next_picture(ctx: *mut c_void);
        pub fn de265_get_image_width(img: *const c_void, ch: c_int) -> c_int;
        pub fn de265_get_image_height(img: *const c_void, ch: c_int) -> c_int;
        pub fn de265_get_bits_per_pixel(img: *const c_void, ch: c_int) -> c_int;
        pub fn de265_get_image_plane(img: *const c_void, ch: c_int, stride: *mut c_int) -> *const u8;
        pub fn de265_get_image_PTS(img: *const c_void) -> i64;
        pub fn de265_get_image_full_range_flag(img: *const c_void) -> c_int;
        pub fn de265_get_image_matrix_coefficients(img: *const c_void) -> c_int;
        pub fn de265_get_image_transfer_characteristics(img: *const c_void) -> c_int;
    }
}

/// YUV→RGB-Umrechnung (Festkomma, 10 Bit) für Wertebereich (voll/eingeschränkt) und Farbmatrix
/// (1 = BT.709, 5/6 = BT.601, 9 = BT.2020; sonst nach Bildbreite: ab 1280 Pixel BT.709).
#[derive(Clone, Copy, Debug)]
pub struct Conv {
    yoff: i32,
    ys: i32,
    rv: i32,
    gu: i32,
    gv: i32,
    bu: i32,
    /// 0 = SDR, 1 = HLG (ARIB STD-B67), 2 = PQ (SMPTE ST 2084): wird auf SDR/BT.709 abgebildet
    hdr: u8,
}

/// Tabellen für die HDR→SDR-Abbildung: Signal (8 Bit) → linear, linear (12 Bit) → sRGB-Signal.
struct HdrLut {
    dec: [f32; 256],
    enc: Vec<u8>,
}

/// Referenzweiß (diffuses Weiß, 203 cd/m²) im SDR-Bild: knapp unter Weiß, damit Himmel und Wolken nicht ausbrennen.
const WHITE: f32 = 0.78;

fn tone(x: f32, lw: f32) -> f32 {
    const K: f32 = 0.7;
    if x <= K {
        x.max(0.0)
    } else if x >= lw {
        1.0
    } else {
        let t = (x - K) / (lw - K);
        K + (1.0 - K) * (1.0 - (1.0 - t) * (1.0 - t))
    }
}

fn hdr_lut(kind: u8) -> &'static HdrLut {
    static HLG: std::sync::OnceLock<HdrLut> = std::sync::OnceLock::new();
    static PQ: std::sync::OnceLock<HdrLut> = std::sync::OnceLock::new();
    let build = |kind: u8| {
        let mut dec = [0f32; 256];
        for (i, d) in dec.iter_mut().enumerate() {
            let e = i as f32 / 255.0;
            *d = if kind == 1 {
                // HLG: Umkehr der OETF → Szenenlicht; 0,265 entspricht Referenzweiß (75 % Signal, 203 cd/m²)
                const A: f32 = 0.178_832_77;
                const B: f32 = 0.284_668_92;
                const C: f32 = 0.559_910_73;
                let sc = if e <= 0.5 { e * e / 3.0 } else { (((e - C) / A).exp() + B) / 12.0 };
                let x = sc / 0.265;
                tone(x.powf(1.1) * WHITE, WHITE / 0.265)
            } else {
                // PQ: absolute Helligkeit, 203 cd/m² = Weiß, bis 1000 cd/m² Spitzenlicht
                const M1: f32 = 0.159_301_76;
                const M2: f32 = 78.843_75;
                const C1: f32 = 0.835_937_5;
                const C2: f32 = 18.851_563;
                const C3: f32 = 18.6875;
                let p = e.powf(1.0 / M2);
                let nits = 10000.0 * ((p - C1).max(0.0) / (C2 - C3 * p)).powf(1.0 / M1);
                tone(nits / 203.0 * WHITE, 1000.0 / 203.0 * WHITE)
            };
        }
        let enc = (0..4096)
            .map(|i| {
                let l = i as f32 / 4095.0;
                let v = if l <= 0.003_130_8 { 12.92 * l } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
                (v * 255.0 + 0.5).clamp(0.0, 255.0) as u8
            })
            .collect();
        HdrLut { dec, enc }
    };
    if kind == 1 { HLG.get_or_init(|| build(1)) } else { PQ.get_or_init(|| build(2)) }
}

impl Conv {
    /// Übertragungsfunktion aus der VUI (16 = PQ, 18 = HLG) aktivieren; andere Werte bleiben SDR.
    pub fn with_transfer(mut self, transfer: u8) -> Conv {
        self.hdr = match transfer {
            18 => 1,
            16 => 2,
            _ => 0,
        };
        self
    }

    #[inline]
    fn tone_map(&self, rgb: [u8; 3]) -> [u8; 3] {
        let t = hdr_lut(self.hdr);
        let (r, g, b) = (t.dec[rgb[0] as usize], t.dec[rgb[1] as usize], t.dec[rgb[2] as usize]);
        // BT.2020 → BT.709 im linearen Licht
        let o = [
            1.6605 * r - 0.5876 * g - 0.0728 * b,
            -0.1246 * r + 1.1329 * g - 0.0083 * b,
            -0.0182 * r - 0.1006 * g + 1.1187 * b,
        ];
        let e = |v: f32| t.enc[((v.clamp(0.0, 1.0)) * 4095.0 + 0.5) as usize];
        [e(o[0]), e(o[1]), e(o[2])]
    }

    pub fn new(full: bool, matrix: u8, width: usize) -> Conv {
        let m = match matrix {
            1 | 5 | 6 | 9 => matrix,
            _ => if width >= 1280 { 1 } else { 6 },
        };
        let (yoff, ys) = if full { (0, 1024) } else { (16, 1192) };
        let (rv, gu, gv, bu) = match (m, full) {
            (1, false) => (1836, 218, 546, 2163),
            (1, true) => (1613, 192, 479, 1900),
            (9, false) => (1719, 192, 666, 2193),
            (9, true) => (1510, 169, 585, 1927),
            (_, false) => (1634, 401, 833, 2066),
            (_, true) => (1436, 352, 731, 1815),
        };
        Conv { yoff, ys, rv, gu, gv, bu, hdr: 0 }
    }

    /// `y`, `u`, `v` als 8-Bit-Werte (0…255).
    #[inline]
    pub fn rgb(&self, y: i32, u: i32, v: i32) -> [u8; 3] {
        let c = (y - self.yoff) * self.ys;
        let (u, v) = (u - 128, v - 128);
        let rgb = [((c + self.rv * v) >> 10).clamp(0, 255) as u8, ((c - self.gu * u - self.gv * v) >> 10).clamp(0, 255) as u8, ((c + self.bu * u) >> 10).clamp(0, 255) as u8];
        if self.hdr == 0 { rgb } else { self.tone_map(rgb) }
    }
}

// ---------------------------------------------------------------------------------------------
// MP4/MOV (ISO-BMFF)
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Sample {
    pub offset: u64,
    pub size: u32,
    /// Anzeigezeit in Zeitskala-Einheiten (nach Abzug der Schnittliste); negativ = wird nicht angezeigt.
    pub pts: i64,
    pub sync: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Track {
    pub timescale: u32,
    pub duration_ticks: u64,
    pub rotation: u32,
    pub nal_len: usize,
    /// VPS/SPS/PPS aus `hvcC`
    pub params: Vec<Vec<u8>>,
    pub samples: Vec<Sample>,
    pub audio: bool,
}

fn be32(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(o..o + 4)?.try_into().ok()?))
}
fn be64(b: &[u8], o: usize) -> Option<u64> {
    Some(u64::from_be_bytes(b.get(o..o + 8)?.try_into().ok()?))
}
fn be16(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(o..o + 2)?.try_into().ok()?))
}

/// Teilt einen Bereich in aufeinanderfolgende Boxen (Typ, Inhalt).
fn boxes(data: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut out = vec![];
    let mut p = 0usize;
    while p + 8 <= data.len() {
        let mut size = be32(data, p).unwrap() as u64;
        let ty: [u8; 4] = data[p + 4..p + 8].try_into().unwrap();
        let mut head = 8usize;
        if size == 1 {
            let Some(s) = be64(data, p + 8) else { break };
            size = s;
            head = 16;
        } else if size == 0 {
            size = (data.len() - p) as u64;
        }
        if size < head as u64 || p as u64 + size > data.len() as u64 {
            break;
        }
        out.push((ty, &data[p + head..p + size as usize]));
        p += size as usize;
    }
    out
}

fn find<'a>(data: &'a [u8], ty: &[u8; 4]) -> Option<&'a [u8]> {
    boxes(data).into_iter().find(|(t, _)| t == ty).map(|(_, d)| d)
}

fn read_moov(f: &mut File) -> Result<Vec<u8>, String> {
    let len = f.metadata().map_err(|e| e.to_string())?.len();
    let mut pos = 0u64;
    while pos + 8 <= len {
        f.seek(SeekFrom::Start(pos)).map_err(|e| e.to_string())?;
        let mut h = [0u8; 16];
        let n = f.read(&mut h).map_err(|e| e.to_string())?;
        if n < 8 {
            break;
        }
        let mut size = u32::from_be_bytes(h[0..4].try_into().unwrap()) as u64;
        let mut head = 8u64;
        if size == 1 && n >= 16 {
            size = u64::from_be_bytes(h[8..16].try_into().unwrap());
            head = 16;
        } else if size == 0 {
            size = len - pos;
        }
        if size < head {
            break;
        }
        if &h[4..8] == b"moov" {
            let psize = size - head;
            if psize > 512 << 20 {
                return Err("Videokopf zu groß".into());
            }
            let mut buf = vec![0u8; psize as usize];
            f.seek(SeekFrom::Start(pos + head)).map_err(|e| e.to_string())?;
            f.read_exact(&mut buf).map_err(|e| e.to_string())?;
            return Ok(buf);
        }
        pos += size;
    }
    Err("Kein „moov“-Block: keine MP4/MOV-Datei".into())
}

/// Liest die erste HEVC-Videospur (`hvc1`/`hev1`). Fehler, wenn die Datei keine enthält.
pub fn parse_file(path: &Path) -> Result<Track, String> {
    let mut f = File::open(path).map_err(|e| format!("Datei nicht lesbar: {e}"))?;
    let moov = read_moov(&mut f)?;
    parse_moov(&moov)
}

pub fn parse_moov(moov: &[u8]) -> Result<Track, String> {
    let mut audio = false;
    let mut found: Option<Track> = None;
    for (ty, trak) in boxes(moov) {
        if &ty != b"trak" {
            continue;
        }
        let Some(mdia) = find(trak, b"mdia") else { continue };
        let handler = find(mdia, b"hdlr").and_then(|h| h.get(8..12)).map(|h| h.to_vec()).unwrap_or_default();
        if handler == b"soun" {
            audio = true;
            continue;
        }
        if handler != b"vide" || found.is_some() {
            continue;
        }
        if let Some(t) = parse_video_trak(trak, mdia) {
            found = Some(t);
        }
    }
    let mut t = found.ok_or("Keine HEVC-Videospur gefunden")?;
    t.audio = audio;
    Ok(t)
}

fn parse_video_trak(trak: &[u8], mdia: &[u8]) -> Option<Track> {
    let mut t = Track::default();
    // Drehung aus der Matrix von tkhd
    if let Some(tkhd) = find(trak, b"tkhd") {
        let m = if tkhd.first() == Some(&1) { 52 } else { 40 };
        let g = |i: usize| be32(tkhd, m + 4 * i).map(|v| v as i32);
        if let (Some(a), Some(b), Some(c), Some(d)) = (g(0), g(1), g(3), g(4)) {
            t.rotation = match (a.signum(), b.signum(), c.signum(), d.signum()) {
                (0, 1, -1, 0) => 90,
                (-1, 0, 0, -1) => 180,
                (0, -1, 1, 0) => 270,
                _ => 0,
            };
        }
    }
    let mdhd = find(mdia, b"mdhd")?;
    if mdhd.first() == Some(&1) {
        t.timescale = be32(mdhd, 20)?;
        t.duration_ticks = be64(mdhd, 24)?;
    } else {
        t.timescale = be32(mdhd, 12)?;
        t.duration_ticks = be32(mdhd, 16)? as u64;
    }
    if t.timescale == 0 {
        return None;
    }
    let stbl = find(find(mdia, b"minf")?, b"stbl")?;
    // Beschreibung: hvc1/hev1 + hvcC
    let stsd = find(stbl, b"stsd")?;
    let entries = boxes(stsd.get(8..)?);
    let (fmt, entry) = entries.first()?;
    if fmt != b"hvc1" && fmt != b"hev1" {
        return None;
    }
    let hvcc = find(entry.get(78..)?, b"hvcC")?;
    t.nal_len = (*hvcc.get(21)? & 3) as usize + 1;
    let arrays = *hvcc.get(22)? as usize;
    let mut p = 23;
    for _ in 0..arrays {
        let n = be16(hvcc, p + 1)? as usize;
        p += 3;
        for _ in 0..n {
            let l = be16(hvcc, p)? as usize;
            t.params.push(hvcc.get(p + 2..p + 2 + l)?.to_vec());
            p += 2 + l;
        }
    }

    // Tabellen
    let stsz = find(stbl, b"stsz")?;
    let fixed = be32(stsz, 4)?;
    let count = be32(stsz, 8)? as usize;
    let size_of = |i: usize| -> Option<u32> { if fixed != 0 { Some(fixed) } else { be32(stsz, 12 + 4 * i) } };
    // Offsets je Chunk
    let chunk_offsets: Vec<u64> = if let Some(co) = find(stbl, b"stco") {
        (0..be32(co, 4)? as usize).map(|i| be32(co, 8 + 4 * i).map(|v| v as u64)).collect::<Option<_>>()?
    } else {
        let co = find(stbl, b"co64")?;
        (0..be32(co, 4)? as usize).map(|i| be64(co, 8 + 8 * i)).collect::<Option<_>>()?
    };
    let stsc = find(stbl, b"stsc")?;
    let nstsc = be32(stsc, 4)? as usize;
    let runs: Vec<(u32, u32)> = (0..nstsc).map(|i| Some((be32(stsc, 8 + 12 * i)?, be32(stsc, 12 + 12 * i)?))).collect::<Option<_>>()?;
    let mut offsets = Vec::with_capacity(count);
    'outer: for (ci, &co) in chunk_offsets.iter().enumerate() {
        let chunk_no = ci as u32 + 1;
        let per = runs.iter().rev().find(|(first, _)| *first <= chunk_no).map(|r| r.1)?;
        let mut o = co;
        for _ in 0..per {
            if offsets.len() >= count {
                break 'outer;
            }
            offsets.push(o);
            o += size_of(offsets.len() - 1)? as u64;
        }
    }
    if offsets.len() != count {
        return None;
    }
    // Zeiten
    let stts = find(stbl, b"stts")?;
    let mut dts = Vec::with_capacity(count);
    let mut acc = 0i64;
    for e in 0..be32(stts, 4)? as usize {
        let (n, d) = (be32(stts, 8 + 8 * e)?, be32(stts, 12 + 8 * e)? as i64);
        for _ in 0..n {
            dts.push(acc);
            acc += d;
        }
    }
    dts.resize(count, acc);
    let mut cto = vec![0i64; count];
    if let Some(ctts) = find(stbl, b"ctts") {
        let mut i = 0usize;
        for e in 0..be32(ctts, 4)? as usize {
            let (n, o) = (be32(ctts, 8 + 8 * e)?, be32(ctts, 12 + 8 * e)?);
            // QuickTime-Dateien (iPhone) schreiben auch bei Version 0 vorzeichenbehaftete Werte
            let o = o as i32 as i64;
            for _ in 0..n {
                if i < count {
                    cto[i] = o;
                    i += 1;
                }
            }
        }
    }
    // Schnittliste: erster nicht-leerer Eintrag bestimmt den Startversatz (Medienzeit des ersten sichtbaren Bildes)
    let mut shift = 0i64;
    if let Some(elst) = find(find(trak, b"edts").unwrap_or(&[]), b"elst") {
        let v1 = elst.first() == Some(&1);
        for e in 0..be32(elst, 4).unwrap_or(0) as usize {
            let media_time = if v1 { be64(elst, 8 + 20 * e + 8).map(|v| v as i64) } else { be32(elst, 8 + 12 * e + 4).map(|v| v as i32 as i64) };
            if let Some(mt) = media_time {
                if mt >= 0 {
                    shift = mt;
                    break;
                }
            }
        }
    }
    let sync: Option<Vec<usize>> = find(stbl, b"stss").and_then(|s| (0..be32(s, 4)? as usize).map(|i| be32(s, 8 + 4 * i).map(|v| v as usize - 1)).collect());
    let mut is_sync = vec![sync.is_none(); count];
    if let Some(s) = sync {
        for i in s {
            if i < count {
                is_sync[i] = true;
            }
        }
    }
    t.samples = (0..count).map(|i| Some(Sample { offset: offsets[i], size: size_of(i)?, pts: dts[i] + cto[i] - shift, sync: is_sync[i] })).collect::<Option<_>>()?;
    Some(t)
}

// ---------------------------------------------------------------------------------------------
// Decoder
// ---------------------------------------------------------------------------------------------

pub struct HevcSource {
    file: File,
    pub track: Track,
    ctx: *mut c_void,
    /// Indizes der anzeigbaren Samples (pts ≥ 0), nach Anzeigezeit sortiert
    order: Vec<usize>,
    syncs: Vec<usize>,
    started: bool,
    /// nächstes zu fütterndes Sample (Decodierreihenfolge)
    fed: usize,
    /// Anzeigezeit des zuletzt aus dem Decoder geholten Bildes
    last_out_pts: i64,
    /// zuletzt ausgelieferte Anzeigezeit
    cur_pts: i64,
    ahead: Option<(i64, Frame)>,
    /// Abtastschritt der zwischengespeicherten Bilder
    step_used: usize,
    /// Hardware-Decoder (Grafikkarte); `None` = Software-Decoder libde265
    gpu: Option<crate::hevc_gpu::GpuDecoder>,
    gpu_pending: std::collections::BTreeMap<i64, Frame>,
    gpu_next: usize,
    gpu_started: bool,
    gpu_step: usize,
    pub info: Info,
    buf: Vec<u8>,
}

fn decode_all(ctx: *mut c_void) {
    unsafe {
        for _ in 0..10_000 {
            let mut more: c_int = 0;
            let err = ffi::de265_decode(ctx, &mut more);
            // Fehler (auch „warte auf Eingabedaten“) beenden die Schleife; `more == 0`: fertig.
            if err != 0 || more == 0 {
                break;
            }
        }
    }
}

impl HevcSource {
    pub fn open(path: &Path) -> Result<Self, String> {
        Self::open_with(path, std::env::var_os("VSB_HEVC_CPU").is_none())
    }

    /// `gpu`: Decodierung auf der Grafikkarte versuchen (sonst immer Software).
    pub fn open_with(path: &Path, gpu: bool) -> Result<Self, String> {
        let track = parse_file(path)?;
        let file = File::open(path).map_err(|e| e.to_string())?;
        let mut order: Vec<usize> = (0..track.samples.len()).filter(|&i| track.samples[i].pts >= 0).collect();
        order.sort_by_key(|&i| track.samples[i].pts);
        let syncs = (0..track.samples.len()).filter(|&i| track.samples[i].sync).collect();
        let mut s = HevcSource {
            file,
            track,
            ctx: std::ptr::null_mut(),
            order,
            syncs,
            started: false,
            fed: 0,
            last_out_pts: i64::MIN,
            cur_pts: i64::MIN,
            ahead: None,
            step_used: 1,
            gpu: None,
            gpu_pending: Default::default(),
            gpu_next: 0,
            gpu_started: false,
            gpu_step: 1,
            info: Info::default(),
            buf: vec![],
        };
        if s.order.is_empty() || s.syncs.is_empty() {
            return Err("Die Videospur enthält keine Bilder".into());
        }
        if gpu {
            s.gpu = crate::hevc_gpu::GpuDecoder::new(&s.track.params, s.track.nal_len);
        }
        // erstes Bild decodieren, um Größe und Format zu erfahren
        let first = s.frame_at(0.0, 1).ok_or("Erstes Bild nicht decodierbar")?;
        let (cw, ch) = (first.width, first.height);
        let rot = s.track.rotation;
        let (w, h) = if rot == 90 || rot == 270 { (ch, cw) } else { (cw, ch) };
        let n = s.order.len();
        let span = (s.track.samples[s.order[n - 1]].pts - s.track.samples[s.order[0]].pts) as f64 / s.track.timescale as f64;
        let fps = if n > 1 { (n - 1) as f64 / span.max(1e-6) } else { 30.0 };
        s.info = Info {
            duration: s.track.duration_ticks as f64 / s.track.timescale as f64,
            width: w as u32,
            height: h as u32,
            fps,
            has_video: true,
            has_audio: s.track.audio,
            rotation: 0, // Das gelieferte Bild ist bereits gedreht
            hdr: s.track.params.iter().filter(|n| n.first().is_some_and(|b| (b >> 1) & 0x3f == 33)).filter_map(|n| crate::hevc_gpu::parse_sps(n)).any(|sps| matches!(sps.transfer, 16 | 18)),
        };
        // Zustand zurücksetzen: das nächste Bild wird wieder vom Anfang geholt
        s.started = false;
        Ok(s)
    }

    /// Einmal pro Datei: GPU-Bild gegen Software-Bild vergleichen; bei zu großer Abweichung GPU abschalten.
    pub fn verify_gpu(&mut self, path: &Path) {
        use std::sync::{Mutex, OnceLock};
        static CACHE: OnceLock<Mutex<std::collections::HashMap<std::path::PathBuf, bool>>> = OnceLock::new();
        if self.gpu.is_none() {
            return;
        }
        let cache = CACHE.get_or_init(Default::default);
        let known = cache.lock().unwrap().get(path).copied();
        let ok = match known {
            Some(ok) => ok,
            None => {
                let t = (self.info.duration * 0.3).min(1.0);
                let gpu_frame = self.frame_at(t, 4);
                // Software-Bild an derselben Stelle
                let gpu = self.gpu.take();
                self.started = false;
                self.ahead = None;
                let cpu_frame = self.frame_at(t, 4);
                self.gpu = gpu;
                self.gpu_started = false;
                self.gpu_pending.clear();
                self.started = false;
                self.ahead = None;
                let ok = match (gpu_frame, cpu_frame) {
                    (Some(a), Some(b)) if a.rgba.len() == b.rgba.len() && (a.time - b.time).abs() < 1e-9 => {
                        let diff: f64 = a.rgba.iter().zip(&b.rgba).map(|(x, y)| (*x as i32 - *y as i32).unsigned_abs() as f64).sum::<f64>() / a.rgba.len() as f64;
                        crate::video::dbg(&format!("GPU-Vergleich {}: Abweichung {diff:.2}", path.display()));
                        diff < 6.0
                    }
                    _ => false,
                };
                cache.lock().unwrap().insert(path.to_path_buf(), ok);
                ok
            }
        };
        if !ok {
            self.gpu = None;
            self.gpu_pending.clear();
        }
    }

    /// true, wenn auf der Grafikkarte decodiert wird.
    pub fn uses_gpu(&self) -> bool {
        self.gpu.is_some()
    }

    fn restart(&mut self, from: usize) {
        unsafe {
            if !self.ctx.is_null() {
                ffi::de265_free_decoder(self.ctx);
            }
            self.ctx = ffi::de265_new_decoder();
            let threads = std::env::var("VSB_HEVC_THREADS").ok().and_then(|v| v.parse().ok()).unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).min(4));
            ffi::de265_start_worker_threads(self.ctx, threads as c_int);
            for nal in &self.track.params {
                ffi::de265_push_NAL(self.ctx, nal.as_ptr() as *const c_void, nal.len() as c_int, 0, std::ptr::null_mut());
            }
        }
        self.fed = from;
        self.started = true;
        self.last_out_pts = i64::MIN;
        self.ahead = None;
    }

    fn feed_one(&mut self) -> bool {
        let Some(s) = self.track.samples.get(self.fed).cloned() else { return false };
        self.buf.resize(s.size as usize, 0);
        if self.file.seek(SeekFrom::Start(s.offset)).is_err() || self.file.read_exact(&mut self.buf).is_err() {
            self.fed += 1;
            return true;
        }
        let nl = self.track.nal_len;
        let mut p = 0usize;
        unsafe {
            while p + nl <= self.buf.len() {
                let len = self.buf[p..p + nl].iter().fold(0usize, |a, &b| (a << 8) | b as usize);
                p += nl;
                if len == 0 || p + len > self.buf.len() {
                    break;
                }
                ffi::de265_push_NAL(self.ctx, self.buf[p..].as_ptr() as *const c_void, len as c_int, s.pts, std::ptr::null_mut());
                p += len;
            }
        }
        decode_all(self.ctx);
        self.fed += 1;
        true
    }

    fn convert(&self, img: *const c_void, time_ticks: i64, step: usize) -> Option<Frame> {
        unsafe {
            let (w, h) = (ffi::de265_get_image_width(img, 0) as usize, ffi::de265_get_image_height(img, 0) as usize);
            if w == 0 || h == 0 {
                return None;
            }
            let bits = ffi::de265_get_bits_per_pixel(img, 0).max(8) as u32;
            let wide = bits > 8;
            let shift = bits - 8;
            let full = ffi::de265_get_image_full_range_flag(img) != 0;
            let (mut sy, mut su, mut sv) = (0, 0, 0);
            let py = ffi::de265_get_image_plane(img, 0, &mut sy);
            let pu = ffi::de265_get_image_plane(img, 1, &mut su);
            let pv = ffi::de265_get_image_plane(img, 2, &mut sv);
            if py.is_null() {
                return None;
            }
            let (cw, ch) = (ffi::de265_get_image_width(img, 1) as usize, ffi::de265_get_image_height(img, 1) as usize);
            let chroma = !pu.is_null() && !pv.is_null() && cw > 0 && ch > 0;
            let (xs, ys) = if chroma { ((w / cw.max(1)).max(1), (h / ch.max(1)).max(1)) } else { (1, 1) };
            let (ow, oh) = (w / step, h / step);
            let mut out = vec![255u8; ow * oh * 4];
            let rd = |p: *const u8, stride: i32, x: usize, y: usize| -> i32 {
                if wide {
                    let q = p.add(y * stride as usize + x * 2) as *const u16;
                    (q.read_unaligned() >> shift) as i32
                } else {
                    *p.add(y * stride as usize + x) as i32
                }
            };
            let conv = Conv::new(full, ffi::de265_get_image_matrix_coefficients(img) as u8, w).with_transfer(ffi::de265_get_image_transfer_characteristics(img) as u8);
            for oy in 0..oh {
                let y = oy * step;
                for ox in 0..ow {
                    let x = ox * step;
                    let (u, v) = if chroma { (rd(pu, su, x / xs, y / ys), rd(pv, sv, x / xs, y / ys)) } else { (128, 128) };
                    let d = (oy * ow + ox) * 4;
                    out[d..d + 3].copy_from_slice(&conv.rgb(rd(py, sy, x, y), u, v));
                }
            }
            let (fw, fh, rgba) = crate::video::rotate(out, ow, oh, self.track.rotation);
            Some(Frame { width: fw, height: fh, rgba, time: time_ticks as f64 / self.track.timescale as f64 })
        }
    }

    fn frame_at_gpu(&mut self, ts: i64, step: usize, ti: usize, target_pts: i64, dt: i64) -> Result<Option<Frame>, String> {
        if step != self.gpu_step {
            self.gpu_step = step;
            self.gpu_pending.clear();
            self.gpu_started = false;
        }
        if let Some((_, f)) = self.gpu_pending.range(target_pts..=target_pts).next() {
            return Ok(Some(Frame { width: f.width, height: f.height, rgba: f.rgba.clone(), time: f.time }));
        }
        let continuing = self.gpu_started && ti >= self.gpu_next && ti - self.gpu_next <= 120;
        if !continuing {
            let s = match self.syncs.partition_point(|&s| s <= ti) {
                0 => self.syncs[0],
                n => self.syncs[n - 1],
            };
            self.gpu.as_mut().unwrap().reset();
            self.gpu_next = s;
            self.gpu_pending.clear();
            self.gpu_started = true;
        }
        let tick = self.track.timescale as f64;
        while self.gpu_next <= ti {
            let s = self.track.samples[self.gpu_next].clone();
            self.buf.resize(s.size as usize, 0);
            self.file.seek(SeekFrom::Start(s.offset)).map_err(|e| e.to_string())?;
            self.file.read_exact(&mut self.buf).map_err(|e| e.to_string())?;
            let gpu = self.gpu.as_mut().unwrap();
            let slot = gpu.decode_sample(&self.buf)?;
            self.gpu_next += 1;
            if let Some(slot) = slot {
                if s.pts + 3 * dt >= ts && s.pts >= 0 {
                    if let Some((w, h, px)) = gpu.read_rgba(slot, step) {
                        let (fw, fh, rgba) = crate::video::rotate(px, w, h, self.track.rotation);
                        self.gpu_pending.insert(s.pts, Frame { width: fw, height: fh, rgba, time: s.pts as f64 / tick });
                    }
                }
            }
        }
        // Zwischenspeicher aufräumen: nur Bilder in Zeitnähe behalten
        let floor = ts - 4 * dt;
        self.gpu_pending = self.gpu_pending.split_off(&floor);
        let f = self.gpu_pending.range(..=ts).next_back().map(|(_, f)| Frame { width: f.width, height: f.height, rgba: f.rgba.clone(), time: f.time });
        Ok(f)
    }

    /// Bild, das zur Quellzeit `t` (Sekunden) angezeigt wird. `step` = Abtastschritt (2 = halbe Auflösung).
    pub fn frame_at(&mut self, t: f64, step: usize) -> Option<Frame> {
        let ts = (t.max(0.0) * self.track.timescale as f64).round() as i64;
        let step = step.max(1);
        if step != self.step_used {
            // andere Auflösung gewünscht: Zwischenspeicher verwerfen, ab dem nächsten Sync-Bild neu aufsetzen
            self.step_used = step;
            self.ahead = None;
            self.started = false;
        }
        // Ziel: letztes Bild mit pts ≤ ts
        let pos = self.order.partition_point(|&i| self.track.samples[i].pts <= ts);
        let ti = self.order[pos.saturating_sub(1)];
        let target_pts = self.track.samples[ti].pts;
        let dt = (self.track.timescale as f64 / self.info.fps.max(1.0)).max(1.0) as i64;
        if self.gpu.is_some() {
            match self.frame_at_gpu(ts, step, ti, target_pts, dt) {
                Ok(f) => return f,
                Err(e) => {
                    crate::video::dbg(&format!("GPU-Decoder aufgegeben: {e}"));
                    self.gpu = None;
                    self.started = false;
                    self.ahead = None;
                }
            }
        }

        let floor_pts = self.ahead.as_ref().map(|a| a.0).unwrap_or(self.last_out_pts);
        let continuing = self.started && target_pts >= floor_pts && ti <= self.fed + 90;
        if !continuing {
            let s = match self.syncs.partition_point(|&s| s <= ti) {
                0 => self.syncs[0],
                n => self.syncs[n - 1],
            };
            self.restart(s);
        }

        let mut cand: Option<Frame> = None;
        if let Some((p, f)) = self.ahead.take() {
            if p <= ts {
                cand = Some(f);
            } else {
                self.ahead = Some((p, f));
            }
        }
        if self.ahead.is_none() {
            let mut flushed = false;
            'outer: loop {
                unsafe {
                    loop {
                        let img = ffi::de265_get_next_picture(self.ctx);
                        if img.is_null() {
                            break;
                        }
                        let pts = ffi::de265_get_image_PTS(img);
                        self.last_out_pts = pts;
                        if pts <= ts {
                            if pts + 2 * dt >= ts {
                                cand = self.convert(img, pts, step);
                                self.cur_pts = pts;
                            }
                            ffi::de265_release_next_picture(self.ctx);
                        } else {
                            let f = self.convert(img, pts, step);
                            ffi::de265_release_next_picture(self.ctx);
                            if let Some(f) = f {
                                self.ahead = Some((pts, f));
                            }
                            break 'outer;
                        }
                    }
                }
                if flushed {
                    break;
                }
                if !self.feed_one() {
                    unsafe {
                        ffi::de265_flush_data(self.ctx);
                    }
                    decode_all(self.ctx);
                    flushed = true;
                    self.started = false; // nach dem Leeren ist ein Neustart nötig
                }
            }
        }
        cand.or_else(|| self.ahead.as_ref().map(|a| a.1.clone_frame()))
    }
}

impl Drop for HevcSource {
    fn drop(&mut self) {
        if !self.ctx.is_null() {
            unsafe {
                ffi::de265_free_decoder(self.ctx);
            }
        }
    }
}

trait CloneFrame {
    fn clone_frame(&self) -> Frame;
}
impl CloneFrame for Frame {
    fn clone_frame(&self) -> Frame {
        Frame { width: self.width, height: self.height, rgba: self.rgba.clone(), time: self.time }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hdr_tone_mapping_keeps_black_and_white_and_is_monotonic() {
        for t in [18u8, 16] {
            let c = Conv::new(false, 9, 3840).with_transfer(t);
            assert_eq!(c.rgb(16, 128, 128), [0, 0, 0], "Schwarz bleibt Schwarz (Übertragung {t})");
            let mut last = 0;
            for y in (16..=235).step_by(5) {
                let g = c.rgb(y, 128, 128)[1];
                assert!(g >= last, "monoton bei Y={y}: {g} < {last}");
                last = g;
            }
            assert!(last >= 250, "Spitzenweiß wird (fast) weiß: {last}");
        }
        // HLG-Referenzweiß (75 % Signal) liegt im SDR knapp unter Weiß, nicht mehr als flaches Grau
        let c = Conv::new(false, 9, 3840).with_transfer(18);
        let w = c.rgb(16 + (219.0 * 0.75) as i32, 128, 128)[1];
        assert!((200..=250).contains(&w), "Referenzweiß {w}");
        // SDR bleibt unverändert
        assert_eq!(Conv::new(false, 1, 1920).with_transfer(1).rgb(126, 128, 128), Conv::new(false, 1, 1920).rgb(126, 128, 128));
    }

    // --- Hilfen zum Bauen eines winzigen MP4-Containers ---
    fn bx(ty: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        v.extend(ty);
        v.extend(payload);
        v
    }
    fn u32s(v: &[u32]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_be_bytes()).collect()
    }
    fn full(ver: u8, body: &[u8]) -> Vec<u8> {
        let mut v = vec![ver, 0, 0, 0];
        v.extend(body);
        v
    }

    /// Spur mit 6 Samples: 2 Chunks (4 + 2), B-Frame-Verzögerung per ctts, Schnittliste, IDR nur bei 0 und 4.
    fn synthetic_moov(rotation_matrix: [i32; 4], last_cto: u32) -> Vec<u8> {
        let mut tkhd = vec![0u8; 84];
        let m = 40;
        let mat = [rotation_matrix[0], rotation_matrix[1], 0, rotation_matrix[2], rotation_matrix[3], 0, 0, 0, 0x4000_0000];
        for (i, v) in mat.iter().enumerate() {
            tkhd[m + 4 * i..m + 4 * i + 4].copy_from_slice(&v.to_be_bytes());
        }
        let mdhd = full(0, &u32s(&[0, 0, 600, 3600, 0]));
        let hdlr = full(0, &[&u32s(&[0]), b"vide".as_slice(), &[0u8; 12]].concat());
        // hvcC: 21 Byte Kopf, lengthSizeMinusOne=3, 1 Array mit 1 NAL (Typ 33, 3 Byte)
        let mut hvcc = vec![0u8; 21];
        hvcc[0] = 1;
        hvcc.extend([0xFF, 1, 33, 0, 1, 0, 3, 0x42, 0x01, 0x01]);
        let mut entry_payload = vec![0u8; 78];
        entry_payload.extend(bx(b"hvcC", &hvcc));
        let mut stsd_body = u32s(&[1]);
        stsd_body.extend(bx(b"hvc1", &entry_payload));
        let stsd = bx(b"stsd", &full(0, &stsd_body));
        let stts = bx(b"stts", &full(0, &u32s(&[1, 6, 100])));
        // dts 0,100,…; Versätze 200,500,100,100,300,0 → pts 200,600,300,400,700,500 (vor Abzug der Schnittliste)
        let ctts = bx(b"ctts", &full(0, &u32s(&[6, 1, 200, 1, 500, 1, 100, 1, 100, 1, 300, 1, last_cto])));
        let stss = bx(b"stss", &full(0, &u32s(&[2, 1, 5])));
        let stsz = bx(b"stsz", &full(0, &u32s(&[0, 6, 10, 11, 12, 13, 14, 15])));
        let stsc = bx(b"stsc", &full(0, &u32s(&[2, 1, 4, 1, 2, 2, 1])));
        let stco = bx(b"stco", &full(0, &u32s(&[2, 1000, 5000])));
        let stbl = bx(b"stbl", &[stsd, stts, ctts, stss, stsz, stsc, stco].concat());
        let minf = bx(b"minf", &stbl);
        let mdia = bx(b"mdia", &[bx(b"mdhd", &mdhd), bx(b"hdlr", &hdlr), minf].concat());
        // Schnittliste: media_time = 200 (Verzögerung der ersten Anzeige)
        let elst = bx(b"elst", &full(0, &u32s(&[1, 3600, 200, 0x10000])));
        let edts = bx(b"edts", &elst);
        let trak = bx(b"trak", &[bx(b"tkhd", &tkhd), edts, mdia].concat());
        let atrak = bx(b"trak", &bx(b"mdia", &[bx(b"mdhd", &mdhd), bx(b"hdlr", &full(0, &[&u32s(&[0]), b"soun".as_slice(), &[0u8; 12]].concat()))].concat()));
        [trak, atrak].concat()
    }

    #[test]
    fn parses_sample_tables_chunks_and_edit_list() {
        let t = parse_moov(&synthetic_moov([0x10000, 0, 0, 0x10000], 0)).expect("parse");
        assert_eq!(t.timescale, 600);
        assert_eq!(t.nal_len, 4);
        assert_eq!(t.params, vec![vec![0x42, 0x01, 0x01]]);
        assert!(t.audio);
        assert_eq!(t.rotation, 0);
        assert_eq!(t.samples.len(), 6);
        // Chunk 1: Samples 0–3 ab 1000, Chunk 2: Samples 4–5 ab 5000
        let off: Vec<u64> = t.samples.iter().map(|s| s.offset).collect();
        assert_eq!(off, vec![1000, 1010, 1021, 1033, 5000, 5014]);
        assert_eq!(t.samples.iter().map(|s| s.size).collect::<Vec<_>>(), vec![10, 11, 12, 13, 14, 15]);
        // pts = dts + ctts − media_time(200)
        let pts: Vec<i64> = t.samples.iter().map(|s| s.pts).collect();
        assert_eq!(pts, vec![0, 400, 100, 200, 500, 300]);
        assert_eq!(t.samples.iter().map(|s| s.sync).collect::<Vec<_>>(), vec![true, false, false, false, true, false]);
    }

    #[test]
    fn negative_composition_offsets_in_version_0_are_signed() {
        // iPhone schreibt negative Versätze als vorzeichenlose 32-Bit-Zahl (0xFFFFFFF6 = −10)
        let t = parse_moov(&synthetic_moov([0x10000, 0, 0, 0x10000], 0xFFFF_FFF6)).unwrap();
        assert_eq!(t.samples[5].pts, 500 - 10 - 200);
    }

    #[test]
    fn detects_rotation_from_matrix() {
        for (m, rot) in [([0x10000, 0, 0, 0x10000], 0), ([0, 0x10000, -0x10000, 0], 90), ([-0x10000, 0, 0, -0x10000], 180), ([0, -0x10000, 0x10000, 0], 270)] {
            assert_eq!(parse_moov(&synthetic_moov(m, 0)).unwrap().rotation, rot);
        }
    }

    #[test]
    fn rejects_non_hevc_and_garbage() {
        assert!(parse_moov(&[]).is_err());
        assert!(parse_moov(&bx(b"trak", &bx(b"mdia", &[]))).is_err());
        let dir = std::env::temp_dir().join("video-superbe-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("not_a_video.mp4");
        std::fs::write(&f, b"hello world, not an mp4").unwrap();
        assert!(parse_file(&f).is_err());
        assert!(HevcSource::open(&f).is_err());
    }

    #[test]
    fn box_walker_handles_64bit_and_truncated_sizes() {
        let mut v = 1u32.to_be_bytes().to_vec();
        v.extend(b"free");
        v.extend(24u64.to_be_bytes());
        v.extend([7u8; 8]);
        v.extend(bx(b"abcd", &[1, 2, 3]));
        let b = boxes(&v);
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].1, &[7u8; 8]);
        assert_eq!(b[1].1, &[1, 2, 3]);
        let mut bad = bx(b"abcd", &[1, 2, 3]);
        bad[3] = 200; // behauptet mehr Bytes als vorhanden
        assert!(boxes(&bad).is_empty());
    }

    /// Echte HEVC-Dateien (iPhone/GoPro) aus dem Videos-Ordner des Entwicklers; ohne sie wird übersprungen.
    fn sample_files() -> Vec<std::path::PathBuf> {
        let Some(dir) = crate::app::videos_dir() else { return vec![] };
        let d = dir.join("test_vsp");
        let Ok(rd) = std::fs::read_dir(d) else { return vec![] };
        let mut v: Vec<_> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| parse_file(p).is_ok()).collect();
        v.sort();
        v
    }

    #[test]
    fn decodes_real_hevc_files() {
        let files = sample_files();
        if files.is_empty() {
            eprintln!("keine HEVC-Beispieldateien, Test übersprungen");
            return;
        }
        for f in files {
            let mut s = HevcSource::open(&f).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
            assert!(s.info.width > 100 && s.info.height > 100 && s.info.duration > 0.5, "{:?}", s.info);
            let mid = (s.info.duration / 2.0).min(3.0);
            let a = s.frame_at(0.0, 2).expect("Bild bei 0 s");
            let b = s.frame_at(mid, 2).expect("Bild in der Mitte");
            for fr in [&a, &b] {
                assert_eq!(fr.rgba.len(), fr.width * fr.height * 4);
                let lum: f64 = fr.rgba.chunks(4).map(|p| p[0] as f64 + p[1] as f64 + p[2] as f64).sum::<f64>() / (3.0 * (fr.rgba.len() / 4) as f64);
                assert!(lum > 2.0 && lum < 253.0, "Bild nicht leer/uniform: {lum}");
            }
            assert!((b.time - mid).abs() < 0.2, "Zeit {} statt {mid}", b.time);
            assert!(a.rgba != b.rgba, "unterschiedliche Bilder zu unterschiedlichen Zeiten");
        }
    }

    #[test]
    fn sequential_and_seeked_frames_are_identical() {
        let files = sample_files();
        let Some(f) = files.first() else { return };
        let mut seq = HevcSource::open(f).unwrap();
        let dt = 1.0 / seq.info.fps;
        // sequentiell zu t = 1,0 s laufen
        let mut t = 0.0;
        let mut last = None;
        while t <= 1.0 + 1e-9 {
            last = seq.frame_at(t, 2);
            t += dt;
        }
        let a = last.unwrap();
        // frischer Decoder springt direkt dorthin
        let mut jump = HevcSource::open(f).unwrap();
        let b = jump.frame_at(t - dt, 2).unwrap();
        assert!((a.time - b.time).abs() < 1e-9, "{} vs {}", a.time, b.time);
        assert!(a.rgba == b.rgba, "Springen und sequentielles Decodieren liefern dasselbe Bild");
        // rückwärts springen
        let c = seq.frame_at(0.2, 2).unwrap();
        let d = jump.frame_at(0.2, 2).unwrap();
        assert!(c.rgba == d.rgba && (c.time - d.time).abs() < 1e-9);
    }

    #[test]
    fn gpu_and_software_decoders_agree() {
        let files = sample_files();
        if files.is_empty() {
            return;
        }
        for f in files {
            let mut cpu = HevcSource::open_with(&f, false).unwrap();
            let mut gpu = HevcSource::open_with(&f, true).unwrap();
            assert!(!cpu.uses_gpu());
            if !gpu.uses_gpu() {
                eprintln!("{}: keine Hardware-Decodierung verfügbar, Vergleich übersprungen", f.display());
                continue;
            }
            for t in [0.0, 0.7, 2.0, 0.3] {
                let a = cpu.frame_at(t, 2).expect("CPU-Bild");
                let b = gpu.frame_at(t, 2).expect("GPU-Bild");
                assert_eq!((a.width, a.height), (b.width, b.height), "Größe");
                assert!((a.time - b.time).abs() < 1e-9, "dasselbe Bild gewählt: {} vs {}", a.time, b.time);
                let diff: f64 = a.rgba.iter().zip(&b.rgba).map(|(x, y)| (*x as i32 - *y as i32).unsigned_abs() as f64).sum::<f64>() / a.rgba.len() as f64;
                assert!(diff < 2.5, "{} bei t={t}: mittlere Abweichung {diff:.2}", f.display());
            }
        }
    }
}
