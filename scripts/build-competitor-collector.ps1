param(
    [string]$PythonVersion = "3.12"
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $repoRoot
try {
    uv run --python $PythonVersion --with playwright --with pyinstaller `
        pyinstaller --noconfirm --clean --onefile `
        --name competitor-collector --collect-all playwright `
        competitor_collector.py
    if ($LASTEXITCODE -ne 0) {
        throw "competitor-collector.exe build failed"
    }
    $releaseDir = Join-Path $repoRoot "desktop-next\src-tauri\target\release"
    if (Test-Path -LiteralPath $releaseDir) {
        Copy-Item -LiteralPath (Join-Path $repoRoot "dist\competitor-collector.exe") `
            -Destination (Join-Path $releaseDir "competitor-collector.exe") -Force
    }
} finally {
    Pop-Location
}
