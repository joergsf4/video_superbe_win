//! Baut den H.265/HEVC-Decoder libde265 (LGPL, vendor/libde265) statisch in die EXE ein.
//! Windows hat für Programme ohne Installation keinen HEVC-Decoder; so laufen iPhone/GoPro-Videos trotzdem.

fn main() {
    let dir = "vendor/libde265";
    let mut b = cc::Build::new();
    b.cpp(true)
        .include("vendor")
        .include("vendor/libde265")
        .define("HAVE_SSE4_1", None)
        .flag("-msse4.1")
        .define("LIBDE265_STATIC_BUILD", None)
        .define("NDEBUG", None)
        .flag("-fno-exceptions")
        .flag("-O2")
        .warnings(false);
    let mut files: Vec<_> = ["", "/x86"]
        .iter()
        .flat_map(|sub| std::fs::read_dir(format!("{dir}{sub}")).expect("vendor/libde265"))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "cc"))
        .collect();
    files.sort();
    for f in &files {
        b.file(f);
        println!("cargo:rerun-if-changed={}", f.display());
    }
    b.compile("de265");
    // Bedingungsvariablen für Windows (C-Datei aus libde265/extra)
    cc::Build::new().file("vendor/extra/win32cond.c").warnings(false).compile("win32cond");
    println!("cargo:rerun-if-changed=vendor/extra/win32cond.c");
    // C++-Laufzeit (libc++ aus llvm-mingw), statisch
    println!("cargo:rustc-link-lib=static=c++");
    println!("cargo:rerun-if-changed=build.rs");
}
