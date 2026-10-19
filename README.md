# Video superbe

**Ein schlanker Videoeditor für Windows 11: eine einzelne EXE, keine Installation, keine Abhängigkeiten.**

Schneiden, überblenden, animieren, vertonen und als MP4 exportieren, auch mit iPhone- und GoPro-Material (HEVC/H.265,
HDR). Video superbe ist die Windows-Version des macOS-Programms *MacVideo Deluxe*; Projektdateien (`*.mvd.json`) lassen sich
zwischen beiden austauschen.

![Übersicht](docs/uebersicht.png)

## Download

Die fertige `VideoSuperbe.exe` (64 Bit, rund 8 MB) gibt es unter [Releases](../../releases). Herunterladen, irgendwo
ablegen, doppelklicken. Das Programm braucht nur Windows 11 (bzw. Windows 10 ab 1809) mit den eingebauten Komponenten
(Media Foundation, Direct3D 11) und legt keine Registry-Einträge an. Was es selbst erzeugt (Autosave, Aufnahmen,
Standbilder), liegt im Ordner `VideoSuperbe-Daten` neben der EXE.

## Funktionen

| | |
|---|---|
| **Timeline** | beliebig viele Spuren, Schneiden, Verschieben, Kürzen mit Kantenvorschau, Ripple-Löschen, Mehrfachauswahl, Zwischenablage, magnetisches Einrasten (Clips, Marken, Beat-Linien), Überblendungen und Übergänge, Rückgängig/Wiederholen |
| **Medienpool** | Videos, Bilder und Ton per Dialog oder Drag & Drop; doppelte Dateien werden erkannt; das erste Video legt Format (auch 9:16) und Bildrate fest |
| **Quellmonitor** | Medium ansehen, Anfang/Ende wählen (I/O), Auswahl auf die Timeline ziehen oder einfügen |
| **Bild** | Position, Größe, Drehung, Beschnitt, Look (Helligkeit, Kontrast, Sättigung, Wärme, Schärfe, Weichzeichner, Vignette), Voreinstellungen |
| **Animator** | Keyframes für alle Kanäle mit Interpolation (linear, Bézier, Halten), Objektrahmen direkt im Monitor |
| **Titel** | Text, Schriftart, Größe, Farbe, Kontur, Hintergrund, Bauchbinde, Untertitel-Clips, SRT-Export |
| **Ton** | Wellenform, Lautstärkekurve im Clip, Ein-/Ausblenden, Geschwindigkeit, Windfilter, Rauschminderung, Pegelanzeige |
| **Automatik** | Musik bei Sprache abdunkeln (Ducking), Beat-Erkennung mit Beat-Linien, Sprecher aufnehmen |
| **Sonstiges** | Standbild einfügen und exportieren, Kapitelmarken für YouTube, Autosave mit Wiederherstellung, fehlende Medien neu verknüpfen |
| **Export** | MP4 (H.264 + AAC), Qualität wählbar, nur der Bereich zwischen den Marken, Lautheit auf Zielwert normalisieren (−14 LUFS, ITU-R BS.1770) |

Die Vorschau benutzt denselben Renderer wie der Export: Was man sieht, kommt auch in die Datei.

<table>
<tr>
<td><img src="docs/quellmonitor.png" alt="Quellmonitor"><br><sub>Quellmonitor mit Anfangs- und Endpunkt</sub></td>
<td><img src="docs/export.png" alt="Export"><br><sub>Export mit Lautheits-Normalisierung</sub></td>
</tr>
<tr>
<td><img src="docs/titel.png" alt="Titel"><br><sub>Titel, Bauchbinde, Schriftart und Kontur</sub></td>
<td><img src="docs/ton.png" alt="Ton"><br><sub>Lautstärkekurve (Ducking), Beat-Linien, Tonbearbeitung</sub></td>
</tr>
</table>

Nicht enthalten: Person freistellen und automatische Untertitel per Spracherkennung (beides braucht Modelle bzw. Dienste).

## HEVC / H.265 (iPhone, GoPro) und HDR

Windows hat für Programme ohne Installation keinen HEVC-Decoder. Video superbe bringt deshalb einen eigenen Weg mit:

