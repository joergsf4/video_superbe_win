//! Export nach MP4 (H.264 + AAC) über den Media-Foundation-Sink-Writer (in Windows eingebaut).

use crate::audio::{Mixer, RATE};
use crate::model::*;
use crate::render::Renderer;
use std::path::Path;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Media::MediaFoundation::*;

#[derive(Clone, Debug)]
pub struct ExportOpts {
    pub width: usize,
    pub height: usize,
    pub fps: u32,
    pub video_kbps: u32,
    /// Nur diesen Zeitbereich (Sekunden) exportieren; `None` = ganzer Film
    pub range: Option<(f64, f64)>,
    /// Lineare Verstärkung des Tons (Lautheits-Normalisierung); über 1 bzw. bei `limiter` hält ein Limiter die Spitzen
    pub audio_gain: f32,
    pub limiter: bool,
    /// Konstante statt variabler Bitrate; Abstand der Schlüsselbilder (Sekunden); H.264-Profil High (sonst Main)
    pub constant_bitrate: bool,
    pub gop: f64,
    pub profile_high: bool,
    /// AAC-Bitrate (kbit/s: 96, 128, 160, 192) und Mono
    pub audio_kbps: u32,
    pub mono: bool,
    /// Index vor die Daten („Fast Start“) und Metadaten
    pub fast_start: bool,
    pub title: String,
    pub author: String,
}

/// Grund-Bitrate (kbit/s) für Bildgröße und Bildrate: 12 Mbit/s bei Full HD und 30 Bildern/s.
pub fn base_kbps(w: usize, h: usize, fps: u32) -> u32 {
    let k = if fps > 30 { 1.5 } else { 1.0 };
    ((w * h) as f64 / (1920.0 * 1080.0) * 12000.0 * k).clamp(1000.0, 60000.0) as u32
}

impl ExportOpts {
    pub fn from_project(p: &Project) -> Self {
        let (w, h) = (p.render_width.max(16) as usize, p.render_height.max(16) as usize);
        let kbps = ((w * h) as f64 / (1920.0 * 1080.0) * 12000.0).clamp(1000.0, 40000.0) as u32;
        Self {
            width: w & !1,
            height: h & !1,
            fps: p.fps.clamp(1, 120) as u32,
            video_kbps: kbps,
            range: None,
            audio_gain: 1.0,
            limiter: false,
            constant_bitrate: false,
            gop: 2.0,
            profile_high: true,
            audio_kbps: 192,
            mono: false,
            fast_start: true,
            title: String::new(),
            author: String::new(),
        }
    }
}

/// Gespeicherte Exporteinstellungen (Voreinstellung im Export-Dialog; Port von ExportPresets.swift ohne HEVC).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ExportSettings {
    /// Ausgabe: kurze Seite in Pixeln (`None` = Filmformat) und Bildrate (`None` = Filmformat)
    pub height: Option<i64>,
    pub fps: Option<i64>,
    /// Faktor auf die automatische Bitrate; `kbps` überschreibt sie
    pub quality: f64,
    pub kbps: Option<u32>,
    pub constant_bitrate: bool,
    pub gop: f64,
    pub profile_high: bool,
    pub audio_kbps: u32,
    pub mono: bool,
    pub fast_start: bool,
    /// Ziel-Lautheit (`None` = Ton unverändert)
    pub lufs: Option<f64>,
}

impl Default for ExportSettings {
    fn default() -> Self {
        ExportSettings { height: None, fps: None, quality: 1.0, kbps: None, constant_bitrate: false, gop: 2.0, profile_high: true, audio_kbps: 192, mono: false, fast_start: true, lufs: Some(-14.0) }
    }
}

/// AAC-Bitraten, die der Windows-Encoder kann.
pub const AAC_KBPS: [u32; 4] = [96, 128, 160, 192];

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExportPresetItem {
    pub name: String,
    pub settings: ExportSettings,
}

