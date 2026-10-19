# Führt jeden Test einzeln aus (ein Absturz/Abbruch eines Tests beendet sonst alle) und zeigt nur Fehlschläge.
# Aufruf: powershell -File tests\run-each.ps1 [-Filter gui_tests]
param([string]$Filter = "")
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root
$env:Path = [Environment]::GetEnvironmentVariable('Path','User') + ';' + [Environment]::GetEnvironmentVariable('Path','Machine') + ';' + $env:Path + ";$env:USERPROFILE\.cargo\bin"
Get-Process video-superbe -ErrorAction SilentlyContinue | Stop-Process -Force
$o = cargo test --no-run 2>&1 | Out-String
($o -split "`n" | Select-String -Pattern "^error" -Context 0,12 | Select-Object -First 40 | ForEach-Object { $_.ToString().TrimEnd() })
$exe = (Get-ChildItem target\x86_64-pc-windows-gnu\debug\deps\video_superbe-*.exe | Where-Object { $_.Extension -eq '.exe' } | Sort-Object LastWriteTime -Descending | Select-Object -First 1).FullName
$names = & $exe --list 2>$null | Where-Object { $_ -like "*: test" } | ForEach-Object { $_ -replace ': test$','' } | Where-Object { $_ -like "*$Filter*" }
$bad = 0
foreach ($n in $names) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $out = & $exe $n --exact --nocapture 2>&1 | Out-String
    if ($LASTEXITCODE -ne 0) {
        $bad++
        "FAIL $n"
        ($out -split "`n" | Select-String -NotMatch "RemoteException|CategoryInfo|FullyQualified|^\+|^In Zeile|^\s*$|running 1" | Select-Object -First 5 | ForEach-Object { "     " + $_.ToString().Trim() })
    } elseif ($sw.Elapsed.TotalSeconds -gt 20) {
        "langsam: {0} {1:N0}s" -f $n, $sw.Elapsed.TotalSeconds
    }
}
"$($names.Count) Tests, $bad fehlgeschlagen"
