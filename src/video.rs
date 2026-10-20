//! Video-Decodierung über Windows Media Foundation (IMFSourceReader, in Windows eingebaut).

use std::path::Path;
use windows::core::{GUID, HSTRING, PCWSTR};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};

const VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
const AUDIO: u32 = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
const ROTATION: GUID = GUID::from_u128(0xC380465D_2271_428C_9B83_ECEA3B4A85C1);

pub(crate) fn dbg(m: &str) {
    if std::env::var_os("VSB_TRACE").is_some() {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("mvd-trace.log") {
            let _ = writeln!(f, "{m}");
        }
    }
}

/// Direct3D-11-Gerät mit DXGI-Geräte-Manager für Hardware-Decoder (DXVA).
fn make_d3d_manager() -> Option<IMFDXGIDeviceManager> {
    use windows::core::Interface;
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
    use windows::Win32::Graphics::Direct3D11::*;
    unsafe {
        let mut dev: Option<ID3D11Device> = None;
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_VIDEO_SUPPORT | D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut dev),
            None,
            None,
        )
        .ok()?;
        let dev = dev?;
        if let Ok(mt) = dev.cast::<ID3D11Multithread>() {
            let _ = mt.SetMultithreadProtected(true);
        }
        let mut token = 0u32;
        let mut mgr: Option<IMFDXGIDeviceManager> = None;
        MFCreateDXGIDeviceManager(&mut token, &mut mgr).ok()?;
        let mgr = mgr?;
        mgr.ResetDevice(&dev, token).ok()?;
        Some(mgr)
    }
}

pub fn init() {
    unsafe {
        // winit (Drag & Drop, OleInitialize) verlangt ein Single-Thread-Apartment.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let _ = MFStartup(MF_VERSION, MFSTARTUP_FULL);
    }
}

#[derive(Clone, Debug, Default)]
pub struct Info {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub has_video: bool,
    pub has_audio: bool,
    pub rotation: u32,
    /// HDR (HLG oder PQ): wird beim Decodieren in SDR umgerechnet
    pub hdr: bool,
}

pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
    pub time: f64,
}

pub struct Decoder {
    reader: Option<IMFSourceReader>,
    /// Eigener H.265-Decoder, wenn Windows das Video nicht decodieren kann.
    hevc: Option<Box<crate::hevc::HevcSource>>,
    pub info: Info,
    stride_w: usize,
    stride_h: usize,
    stride: usize,
    /// Ausgabe ist RGB32 (BGRA) statt NV12 (Direct3D-Pfad); `flip`: Zeilen stehen unten-oben.
    rgb: bool,
    flip: bool,
    /// Abtastschritt für die Vorschau (1 = volle Auflösung).
    pub step: usize,
    pub last: Option<Frame>,
    /// Zeit, ab der die Quelle nach dem letzten Lesen weiterläuft (für sequentielles Lesen).
    next_time: f64,
    eof: bool,
}

fn pack_u32_pair(v: u64) -> (u32, u32) {
    ((v >> 32) as u32, v as u32)
}

pub fn probe(path: &Path) -> Result<Info, String> {
    Decoder::open(path).map(|d| d.info)
}

impl Decoder {
    /// Wie `open`, prüft bei HEVC zusätzlich einmal pro Datei, ob die Grafikkarte dasselbe Bild liefert wie der
    /// Software-Decoder (Schutz vor Treiberbesonderheiten); bei Abweichung wird die Software-Decodierung benutzt.
    pub fn open_checked(path: &Path) -> Result<Self, String> {
        let mut d = Self::open(path)?;
        if let Some(h) = d.hevc.as_mut() {
            h.verify_gpu(path);
            let step = d.step;
            d.last = h.frame_at(0.0, step);
            d.next_time = d.last.as_ref().map_or(-1.0, |f| f.time);
        }
        Ok(d)
    }