pub fn builtin_presets() -> Vec<ExportPresetItem> {
    let s = ExportSettings::default;
    let item = |name: &str, settings| ExportPresetItem { name: name.into(), settings };
    vec![
        item("YouTube Full HD", ExportSettings { height: Some(1080), ..s() }),
        item("YouTube 4K", ExportSettings { height: Some(2160), ..s() }),
        item("Handy (klein)", ExportSettings { height: Some(1080), quality: 0.6, audio_kbps: 128, ..s() }),
        item("Messenger / E-Mail (klein)", ExportSettings { height: Some(720), quality: 0.6, audio_kbps: 128, lufs: None, ..s() }),
        item("Beste Qualität (Archiv)", ExportSettings { quality: 1.6, audio_kbps: 192, lufs: None, ..s() }),
    ]
}

/// Ausgabegröße bei gewählter Auflösung: „1080p“ meint die kurze Seite (Querformat 1920×1080, Hochformat 1080×1920);
/// das Seitenverhältnis des Films bleibt, beide Seiten werden auf gerade Pixelzahlen gerundet.
pub fn output_size(w: i64, h: i64, height: Option<i64>) -> (usize, usize) {
    let even = |v: f64| ((v / 2.0).round() as usize * 2).max(2);
    match height {
        Some(t) if w > 0 && h > 0 => {
            let k = t as f64 / w.min(h) as f64;
            (even(w as f64 * k), even(h as f64 * k))
        }
        _ => (even(w.max(2) as f64), even(h.max(2) as f64)),
    }
}

impl ExportSettings {
    pub fn opts(&self, p: &Project, range: Option<(f64, f64)>, title: &str, author: &str) -> ExportOpts {
        let (w, h) = output_size(p.render_width, p.render_height, self.height);
        let fps = self.fps.unwrap_or(p.fps).clamp(1, 120) as u32;
        ExportOpts {
            width: w,
            height: h,
            fps,
            video_kbps: self.kbps.unwrap_or_else(|| (base_kbps(w, h, fps) as f64 * self.quality) as u32).clamp(300, 150_000),
            range,
            audio_gain: 1.0,
            limiter: false,
            constant_bitrate: self.constant_bitrate,
            gop: self.gop.max(0.5),
            profile_high: self.profile_high,
            audio_kbps: *AAC_KBPS.iter().min_by_key(|k| (**k as i64 - self.audio_kbps as i64).abs()).unwrap(),
            mono: self.mono,
            fast_start: self.fast_start,
            title: title.trim().to_string(),
            author: author.trim().to_string(),
        }
    }
}

/// RGBA → NV12 (BT.709, Limited Range); Breite und Höhe müssen gerade sein.
pub fn rgba_to_nv12(rgba: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h * 3 / 2];
    let (yp, uv) = out.split_at_mut(w * h);
    for y in 0..h {
        for x in 0..w {
            let o = (y * w + x) * 4;
            let (r, g, b) = (rgba[o] as f32, rgba[o + 1] as f32, rgba[o + 2] as f32);
            let yy = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            yp[y * w + x] = (16.0 + yy * 219.0 / 255.0 + 0.5) as u8;
        }
    }
    for cy in 0..h / 2 {
        for cx in 0..w / 2 {
            let (mut su, mut sv) = (0.0, 0.0);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let o = ((cy * 2 + dy) * w + cx * 2 + dx) * 4;
                let (r, g, b) = (rgba[o] as f32, rgba[o + 1] as f32, rgba[o + 2] as f32);
                let yy = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                su += (b - yy) / 1.8556;
                sv += (r - yy) / 1.5748;
            }
            let i = cy * w + cx * 2;
            uv[i] = (128.0 + su / 4.0 * 224.0 / 255.0 + 0.5).clamp(16.0, 240.0) as u8;
            uv[i + 1] = (128.0 + sv / 4.0 * 224.0 / 255.0 + 0.5).clamp(16.0, 240.0) as u8;
        }
    }
    out
}

fn pack(a: u32, b: u32) -> u64 {
    ((a as u64) << 32) | b as u64
}

fn sample_from(data: &[u8], time: i64, dur: i64) -> Result<IMFSample, String> {
    let e = |e: windows::core::Error| e.to_string();
    unsafe {
        let buf = MFCreateMemoryBuffer(data.len() as u32).map_err(e)?;
        let mut p = std::ptr::null_mut::<u8>();
        buf.Lock(&mut p, None, None).map_err(e)?;
        std::ptr::copy_nonoverlapping(data.as_ptr(), p, data.len());
        buf.Unlock().map_err(e)?;
        buf.SetCurrentLength(data.len() as u32).map_err(e)?;
        let s = MFCreateSample().map_err(e)?;
        s.AddBuffer(&buf).map_err(e)?;
        s.SetSampleTime(time).map_err(e)?;
        s.SetSampleDuration(dur).map_err(e)?;
        Ok(s)
    }
}

