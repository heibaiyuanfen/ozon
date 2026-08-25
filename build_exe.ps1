$ErrorActionPreference = "Stop"

$ProjectRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$PythonExe = Join-Path $env:LOCALAPPDATA "Programs\Python\Python312\python.exe"
$AppName = "OzonAnalytics"
$Version = "0.15.3"
$BuildDir = Join-Path $ProjectRoot "build"
$DistDir = Join-Path $ProjectRoot "dist"
$ReleaseDir = Join-Path $ProjectRoot "release"
$PackageDir = Join-Path $ReleaseDir ("OzonAnalytics_v" + $Version)
$ZipPath = $PackageDir + ".zip"
$IconPath = Join-Path $ProjectRoot "app_icon.ico"
$VersionFile = Join-Path $ProjectRoot "version_info.txt"

if (-not (Test-Path -LiteralPath $PythonExe)) {
    throw "Python 3.12 not found: $PythonExe"
}

$ResolvedProject = [System.IO.Path]::GetFullPath($ProjectRoot).TrimEnd('\') + '\'
foreach ($Target in @($BuildDir, $DistDir, $ReleaseDir, $PackageDir, $ZipPath)) {
    $ResolvedTarget = [System.IO.Path]::GetFullPath($Target)
    if (-not $ResolvedTarget.StartsWith($ResolvedProject, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to clean path outside project: $ResolvedTarget"
    }
}

foreach ($Target in @($BuildDir, $DistDir, $PackageDir, $ZipPath)) {
    if (Test-Path -LiteralPath $Target) {
        Remove-Item -LiteralPath $Target -Recurse -Force
    }
}
New-Item -ItemType Directory -Path $BuildDir, $DistDir, $ReleaseDir, $PackageDir -Force | Out-Null
& $PythonExe (Join-Path $ProjectRoot "build_icon.py")
if ($LASTEXITCODE -ne 0) { throw "Icon generation failed." }

& $PythonExe -m PyInstaller `
    --noconfirm `
    --clean `
    --onefile `
    --windowed `
    --name $AppName `
    --icon $IconPath `
    --version-file $VersionFile `
    --distpath $DistDir `
    --workpath (Join-Path $BuildDir "work") `
    --specpath (Join-Path $BuildDir "spec") `
    --collect-all docx `
    --collect-all reportlab `
    --collect-all openpyxl `
    --exclude-module pandas `
    --exclude-module numpy `
    --exclude-module scipy `
    --exclude-module matplotlib `
    --exclude-module pyarrow `
    --exclude-module IPython `
    --exclude-module PyQt5 `
    --exclude-module sqlalchemy `
    --exclude-module pytest `
    --exclude-module aiohttp `
    --hidden-import lxml `
    --hidden-import lxml.etree `
    (Join-Path $ProjectRoot "main.py")
if ($LASTEXITCODE -ne 0) { throw "PyInstaller build failed." }

$BuiltExe = Join-Path $DistDir ($AppName + ".exe")
if (-not (Test-Path -LiteralPath $BuiltExe)) {
    throw "Built EXE not found: $BuiltExe"
}

Copy-Item -LiteralPath $BuiltExe -Destination $PackageDir -Force
Copy-Item -LiteralPath (Join-Path $ProjectRoot "PACKAGE_README.txt") -Destination $PackageDir -Force
$PackageData = Join-Path $PackageDir "data"
New-Item -ItemType Directory -Path $PackageData -Force | Out-Null
$SourceData = Join-Path $ProjectRoot "data"
if (Test-Path -LiteralPath $SourceData) {
    Get-ChildItem -LiteralPath $SourceData -Force | ForEach-Object {
        Copy-Item -LiteralPath $_.FullName -Destination $PackageData -Recurse -Force
    }
}

# Bundle the RFBS listing executable/runtime, but deliberately exclude its
# persistent program-data folder (config.json may contain private API keys).
# Discover by its ASCII filename so Windows PowerShell 5.1 cannot corrupt a
# Chinese source path when this UTF-8 script is launched from Explorer.
$ListingExe = Get-ChildItem -LiteralPath "D:\" -Directory -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -like "OZON *" } |
    ForEach-Object {
        Get-ChildItem -LiteralPath $_.FullName -Recurse -File `
            -Filter "Ozon_RFBS*.exe" -ErrorAction SilentlyContinue
    } |
    Where-Object { Test-Path -LiteralPath (Join-Path $_.Directory.FullName "_internal") } |
    Select-Object -First 1
if ($null -ne $ListingExe) {
    $ListingSource = $ListingExe.Directory.FullName
    $ListingTarget = Join-Path $PackageDir "tools\Ozon_RFBS_ListingTool"
    New-Item -ItemType Directory -Path $ListingTarget -Force | Out-Null
    Copy-Item -LiteralPath $ListingExe.FullName `
        -Destination (Join-Path $ListingTarget "Ozon_RFBS_ListingTool.exe") -Force
    Copy-Item -LiteralPath (Join-Path $ListingSource "_internal") `
        -Destination $ListingTarget -Recurse -Force
    New-Item -ItemType Directory -Path (Join-Path $ListingTarget "program_data") -Force | Out-Null
}

$Smoke = Start-Process -FilePath (Join-Path $PackageDir ($AppName + ".exe")) `
    -ArgumentList "--packaging-smoke-test" -WindowStyle Hidden -Wait -PassThru
if ($Smoke.ExitCode -ne 0) {
    throw "Packaged EXE smoke test failed with exit code $($Smoke.ExitCode)."
}

Compress-Archive -LiteralPath $PackageDir -DestinationPath $ZipPath -CompressionLevel Optimal

$ExeInfo = Get-Item -LiteralPath (Join-Path $PackageDir ($AppName + ".exe"))
$ZipInfo = Get-Item -LiteralPath $ZipPath
Write-Host "Build completed."
Write-Host ("EXE: " + $ExeInfo.FullName + " (" + [math]::Round($ExeInfo.Length / 1MB, 1) + " MB)")
Write-Host ("ZIP: " + $ZipInfo.FullName + " (" + [math]::Round($ZipInfo.Length / 1MB, 1) + " MB)")
