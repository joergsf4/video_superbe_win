//! Audio-Decodierung (Media Foundation → 48 kHz, Stereo, f32) und Mischen des Projekttons.

use crate::model::*;
use std::collections::HashMap;
use std::path::Path;
use uuid::Uuid;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Media::MediaFoundation::*;

pub const RATE: i64 = 48000;
const AUDIO: u32 = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
const VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

pub struct AudioDecoder {
    reader: IMFSourceReader,
    /// Decodierte Frames (Stereo interleaved), `buf_start` = Frame-Index des ersten Eintrags.
    buf: Vec<f32>,
    buf_start: i64,
    /// Frame, bei dem das nächste `read` ohne Seek weitermacht.
    pos: Option<i64>,
    eof: bool,
}

impl AudioDecoder {
    pub fn open(path: &Path) -> Result<Self, String> {
        Self::open_format(path, RATE as u32, 2)
    }

    /// `rate`/`channels`: Zielformat der decodierten Daten (Stereo 48 kHz für die Wiedergabe, Mono 8 kHz für Wellenformen).
    pub fn open_format(path: &Path, rate: u32, channels: u32) -> Result<Self, String> {
        crate::video::init();
        unsafe {
            let wpath = HSTRING::from(path.as_os_str());
            let reader = MFCreateSourceReaderFromURL(PCWSTR(wpath.as_ptr()), None)
                .map_err(|e| format!("Datei nicht lesbar: {e}"))?;
            reader.GetNativeMediaType(AUDIO, 0).map_err(|_| "Kein Tonstrom".to_string())?;
            let _ = reader.SetStreamSelection(VIDEO, false);
            let mt = MFCreateMediaType().map_err(|e| e.to_string())?;
            let e = |e: windows::core::Error| e.to_string();
            mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio).map_err(e)?;
            mt.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_Float).map_err(e)?;
            mt.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, channels).map_err(e)?;
            mt.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, rate).map_err(e)?;
            mt.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 32).map_err(e)?;
            mt.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, 4 * channels).map_err(e)?;
            mt.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, rate * 4 * channels).map_err(e)?;
            mt.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1).map_err(e)?;
            reader.SetCurrentMediaType(AUDIO, None, &mt).map_err(|er| format!("Tonformat: {er}"))?;
            reader.SetStreamSelection(AUDIO, true).map_err(e)?;
            Ok(Self { reader, buf: vec![], buf_start: 0, pos: None, eof: false })
        }
    }

    fn seek(&mut self, frame: i64) {
        unsafe {
            let v = windows::Win32::System::Com::StructuredStorage::PROPVARIANT::from(frame * 10_000_000 / RATE);
            let _ = self.reader.SetCurrentPosition(&windows::core::GUID::zeroed(), &v);
        }
        self.buf.clear();
        self.buf_start = -1;
        self.eof = false;
    }

    /// Hängt das nächste decodierte Sample an. Gibt false zurück, wenn nichts mehr kommt.
    fn pull(&mut self) -> bool {
        unsafe {
            let (mut flags, mut ts) = (0u32, 0i64);
            let mut sample: Option<IMFSample> = None;
            if self.reader.ReadSample(AUDIO, 0, None, Some(&mut flags), Some(&mut ts), Some(&mut sample)).is_err() {
                self.eof = true;
                return false;
            }
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                self.eof = true;
            }
            let Some(sample) = sample else { return !self.eof };
            let Ok(b) = sample.ConvertToContiguousBuffer() else { return !self.eof };
            let (mut p, mut len) = (std::ptr::null_mut::<u8>(), 0u32);
            if b.Lock(&mut p, None, Some(&mut len)).is_err() {
                return !self.eof;
            }
            let n = len as usize / 4;
            let data = std::slice::from_raw_parts(p as *const f32, n);
            if self.buf_start < 0 {
                self.buf_start = ts * RATE / 10_000_000;
            }
            self.buf.extend_from_slice(data);
            let _ = b.Unlock();
            true
        }
    }

    /// `n` Stereo-Frames ab Frame-Index `start` (zeitlich gesehen: start/48000 s). Fehlendes wird mit Stille gefüllt.
    pub fn read(&mut self, start: i64, n: usize) -> Vec<f32> {
        let mut out = vec![0f32; n * 2];
        if self.pos != Some(start) {
            self.seek(start);
        }
        let end = start + n as i64;
        while !self.eof && (self.buf_start < 0 || self.buf_start + (self.buf.len() / 2) as i64 <= end) {
            if !self.pull() {
                break;
            }
        }
        if self.buf_start >= 0 {
            let have = (self.buf.len() / 2) as i64;
            for i in 0..n as i64 {
                let idx = start + i - self.buf_start;
                if idx >= 0 && idx < have {
                    out[i as usize * 2] = self.buf[idx as usize * 2];
                    out[i as usize * 2 + 1] = self.buf[idx as usize * 2 + 1];
                }
            }
            // Verbrauchtes verwerfen
            let drop = ((end - self.buf_start).clamp(0, have)) as usize;
            self.buf.drain(..drop * 2);
            self.buf_start += drop as i64;
        }
        self.pos = Some(end);
        out
    }
}