pub fn has_audio(p: &Project) -> bool {
    p.tracks.iter().flat_map(|t| &t.clips).any(|c| c.is_audio() && c.asset_id().and_then(|a| p.asset(a)).is_some_and(|a| a.has_audio && !a.is_image))
}

/// Exportiert das ganze Projekt. `progress(0…1)` gibt `false` zurück, um abzubrechen (dann Fehler "Abgebrochen").
pub fn export(p: &Project, path: &Path, o: &ExportOpts, progress: &mut dyn FnMut(f64) -> bool) -> Result<(), String> {
    crate::video::init();
    let total = p.duration();
    let (t0, t1) = o.range.map(|(a, b)| (a.clamp(0.0, total), b.clamp(0.0, total))).unwrap_or((0.0, total));
    let dur = t1 - t0;
    if total <= 0.0 {
        return Err("Das Projekt ist leer.".into());
    }
    if dur <= 0.0 {
        return Err("Der gewählte Bereich ist leer.".into());
    }
    let a0 = (t0 * RATE as f64).round() as i64;
    let (w, h, fps) = (o.width & !1, o.height & !1, o.fps.max(1));
    let e = |e: windows::core::Error| e.to_string();
    let audio = has_audio(p);
    unsafe {
        let mut attrs: Option<IMFAttributes> = None;
        MFCreateAttributes(&mut attrs, 2).map_err(e)?;
        let attrs = attrs.ok_or("Attribute")?;
        attrs.SetUINT32(&MF_SINK_WRITER_DISABLE_THROTTLING, 1).map_err(e)?;
        let _ = std::fs::remove_file(path);
        let wpath = HSTRING::from(path.as_os_str());
        let writer = MFCreateSinkWriterFromURL(PCWSTR(wpath.as_ptr()), None, &attrs).map_err(|er| format!("Datei kann nicht angelegt werden: {er}"))?;

        // Video
        let vout = MFCreateMediaType().map_err(e)?;
        vout.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).map_err(e)?;
        vout.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264).map_err(e)?;
        vout.SetUINT32(&MF_MT_AVG_BITRATE, o.video_kbps * 1000).map_err(e)?;
        vout.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32).map_err(e)?;
        vout.SetUINT64(&MF_MT_FRAME_SIZE, pack(w as u32, h as u32)).map_err(e)?;
        vout.SetUINT64(&MF_MT_FRAME_RATE, pack(fps, 1)).map_err(e)?;
        vout.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1)).map_err(e)?;
        vout.SetUINT32(&MF_MT_MPEG2_PROFILE, if o.profile_high { 100 } else { 77 }).map_err(e)?; // High bzw. Main
        let vi = writer.AddStream(&vout).map_err(e)?;
        let vin = MFCreateMediaType().map_err(e)?;
        vin.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).map_err(e)?;
        vin.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12).map_err(e)?;
        vin.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32).map_err(e)?;
        vin.SetUINT64(&MF_MT_FRAME_SIZE, pack(w as u32, h as u32)).map_err(e)?;
        vin.SetUINT64(&MF_MT_FRAME_RATE, pack(fps, 1)).map_err(e)?;
        vin.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1)).map_err(e)?;
        // Keine B-Bilder: sonst beginnt das Bild um ein Bild später als der Ton (Zeitversatz im Container).
        // Bitratenmodus und Schlüsselbildabstand; lehnt ein Encoder sie ab, geht es mit seinen Standardwerten weiter.
        let enc_attrs = |full: bool| -> Result<IMFAttributes, String> {
            let mut enc: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut enc, 3).map_err(e)?;
            let enc = enc.ok_or("Attribute")?;
            enc.SetUINT32(&CODECAPI_AVEncMPVDefaultBPictureCount, 0).map_err(e)?;
            if full {
                // eAVEncCommonRateControlMode: 0 = CBR, 2 = variabel (Durchschnitt)
                enc.SetUINT32(&CODECAPI_AVEncCommonRateControlMode, if o.constant_bitrate { 0 } else { 2 }).map_err(e)?;
                enc.SetUINT32(&CODECAPI_AVEncMPVGOPSize, ((o.gop.max(0.5) * fps as f64).round() as u32).max(1)).map_err(e)?;
            }
            Ok(enc)
        };
        if writer.SetInputMediaType(vi, &vin, Some(&enc_attrs(true)?)).is_err() {
            writer.SetInputMediaType(vi, &vin, Some(&enc_attrs(false)?)).map_err(|er| format!("Videoformat: {er}"))?;
        }

        // Audio
        let mut ai = 0;
        let chans: u32 = if o.mono { 1 } else { 2 };
        if audio {
            let aout = MFCreateMediaType().map_err(e)?;
            aout.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio).map_err(e)?;
            aout.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC).map_err(e)?;
            aout.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16).map_err(e)?;
            aout.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, RATE as u32).map_err(e)?;
            aout.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, chans).map_err(e)?;
            // der Windows-AAC-Encoder kennt 96/128/160/192 kbit/s (bei Mono halbiert)
            let kbps = if AAC_KBPS.contains(&o.audio_kbps) { o.audio_kbps } else { 192 };
            aout.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, if o.mono { (kbps / 2).max(96) * 125 } else { kbps * 125 }).map_err(e)?;
            aout.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, 1).map_err(e)?;
            aout.SetUINT32(&MF_MT_AAC_AUDIO_PROFILE_LEVEL_INDICATION, 0x29).map_err(e)?;
            aout.SetUINT32(&MF_MT_AAC_PAYLOAD_TYPE, 0).map_err(e)?;
            ai = writer.AddStream(&aout).map_err(e)?;
            let ain = MFCreateMediaType().map_err(e)?;
            ain.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio).map_err(e)?;
            ain.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM).map_err(e)?;
            ain.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16).map_err(e)?;
            ain.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, RATE as u32).map_err(e)?;
            ain.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, chans).map_err(e)?;
            ain.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, 2 * chans).map_err(e)?;
            ain.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, RATE as u32 * 2 * chans).map_err(e)?;
            writer.SetInputMediaType(ai, &ain, None).map_err(|er| format!("Tonformat: {er}"))?;
        }
        writer.BeginWriting().map_err(|er| format!("Start: {er}"))?;

        let mut renderer = Renderer::new();
        let mut mixer = Mixer::new();
        // Verstärkung über den Limiter (Zustand über alle Tonpuffer hinweg); ohne Normalisierung bleibt es beim harten Begrenzen
        let mut limiter = (o.limiter || o.audio_gain > 1.0).then(crate::loudness::PeakLimiter::new);
        let frames = (dur * fps as f64).ceil() as i64;
        let frame_dur = 10_000_000 / fps as i64;
        let mut a_done: i64 = 0;
        for i in 0..frames {
            if !progress(i as f64 / frames as f64) {
                let _ = writer.Finalize();
                let _ = std::fs::remove_file(path);
                return Err("Abgebrochen".into());
            }
            let rgba = renderer.render(p, t0 + i as f64 / fps as f64, w, h);
            let nv12 = rgba_to_nv12(&rgba, w, h);
            let s = sample_from(&nv12, i * 10_000_000 / fps as i64, frame_dur)?;
            writer.WriteSample(vi, &s).map_err(|er| format!("Video schreiben: {er}"))?;
            if audio {
                let target = ((i + 1) * RATE / fps as i64).min((dur * RATE as f64).ceil() as i64);
                if target > a_done {
                    let n = (target - a_done) as usize;
                    let mut f = mixer.mix(p, a0 + a_done, n);
                    let g = match limiter.as_mut() {
                        Some(l) => {
                            l.process(&mut f, 2, o.audio_gain as f64);
                            1.0
                        }
                        None => o.audio_gain,
                    };
                    let mut pcm = Vec::with_capacity(n * 2 * chans as usize);
                    let mut put = |v: f32| pcm.extend((((v * g).clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
                    if o.mono {
                        for fr in f.chunks_exact(2) {
                            put((fr[0] + fr[1]) * 0.5);
                        }
                    } else {
                        for v in &f {
                            put(*v);
                        }
                    }
                    let s = sample_from(&pcm, a_done * 10_000_000 / RATE, n as i64 * 10_000_000 / RATE)?;
                    writer.WriteSample(ai, &s).map_err(|er| format!("Ton schreiben: {er}"))?;
                    a_done = target;
                }
            }
        }
        writer.Finalize().map_err(|er| format!("Abschluss: {er}"))?;
    }
    if o.fast_start || !o.title.is_empty() || !o.author.is_empty() {
        crate::mp4post::finish(path, o.fast_start, &o.title, &o.author).map_err(|er| format!("Nachbearbeitung (Fast Start/Metadaten): {er}"))?;
    }
    progress(1.0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::tests::{rms, write_sine_wav};
    use crate::audio::AudioDecoder;
    use crate::render::tests::{clip, image_asset, solid_png};
    use crate::video::Decoder;

    #[test]
    fn nv12_levels() {
        let white = vec![255u8; 4 * 4 * 4];
        let n = rgba_to_nv12(&white, 4, 4);
        assert_eq!(n.len(), 4 * 4 * 3 / 2);
        assert!(n[..16].iter().all(|&y| (234..=236).contains(&y)));
        assert!(n[16..].iter().all(|&c| (127..=129).contains(&c)));
        let black: Vec<u8> = (0..16).flat_map(|_| [0u8, 0, 0, 255]).collect();
        let n = rgba_to_nv12(&black, 4, 4);
        assert!(n[..16].iter().all(|&y| y == 16));
        // reines Rot: V hoch, U niedrig
        let red: Vec<u8> = (0..16).flat_map(|_| [255u8, 0, 0, 255]).collect();
        let n = rgba_to_nv12(&red, 4, 4);
        assert!(n[16] < 110 && n[17] > 200, "U={} V={}", n[16], n[17]);
    }

    fn test_project() -> Project {
        let mut p = Project::default();
        let red = solid_png("exp_red.png", 320, 180, [220, 20, 20]);
        let blue = solid_png("exp_blue.png", 320, 180, [20, 20, 220]);
        let ra = image_asset(&mut p, &red, 320.0, 180.0);
        let ba = image_asset(&mut p, &blue, 320.0, 180.0);
        p.tracks[0].clips.push(clip(ra, 0.0, 1.0));
        p.tracks[0].clips.push(clip(ba, 1.0, 1.0));
        p.render_width = 320;
        p.render_height = 180;
        p.fps = 15;
        p
    }

    fn mean_rgb(f: &crate::video::Frame) -> [f64; 3] {
        let n = (f.rgba.len() / 4) as f64;
        let mut s = [0f64; 3];
        for px in f.rgba.chunks(4) {
            for i in 0..3 {
                s[i] += px[i] as f64;
            }
        }
        [s[0] / n, s[1] / n, s[2] / n]
    }

    #[test]
    fn exports_video_with_audio_and_reads_back() {
        let mut p = test_project();
        let wav = write_sine_wav("exp_tone.wav", 440.0, 2.0, 48000, 0.7);
        let a = MediaAsset { path: wav.to_string_lossy().into_owned(), name: "tone".into(), duration: 2.0, has_audio: true, ..Default::default() };
        let aid = a.id;
        p.assets.push(a);
        p.tracks[1].clips.push(Clip { part: ClipPart::Audio, ..clip(aid, 0.0, 2.0) });

        let out = std::env::temp_dir().join("video-superbe-tests").join("export_test.mp4");
        let mut last = -1.0;
        let mut calls = 0;
        export(&p, &out, &ExportOpts::from_project(&p), &mut |f| {
            assert!(f >= last, "Fortschritt wächst");
            last = f;
            calls += 1;
            true
        })
        .expect("export");
        assert!(calls >= 30 && last > 0.99, "calls={calls} last={last}");
        assert!(std::fs::metadata(&out).unwrap().len() > 2000);

        let info = crate::video::probe(&out).expect("probe");
        assert!(info.has_video && info.has_audio, "{info:?}");
        assert_eq!((info.width, info.height), (320, 180));
        assert!((info.duration - 2.0).abs() < 0.25, "Dauer {}", info.duration);

        let mut d = Decoder::open(&out).unwrap();
        let f1 = mean_rgb(d.frame_at(0.4).unwrap());
        assert!(f1[0] > 150.0 && f1[2] < 90.0, "erstes Bild rot: {f1:?}");
        let f2 = mean_rgb(d.frame_at(1.5).unwrap());
        assert!(f2[2] > 150.0 && f2[0] < 90.0, "zweites Bild blau: {f2:?}");

        let mut ad = AudioDecoder::open(&out).unwrap();
        let a = ad.read(RATE / 2, RATE as usize / 2);
        let r = rms(&a);
        assert!(r > 0.25 && r < 0.7, "Ton vorhanden, rms={r}");
    }

    #[test]
    fn export_without_audio_and_cancel() {
        let p = test_project();
        let out = std::env::temp_dir().join("video-superbe-tests").join("export_noaudio.mp4");
        export(&p, &out, &ExportOpts::from_project(&p), &mut |_| true).expect("export");
        let info = crate::video::probe(&out).unwrap();
        assert!(info.has_video && !info.has_audio, "{info:?}");
        // Abbruch löscht die Datei
        let out2 = std::env::temp_dir().join("video-superbe-tests").join("export_cancel.mp4");
        let r = export(&p, &out2, &ExportOpts::from_project(&p), &mut |f| f < 0.3);
        assert_eq!(r.unwrap_err(), "Abgebrochen");
        assert!(!out2.exists());
    }

    #[test]
    fn empty_project_is_an_error() {
        let out = std::env::temp_dir().join("video-superbe-tests").join("export_empty.mp4");
        let p = Project::default();
        assert!(export(&p, &out, &ExportOpts::from_project(&p), &mut |_| true).is_err());
    }

    #[test]
    fn renders_video_clip_with_source_offset() {
        let p = test_project();
        let mp4 = std::env::temp_dir().join("video-superbe-tests").join("export_src.mp4");
        export(&p, &mp4, &ExportOpts::from_project(&p), &mut |_| true).unwrap();
        let info = crate::video::probe(&mp4).unwrap();
        let mut q = Project::default();
        let a = MediaAsset {
            path: mp4.to_string_lossy().into_owned(),
            name: "v".into(),
            duration: info.duration,
            has_video: true,
            width: 320.0,
            height: 180.0,
            fps: info.fps,
            ..Default::default()
        };
        let aid = a.id;
        q.assets.push(a);
        let mut c = clip(aid, 0.5, 1.4);
        c.source_in = 0.2;
        q.tracks[0].clips.push(c);
        let mut r = Renderer::new();
        let px = |t: f64, r: &mut Renderer| {
            let b = r.render(&q, t, 64, 36);
            let o = (18 * 64 + 32) * 4;
            [b[o] as i32, b[o + 1] as i32, b[o + 2] as i32]
        };
        let early = px(0.6, &mut r); // Quelle 0,3 s → rot
        assert!(early[0] > 150 && early[2] < 90, "{early:?}");
        let late = px(1.7, &mut r); // Quelle 1,4 s → blau
        assert!(late[2] > 150 && late[0] < 90, "{late:?}");
        let before = px(0.2, &mut r);
        assert_eq!(before, [0, 0, 0], "vor dem Clip schwarz");
    }

    #[test]
    fn exports_hevc_source_video() {
        let Some(dir) = crate::app::videos_dir() else { return };
        let Some(src) = std::fs::read_dir(dir.join("test_vsp")).ok().and_then(|rd| {
            let mut v: Vec<_> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| crate::hevc::parse_file(p).is_ok()).collect();
            v.sort_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(u64::MAX));
            v.into_iter().next()
        }) else {
            eprintln!("keine HEVC-Beispieldatei, Test übersprungen");
            return;
        };
        let info = crate::video::probe(&src).unwrap();
        let mut p = Project::default();
        let a = MediaAsset {
            path: src.to_string_lossy().into_owned(),
            name: "hevc".into(),
            duration: info.duration,
            has_video: true,
            width: info.width as f64,
            height: info.height as f64,
            fps: info.fps,
            ..Default::default()
        };
        let aid = a.id;
        p.assets.push(a);
        let mut c = clip(aid, 0.0, 1.0);
        c.source_in = 1.0;
        p.tracks[0].clips.push(c);
        p.render_width = 320;
        p.render_height = 180;
        p.fps = 10;
        let out = std::env::temp_dir().join("video-superbe-tests").join("export_hevc.mp4");
        export(&p, &out, &ExportOpts::from_project(&p), &mut |_| true).expect("export");
        let mut d = Decoder::open(&out).unwrap();
        let f = mean_rgb(d.frame_at(0.5).unwrap());
        assert!(f.iter().sum::<f64>() / 3.0 > 20.0 && f.iter().sum::<f64>() / 3.0 < 250.0, "Bild vorhanden: {f:?}");
        let g = mean_rgb(d.frame_at(0.05).unwrap());
        assert!((f[0] - g[0]).abs() < 60.0, "ähnliche Bilder im Abstand von 0,45 s");
    }

    #[test]
    fn output_size_uses_the_short_side_and_presets_are_sane() {
        assert_eq!(output_size(1920, 1080, Some(720)), (1280, 720));
        assert_eq!(output_size(1080, 1920, Some(1080)), (1080, 1920), "Hochformat: 1080p = kurze Seite");
        assert_eq!(output_size(1080, 1920, Some(2160)), (2160, 3840));
        assert_eq!(output_size(1920, 1080, None), (1920, 1080));
        assert_eq!(output_size(1000, 563, Some(480)), (852, 480), "gerade Pixelzahlen");
        let p = Project { render_width: 1080, render_height: 1920, fps: 30, ..Default::default() };
        let all = builtin_presets();
        assert_eq!(all[0].name, "YouTube Full HD");
        let o = all[0].settings.opts(&p, None, " T ", "");
        assert_eq!((o.width, o.height, o.fps, o.title.as_str()), (1080, 1920, 30, "T"));
        let s = ExportSettings { fps: Some(60), kbps: Some(5000), audio_kbps: 320, ..Default::default() };
        let o = s.opts(&p, Some((1.0, 2.0)), "", "");
        assert_eq!((o.fps, o.video_kbps, o.audio_kbps, o.range), (60, 5000, 192, Some((1.0, 2.0))), "320 kbit/s gibt es unter Windows nicht → 192");
        assert!(base_kbps(1920, 1080, 60) > base_kbps(1920, 1080, 30));
        // Voreinstellungen überleben JSON (eigene werden im Datenordner gespeichert)
        let j = serde_json::to_string(&all).unwrap();
        assert_eq!(serde_json::from_str::<Vec<ExportPresetItem>>(&j).unwrap(), all);
    }

    #[test]
    fn exports_portrait_mono_main_cbr_with_fast_start_and_metadata() {
        let mut p = test_project();
        (p.render_width, p.render_height) = (180, 320);
        let wav = write_sine_wav("exp_mono_tone.wav", 440.0, 2.0, 48000, 0.3);
        let a = MediaAsset { path: wav.to_string_lossy().into_owned(), name: "tone".into(), duration: 2.0, has_audio: true, ..Default::default() };
        let aid = a.id;
        p.assets.push(a);
        p.tracks[1].clips.push(Clip { part: ClipPart::Audio, ..clip(aid, 0.0, 2.0) });
        let s = ExportSettings { height: Some(240), fps: Some(25), mono: true, audio_kbps: 128, constant_bitrate: true, profile_high: false, gop: 1.0, ..Default::default() };
        let o = s.opts(&p, None, "Mein Film", "Jörg");
        let out = std::env::temp_dir().join("video-superbe-tests").join("export_settings.mp4");
        export(&p, &out, &o, &mut |_| true).expect("export");
        let info = crate::video::probe(&out).expect("probe");
        assert_eq!((info.width, info.height), (240, 426), "{info:?}");
        assert!((info.fps - 25.0).abs() < 0.5, "Bildrate {}", info.fps);
        assert!(info.has_audio);
        let kinds = crate::mp4post::top_level_kinds(&out).unwrap();
        let (m, d) = (kinds.iter().position(|k| k == "moov").unwrap(), kinds.iter().position(|k| k == "mdat").unwrap());
        assert!(m < d, "Fast Start: {kinds:?}");
        assert_eq!(crate::mp4post::read_metadata(&out).unwrap(), ("Mein Film".to_string(), "Jörg".to_string()));
        // die Datei spielt nach dem Umbau noch: Bild und Ton lesbar
        let mut dec = Decoder::open(&out).unwrap();
        let fr = dec.frame_at(0.4).unwrap();
        // Querformat-Bild im Hochformat: Mitte rot, oben und unten schwarze Balken
        let at = |x: usize, y: usize| { let o = (y * fr.width + x) * 4; [fr.rgba[o], fr.rgba[o + 1], fr.rgba[o + 2]] };
        let (mid, top) = (at(120, 213), at(120, 10));
        assert!(mid[0] > 150 && mid[2] < 90, "Mitte rot: {mid:?}");
        assert!(top.iter().all(|v| *v < 40), "Balken schwarz: {top:?}");
        let mut ad = AudioDecoder::open(&out).unwrap();
        let r = rms(&ad.read(RATE / 2, RATE as usize / 2));
        assert!(r > 0.12 && r < 0.3, "Mono-Ton vorhanden (beide Kanäle gleich), rms={r}");
        // ohne Fast Start und ohne Metadaten bleibt die Datei, wie Media Foundation sie schreibt
        let mut o2 = o.clone();
        (o2.fast_start, o2.title, o2.author) = (false, String::new(), String::new());
        export(&p, &out, &o2, &mut |_| true).expect("export");
        assert!(crate::video::probe(&out).is_ok());
    }

    #[test]
    fn normalized_export_uses_the_limiter() {
        let mut p = test_project();
        let wav = write_sine_wav("exp_lim_tone.wav", 440.0, 2.0, 48000, 0.8);
        let a = MediaAsset { path: wav.to_string_lossy().into_owned(), name: "tone".into(), duration: 2.0, has_audio: true, ..Default::default() };
        let aid = a.id;
        p.assets.push(a);
        p.tracks[1].clips.push(Clip { part: ClipPart::Audio, ..clip(aid, 0.0, 2.0) });
        let mut o = ExportOpts::from_project(&p);
        (o.audio_gain, o.limiter) = (3.0, true);
        let out = std::env::temp_dir().join("video-superbe-tests").join("export_limiter.mp4");
        export(&p, &out, &o, &mut |_| true).expect("export");
        let mut ad = AudioDecoder::open(&out).unwrap();
        let s = ad.read(RATE / 2, RATE as usize);
        let peak = s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        // −1 dBFS = 0,891; AAC schwingt beim Decodieren etwas darüber (True Peak), übersteuert aber nicht
        assert!(peak < 0.99 && peak > 0.7, "Spitzen auf etwa −1 dBFS gehalten: {peak}");
        let r = rms(&s);
        assert!(r > 0.55 && r < 0.68, "Sinus bleibt Sinus (rms ≈ 0,89/√2), kein Rechteck: {r}");
    }

    #[test]
    fn exports_only_the_chosen_range_with_gain() {
        let mut p = test_project();
        let wav = write_sine_wav("exp_range_tone.wav", 440.0, 2.0, 48000, 0.2);
        let a = MediaAsset { path: wav.to_string_lossy().into_owned(), name: "tone".into(), duration: 2.0, has_audio: true, ..Default::default() };
        let aid = a.id;
        p.assets.push(a);
        p.tracks[1].clips.push(Clip { part: ClipPart::Audio, ..clip(aid, 0.0, 2.0) });
        let out = std::env::temp_dir().join("video-superbe-tests").join("export_range.mp4");
        let mut o = ExportOpts::from_project(&p);
        o.range = Some((1.0, 2.0));
        o.audio_gain = 2.0;
        export(&p, &out, &o, &mut |_| true).expect("export");
        let info = crate::video::probe(&out).unwrap();
        assert!((info.duration - 1.0).abs() < 0.25, "Dauer {}", info.duration);
        let mut d = Decoder::open(&out).unwrap();
        let f = mean_rgb(d.frame_at(0.3).unwrap());
        assert!(f[2] > 150.0 && f[0] < 90.0, "Bereich beginnt im blauen Teil: {f:?}");
        let mut ad = AudioDecoder::open(&out).unwrap();
        let r = rms(&ad.read(RATE / 4, RATE as usize / 2));
        assert!(r > 0.2 && r < 0.36, "Ton mit doppelter Verstärkung (0,2 → 0,4 Spitze), rms={r}");
        o.range = Some((1.5, 1.5));
        assert!(export(&p, &out, &o, &mut |_| true).is_err(), "leerer Bereich");
    }
}
