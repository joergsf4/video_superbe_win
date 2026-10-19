//! Tonwiedergabe über waveOut (in Windows eingebaut). Ein Hintergrund-Thread mischt den Projektton
//! und liefert die Abspielposition als Uhr für die Vorschau.

use crate::audio::{Mixer, RATE};
use crate::model::Project;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::Arc;
use windows::Win32::Media::Audio::*;
use windows::Win32::Media::{MMSYSERR_NOERROR, MMTIME, TIME_SAMPLES};

const BUF_FRAMES: usize = 4800; // 100 ms
const NBUF: usize = 4;

pub struct Player {
    stop: Arc<AtomicBool>,
    /// Bereits abgespielte Frames seit Start (Audio-Uhr); -1 = Gerät nicht verfügbar.
    played: Arc<AtomicI64>,
    pub start: f64,
    /// Spitzenwerte (links, rechts; linear als f32-Bits) des gerade hörbaren 100-ms-Blocks
    levels: Arc<[AtomicU32; 2]>,
    handle: Option<std::thread::JoinHandle<()>>,
}

struct Wave(HWAVEOUT);
// HWAVEOUT ist nur ein Handle und wird ausschließlich vom Wiedergabe-Thread benutzt.
unsafe impl Send for Wave {}

impl Player {
    pub fn start(project: Project, start: f64) -> Player {
        let stop = Arc::new(AtomicBool::new(false));
        let played = Arc::new(AtomicI64::new(-1));
        let levels = Arc::new([AtomicU32::new(0), AtomicU32::new(0)]);
        let (s2, p2, l2) = (stop.clone(), played.clone(), levels.clone());
        let handle = std::thread::spawn(move || run(project, start, s2, p2, l2));
        Player { stop, played, start, levels, handle: Some(handle) }
    }

    /// true, sobald das Audiogerät tatsächlich Ton abspielt (Position > 0).
    pub fn clock_running(&self) -> bool {
        self.played.load(Ordering::Relaxed) > 0
    }

    /// Aktuelle Spitzenpegel (links, rechts), linear 0…1.
    pub fn levels(&self) -> [f32; 2] {
        [f32::from_bits(self.levels[0].load(Ordering::Relaxed)), f32::from_bits(self.levels[1].load(Ordering::Relaxed))]
    }

