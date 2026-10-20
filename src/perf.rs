//! Leistungsprotokoll der Vorschau (nur mit `VSB_PERF=1`): einmal pro Sekunde eine Zeile in `perf.log` im Datenordner
//! mit Anzahl gerechneter Bilder, Rechen- und Hochladezeit (Mittel/Maximum in ms) und der Bildgröße.

use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Default)]
struct Acc {
    since: Option<Instant>,
    n: u32,
    render: f64,
    render_max: f64,
    upload: f64,
}

static ACC: Mutex<Acc> = Mutex::new(Acc { since: None, n: 0, render: 0.0, render_max: 0.0, upload: 0.0 });

fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("VSB_PERF").is_ok_and(|v| v == "1"))
}

pub fn record(render: Duration, upload: Duration, w: usize, h: usize) {
    if !enabled() {
        return;
    }
    let mut a = ACC.lock().unwrap();
    let now = Instant::now();
    let since = *a.since.get_or_insert(now);
    let (r, u) = (render.as_secs_f64() * 1000.0, upload.as_secs_f64() * 1000.0);
    a.n += 1;
    a.render += r;
    a.upload += u;
    a.render_max = a.render_max.max(r);
    if now - since >= Duration::from_secs(1) {
        let line = format!("{:.1} Bilder/s  Rechnen {:.1} ms (max {:.1})  Hochladen {:.1} ms  {w}x{h}\n", a.n as f64 / (now - since).as_secs_f64(), a.render / a.n as f64, a.render_max, a.upload / a.n as f64);
        let dir = crate::features::default_data_dir();
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("perf.log")) {
            use std::io::Write;
            let _ = f.write_all(line.as_bytes());
        }
        *a = Acc { since: Some(now), ..Default::default() };
    }
}
