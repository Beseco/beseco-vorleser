# Baut die Veröffentlichung:  dist\Vorleser-Setup-<Version>.exe  und  dist\Vorleser-<Version>-portable.zip
# Aufruf im Projektordner:  powershell -ExecutionPolicy Bypass -File release.ps1
# Die Version kommt aus Cargo.toml. Braucht Inno Setup 6 (winget install -e --id JRSoftware.InnoSetup).
param(
    # Die Tests der Sprachausgabe brauchen Windows-Stimmen und ein Audiogerät; auf Build-Servern fehlt beides.
    [switch]$SkipTests
)
$ErrorActionPreference = "Stop"
$root = $PSScriptRoot
$version = [regex]::Match((Get-Content "$root\Cargo.toml" -Raw), '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
if (-not $version) { throw "Version in Cargo.toml nicht gefunden." }

$iscc = @("$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe", "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe") |
    Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { throw "Inno Setup 6 nicht gefunden." }

Push-Location $root
try {
    $target = (& "$root\cargo.ps1" metadata --format-version 1 --no-deps | ConvertFrom-Json).target_directory
    if (-not $target) { throw "Build-Ordner nicht ermittelbar." }
    $release = Join-Path $target "release"

    # Läuft gerade ein Vorleser aus dem Build-Ordner, ist dessen EXE gesperrt und der Linker scheitert.
    # Umbenennen geht trotzdem; das laufende Programm stört das nicht.
    Get-ChildItem $release, "$release\deps" -Filter "vorleser*.exe" -ErrorAction SilentlyContinue | ForEach-Object {
        try { [IO.File]::Open($_.FullName, 'Open', 'ReadWrite', 'None').Close() }
        catch { Rename-Item $_.FullName ("{0}.{1}.alt" -f $_.Name, [DateTime]::Now.Ticks) }
    }
    Get-ChildItem $release, "$release\deps" -Filter "*.alt" -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue

    if (-not $SkipTests) {
        & "$root\cargo.ps1" test --quiet
        if ($LASTEXITCODE -ne 0) { throw "Tests fehlgeschlagen." }
    }
    & "$root\cargo.ps1" build --release
    if ($LASTEXITCODE -ne 0) { throw "Build fehlgeschlagen." }
    $exe = Join-Path $release "vorleser.exe"

    $dist = Join-Path $root "dist"
    New-Item -ItemType Directory -Force $dist | Out-Null
    & $iscc /Qp "/DAppVersion=$version" "/DSourceExe=$exe" "$root\installer\vorleser.iss"
    if ($LASTEXITCODE -ne 0) { throw "Inno Setup ist gescheitert." }

    $zip = Join-Path $dist "Vorleser-$version-portable.zip"
    Remove-Item $zip -Force -ErrorAction SilentlyContinue
    Compress-Archive -Path $exe, "$root\LICENSE" -DestinationPath $zip

    Get-ChildItem $dist -Filter "Vorleser-*$version*" | ForEach-Object { "{0}  ({1:N0} KB)" -f $_.FullName, ($_.Length / 1KB) }
} finally {
    Pop-Location
}
