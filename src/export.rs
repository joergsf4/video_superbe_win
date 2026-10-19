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
    /// Lineare Verstärkung des Tons (Lautheits-Normalisierung)
    pub audio_gain: f32,
}

impl ExportOpts {
    pub fn from_project(p: &Project) -> Self {
        let (w, h) = (p.render_width.max(16) as usize, p.render_height.max(16) as usize);
        let kbps = ((w * h) as f64 / (1920.0 * 1080.0) * 12000.0).clamp(1000.0, 40000.0) as u32;
        Self { width: w & !1, height: h & !1, fps: p.fps.clamp(1, 120) as u32, video_kbps: kbps, range: None, audio_gain: 1.0 }
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
        vout.SetUINT32(&MF_MT_MPEG2_PROFILE, 100).map_err(e)?; // High
        let vi = writer.AddStream(&vout).map_err(e)?;
        let vin = MFCreateMediaType().map_err(e)?;
        vin.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).map_err(e)?;
        vin.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12).map_err(e)?;
        vin.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32).map_err(e)?;
        vin.SetUINT64(&MF_MT_FRAME_SIZE, pack(w as u32, h as u32)).map_err(e)?;
        vin.SetUINT64(&MF_MT_FRAME_RATE, pack(fps, 1)).map_err(e)?;
        vin.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1)).map_err(e)?;
        writer.SetInputMediaType(vi, &vin, None).map_err(|er| format!("Videoformat: {er}"))?;

        // Audio
        let mut ai = 0;
        if audio {
            let aout = MFCreateMediaType().map_err(e)?;
            aout.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio).map_err(e)?;
            aout.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC).map_err(e)?;
            aout.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16).map_err(e)?;
            aout.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, RATE as u32).map_err(e)?;
            aout.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2).map_err(e)?;
            aout.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 24000).map_err(e)?;
            aout.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, 1).map_err(e)?;
            aout.SetUINT32(&MF_MT_AAC_AUDIO_PROFILE_LEVEL_INDICATION, 0x29).map_err(e)?;
            aout.SetUINT32(&MF_MT_AAC_PAYLOAD_TYPE, 0).map_err(e)?;
            ai = writer.AddStream(&aout).map_err(e)?;
            let ain = MFCreateMediaType().map_err(e)?;
            ain.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio).map_err(e)?;
            ain.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM).map_err(e)?;
            ain.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16).map_err(e)?;
            ain.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, RATE as u32).map_err(e)?;
            ain.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2).map_err(e)?;
            ain.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, 4).map_err(e)?;
            ain.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, (RATE * 4) as u32).map_err(e)?;
            writer.SetInputMediaType(ai, &ain, None).map_err(|er| format!("Tonformat: {er}"))?;
        }
        writer.BeginWriting().map_err(|er| format!("Start: {er}"))?;

        let mut renderer = Renderer::new();
        let mut mixer = Mixer::new();
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
                    let f = mixer.mix(p, a0 + a_done, n);
                    let mut pcm = Vec::with_capacity(n * 4);
                    for v in &f {
                        pcm.extend((((v * o.audio_gain).clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
                    }
                    let s = sample_from(&pcm, a_done * 10_000_000 / RATE, n as i64 * 10_000_000 / RATE)?;
                    writer.WriteSample(ai, &s).map_err(|er| format!("Ton schreiben: {er}"))?;
                    a_done = target;
                }
            }
        }
        writer.Finalize().map_err(|er| format!("Abschluss: {er}"))?;
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