    /// Aktuelle Position in Sekunden laut Audio-Uhr, falls verfügbar.
    pub fn position(&self) -> Option<f64> {
        let p = self.played.load(Ordering::Relaxed);
        (p >= 0).then(|| self.start + p as f64 / RATE as f64)
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn run(project: Project, start: f64, stop: Arc<AtomicBool>, played: Arc<AtomicI64>, levels: Arc<[AtomicU32; 2]>) {
    crate::video::init();
    unsafe {
        let fmt = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_PCM as u16,
            nChannels: 2,
            nSamplesPerSec: RATE as u32,
            nAvgBytesPerSec: (RATE * 4) as u32,
            nBlockAlign: 4,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        let mut hwo = HWAVEOUT::default();
        if waveOutOpen(Some(&mut hwo), WAVE_MAPPER, &fmt, None, None, CALLBACK_NULL) != MMSYSERR_NOERROR {
            return; // kein Audiogerät: Vorschau läuft mit Systemuhr weiter
        }
        let wave = Wave(hwo);
        let hwo = wave.0;
        let mut mixer = Mixer::new();
        let mut data: Vec<Vec<i16>> = (0..NBUF).map(|_| vec![0i16; BUF_FRAMES * 2]).collect();
        let mut hdrs: Vec<WAVEHDR> = (0..NBUF).map(|_| WAVEHDR::default()).collect();
        let hs = std::mem::size_of::<WAVEHDR>() as u32;
        let mut next_frame = (start * RATE as f64) as i64;
        let first = next_frame;
        let end_frame = (project.duration() * RATE as f64) as i64 + RATE; // eine Sekunde Nachlauf
        let mut queued = [false; NBUF];
        // Startframe und Spitzenwerte der in den Puffern wartenden Blöcke (für die Pegelanzeige)
        let mut meta = [(0i64, [0f32; 2]); NBUF];
        played.store(0, Ordering::Relaxed);
        let mut fed_all = false;

        while !stop.load(Ordering::Relaxed) {
            let mut any_busy = false;
            for i in 0..NBUF {
                let busy = queued[i] && (hdrs[i].dwFlags & WHDR_DONE) == 0;
                if busy {
                    any_busy = true;
                    continue;
                }
                if queued[i] {
                    waveOutUnprepareHeader(hwo, &mut hdrs[i], hs);
                    queued[i] = false;
                }
                if fed_all || next_frame >= end_frame {
                    fed_all = true;
                    continue;
                }
                let f = mixer.mix(&project, next_frame, BUF_FRAMES);
                let mut pk = [0f32; 2];
                for (k, (d, v)) in data[i].iter_mut().zip(&f).enumerate() {
                    *d = (v.clamp(-1.0, 1.0) * 32767.0) as i16;
                    pk[k & 1] = pk[k & 1].max(v.abs().min(1.0));
                }
                meta[i] = (next_frame, pk);
                hdrs[i] = WAVEHDR {
                    lpData: windows::core::PSTR(data[i].as_mut_ptr() as *mut u8),
                    dwBufferLength: (BUF_FRAMES * 4) as u32,
                    ..Default::default()
                };
                if waveOutPrepareHeader(hwo, &mut hdrs[i], hs) == MMSYSERR_NOERROR
                    && waveOutWrite(hwo, &mut hdrs[i], hs) == MMSYSERR_NOERROR
                {
                    queued[i] = true;
                    any_busy = true;
                    next_frame += BUF_FRAMES as i64;
                }
            }
            let mut t = MMTIME { wType: TIME_SAMPLES, ..Default::default() };
            if waveOutGetPosition(hwo, &mut t, std::mem::size_of::<MMTIME>() as u32) == MMSYSERR_NOERROR && t.wType == TIME_SAMPLES {
                let pos = first + t.u.sample as i64;
                played.store(t.u.sample as i64, Ordering::Relaxed);
                let cur = (0..NBUF).find(|&i| queued[i] && pos >= meta[i].0 && pos < meta[i].0 + BUF_FRAMES as i64);
                let pk = cur.map(|i| meta[i].1).unwrap_or([0.0; 2]);
                levels[0].store(pk[0].to_bits(), Ordering::Relaxed);
                levels[1].store(pk[1].to_bits(), Ordering::Relaxed);
            }
            if fed_all && !any_busy {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        levels[0].store(0, Ordering::Relaxed);
        levels[1].store(0, Ordering::Relaxed);
        waveOutReset(hwo);
        for i in 0..NBUF {
            if queued[i] {
                waveOutUnprepareHeader(hwo, &mut hdrs[i], hs);
            }
        }
        waveOutClose(hwo);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::tests::write_sine_wav;
    use crate::model::*;

    #[test]
    fn audio_clock_advances_and_stops_cleanly() {
        let wav = write_sine_wav("player_tone.wav", 440.0, 3.0, 48000, 0.05);
        let mut p = Project::default();
        let a = MediaAsset { path: wav.to_string_lossy().into_owned(), name: "t".into(), duration: 3.0, has_audio: true, ..Default::default() };
        let aid = a.id;
        p.assets.push(a);
        p.tracks[0].clips.push(Clip { content: ClipContent::Media(Wrap { v: aid }), part: ClipPart::Audio, start: 0.0, duration: 3.0, ..Default::default() });
        let pl = Player::start(p, 1.0);
        std::thread::sleep(std::time::Duration::from_millis(1200));
        match pl.position() {
            None => eprintln!("kein Audiogerät, Uhrentest übersprungen"),
            Some(t) => {
                assert!(t > 1.3 && t < 2.6, "Audio-Uhr bei {t}");
                let l = pl.levels();
                assert!(l[0] > 0.03 && l[0] < 0.08 && l[1] > 0.03, "Pegel des 0,05-Tons: {l:?}");
            }
        }
        drop(pl); // darf nicht hängen
    }
}
