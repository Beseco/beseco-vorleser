# Ruft cargo mit der passenden Umgebung auf, z. B.:  .\cargo.ps1 build --release
# Die GNU-Toolchain von Rust braucht dlltool, as und windres aus MinGW-w64. Gesucht wird zuerst
# die per winget installierte WinLibs-Variante, sonst muss MinGW-w64 schon im PATH liegen.
$mingw = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Directory -Filter "BrechtSanders.WinLibs*" -ErrorAction SilentlyContinue |
    ForEach-Object { Join-Path $_.FullName "mingw64\bin" } |
    Where-Object { Test-Path (Join-Path $_ "dlltool.exe") } |
    Select-Object -First 1
if ($mingw) {
    $env:PATH = "$mingw;$env:PATH"
} elseif (-not (Get-Command dlltool -ErrorAction SilentlyContinue)) {
    Write-Error "MinGW-w64 nicht gefunden. Installation: winget install -e --id BrechtSanders.WinLibs.POSIX.UCRT --scope user"
    exit 1
}
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
& cargo @args
exit $LASTEXITCODE
