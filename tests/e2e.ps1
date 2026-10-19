# End-to-End-Test der fertigen EXE: (1) GUI startet, öffnet ein Projekt und bleibt stabil,
# (2) Headless-Export erzeugt ein lesbares MP4 mit Bild und Ton, (3) die EXE ist 64-Bit und nutzt nur Windows-DLLs.
param([Parameter(Mandatory)][string]$Exe)
$ErrorActionPreference = "Stop"
$work = Join-Path $env:TEMP "video-superbe-e2e"
Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $work | Out-Null
$script:fail = @()
function Check($name, $ok, $detail = "") {
    if ($ok) { Write-Host "  OK   $name" } else { Write-Host "  FAIL $name $detail"; $script:fail += $name }
}

# --- Testdaten: zwei PNGs und eine WAV
Add-Type -AssemblyName System.Drawing
foreach ($c in @(@('a.png', 200, 30, 30), @('b.png', 30, 30, 200))) {
    $bmp = New-Object Drawing.Bitmap 320, 180
    $g = [Drawing.Graphics]::FromImage($bmp); $g.Clear([Drawing.Color]::FromArgb($c[1], $c[2], $c[3])); $g.Dispose()
    $bmp.Save((Join-Path $work $c[0]), [Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
}
$rate = 48000; $n = $rate * 2
$ms = New-Object IO.MemoryStream; $w = New-Object IO.BinaryWriter $ms
$w.Write([Text.Encoding]::ASCII.GetBytes("RIFF")); $w.Write([int](36 + $n * 2)); $w.Write([Text.Encoding]::ASCII.GetBytes("WAVEfmt "))
$w.Write([int]16); $w.Write([int16]1); $w.Write([int16]1); $w.Write([int]$rate); $w.Write([int]($rate * 2)); $w.Write([int16]2); $w.Write([int16]16)
$w.Write([Text.Encoding]::ASCII.GetBytes("data")); $w.Write([int]($n * 2))
for ($i = 0; $i -lt $n; $i++) { $w.Write([int16](12000 * [Math]::Sin(2 * [Math]::PI * 440 * $i / $rate))) }
[IO.File]::WriteAllBytes((Join-Path $work "tone.wav"), $ms.ToArray())

function J($p) { $p -replace '\\', '\\' }
$a = J (Join-Path $work 'a.png'); $b = J (Join-Path $work 'b.png'); $t = J (Join-Path $work 'tone.wav')
$proj = @"
{"assets":[
 {"id":"11111111-1111-4111-8111-111111111111","path":"$a","name":"a.png","duration":5,"hasVideo":true,"hasAudio":false,"width":320,"height":180,"fps":0,"transform":[1,0,0,1,0,0],"isImage":true},
 {"id":"22222222-2222-4222-8222-222222222222","path":"$b","name":"b.png","duration":5,"hasVideo":true,"hasAudio":false,"width":320,"height":180,"fps":0,"transform":[1,0,0,1,0,0],"isImage":true},
 {"id":"33333333-3333-4333-8333-333333333333","path":"$t","name":"tone.wav","duration":2,"hasVideo":false,"hasAudio":true,"width":0,"height":0,"fps":0,"transform":[1,0,0,1,0,0],"isImage":false}],
 "tracks":[
  {"name":"Spur 1","clips":[
   {"id":"c1000000-0000-4000-8000-000000000001","content":{"media":{"_0":"11111111-1111-4111-8111-111111111111"}},"part":"video","start":0,"duration":1},
   {"id":"c1000000-0000-4000-8000-000000000002","content":{"media":{"_0":"22222222-2222-4222-8222-222222222222"}},"part":"video","start":1,"duration":1}]},
  {"name":"Spur 2","clips":[
   {"id":"c1000000-0000-4000-8000-000000000003","content":{"media":{"_0":"33333333-3333-4333-8333-333333333333"}},"part":"audio","start":0,"duration":2}]},
  {"name":"Spur 3"}],
 "renderWidth":320,"renderHeight":180,"fps":15}
"@
$projFile = Join-Path $work "test.mvd.json"
[IO.File]::WriteAllText($projFile, $proj, (New-Object Text.UTF8Encoding $false))

# --- 1. GUI-Smoke: starten, Projekt öffnen, 5 s stabil, Titel enthält Dateiname
$p = Start-Process $Exe -ArgumentList "`"$projFile`"" -PassThru
Start-Sleep 5; $p.Refresh()
Check "GUI läuft nach 5 s" (-not $p.HasExited)
Check "GUI antwortet" ($p.Responding)
Check "Fenstertitel zeigt Projekt" ($p.MainWindowTitle -like "test.mvd.json*") $p.MainWindowTitle
if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force }

# --- 2. Headless-Export
$out = Join-Path $work "out.mp4"
$e = Start-Process $Exe -ArgumentList "--export", "`"$projFile`"", "`"$out`"" -PassThru
$e.WaitForExit(120000) | Out-Null
Check "Export beendet" ($e.HasExited)
$logf = "$out.log"
Check "Export meldet OK" ((Test-Path $logf) -and ((Get-Content $logf -Raw).Trim() -eq "OK")) (Get-Content $logf -ErrorAction SilentlyContinue)
Check "MP4 existiert und ist nicht leer" ((Test-Path $out) -and ((Get-Item $out).Length -gt 2000))

# --- 3. MP4 zurücklesen (Metadaten)
$v = Start-Process $Exe -ArgumentList "--videotest", "`"$out`"", "0.4", "1.6" -WorkingDirectory $work -PassThru
$v.WaitForExit(60000) | Out-Null
$vt = Get-Content (Join-Path $work "videotest.txt") -Raw -ErrorAction SilentlyContinue
Check "MP4 hat Bild und Ton" ($vt -match "has_video: true" -and $vt -match "has_audio: true") $vt
Check "MP4 hat 320x180" ($vt -match "width: 320" -and $vt -match "height: 180")
Check "Dauer ca. 2 s" ($vt -match "duration: (1\.[89]|2\.[0-3])") $vt
Check "Bilder lesbar" ($vt -match "t=0.4: Frame 320x180" -and $vt -match "t=1.6: Frame 320x180") $vt

# --- 4. Portabilität: 64-Bit, nur Windows-eigene DLLs
$bytes = [IO.File]::ReadAllBytes($Exe)
$pe = [BitConverter]::ToInt32($bytes, 0x3c)
Check "EXE ist x86-64 (64 Bit)" ([BitConverter]::ToUInt16($bytes, $pe + 4) -eq 0x8664)
$dlls = [regex]::Matches([Text.Encoding]::ASCII.GetString($bytes), '(?i)\b[a-z0-9_\-]+\.dll') | ForEach-Object { $_.Value.ToLower() } | Sort-Object -Unique
$allowed = 'kernel32|ntdll|user32|gdi32|shell32|ole32|combase|imm32|shcore|uxtheme|dwmapi|dbghelp|bcryptprimitives|[a-z]*opengl32|libegl|msvcrt|winmm|mf|mfplat|mfreadwrite|mfuuid|propsys|oleaut32|advapi32|api-ms-win-[a-z0-9\-]+|darkmode_explorerntdll|dkernel32|d3d11|dxgi|rpcrt4|userenv|ws2_32|atioglxx|nvoglv32|nvoglv64|igxprd32'
# Namen können im Zeichenketten-Bereich der EXE aneinanderhängen (…windowhandleuser32.dll): nur das Ende zählt
$bad = $dlls | Where-Object { $_ -notmatch "($allowed)\.dll$" }
Check "nur Windows-eigene DLLs importiert" (-not $bad) ($bad -join ", ")

Write-Host ""
if ($script:fail.Count -gt 0) { Write-Host "$($script:fail.Count) Prüfung(en) fehlgeschlagen." -ForegroundColor Red; exit 1 }
Write-Host "E2E bestanden." -ForegroundColor Green
exit 0
