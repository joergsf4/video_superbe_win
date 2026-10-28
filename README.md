# Video superbe

**Ein schlanker Videoeditor für Windows 11: eine einzelne EXE, keine Installation, keine Abhängigkeiten.**

Schneiden, überblenden, animieren, vertonen und als MP4 exportieren, auch mit iPhone- und GoPro-Material (HEVC/H.265,
HDR). Video superbe ist die Windows-Version des macOS-Programms *MacVideo Deluxe*; Projektdateien (`*.mvd.json`) lassen sich
zwischen beiden austauschen.

> ## ⚠️ Wichtiger Hinweis – bitte lesen
>
> **Video superbe ist ein privates Hobbyprojekt, keine Software für den produktiven Einsatz.**
>
> Das Programm greift **nicht destruktiv** auf deine Daten zu: Originaldateien werden nur gelesen, nie verändert oder gelöscht, und Projekte und Exporte landen in neuen Dateien. **Aber:** Es gibt **keine Garantie** – weder für Fehlerfreiheit noch für Datensicherheit noch für Eignung für einen bestimmten Zweck. Deshalb: **Verwende niemals die einzigen Kopien deiner Videos**, sondern arbeite immer mit Kopien und bewahre die Originale an einem sicheren Ort auf. Die Nutzung erfolgt auf eigenes Risiko.

![Übersicht](docs/uebersicht.png)

## Download

Die fertige `VideoSuperbe.exe` (64 Bit, rund 8 MB) gibt es unter [Releases](../../releases). Herunterladen, irgendwo
ablegen, doppelklicken. Das Programm braucht nur Windows 11 (bzw. Windows 10 ab 1809) mit den eingebauten Komponenten
(Media Foundation, Direct3D 11) und legt keine Registry-Einträge an. Was es selbst erzeugt (Autosave, Aufnahmen,
Standbilder), liegt im Ordner `VideoSuperbe-Daten` neben der EXE.

## Funktionen

