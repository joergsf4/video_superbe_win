#![windows_subsystem = "windows"]

mod anim;
mod app;
mod app_ui;
mod editing;
mod export;
mod fx_edit;
mod fxlib;
mod features;
mod hevc;
mod hevc_gpu;
#[cfg(test)]
mod gui_tests;
mod analysis;
mod audio;
mod loudness;
mod model;
mod navigation;
mod ops;
mod player;
mod record;
mod render;
mod source;
mod scopes;
mod scenes;
mod stabilize;
mod sync;
mod ui;
mod video;

fn main() -> eframe::Result {
    std::panic::set_hook(Box::new(|info| {
        let _ = std::fs::write(std::env::temp_dir().join("video-superbe-panic.txt"), format!("{info}
{}", std::backtrace::Backtrace::force_capture()));
    }));
    video::init();
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--export") && args.len() >= 4 {
        return export_cli(&args[2], &args[3]);
    }
    if args.get(1).map(String::as_str) == Some("--gpuinfo") {
        let _ = std::fs::write("gpuinfo.txt", video::gpu_info());
        return Ok(());
    }
    if args.get(1).map(String::as_str) == Some("--mftlist") {
        let _ = std::fs::write("mftlist.txt", video::list_decoders(&args[2]));
        return Ok(());
    }
    if args.get(1).map(String::as_str) == Some("--videotest") {
        return videotest(&args[2..]);
    }
    let opts = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Video superbe")
            .with_inner_size([1360.0, 820.0])
            .with_min_inner_size([900.0, 560.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    let arg = std::env::args().nth(1).map(std::path::PathBuf::from);
    eframe::run_native(
        "Video superbe",
        opts,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, arg.clone())))),
    )
}

/// Selbsttest ohne Fenster: `VideoSuperbe.exe --videotest datei.mp4 [Sekunden ...]` schreibt nach videotest.txt.
fn videotest(args: &[String]) -> eframe::Result {
    use std::fmt::Write;
    let mut out = String::new();
    let path = std::path::Path::new(&args[0]);
    let t_open = std::time::Instant::now();
    match video::Decoder::open(path) {
        Err(e) => writeln!(out, "FEHLER: {e}").unwrap(),
        Ok(mut d) => {
            writeln!(out, "{:?} (geöffnet in {:?})", d.info, t_open.elapsed()).unwrap(); let _ = std::fs::write("videotest.txt", &out);
            let ts: Vec<f64> = args[1..].iter().filter_map(|s| s.parse().ok()).collect();
            let t0 = std::time::Instant::now();
            for t in ts {
                let t1 = std::time::Instant::now();
                let r = d.frame_at(t);
                let ms = t1.elapsed().as_millis();
                match r {
                    Some(f) => {
                        let n = (f.rgba.len() / 4) as u64;
                        let sum: u64 = f.rgba.chunks(4).map(|p| p[0] as u64 + p[1] as u64 + p[2] as u64).sum();
                        let _ = std::fs::write("videotest.txt", &out); writeln!(out, "t={t}: Frame {}x{} @ {:.3}s, mittlere Helligkeit {:.1} ({ms} ms)", f.width, f.height, f.time, sum as f64 / (3 * n) as f64).unwrap();
                    }
                    None => writeln!(out, "t={t}: kein Bild").unwrap(),
                }
            }
            writeln!(out, "Dauer: {:?}", t0.elapsed()).unwrap();
        }
    }
    let _ = std::fs::write("videotest.txt", out);
    Ok(())
}


/// Headless-Export: `VideoSuperbe.exe --export projekt.mvd.json ausgabe.mp4`; Ergebnis in `<ausgabe>.log`.
fn export_cli(project: &str, out: &str) -> eframe::Result {
    let log = format!("{out}.log");
    let r = model::load(std::path::Path::new(project)).and_then(|p| {
        let o = export::ExportOpts::from_project(&p);
        export::export(&p, std::path::Path::new(out), &o, &mut |_| true)
    });
    let _ = std::fs::write(
        &log,
        match r {
            Ok(()) => "OK".to_string(),
            Err(e) => format!("FEHLER: {e}"),
        },
    );
    Ok(())
}