    pub fn open(path: &Path) -> Result<Self, String> {
        init();
        let mut last_err = String::new();
        // H.265 (HEVC) in MP4/MOV: eigener Weg über die Grafikkarte bzw. libde265, ohne Umweg über Media Foundation
        let is_hevc = crate::hevc::parse_file(path).is_ok();
        // Sonst erst Software-Decoder; schlägt das erste Bild fehl, mit Hardware-Transforms (DXVA) erneut versuchen.
        for (hw, d3d) in [(false, false), (true, false), (true, true)] {
            if is_hevc {
                break;
            }
            match Self::open_with(path, hw, d3d) {
                Ok(mut d) => match d.read_one() {
                    Some(f) => {
                        d.next_time = f.time;
                        d.last = Some(f);
                        return Ok(d);
                    }
                    None => last_err = "Erstes Bild nicht decodierbar".into(),
                },
                Err(e) => last_err = e,
            }
        }
        // Windows kann das Video nicht decodieren (typisch HEVC): eigener Decoder
        match crate::hevc::HevcSource::open(path) {
            Ok(mut h) => {
                let step = if h.info.width > 1280 { 2 } else { 1 };
                let first = h.frame_at(0.0, step);
                let next_time = first.as_ref().map_or(-1.0, |f| f.time);
                Ok(Decoder {
                    reader: None,
                    info: h.info.clone(),
                    hevc: Some(Box::new(h)),
                    stride_w: 0,
                    stride_h: 0,
                    stride: 0,
                    rgb: false,
                    flip: false,
                    step,
                    last: first,
                    next_time,
                    eof: false,
                })
            }
            Err(he) => Err(if he.starts_with("Keine HEVC") { last_err } else if is_hevc { format!("Format (HEVC): {he}") } else { format!("{last_err}; HEVC: {he}") }),
        }
    }

