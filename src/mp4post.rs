//! Nachbearbeitung der exportierten MP4-Datei: „Fast Start“ (Index `moov` vor die Mediendaten, damit Streaming sofort
//! beginnen kann) und Metadaten (Titel, Autor als iTunes-Atome `©nam`/`©ART`). Die Mediendaten werden nur kopiert, nie
//! verändert; die Chunk-Offsets (`stco`/`co64`) werden auf die neue Lage umgerechnet.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Oberste Box der Datei: Typ, Anfang und Länge (samt Kopf).
#[derive(Clone, Debug)]
struct Top {
    kind: [u8; 4],
    start: u64,
    len: u64,
}

fn read_tops(f: &mut File) -> Result<Vec<Top>, String> {
    let total = f.metadata().map_err(|e| e.to_string())?.len();
    let mut v = vec![];
    let mut pos = 0u64;
    while pos + 8 <= total {
        f.seek(SeekFrom::Start(pos)).map_err(|e| e.to_string())?;
        let mut h = [0u8; 16];
        f.read_exact(&mut h[..8]).map_err(|e| e.to_string())?;
        let mut len = u32::from_be_bytes([h[0], h[1], h[2], h[3]]) as u64;
        let kind = [h[4], h[5], h[6], h[7]];
        if len == 1 {
            f.read_exact(&mut h[8..16]).map_err(|e| e.to_string())?;
            len = u64::from_be_bytes(h[8..16].try_into().unwrap());
        } else if len == 0 {
            len = total - pos;
        }
        if len < 8 || pos + len > total {
            return Err("MP4-Struktur beschädigt".into());
        }
        v.push(Top { kind, start: pos, len });
        pos += len;
    }
    Ok(v)
}

fn be32(b: &[u8], i: usize) -> u32 {
    u32::from_be_bytes(b[i..i + 4].try_into().unwrap())
}

/// Container, in denen die Chunk-Tabellen bzw. Metadaten liegen.
const CONTAINERS: [&[u8; 4]; 6] = [b"moov", b"trak", b"mdia", b"minf", b"stbl", b"udta"];

/// Rechnet alle Chunk-Offsets im Inhalt einer Box (ohne Kopf) um. `map` liefert zum alten Offset den neuen.
fn patch_offsets(body: &mut [u8], map: &dyn Fn(u64) -> u64) -> Result<(), String> {
    let mut i = 0;
    while i + 8 <= body.len() {
        let len = be32(body, i) as usize;
        if len < 8 || i + len > body.len() {
            return Err("MP4-Box beschädigt".into());
        }
        let kind: [u8; 4] = body[i + 4..i + 8].try_into().unwrap();
        let inner = &mut body[i + 8..i + len];
        if CONTAINERS.contains(&&kind) {
            patch_offsets(inner, map)?;
        } else if &kind == b"stco" && inner.len() >= 8 {
            let n = be32(inner, 4) as usize;
            for k in 0..n.min((inner.len() - 8) / 4) {
                let o = 8 + k * 4;
                let nv = map(be32(inner, o) as u64);
                let nv: u32 = nv.try_into().map_err(|_| "Datei zu groß für 32-Bit-Offsets".to_string())?;
                inner[o..o + 4].copy_from_slice(&nv.to_be_bytes());
            }
        } else if &kind == b"co64" && inner.len() >= 8 {
            let n = be32(inner, 4) as usize;
            for k in 0..n.min((inner.len() - 8) / 8) {
                let o = 8 + k * 8;
                let nv = map(u64::from_be_bytes(inner[o..o + 8].try_into().unwrap()));
                inner[o..o + 8].copy_from_slice(&nv.to_be_bytes());
            }
        }
        i += len;
    }
    Ok(())
}

fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(body.len() + 8);
    v.extend(((body.len() + 8) as u32).to_be_bytes());
    v.extend(kind);
    v.extend(body);
    v
}

/// `meta`-Box (iTunes-Stil) mit Titel und Autor.
fn meta_box(title: &str, author: &str) -> Vec<u8> {
    let item = |kind: &[u8; 4], text: &str| {
        let mut data = vec![0, 0, 0, 1, 0, 0, 0, 0]; // Typ 1 = UTF-8, Sprache 0
        data.extend(text.as_bytes());
        boxed(kind, &boxed(b"data", &data))
    };
    let mut ilst = vec![];
    if !title.is_empty() {
        ilst.extend(item(b"\xA9nam", title));
    }
    if !author.is_empty() {
        ilst.extend(item(b"\xA9ART", author));
    }
    let mut hdlr = vec![0u8; 8]; // Version/Flags, pre_defined
    hdlr.extend(b"mdir");
    hdlr.extend(b"appl");
    hdlr.extend([0u8; 9]); // reserviert (2×4) und leerer Name
    let mut meta = vec![0u8; 4]; // Version/Flags
    meta.extend(boxed(b"hdlr", &hdlr));
    meta.extend(boxed(b"ilst", &ilst));
    boxed(b"meta", &meta)
}