| | |
|---|---|
| **Timeline** | beliebig viele Spuren, Schneiden, Verschieben (mit Positionsanzeige), Kürzen mit Kantenvorschau, Mehrfachauswahl, Zwischenablage, magnetisches Einrasten (Clips, Marken, Beat-Linien), Gruppen, Projektmarker 1–10, Bereichsleiste im Lineal, Abspielbereich als Schleife, Rückgängig/Wiederholen mit Liste der letzten Schritte (Rechtsklick), Kontextmenü mit allen Schnittbefehlen |
| **Spurkopf** | Zielspur für das Einfügen (Klick), Name (Doppelklick), Spurfarbe, Spurhöhe, Lautstärke und Deckkraft der ganzen Spur, Spur einfügen/löschen (Rechtsklick) |
| **Storyboard** | alle Szenen der Hauptspur als Raster, Umsortieren per Ziehen (Blenden bleiben an ihrer Stelle), Anzeigedauer der Fotos (Strg+Alt+B) |
| **Schnittlogik** | Einfügemodi (automatisch, Ripple, Austauschen, Überschreiben), Mausmodi (Auswahl, Ripple-Trim, Rolle, Stretchen, Objektinhalt verschieben, Schneiden), J-/L-Schnitt (Alt-Ziehen), Rollschnitt (Strg-Ziehen), Lückensuche und -schließen, Drei-/Vierpunktschnitt aus dem Quellmonitor, Zeitcode eintippen, Taktschnitt (Schnitte und Blendenmitten auf Beats), Tempo ändern rückt Nachfolger nach |
| **Medienpool** | Videos, Bilder und Ton per Dialog oder Drag & Drop; Ansichten (große/kleine Symbole, Liste, Details), Sortierung, Filter, Suche; Auswahl wie im Explorer (Strg/Umschalt, Auswahlrahmen, Pfeiltasten, Strg+A), mehrere Medien auf einmal ziehen; Doppelklick hängt ans Ende an; doppelte Dateien werden erkannt; das erste Video legt Format (auch 9:16) und Bildrate fest; Quellmaterial lokal sichern (Bearbeiten ▸ Medien lokal sammeln) |
| **Quellmonitor** | Medium ansehen, Anfang/Ende wählen (I/O), Auswahl auf die Timeline ziehen oder einfügen |
| **Blenden** | Überblendung, Abblenden, Blitz, Wischen (seitlich/von oben), Schieben, Zoom, Iris; Kacheln per Drag & Drop auf die Überlappung, Dauer 0,5–2 s |
| **Bild** | Position, Größe, Drehung, Beschnitt, Spiegeln, 90°-Drehung, Horizont-Ausrichtung mit Hilfslinien, Look (Helligkeit, Kontrast, Sättigung, Wärme, Schärfe, Weichzeichner, Vignette), 3D-LUT (`.cube`), Greenscreen (Chroma-Key), **Stabilisierung** (eigene Bewegungsmessung), Effekte kopieren/einfügen, jeder Abschnitt einzeln zurücksetzbar |
| **Masken** | neun Formen (Rechteck, abgerundet, Ellipse, Dreieck, Raute, Sechseck, Stern, Herz, Rahmen) und drei Verläufe (linear, Spiegel, radial) mit weicher Kante, Größe, Lage, Drehung, Invertieren; auch für Titel |
| **Fotos** | Anzeigedauer (einzeln oder für alle; Titel, Musik und Kamerafahrten rücken mit), Auto-Verbessern (Tonwerte, Farbstich), Rauschentfernung |
| **Animator** | Keyframes für alle Kanäle mit Interpolation (linear, Bézier, Halten), Objektrahmen direkt im Monitor, **Zoom auf Bereich** (Rahmen im Monitor aufziehen), Bewegungs-Voreinstellungen auch für mehrere Clips; der Animator erscheint nur auf Wunsch |
| **Titel** | Schrift (fett, kursiv, unterstrichen, Laufweite, Zeilenabstand), Schatten, Umrandung, Hintergrund, Bauchbinde, Ein-/Auslaufen (Fliegen, Zoom, Schreibmaschine), Abspann, Laufschrift, Platzhalter (`{datum}`, `{timecode}` …), Vorlagen, Untertitel-Clips, SRT-Export |
| **Ton** | Wellenform, Lautstärkekurve im Clip, Ein-/Ausblenden, Geschwindigkeit, Windfilter, Rauschminderung, Pegelanzeige |
| **Automatik** | Musik bei Sprache abdunkeln (Ducking), Beat-Erkennung, **Ton-Synchronisation** zweier Aufnahmen, **Szenenerkennung** (Clip an Schnitten teilen), Sprecher aufnehmen |
| **Messinstrumente** | Histogramm und RGB-Parade (Ansicht ▸ Messinstrumente) |
| **Ansicht** | Vollbildvorschau (Doppelklick auf den Monitor oder Alt+Enter) |
| **Sonstiges** | Standbild einfügen und exportieren, Kapitelmarken für YouTube, Autosave mit Wiederherstellung, fehlende Medien neu verknüpfen, Zwischenspeicher-Übersicht (Datei ▸ Zwischenspeicher) |
| **Export** | MP4 (H.264 + AAC) mit Voreinstellungen (auch eigene), Auflösung (kurze Seite, auch Hochformat), Bildrate, Qualität oder feste Bitrate, CBR/VBR, Schlüsselbildabstand, Profil High/Main, AAC 96–192 kbit/s, Mono, Fast Start, Titel/Autor, YouTube-Kapitelliste; nur der Bereich zwischen den Marken; Lautheit normalisieren (−14 / −16 / −23 LUFS, ITU-R BS.1770) mit Limiter (−1 dBFS) und Nachmessen |