    /// `hw`: Hardware-Decoder zulassen; `d3d`: über Direct3D (DXVA) der Grafikkarte decodieren, Ausgabe als RGB32.
    /// Nötig z. B. für HEVC/H.265 von iPhone und GoPro, wenn Windows keinen Software-Decoder dafür hat.
    fn open_with(path: &Path, hw: bool, d3d: bool) -> Result<Self, String> {
        unsafe {
            let mut attrs: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut attrs, 4).map_err(|e| e.to_string())?;
            let attrs = attrs.ok_or("Attribute")?;
            if d3d {
                let mgr = make_d3d_manager().ok_or("Keine Direct3D-Grafikbeschleunigung verfügbar")?;
                attrs.SetUnknown(&MF_SOURCE_READER_D3D_MANAGER, &mgr).map_err(|e| e.to_string())?;
                attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1).map_err(|e| e.to_string())?;
            } else {
                attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1).map_err(|e| e.to_string())?;
            }
            attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, hw as u32).map_err(|e| e.to_string())?;
            let wpath = HSTRING::from(path.as_os_str());
            let reader = MFCreateSourceReaderFromURL(PCWSTR(wpath.as_ptr()), &attrs)
                .map_err(|e| format!("Datei nicht lesbar: {e}"))?;

            let mut info = Info::default();
            // Dauer
            if let Ok(v) = reader.GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION) {
                if let Ok(d) = u64::try_from(&v) {
                    info.duration = d as f64 / 1e7;
                }
            }
            info.has_audio = reader.GetNativeMediaType(AUDIO, 0).is_ok();
            let native = reader.GetNativeMediaType(VIDEO, 0);
            info.has_video = native.is_ok();
            if let Ok(nt) = native {
                info.rotation = nt.GetUINT32(&ROTATION).unwrap_or(0);
            }
            if !info.has_video {
                return Err("Kein Videostrom".into());
            }
            // Ausgabeformat: RGB32
            let mt = MFCreateMediaType().map_err(|e| e.to_string())?;
            mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).map_err(|e| e.to_string())?;
            mt.SetGUID(&MF_MT_SUBTYPE, if d3d { &MFVideoFormat_RGB32 } else { &MFVideoFormat_NV12 }).map_err(|e| e.to_string())?;
            let sub = reader.GetNativeMediaType(VIDEO, 0).ok().and_then(|n| n.GetGUID(&MF_MT_SUBTYPE).ok());
            reader.SetCurrentMediaType(VIDEO, None, &mt).map_err(|e| {
                let cc = sub.map(|g| String::from_utf8_lossy(&g.data1.to_le_bytes()).to_string()).unwrap_or_default();
                format!("Format ({cc}): {e}")
            })?;
            let cur = reader.GetCurrentMediaType(VIDEO).map_err(|e| e.to_string())?;
            let (w, h) = pack_u32_pair(cur.GetUINT64(&MF_MT_FRAME_SIZE).map_err(|e| e.to_string())?);
            let (fn_, fd) = pack_u32_pair(cur.GetUINT64(&MF_MT_FRAME_RATE).unwrap_or((30u64 << 32) | 1));
            info.fps = if fd > 0 { fn_ as f64 / fd as f64 } else { 30.0 };
            let (dw, dh) = if info.rotation == 90 || info.rotation == 270 { (h, w) } else { (w, h) };
            info.width = dw;
            info.height = dh;
            reader.SetStreamSelection(AUDIO, false).ok();
            Ok(Decoder {
                reader: Some(reader),
                hevc: None,
                info,
                stride_w: w as usize,
                stride_h: h as usize,
                step: if w > 1280 { 2 } else { 1 },
                stride: (cur.GetUINT32(&MF_MT_DEFAULT_STRIDE).map(|v| (v as i32).unsigned_abs() as usize).unwrap_or(w as usize)).max(w as usize),
                rgb: d3d,
                flip: d3d && cur.GetUINT32(&MF_MT_DEFAULT_STRIDE).map(|v| (v as i32) < 0).unwrap_or(false),
                last: None,
                next_time: -1.0,
                eof: false,
            })
        }
    }

    fn read_one(&mut self) -> Option<Frame> {
        unsafe {
            dbg("read_one: start");
            let (mut flags, mut ts) = (0u32, 0i64);
            let mut sample: Option<IMFSample> = None;
            let Some(reader) = self.reader.as_ref() else { return None };
            let r = reader.ReadSample(VIDEO, 0, None, Some(&mut flags), Some(&mut ts), Some(&mut sample));
            if let Err(e) = &r {
                dbg(&format!("read_one: ReadSample error {e}"));
            }
            r.ok()?;
            dbg(&format!("read_one: ReadSample done flags={flags:#x} ts={ts} sample={}", sample.is_some()));
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                self.eof = true;
            }
            let sample = sample?;
            let buf = sample.ConvertToContiguousBuffer().ok()?;
            dbg("read_one: buffer ok");
            let (mut p, mut len) = (std::ptr::null_mut::<u8>(), 0u32);
            buf.Lock(&mut p, None, Some(&mut len)).ok()?;
            let (w, h, st) = (self.stride_w, self.stride_h, self.stride);
            let src = std::slice::from_raw_parts(p, len as usize);
            // NV12: Y-Ebene (h Zeilen), danach interleaved UV (h/2 Zeilen), BT.709, Limited Range.
            // Vorschau: ab 1280 px Breite wird jedes zweite Pixel gelesen (4× weniger Rechenaufwand).
            let step = self.step;
            let (ow, oh) = (w / step, h / step);
            let mut out = vec![255u8; ow * oh * 4];
            // Der Decoder liefert oft eine auf 16 Zeilen aufgerundete Höhe; die UV-Ebene beginnt dahinter.
            let ph = (src.len() * 2 / (3 * st)).max(h);
            if self.rgb {
                let rs = (src.len() / h.max(1)).max(w * 4);
                for oy in 0..oh {
                    let y = if self.flip { h - 1 - oy * step } else { oy * step };
                    let Some(row) = src.get(y * rs..y * rs + w * 4) else { break };
                    let drow = &mut out[oy * ow * 4..(oy + 1) * ow * 4];
                    for ox in 0..ow {
                        let x = ox * step * 4;
                        drow[ox * 4] = row[x + 2];
                        drow[ox * 4 + 1] = row[x + 1];
                        drow[ox * 4 + 2] = row[x];
                    }
                }
            } else if src.len() >= st * h * 3 / 2 {
                let (yp, uv) = src.split_at(st * ph);
                for oy in 0..oh {
                    let y = oy * step;
                    let yrow = &yp[y * st..y * st + w];
                    let uvrow = &uv[(y / 2) * st..(y / 2) * st + w];
                    let drow = &mut out[oy * ow * 4..(oy + 1) * ow * 4];
                    for ox in 0..ow {
                        let x = ox * step;
                        let c = (yrow[x] as i32 - 16) * 1192;
                        let u = uvrow[x & !1] as i32 - 128;
                        let v = uvrow[(x & !1) + 1] as i32 - 128;
                        let d = ox * 4;
                        drow[d] = ((c + 1836 * v) >> 10).clamp(0, 255) as u8;
                        drow[d + 1] = ((c - 218 * u - 546 * v) >> 10).clamp(0, 255) as u8;
                        drow[d + 2] = ((c + 2163 * u) >> 10).clamp(0, 255) as u8;
                    }
                }
            }
            let (w, h) = (ow, oh);
            let _ = buf.Unlock();
            let (fw, fh, rgba) = rotate(out, w, h, self.info.rotation);
            Some(Frame { width: fw, height: fh, rgba, time: ts as f64 / 1e7 })
        }
    }

    /// Liefert das Bild, das zur Quellzeit `t` gehört (Sekunden).
    pub fn frame_at(&mut self, t: f64) -> Option<&Frame> {
        let t = t.max(0.0);
        let frame_dt = 1.0 / self.info.fps.max(1.0);
        if let Some(h) = self.hevc.as_mut() {
            if let Some(f) = &self.last {
                if t >= f.time && t < f.time + frame_dt {
                    return self.last.as_ref();
                }
            }
            if let Some(f) = h.frame_at(t, self.step) {
                self.last = Some(f);
            }
            return self.last.as_ref();
        }
        let have = self.last.as_ref().map(|f| f.time);
        let sequential = matches!(have, Some(ft) if t >= ft && t - ft < 0.5) && !self.eof;
        if let Some(ft) = have {
            if t >= ft && t < ft + frame_dt && self.next_time >= 0.0 {
                return self.last.as_ref();
            }
        }
        let at_start = have.is_none() && t < 0.001 && self.next_time < 0.0;
        if !sequential && !at_start {
            unsafe {
                let v = windows::Win32::System::Com::StructuredStorage::PROPVARIANT::from((t * 1e7) as i64);
                if self.reader.as_ref().is_none_or(|r| r.SetCurrentPosition(&GUID::zeroed(), &v).is_err()) {
                    return self.last.as_ref();
                }
            }
            self.eof = false;
            self.last = None;
        }
        let mut guard = 0;
        loop {
            match self.read_one() {
                Some(f) => {
                    let ft = f.time;
                    self.last = Some(f);
                    self.next_time = ft;
                    if ft + frame_dt > t {
                        break;
                    }
                }
                None => break,
            }
            guard += 1;
            if guard > 400 || self.eof {
                break;
            }
        }
        self.last.as_ref()
    }
}

