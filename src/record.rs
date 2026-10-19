//! Sprecheraufnahme über das Standardmikrofon (waveIn, in Windows eingebaut): 48 kHz, mono, 16 Bit.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use windows::Win32::Media::Audio::*;
use windows::Win32::Media::MMSYSERR_NOERROR;

pub const REC_RATE: u32 = 48000;
const BUF_FRAMES: usize = 4800; // 100 ms
const NBUF: usize = 4;

pub struct Recorder {
    stop: Arc<AtomicBool>,
    samples: Arc<Mutex<Vec<i16>>>,
    /// Spitzenwert des letzten Blocks (linear, als f32-Bits)
    level: Arc<AtomicU32>,
    handle: Option<std::thread::JoinHandle<()>>,
    pub started: Instant,
}

struct In(HWAVEIN);
// Das Handle wird ausschließlich vom Aufnahme-Thread benutzt.
unsafe impl Send for In {}

impl Recorder {
    /// Startet die Aufnahme; Fehler (kein Mikrofon, Zugriff verweigert) kommen als Text zurück.
    pub fn start() -> Result<Recorder, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let samples = Arc::new(Mutex::new(Vec::<i16>::new()));
        let level = Arc::new(AtomicU32::new(0));
        let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
        let (s2, d2, l2) = (stop.clone(), samples.clone(), level.clone());
        let handle = std::thread::spawn(move || run(tx, s2, d2, l2));
        match rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Recorder { stop, samples, level, handle: Some(handle), started: Instant::now() }),
            Ok(Err(e)) => {
                let _ = handle.join();
                Err(e)
            }
            Err(_) => Err("Das Mikrofon antwortet nicht.".into()),
        }
    }

    pub fn elapsed(&self) -> f64 {
        self.samples.lock().map(|s| s.len()).unwrap_or(0) as f64 / REC_RATE as f64
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    /// Beendet die Aufnahme und liefert die Abtastwerte.
    pub fn finish(mut self) -> Vec<i16> {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        std::mem::take(&mut *self.samples.lock().unwrap())
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn run(tx: std::sync::mpsc::Sender<Result<(), String>>, stop: Arc<AtomicBool>, out: Arc<Mutex<Vec<i16>>>, level: Arc<AtomicU32>) {
    unsafe {
        let fmt = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_PCM as u16,
            nChannels: 1,
            nSamplesPerSec: REC_RATE,
            nAvgBytesPerSec: REC_RATE * 2,
            nBlockAlign: 2,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        let mut hwi = HWAVEIN::default();
        let r = waveInOpen(Some(&mut hwi), WAVE_MAPPER, &fmt, None, None, CALLBACK_NULL);
        if r != MMSYSERR_NOERROR {
            let _ = tx.send(Err(if r == 6 || r == 2 {
                "Kein Mikrofon gefunden. Bitte eines anschließen und unter Einstellungen ▸ Datenschutz ▸ Mikrofon freigeben.".to_string()
            } else {
                format!("Das Mikrofon lässt sich nicht öffnen (Fehler {r}). Ist der Zugriff in den Windows-Datenschutzeinstellungen erlaubt?")
            }));
            return;
        }
        let wave = In(hwi);
        let hwi = wave.0;
        let hs = std::mem::size_of::<WAVEHDR>() as u32;
        let mut data: Vec<Vec<i16>> = (0..NBUF).map(|_| vec![0i16; BUF_FRAMES]).collect();
        let mut hdrs: Vec<WAVEHDR> = (0..NBUF).map(|_| WAVEHDR::default()).collect();
        for i in 0..NBUF {
            hdrs[i] = WAVEHDR { lpData: windows::core::PSTR(data[i].as_mut_ptr() as *mut u8), dwBufferLength: (BUF_FRAMES * 2) as u32, ..Default::default() };
            waveInPrepareHeader(hwi, &mut hdrs[i], hs);
            waveInAddBuffer(hwi, &mut hdrs[i], hs);
        }
        if waveInStart(hwi) != MMSYSERR_NOERROR {
            let _ = tx.send(Err("Die Aufnahme lässt sich nicht starten.".into()));
            waveInClose(hwi);
            return;
        }
        let _ = tx.send(Ok(()));
        let mut drain = |hdrs: &mut Vec<WAVEHDR>, data: &Vec<Vec<i16>>, requeue: bool| {
            for i in 0..NBUF {
                if hdrs[i].dwFlags & WHDR_DONE != 0 {
                    let n = hdrs[i].dwBytesRecorded as usize / 2;
                    let block = &data[i][..n.min(BUF_FRAMES)];
                    let peak = block.iter().map(|v| (*v as i32).unsigned_abs()).max().unwrap_or(0) as f32 / 32768.0;
                    level.store(peak.to_bits(), Ordering::Relaxed);
                    if let Ok(mut o) = out.lock() {
                        o.extend_from_slice(block);
                    }
                    hdrs[i].dwFlags &= !WHDR_DONE;
                    hdrs[i].dwBytesRecorded = 0;
                    if requeue {
                        waveInAddBuffer(hwi, &mut hdrs[i], hs);
                    }
                }
            }
        };
        while !stop.load(Ordering::Relaxed) {
            drain(&mut hdrs, &data, true);
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        waveInStop(hwi);
        waveInReset(hwi);
        drain(&mut hdrs, &data, false);
        for i in 0..NBUF {
            waveInUnprepareHeader(hwi, &mut hdrs[i], hs);
        }
        waveInClose(hwi);
    }
}

/// Schreibt 16-Bit-Mono-PCM als WAV-Datei.
pub fn write_wav(path: &std::path::Path, samples: &[i16], rate: u32) -> std::io::Result<()> {
    let data_len = (samples.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + samples.len() * 2);
    b.extend(b"RIFF");
    b.extend((36 + data_len).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(rate.to_le_bytes());
    b.extend((rate * 2).to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend(data_len.to_le_bytes());
    for s in samples {
        b.extend(s.to_le_bytes());
    }
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(path, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_roundtrip_is_readable_by_the_audio_decoder() {
        let dir = std::env::temp_dir().join("video-superbe-tests");
        let path = dir.join("rec_roundtrip.wav");
        let s: Vec<i16> = (0..48000).map(|i| ((i as f32 * 0.05).sin() * 12000.0) as i16).collect();
        write_wav(&path, &s, REC_RATE).unwrap();
        let d = crate::audio::probe(&path).unwrap();
        assert!((d - 1.0).abs() < 0.05, "{d}");
        let mut dec = crate::audio::AudioDecoder::open(&path).unwrap();
        let x = dec.read(0, 24000);
        let rms = (x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len() as f64).sqrt();
        assert!((rms - 12000.0 / 32768.0 / 2f64.sqrt()).abs() < 0.03, "{rms}");
    }

    #[test]
    fn recorder_starts_and_stops_or_reports_a_readable_error() {
        match Recorder::start() {
            Ok(r) => {
                std::thread::sleep(std::time::Duration::from_millis(350));
                assert!(r.elapsed() >= 0.1, "es kommen Daten an: {}", r.elapsed());
                let s = r.finish();
                assert!(s.len() >= 4800);
            }
            Err(e) => assert!(e.contains("Mikrofon") || e.contains("Aufnahme"), "verständliche Meldung: {e}"),
        }
    }
}