/// Spitzenwerte je Bin (`PEAKS_PER_SEC` Werte pro Sekunde, 0…1) für die Wellenform; leer, wenn kein Ton lesbar ist.
pub const PEAKS_PER_SEC: usize = 50;

pub fn peaks(path: &Path) -> Vec<f32> {
    let Ok(mut d) = AudioDecoder::open_format(path, 8000, 1) else { return vec![] };
    let per_bin = 8000 / PEAKS_PER_SEC;
    let (mut out, mut m, mut n) = (vec![], 0f32, 0usize);
    let mut guard = 0u64;
    while !d.eof && guard < 10_000_000 {
        guard += 1;
        if !d.pull() {
            break;
        }
        for s in d.buf.drain(..) {
            m = m.max(s.abs());
            n += 1;
            if n == per_bin {
                out.push(m.min(1.0));
                m = 0.0;
                n = 0;
            }
        }
    }
    for s in d.buf.drain(..) {
        m = m.max(s.abs());
        n += 1;
        if n == per_bin {
            out.push(m.min(1.0));
            m = 0.0;
            n = 0;
        }
    }
    if n > 0 {
        out.push(m.min(1.0));
    }
    out
}

/// Liest eine Tondatei als Mono-Strom mit Abtastrate `rate` (höchstens `max_secs` Sekunden) und reicht die Blöcke an `f`.
pub fn stream_mono(path: &Path, rate: u32, max_secs: f64, f: &mut dyn FnMut(&[f32])) -> Result<(), String> {
    let mut d = AudioDecoder::open_format(path, rate, 1)?;
    let cap = (max_secs * rate as f64) as usize;
    let (mut total, mut guard) = (0usize, 0u64);
    while !d.eof && total < cap && guard < 10_000_000 {
        guard += 1;
        if !d.pull() {
            break;
        }
        total += d.buf.len();
        f(&d.buf);
        d.buf.clear();
    }
    Ok(())
}

/// Dauer einer Tondatei in Sekunden (Fehler, wenn kein Tonstrom lesbar ist).
pub fn probe(path: &Path) -> Result<f64, String> {
    crate::video::init();
    unsafe {
        let wpath = HSTRING::from(path.as_os_str());
        let reader = MFCreateSourceReaderFromURL(PCWSTR(wpath.as_ptr()), None).map_err(|e| format!("Datei nicht lesbar: {e}"))?;
        reader.GetNativeMediaType(AUDIO, 0).map_err(|_| "Kein Tonstrom".to_string())?;
        let v = reader
            .GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)
            .map_err(|e| e.to_string())?;
        Ok(u64::try_from(&v).map_err(|e| e.to_string())? as f64 / 1e7)
    }
}

/// Zustand eines Clips beim Mischen: Decoder plus Filter, die über Blockgrenzen hinweg laufen.
struct ClipState {
    dec: Option<AudioDecoder>,
    /// Hochpass (Windfilter): Zustand je Kanal
    hp: [[f32; 2]; 2],
    /// Pegelverfolgung und aktuelle Verstärkung des Rauschtors
    env: f32,
    gate: f32,
    /// Projektframe, an dem der nächste Block nahtlos anschließt
    next: i64,
}