pub(crate) fn rotate(src: Vec<u8>, w: usize, h: usize, rot: u32) -> (usize, usize, Vec<u8>) {
    match rot {
        90 => {
            let mut o = vec![0u8; src.len()];
            for y in 0..h {
                for x in 0..w {
                    let (nx, ny) = (h - 1 - y, x);
                    o[(ny * h + nx) * 4..(ny * h + nx) * 4 + 4].copy_from_slice(&src[(y * w + x) * 4..(y * w + x) * 4 + 4]);
                }
            }
            (h, w, o)
        }
        180 => {
            let mut o = vec![0u8; src.len()];
            for i in 0..w * h {
                let j = w * h - 1 - i;
                o[j * 4..j * 4 + 4].copy_from_slice(&src[i * 4..i * 4 + 4]);
            }
            (w, h, o)
        }
        270 => {
            let mut o = vec![0u8; src.len()];
            for y in 0..h {
                for x in 0..w {
                    let (nx, ny) = (y, w - 1 - x);
                    o[(ny * h + nx) * 4..(ny * h + nx) * 4 + 4].copy_from_slice(&src[(y * w + x) * 4..(y * w + x) * 4 + 4]);
                }
            }
            (h, w, o)
        }
        _ => (w, h, src),
    }
}

/// Diagnose: listet die Media-Foundation-Decoder für einen Codec (`H264`, `HEVC`, …).
pub fn list_decoders(fourcc: &str) -> String {
    use windows::core::Interface;
    init();
    let mut out = String::new();
    let b = fourcc.as_bytes();
    let sub = GUID::from_u128(0x00000000_0000_0010_8000_00AA00389B71 | ((u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as u128) << 96));
    unsafe {
        for (name, flags) in [("alle", MFT_ENUM_FLAG_ALL.0 as u32), ("Software", MFT_ENUM_FLAG_SYNCMFT.0 as u32), ("Hardware", MFT_ENUM_FLAG_HARDWARE.0 as u32)] {
            let input = MFT_REGISTER_TYPE_INFO { guidMajorType: MFMediaType_Video, guidSubtype: sub };
            let mut acts: *mut Option<IMFActivate> = std::ptr::null_mut();
            let mut n = 0u32;
            let r = MFTEnumEx(MFT_CATEGORY_VIDEO_DECODER, MFT_ENUM_FLAG(flags as i32), Some(&input), None, &mut acts, &mut n);
            out += &format!("{name}: Ergebnis {:?}, {n} Decoder
", r.is_ok());
            for i in 0..n as usize {
                if let Some(a) = &*acts.add(i) {
                    let mut len = 0u32;
                    let mut buf = windows::core::PWSTR::null();
                    if a.GetAllocatedString(&MFT_FRIENDLY_NAME_Attribute, &mut buf, &mut len).is_ok() {
                        out += &format!("  - {}
", buf.to_string().unwrap_or_default());
                    }
                }
            }
            let _ = acts.is_null();
        }
    }
    let _ = |g: &GUID| g.to_u128();
    out
}

/// Decoder in einem Hintergrund-Thread: Die Oberfläche fragt nur nach dem passenden Bild und blockiert nie.
/// Der Thread decodiert zusätzlich ein paar Bilder voraus, damit die Wiedergabe gleichmäßig läuft, auch wenn
/// einzelne Bilder länger brauchen.
pub struct AsyncDecoder {
    tx: std::sync::mpsc::Sender<f64>,
    shared: std::sync::Arc<std::sync::Mutex<AsyncShared>>,
    /// Zählt jedes neu bereitgestellte Bild (die Vorschau rechnet dann neu).
    version: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

#[derive(Default)]
struct AsyncShared {
    frames: std::collections::VecDeque<std::sync::Arc<Frame>>,
    error: Option<String>,
    info: Option<Info>,
}

const PREFETCH_FRAMES: usize = 10;

impl AsyncDecoder {
    /// Öffnet `path` im Hintergrund; `wake` wird aufgerufen, sobald ein neues Bild bereitsteht.
    pub fn spawn(path: std::path::PathBuf, wake: impl Fn() + Send + 'static) -> AsyncDecoder {
        use std::sync::Arc;
        let (tx, rx) = std::sync::mpsc::channel::<f64>();
        let shared = Arc::new(std::sync::Mutex::new(AsyncShared::default()));
        let sh = shared.clone();
        let version = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let ver = version.clone();
        let wake = move || {
            ver.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            wake();
        };
        std::thread::spawn(move || {
            let mut dec = match Decoder::open_checked(&path) {
                Ok(d) => d,
                Err(e) => {
                    sh.lock().unwrap().error = Some(e);
                    wake();
                    return;
                }
            };
            sh.lock().unwrap().info = Some(dec.info.clone());
            let dt = (1.0 / dec.info.fps.max(1.0)).clamp(1.0 / 120.0, 0.2);
            let Ok(mut t) = rx.recv() else { return };
            loop {
                while let Ok(n) = rx.try_recv() {
                    t = n; // nur die neueste Anfrage zählt
                }
                let (covered, newest, len) = {
                    let g = sh.lock().unwrap();
                    (g.frames.iter().any(|f| f.time <= t && t < f.time + dt * 1.01), g.frames.back().map(|f| f.time), g.frames.len())
                };
                let want = if !covered {
                    Some((t, false))
                } else if len < PREFETCH_FRAMES && newest.is_some_and(|n| n < t + dt * PREFETCH_FRAMES as f64) {
                    Some((newest.unwrap() + 1.5 * dt, true))
                } else {
                    None
                };
                match want {
                    None => match rx.recv() {
                        Ok(n) => t = n,
                        Err(_) => return,
                    },
                    Some((wt, prefetch)) => {
                        if let Some(f) = dec.frame_at(wt) {
                            let copy = Arc::new(Frame { width: f.width, height: f.height, rgba: f.rgba.clone(), time: f.time });
                            let mut g = sh.lock().unwrap();
                            let newest = g.frames.back().map(|f| f.time);
                            if newest.is_some_and(|n| copy.time > n) && (prefetch || copy.time - newest.unwrap() < 1.0) {
                                g.frames.push_back(copy);
                            } else if newest.is_none_or(|n| copy.time != n) {
                                g.frames.clear(); // Sprung: alte Bilder verwerfen
                                g.frames.push_back(copy);
                            }
                            while g.frames.front().is_some_and(|f| f.time < t - 0.5) && g.frames.len() > 1 {
                                g.frames.pop_front();
                            }
                            drop(g);
                            wake();
                        } else if prefetch {
                            // Ende des Videos erreicht: nicht weiter voraus lesen
                            match rx.recv() {
                                Ok(n) => t = n,
                                Err(_) => return,
                            }
                        }
                    }
                }
            }
        });
        AsyncDecoder { tx, shared, version }
    }

    pub fn version(&self) -> u64 {
        self.version.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Wunschzeit (Quellzeit in Sekunden) setzen.
    pub fn request(&self, t: f64) {
        let _ = self.tx.send(t);
    }

    /// Das zur Zeit `t` passende fertige Bild (das späteste mit `time ≤ t`, sonst das früheste vorhandene).
    pub fn frame_for(&self, t: f64) -> Option<std::sync::Arc<Frame>> {
        let g = self.shared.lock().unwrap();
        g.frames.iter().rev().find(|f| f.time <= t + 1e-9).or_else(|| g.frames.front()).cloned()
    }

    pub fn error(&self) -> Option<String> {
        self.shared.lock().unwrap().error.clone()
    }

    pub fn info(&self) -> Option<Info> {
        self.shared.lock().unwrap().info.clone()
    }
}

/// Diagnose: Direct3D-11-Video-Decoder-Profile der Grafikkarte (für Hardware-Decodierung von HEVC).
pub fn gpu_info() -> String {
    use windows::core::Interface;
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
    use windows::Win32::Graphics::Direct3D11::*;
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_FORMAT_P010};
    let mut out = String::new();
    unsafe {
        let mut dev: Option<ID3D11Device> = None;
        if D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), D3D11_CREATE_DEVICE_VIDEO_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut dev), None, None).is_err() {
            return "Kein Direct3D-11-Gerät".into();
        }
        let dev = dev.unwrap();
        let Ok(vd) = dev.cast::<ID3D11VideoDevice>() else { return "Kein ID3D11VideoDevice".into() };
        let n = vd.GetVideoDecoderProfileCount();
        out += &format!("{n} Decoder-Profile
");
        for i in 0..n {
            if let Ok(g) = vd.GetVideoDecoderProfile(i) {
                let tag = if g == D3D11_DECODER_PROFILE_HEVC_VLD_MAIN { " <- HEVC Main" } else if g == D3D11_DECODER_PROFILE_HEVC_VLD_MAIN10 { " <- HEVC Main10" } else { "" };
                out += &format!("  {g:?}{tag}
");
            }
        }
        for (name, g, fmt) in [("Main/NV12", D3D11_DECODER_PROFILE_HEVC_VLD_MAIN, DXGI_FORMAT_NV12), ("Main10/P010", D3D11_DECODER_PROFILE_HEVC_VLD_MAIN10, DXGI_FORMAT_P010)] {
            let ok = vd.CheckVideoDecoderFormat(&g, fmt).map(|b| b.as_bool()).unwrap_or(false);
            out += &format!("HEVC {name}: {}
", if ok { "unterstützt" } else { "nicht unterstützt" });
        }
    }
    out
}