- **Grafikkarte**: `src/hevc_gpu.rs` liest Parametersätze und Slice-Köpfe selbst und übergibt die Slices über die allgemeine
  Direct3D-11-Video-API (DXVA) an die Grafikkarte. Das ist herstellerunabhängig (Intel, NVIDIA, AMD); bisher wurde nur Intel
  getestet. Vor der ersten Benutzung vergleicht das Programm je Datei ein Bild der Grafikkarte mit dem des Software-Decoders
  und fällt bei Abweichung automatisch auf diesen zurück. Unterstützt: Main und Main10 (4:2:0, 8/10 Bit).
- **Software-Decoder**: [libde265](https://github.com/strukturag/libde265) (LGPL-3.0), statisch eingebunden, braucht SSE4.1 (jeder
  Intel- und AMD-Prozessor seit etwa 2008), Full-HD etwa 10–20 Bilder pro Sekunde. Erzwingen mit `VSB_HEVC_CPU=1`.
- **HDR (HLG, PQ)**: BT.2020-Material wird beim Decodieren nach SDR/BT.709 umgerechnet (Tonemapping mit weichem Spitzenlicht),
  damit die Farben nicht flach wirken.
- Die Vorschau decodiert im Hintergrund mit Vorlauf; die Oberfläche blockiert nie.

## Selbst bauen

Voraussetzung: Rust (`rustup`, Toolchain siehe `rust-toolchain.toml`) und llvm-mingw
(`winget install MartinStorsjo.LLVM-MinGW.MSVCRT`), nur zum Bauen. Die Linker-Einstellungen stehen in `.cargo\config.toml`
(Ersatz für die in llvm-mingw fehlende libgcc in `shim64\`). `dlltool.exe` muss im PATH liegen (Kopie von `llvm-dlltool.exe`
nach `%USERPROFILE%\.cargo\bin`).

```powershell
cargo build --release     # target\x86_64-pc-windows-gnu\release\video-superbe.exe
```

## Tests

```powershell
powershell -File tests\run-each.ps1      # jeden Test einzeln (ein Absturz reißt nicht alle mit)
powershell -File tests\e2e.ps1 -Exe target\x86_64-pc-windows-gnu\release\video-superbe.exe
```

- `cargo test`: Modell und Schnittregeln, Keyframes, Audio (Decodieren, Mischen, Geschwindigkeit, Filter), Lautheit
  (Referenzwerte der Norm), Analyse (Sprache, Beats, Ducking), Renderer, Export (Zurücklesen, Bereich, Verstärkung, HEVC),
  HEVC (GPU gegen Software, HDR), Quellmonitor, Aufnahme, Autosave, Relink, Standbild, SRT.
- GUI-Tests (`src/gui_tests.rs`): die komplette Oberfläche läuft ohne Fenster; eingespeiste Maus- und Tastaturereignisse prüfen
  die Bedienung.
- `tests/e2e.ps1`: startet die Release-EXE, exportiert headless ein Projekt, liest es zurück, prüft 64 Bit und dass nur
  Windows-eigene DLLs importiert werden.
- Tests mit echten HEVC-Dateien suchen sie im Ordner `Videos\test_vsp` und werden ohne diese Dateien übersprungen.

## Kommandozeile

```
VideoSuperbe.exe [projekt.mvd.json]
VideoSuperbe.exe --export projekt.mvd.json ausgabe.mp4     # Ergebnis in ausgabe.mp4.log
VideoSuperbe.exe --videotest datei.mp4 [sekunden ...]      # schreibt videotest.txt
VideoSuperbe.exe --gpuinfo                                 # Grafikkarte und HEVC-Unterstützung
```

## Lizenzen von Drittkomponenten

- libde265 (`vendor/libde265`): LGPL-3.0, Lizenztext `vendor/libde265/COPYING`, Änderung siehe `vendor/libde265/ABOUT.txt`.
  Der Quellcode liegt diesem Repository bei; die EXE bindet ihn statisch ein.
- Rust-Bibliotheken (eframe/egui, image, rfd, serde, uuid, ab_glyph, windows) unter MIT bzw. Apache-2.0.