/// Biquad-Hochpass (Butterworth) bei `fc` Hz: (b0, b1, b2, a1, a2), normiert.
fn highpass(fc: f64) -> [f32; 5] {
    let w = 2.0 * std::f64::consts::PI * fc / RATE as f64;
    let (cs, alpha) = (w.cos(), w.sin() / (2.0 * std::f64::consts::FRAC_1_SQRT_2));
    let a0 = 1.0 + alpha;
    [((1.0 + cs) / 2.0 / a0) as f32, (-(1.0 + cs) / a0) as f32, ((1.0 + cs) / 2.0 / a0) as f32, (-2.0 * cs / a0) as f32, ((1.0 - alpha) / a0) as f32]
}

/// Wendet Windfilter (Hochpass 120 Hz) und Rauschminderung (Rauschtor mit weichem Ein-/Ausblenden) an.
fn apply_fx(st: &mut ClipState, fx: &AudioFx, data: &mut [f32]) {
    if fx.wind_filter {
        let [b0, b1, b2, a1, a2] = highpass(120.0);
        for ch in 0..2 {
            let z = &mut st.hp[ch];
            for i in (ch..data.len()).step_by(2) {
                let x = data[i];
                let y = b0 * x + z[0];
                z[0] = b1 * x - a1 * y + z[1];
                z[1] = b2 * x - a2 * y;
                data[i] = y;
            }
        }
    }
    if fx.denoise > 0.001 {
        let d = fx.denoise.clamp(0.0, 1.0) as f32;
        let (thr, floor) = (0.003 + 0.04 * d, 1.0 - 0.85 * d);
        let rel = (-1.0 / (0.1 * RATE as f32)).exp();
        let (open, close) = (1.0 / (0.005 * RATE as f32), 1.0 / (0.12 * RATE as f32));
        for f in data.chunks_exact_mut(2) {
            let lvl = f[0].abs().max(f[1].abs());
            st.env = lvl.max(st.env * rel);
            let target = if st.env < thr { floor } else { 1.0 };
            st.gate += (target - st.gate) * if target > st.gate { open } else { close };
            f[0] *= st.gate;
            f[1] *= st.gate;
        }
    }
}

/// Mischt den Ton aller Audioclips; hält pro Clip einen Decoder für sequentielles Lesen.
#[derive(Default)]
pub struct Mixer {
    clips: HashMap<Uuid, ClipState>,
}