/// Hängt die Metadaten an die `moov`-Box (Inhalt ohne Kopf) an; eine vorhandene `udta` wird erweitert.
fn add_metadata(moov: &mut Vec<u8>, title: &str, author: &str) {
    if title.is_empty() && author.is_empty() {
        return;
    }
    let meta = meta_box(title, author);
    let mut i = 0;
    while i + 8 <= moov.len() {
        let len = be32(moov, i) as usize;
        if len < 8 || i + len > moov.len() {
            break;
        }
        if &moov[i + 4..i + 8] == b"udta" {
            let new_len = (len + meta.len()) as u32;
            moov.splice(i + len..i + len, meta.iter().copied());
            moov[i..i + 4].copy_from_slice(&new_len.to_be_bytes());
            return;
        }
        i += len;
    }
    moov.extend(boxed(b"udta", &meta));
}

/// Schreibt die Datei um: `moov` direkt hinter `ftyp` (bei `fast_start`) und mit Metadaten. Ohne Änderungsbedarf bleibt die
/// Datei unberührt. Die Mediendaten werden blockweise kopiert (auch sehr große Dateien brauchen wenig Speicher).
pub fn finish(path: &Path, fast_start: bool, title: &str, author: &str) -> Result<(), String> {
    let has_meta = !title.trim().is_empty() || !author.trim().is_empty();
    let mut f = File::open(path).map_err(|e| e.to_string())?;
    let tops = read_tops(&mut f)?;
    let Some(mi) = tops.iter().position(|t| &t.kind == b"moov") else { return Err("MP4 ohne moov".into()) };
    let first_data = tops.iter().position(|t| &t.kind == b"mdat");
    let moov_after_data = first_data.is_some_and(|d| d < mi);
    if !has_meta && !(fast_start && moov_after_data) {
        return Ok(());
    }
    let moov_top = tops[mi].clone();
    let hdr = if moov_top.len > u32::MAX as u64 { return Err("moov zu groß".into()) } else { 8u64 };
    let mut body = vec![0u8; (moov_top.len - hdr) as usize];
    f.seek(SeekFrom::Start(moov_top.start + hdr)).map_err(|e| e.to_string())?;
    f.read_exact(&mut body).map_err(|e| e.to_string())?;
    add_metadata(&mut body, title.trim(), author.trim());
    let new_moov_len = body.len() as u64 + 8;

    // neue Reihenfolge: moov nach ftyp (Fast Start) bzw. an seiner Stelle
    let mut order: Vec<Option<usize>> = tops.iter().enumerate().filter(|(i, _)| *i != mi).map(|(i, _)| Some(i)).collect();
    let insert_at = if fast_start { order.iter().position(|o| o.is_some_and(|i| &tops[i].kind != b"ftyp")).unwrap_or(order.len()) } else { order.iter().position(|o| o.is_some_and(|i| i > mi)).unwrap_or(order.len()) };
    order.insert(insert_at, None);
    // neue Lage jeder Box → Verschiebung je alter Box
    let mut shifts: Vec<(u64, u64, i64)> = vec![]; // (alter Anfang, Länge, Verschiebung)
    let mut pos = 0u64;
    for o in &order {
        match o {
            Some(i) => {
                shifts.push((tops[*i].start, tops[*i].len, pos as i64 - tops[*i].start as i64));
                pos += tops[*i].len;
            }
            None => pos += new_moov_len,
        }
    }
    let map = |off: u64| -> u64 {
        match shifts.iter().find(|(s, l, _)| off >= *s && off < s + l) {
            Some((_, _, d)) => (off as i64 + d) as u64,
            None => off,
        }
    };
    patch_offsets(&mut body, &map)?;

    let tmp = path.with_extension("mp4.tmp");
    {
        let mut out = std::io::BufWriter::new(File::create(&tmp).map_err(|e| e.to_string())?);
        for o in &order {
            match o {
                Some(i) => {
                    f.seek(SeekFrom::Start(tops[*i].start)).map_err(|e| e.to_string())?;
                    std::io::copy(&mut (&mut f).take(tops[*i].len), &mut out).map_err(|e| e.to_string())?;
                }
                None => {
                    out.write_all(&(new_moov_len as u32).to_be_bytes()).map_err(|e| e.to_string())?;
                    out.write_all(b"moov").map_err(|e| e.to_string())?;
                    out.write_all(&body).map_err(|e| e.to_string())?;
                }
            }
        }
        out.flush().map_err(|e| e.to_string())?;
    }
    drop(f);
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

/// Reihenfolge der obersten Boxen (für Tests und Diagnose).
pub fn top_level_kinds(path: &Path) -> Result<Vec<String>, String> {
    let mut f = File::open(path).map_err(|e| e.to_string())?;
    Ok(read_tops(&mut f)?.iter().map(|t| String::from_utf8_lossy(&t.kind).into_owned()).collect())
}

/// Liest Titel und Autor (iTunes-Atome) aus der Datei.
pub fn read_metadata(path: &Path) -> Option<(String, String)> {
    let data = std::fs::read(path).ok()?;
    let find = |k: &[u8; 4]| -> String {
        data.windows(4).position(|w| w == k).and_then(|i| {
            // ©nam/©ART → data-Box: Länge(4) "data"(4) Typ(4) Sprache(4) Text
            let d = i + 4;
            (data.get(d + 4..d + 8)? == b"data").then_some(())?;
            let len = be32(&data, d) as usize;
            Some(String::from_utf8_lossy(data.get(d + 16..d + len)?).into_owned())
        }).unwrap_or_default()
    };
    Some((find(b"\xA9nam"), find(b"\xA9ART")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Kleine künstliche MP4: ftyp, mdat (zwei Chunks), moov mit trak/mdia/minf/stbl/stco auf die beiden Chunks.
    fn fake_mp4(path: &Path) -> (u32, u32) {
        let ftyp = boxed(b"ftyp", b"isom\0\0\x02\0isomiso2mp41");
        let payload = b"AAAABBBBBBBB";
        let mdat = boxed(b"mdat", payload);
        let c1 = (ftyp.len() + 8) as u32;
        let c2 = c1 + 4;
        let mut stco = vec![0u8; 4];
        stco.extend(2u32.to_be_bytes());
        stco.extend(c1.to_be_bytes());
        stco.extend(c2.to_be_bytes());
        let stbl = boxed(b"stbl", &boxed(b"stco", &stco));
        let moov = boxed(b"moov", &boxed(b"trak", &boxed(b"mdia", &boxed(b"minf", &stbl))));
        let mut f = ftyp;
        f.extend(mdat);
        f.extend(moov);
        std::fs::write(path, f).unwrap();
        (c1, c2)
    }

    /// Liest die Chunk-Offsets und prüft, dass dort noch dieselben Daten liegen.
    fn chunk_data(path: &Path) -> Vec<Vec<u8>> {
        let d = std::fs::read(path).unwrap();
        let i = d.windows(4).position(|w| w == b"stco").unwrap() + 4;
        let n = be32(&d, i + 4) as usize;
        (0..n).map(|k| {
            let o = be32(&d, i + 8 + k * 4) as usize;
            d[o..o + 4].to_vec()
        }).collect()
    }

    #[test]
    fn fast_start_moves_moov_and_fixes_offsets() {
        let dir = std::env::temp_dir().join("video-superbe-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("faststart.mp4");
        fake_mp4(&p);
        assert_eq!(top_level_kinds(&p).unwrap(), vec!["ftyp", "mdat", "moov"]);
        let before = chunk_data(&p);
        assert_eq!(before, vec![b"AAAA".to_vec(), b"BBBB".to_vec()]);
        finish(&p, true, "", "").unwrap();
        assert_eq!(top_level_kinds(&p).unwrap(), vec!["ftyp", "moov", "mdat"]);
        assert_eq!(chunk_data(&p), before, "Offsets zeigen auf dieselben Daten");
        // zweites Mal: nichts zu tun, Datei bleibt gleich
        let bytes = std::fs::read(&p).unwrap();
        finish(&p, true, "", "").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), bytes);
    }

    #[test]
    fn metadata_is_written_with_and_without_fast_start() {
        let dir = std::env::temp_dir().join("video-superbe-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("meta.mp4");
        fake_mp4(&p);
        let before = chunk_data(&p);
        finish(&p, false, "Sommer am See", "Jörg").unwrap();
        assert_eq!(top_level_kinds(&p).unwrap(), vec!["ftyp", "mdat", "moov"], "ohne Fast Start bleibt die Reihenfolge");
        assert_eq!(read_metadata(&p).unwrap(), ("Sommer am See".to_string(), "Jörg".to_string()));
        assert_eq!(chunk_data(&p), before);
        // mit Fast Start: Offsets wachsen um das größere moov
        fake_mp4(&p);
        finish(&p, true, "Titel", "").unwrap();
        assert_eq!(top_level_kinds(&p).unwrap(), vec!["ftyp", "moov", "mdat"]);
        assert_eq!(chunk_data(&p), before);
        assert_eq!(read_metadata(&p).unwrap().0, "Titel");
    }
}