Die Vorschau benutzt denselben Renderer wie der Export: Was man sieht, kommt auch in die Datei. Der Renderer rechnet in parallelen Bildbändern auf allen Prozessorkernen und setzt das Vorschaubild nur neu zusammen, wenn sich etwas ändert. Mit `VSB_PERF=1` schreibt das Programm ein Leistungsprotokoll (`perf.log` im Datenordner).

<table>
<tr>
<td><img src="docs/quellmonitor.png" alt="Quellmonitor"><br><sub>Quellmonitor mit Anfangs- und Endpunkt</sub></td>
<td><img src="docs/export.png" alt="Export"><br><sub>Export mit Lautheits-Normalisierung</sub></td>
</tr>
<tr>
<td><img src="docs/titel.png" alt="Titel"><br><sub>Titel: Schrift, Schatten, Umrandung, Bewegung, Vorlagen</sub></td>
<td><img src="docs/ton.png" alt="Ton"><br><sub>Lautstärkekurve (Ducking), Beat-Linien, Tonbearbeitung</sub></td>
</tr>
<tr>
<td><img src="docs/blenden.png" alt="Blenden"><br><sub>Blenden-Kacheln und Messinstrumente (Histogramm, RGB-Parade)</sub></td>
<td><img src="docs/messinstrumente.png" alt="Effekte"><br><sub>Effekte, Animator und Messinstrumente</sub></td>
</tr>
</table>

## Wichtige Tasten

Wie in der Mac-Version, mit Strg statt ⌘ und Alt statt ⌥.

| Taste | Wirkung |
|---|---|
| Leertaste, K, L, J | Wiedergabe/Pause, anhalten, abspielen, 1 s zurück |
| S oder T | Teilen an der Abspielposition |
| Z / U | Clip-Anfang / -Ende auf die Abspielposition kürzen |
| 1 / 2 / 3 / 4 / 5 | Einfügemodus: automatisch, Einspur-Ripple, Mehrspur-Ripple, Austauschen, Überschreiben |
| 6 / 7 / 8 / 0, Esc | Mausmodus: Auswahl, Ripple-Trim, Rolle, Stretchen; Esc zurück zum Standard |
| Umschalt+Ziffer / Strg+Ziffer | Projektmarker 1–10 setzen / dorthin springen |
| `<` / `>`, X, Strg+Umschalt+C | Lücke davor/danach, Lücke markieren, nächste Lücke suchen |
| W / Q | zum nächsten / vorigen Marker (Alt: Schnittpunkt, Umschalt: gewählter Clip) |
| D, F, Alt+F, N | Duplizieren, Zoom auf alles, Zoom auf Bereich, Einrasten |
| Alt+Pfeile | gewählte Clips verschieben |
| G / Umschalt+G | Gruppieren / Gruppe lösen |
| Strg+Alt+C / V | Effekte kopieren / einfügen |
| Umschalt+Z | Szenen erkennen und teilen |
| Strg+Umschalt+M | Schnitte auf Beats legen |
| Strg+Alt+B / F / L | Storyboard, Zoom auf die gewählten Objekte, Abspielbereich als Schleife |
| Alt+Enter | Vollbildvorschau |

Nicht enthalten (sie brauchen Apples Frameworks): Person freistellen, KI-Verbesserung (Rauschfilter und Hochskalieren mit
VideoToolbox), Hintergrund-Effekte hinter dem Motiv und Smart-Kamerafahrt (Motiv- und Gesichtserkennung mit Vision);
außerdem Webcam- und Bildschirmaufnahme, rückwärts abspielen, HEVC-Export und automatische Untertitel per Spracherkennung.
Projekte mit diesen Einstellungen lassen sich trotzdem öffnen und bearbeiten; die Einstellungen bleiben in der Datei erhalten.

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

Scheitern die Build-Skripte der Abhängigkeiten mit `unable to find library -lgcc`, hilft
`$env:LIBRARY_PATH = "<Repo>\shim64"` (die Linker-Einstellung `-L shim64` gilt nur für das Programm selbst).

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
