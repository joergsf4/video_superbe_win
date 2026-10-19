# Führt alle automatisierten Tests aus: Unit-/Integrationstests (cargo test) und den End-to-End-Test der EXE.
# Aufruf: powershell -File tests\run-tests.ps1
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root
$env:Path = [Environment]::GetEnvironmentVariable('Path','User') + ';' + [Environment]::GetEnvironmentVariable('Path','Machine') + ';' + $env:Path + ";$env:USERPROFILE\.cargo\bin"

Write-Host "== cargo test =="
cargo test -- --test-threads=1
if ($LASTEXITCODE -ne 0) { throw "cargo test fehlgeschlagen" }

Write-Host "== End-to-End (Release-EXE) =="
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "Release-Build fehlgeschlagen" }
& "$PSScriptRoot\e2e.ps1" -Exe "$root\target\x86_64-pc-windows-gnu\release\video-superbe.exe"
if ($LASTEXITCODE -ne 0) { throw "E2E fehlgeschlagen" }
Write-Host "Alle Tests bestanden."
