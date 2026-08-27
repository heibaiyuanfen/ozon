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
    $builtCollector = Join-Path $repoRoot "dist\competitor-collector.exe"
    $rootCollector = Join-Path $repoRoot "competitor-collector.exe"
    if (-not (Test-Path -LiteralPath $builtCollector)) {
        throw "Built collector not found: $builtCollector"
    }
    # The ERP resolves this sidecar beside its own executable. Always refresh
    # the repository/release root so an ignored stale EXE cannot survive a Git
    # pull and silently continue running old Python code.
    Copy-Item -LiteralPath $builtCollector -Destination $rootCollector -Force
    $releaseDir = Join-Path $repoRoot "desktop-next\src-tauri\target\release"
    if (Test-Path -LiteralPath $releaseDir) {
        Copy-Item -LiteralPath $builtCollector `
            -Destination (Join-Path $releaseDir "competitor-collector.exe") -Force
    }
    Get-FileHash -LiteralPath $rootCollector -Algorithm SHA256 | Format-List
} finally {
    Pop-Location
}