impl Mixer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Gemischte Stereo-Frames [start, start+n) in Projektzeit (Frame-Index bei 48 kHz).
    pub fn mix(&mut self, p: &Project, start: i64, n: usize) -> Vec<f32> {
        let mut out = vec![0f32; n * 2];
        let any_solo = p.tracks.iter().any(|t| t.solo == Some(true));
        let (t0, t1) = (start as f64 / RATE as f64, (start + n as i64) as f64 / RATE as f64);
        for tr in &p.tracks {
            if tr.muted || (any_solo && tr.solo != Some(true)) {
                continue;
            }
            for c in &tr.clips {
                if !c.is_audio() || c.end() <= t0 || c.start >= t1 {
                    continue;
                }
                let Some(aid) = c.asset_id() else { continue };
                let Some(asset) = p.asset(aid) else { continue };
                if asset.is_image || !asset.has_audio {
                    continue;
                }
                let st = self.clips.entry(c.id).or_insert_with(|| ClipState {
                    dec: AudioDecoder::open(Path::new(&asset.path)).ok(),
                    hp: [[0.0; 2]; 2],
                    env: 0.0,
                    gate: 1.0,
                    next: i64::MIN,
                });
                if st.dec.is_none() {
                    continue;
                }
                // Bereich des Clips innerhalb des Blocks
                let f_from = ((c.start * RATE as f64).ceil() as i64).max(start);
                let f_to = ((c.end() * RATE as f64).floor() as i64).min(start + n as i64);
                if f_to <= f_from {
                    continue;
                }
                if st.next != f_from {
                    st.hp = [[0.0; 2]; 2];
                    st.env = 0.0;
                    st.gate = 1.0;
                }
                st.next = f_to;
                let m = (f_to - f_from) as usize;
                let speed = if c.speed > 0.0 { c.speed } else { 1.0 };
                let src_of = |f: i64| (c.source_in + (f as f64 / RATE as f64 - c.start) * speed) * RATE as f64;
                let s0 = src_of(f_from).round() as i64;
                let s_end = src_of(f_to).round() as i64;
                let len = ((s_end - s0).max(1)) as usize;
                let raw = st.dec.as_mut().unwrap().read(s0, len);
                let mut data = if (speed - 1.0).abs() < 1e-9 && len == m {
                    raw
                } else {
                    // Geschwindigkeit ≠ 1: lineare Interpolation (Tonhöhe folgt dem Tempo)
                    let mut d = vec![0f32; m * 2];
                    for i in 0..m {
                        let pos = src_of(f_from + i as i64) - s0 as f64;
                        let (i0, fr) = (pos.floor().max(0.0) as usize, pos.fract().max(0.0) as f32);
                        let (i0, i1) = (i0.min(len - 1), (i0 + 1).min(len - 1));
                        for ch in 0..2 {
                            d[i * 2 + ch] = raw[i0 * 2 + ch] * (1.0 - fr) + raw[i1 * 2 + ch] * fr;
                        }
                    }
                    d
                };
                apply_fx(st, &c.audio_fx, &mut data);
                let (xin, xout) = p.overlaps(c.id);
                for i in 0..m {
                    let t = (f_from + i as i64) as f64 / RATE as f64 - c.start;
                    let g = c.gain(t, xin, xout) as f32;
                    let o = ((f_from - start) as usize + i) * 2;
                    out[o] += data[i * 2] * g;
                    out[o + 1] += data[i * 2 + 1] * g;
                }
            }
        }
        for v in &mut out {
            *v = v.clamp(-1.0, 1.0);
        }
        out
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Schreibt eine Sinus-WAV (16 Bit, mono) und gibt den Pfad zurück.
    pub fn write_sine_wav(name: &str, hz: f64, secs: f64, rate: u32, amp: f64) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("video-superbe-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let n = (secs * rate as f64) as usize;
        let mut d: Vec<u8> = Vec::new();
        d.extend(b"RIFF");
        d.extend(((36 + n * 2) as u32).to_le_bytes());
        d.extend(b"WAVEfmt ");
        d.extend(16u32.to_le_bytes());
        d.extend(1u16.to_le_bytes());
        d.extend(1u16.to_le_bytes());
        d.extend(rate.to_le_bytes());
        d.extend((rate * 2).to_le_bytes());
        d.extend(2u16.to_le_bytes());
        d.extend(16u16.to_le_bytes());
        d.extend(b"data");
        d.extend(((n * 2) as u32).to_le_bytes());
        for i in 0..n {
            let v = (amp * (2.0 * std::f64::consts::PI * hz * i as f64 / rate as f64).sin() * 32767.0) as i16;
            d.extend(v.to_le_bytes());
        }
        std::fs::write(&path, d).unwrap();
        path
    }

    pub fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len().max(1) as f64).sqrt()
    }

    #[test]
    fn peaks_follow_signal_level() {
        let p = write_sine_wav("peaks_sine.wav", 440.0, 2.0, 44100, 0.5);
        let pk = peaks(&p);
        assert!((pk.len() as i64 - 100).abs() <= 2, "50 Werte je Sekunde: {}", pk.len());
        let mid = pk[10..90].iter().copied().fold(1.0f32, f32::min);
        assert!(mid > 0.4 && mid < 0.6, "Sinus mit Amplitude 0,5: {mid}");
        let loud = peaks(&write_sine_wav("peaks_loud.wav", 440.0, 1.0, 48000, 0.9));
        assert!(loud.iter().copied().fold(0.0, f32::max) > 0.85);
        assert!(peaks(&crate::render::tests::solid_png("peaks_not_audio.png", 8, 8, [0, 0, 0])).is_empty());
    }

    #[test]
    fn probe_reads_duration_and_rejects_non_audio() {
        let p = write_sine_wav("probe.wav", 440.0, 1.5, 44100, 0.3);
        let d = probe(&p).unwrap();
        assert!((d - 1.5).abs() < 0.05, "{d}");
        let png = crate::render::tests::solid_png("probe_not_audio.png", 8, 8, [0, 0, 0]);
        assert!(probe(&png).is_err());
    }

    #[test]
    fn decodes_and_resamples_wav() {
        let p = write_sine_wav("sine440_44k.wav", 440.0, 1.0, 44100, 0.5);
        let mut d = AudioDecoder::open(&p).expect("open");
        let a = d.read(0, 24000); // 0,5 s
        assert_eq!(a.len(), 48000);
        let r = rms(&a);
        assert!((r - 0.5 / 2f64.sqrt()).abs() < 0.03, "rms={r}");
        // sequentiell weiterlesen (kein Seek) und Stereo (beide Kanäle gleich)
        let b = d.read(24000, 12000);
        assert!((rms(&b) - r).abs() < 0.03);
        assert!((b[100] - b[101]).abs() < 1e-6);
        // hinter dem Ende: Stille
        let c = d.read(RATE * 2, 1000);
        assert!(rms(&c) < 1e-6);
    }

    #[test]
    fn seek_gives_same_samples_as_sequential() {
        let p = write_sine_wav("sine1k.wav", 1000.0, 2.0, 48000, 0.8);
        let mut d1 = AudioDecoder::open(&p).unwrap();
        let all = d1.read(0, 48000);
        let mut d2 = AudioDecoder::open(&p).unwrap();
        let part = d2.read(30000, 4000);
        for i in 0..4000 {
            assert!((all[(30000 + i) * 2] - part[i * 2]).abs() < 0.02, "i={i}");
        }
    }

    fn project_with_tone(path: &std::path::Path, dur: f64) -> (Project, Uuid) {
        let mut p = Project::default();
        let a = MediaAsset {
            path: path.to_string_lossy().into_owned(),
            name: "t".into(),
            duration: dur,
            has_audio: true,
            ..Default::default()
        };
        let aid = a.id;
        p.assets.push(a);
        let clip = Clip {
            content: ClipContent::Media(Wrap { v: aid }),
            part: ClipPart::Audio,
            start: 1.0,
            duration: dur,
            ..Default::default()
        };
        let cid = clip.id;
        p.tracks[0].clips.push(clip);
        (p, cid)
    }

    #[test]
    fn mixer_places_clip_and_applies_volume_fade_mute() {
        let wav = write_sine_wav("sine_mix.wav", 440.0, 2.0, 48000, 0.8);
        let (mut p, cid) = project_with_tone(&wav, 2.0);
        let mut m = Mixer::new();
        // vor dem Clip: Stille
        assert!(rms(&m.mix(&p, 0, 48000)) < 1e-6);
        // im Clip
        let loud = rms(&m.mix(&p, 48000, 24000));
        assert!((loud - 0.8 / 2f64.sqrt()).abs() < 0.04, "{loud}");
        // Lautstärke 50 %
        let mut m2 = Mixer::new();
        p.clip_mut(cid).unwrap().volume = 0.5;
        let half = rms(&m2.mix(&p, 48000, 24000));
        assert!((half - loud / 2.0).abs() < 0.03, "{half} vs {loud}");
        // Einblenden: erste 10 ms sehr leise
        let mut m3 = Mixer::new();
        {
            let c = p.clip_mut(cid).unwrap();
            c.volume = 1.0;
            c.fade_in = 1.0;
        }
        let start = rms(&m3.mix(&p, 48000, 480));
        assert!(start < 0.02, "{start}");
        // Spur stumm
        p.tracks[0].muted = true;
        let mut m4 = Mixer::new();
        assert!(rms(&m4.mix(&p, 48000, 24000)) < 1e-6);
        // Solo auf anderer Spur → diese Spur still
        p.tracks[0].muted = false;
        p.tracks[1].solo = Some(true);
        let mut m5 = Mixer::new();
        assert!(rms(&m5.mix(&p, 48000, 24000)) < 1e-6);
    }

    #[test]
    fn mixer_sums_and_clips() {
        let wav = write_sine_wav("sine_sum.wav", 440.0, 1.0, 48000, 0.9);
        let (mut p, _) = project_with_tone(&wav, 1.0);
        let aid = p.assets[0].id;
        p.tracks[1].clips.push(Clip {
            content: ClipContent::Media(Wrap { v: aid }),
            part: ClipPart::Audio,
            start: 1.0,
            duration: 1.0,
            ..Default::default()
        });
        let mut m = Mixer::new();
        let out = m.mix(&p, 48000, 4800);
        assert!(out.iter().all(|v| v.abs() <= 1.0));
        assert!(out.iter().any(|v| v.abs() > 0.99), "Summe muss begrenzt werden");
    }

    #[test]
    fn speed_and_fx_change_the_sound() {
        // 2 s Ton mit doppelter Geschwindigkeit: Clip dauert 1 s und liest 2 s Quelle
        let wav = write_sine_wav("fx_tone.wav", 440.0, 2.0, 48000, 0.6);
        let (mut p, cid) = project_with_tone(&wav, 2.0);
        {
            let c = p.clip_mut(cid).unwrap();
            c.speed = 2.0;
            c.duration = 1.0;
        }
        let mut m = Mixer::new();
        let a = m.mix(&p, 48000, 24000);
        assert!((rms(&a) - 0.6 / 2f64.sqrt()).abs() < 0.05, "rms {}", rms(&a));
        // Nulldurchgänge: 880 Hz statt 440 Hz → in 0,5 s etwa 880 Wechsel
        let zc = a.chunks(2).map(|f| f[0]).collect::<Vec<_>>().windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
        assert!((zc as i64 - 880).abs() < 40, "Nulldurchgänge {zc}");
        // hinter dem Clipende: Stille
        assert!(rms(&m.mix(&p, 48000 * 2 + 100, 4800)) < 1e-6);

        // Windfilter: tiefes Brummen (50 Hz) wird gedämpft, Sprache (1 kHz) bleibt
        let low = write_sine_wav("fx_low.wav", 50.0, 2.0, 48000, 0.6);
        let (mut pl, cl) = project_with_tone(&low, 2.0);
        let plain = rms(&Mixer::new().mix(&pl, 48000 + 24000, 24000));
        pl.clip_mut(cl).unwrap().audio_fx.wind_filter = true;
        let filtered = rms(&Mixer::new().mix(&pl, 48000 + 24000, 24000));
        assert!(filtered < plain * 0.35, "50 Hz: {filtered} vs {plain}");
        let hi = write_sine_wav("fx_hi.wav", 1000.0, 2.0, 48000, 0.6);
        let (mut ph, ch) = project_with_tone(&hi, 2.0);
        let plain = rms(&Mixer::new().mix(&ph, 48000 + 24000, 24000));
        ph.clip_mut(ch).unwrap().audio_fx.wind_filter = true;
        let filtered = rms(&Mixer::new().mix(&ph, 48000 + 24000, 24000));
        assert!((filtered - plain).abs() < plain * 0.06, "1 kHz: {filtered} vs {plain}");

        // Rauschminderung: leises Rauschen wird abgesenkt, lauter Ton bleibt
        let quiet = write_sine_wav("fx_quiet.wav", 800.0, 2.0, 48000, 0.01);
        let (mut pq, cq) = project_with_tone(&quiet, 2.0);
        let plain = rms(&Mixer::new().mix(&pq, 48000 + 24000, 24000));
        pq.clip_mut(cq).unwrap().audio_fx.denoise = 1.0;
        let gated = rms(&Mixer::new().mix(&pq, 48000 + 24000, 48000));
        assert!(gated < plain * 0.4, "leise: {gated} vs {plain}");
        let loud = write_sine_wav("fx_loud.wav", 800.0, 2.0, 48000, 0.6);
        let (mut pn, cn) = project_with_tone(&loud, 2.0);
        pn.clip_mut(cn).unwrap().audio_fx.denoise = 1.0;
        let kept = rms(&Mixer::new().mix(&pn, 48000 + 24000, 24000));
        assert!(kept > 0.6 / 2f64.sqrt() * 0.95, "laut bleibt: {kept}");
    }
}
