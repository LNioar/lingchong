param(
    [ValidateSet('Release', 'Debug')][string]$Configuration = 'Release',
    [switch]$SkipBuild
)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$profileName = $Configuration.ToLowerInvariant()
$buildRoot = Join-Path $projectRoot "target/$profileName"
$runtimeRoot = Join-Path $projectRoot 'crates/deskpet-app/resources/tts'

# Fail before building or creating a deliverable when speech dependencies are absent.
foreach ($relativePath in @('python/python.exe', 'mpv/mpv.exe', 'edge_worker.py')) {
    if (-not (Test-Path -LiteralPath (Join-Path $runtimeRoot $relativePath) -PathType Leaf)) {
        throw "Missing $relativePath. Run scripts/prepare-windows-tts.ps1 first."
    }
}
if (-not $SkipBuild) {
    Push-Location $projectRoot
    try {
        $cargoArguments = @('build', '-p', 'deskpet-app', '--features', 'tauri')
        if ($Configuration -eq 'Release') { $cargoArguments += '--release' }
        & cargo @cargoArguments
        if ($LASTEXITCODE -ne 0) { throw 'GUI build failed.' }
    } finally { Pop-Location }
}
$appExecutable = Join-Path $buildRoot 'deskpet-app.exe'
if (-not (Test-Path -LiteralPath $appExecutable -PathType Leaf)) {
    throw "GUI executable missing: $appExecutable"
}
# Unique output avoids overwriting existing packages or including stale files.
$packageName = "DeskPet-windows-x64-$profileName-$(Get-Date -Format 'yyyyMMdd-HHmmss')-$([guid]::NewGuid().ToString('N').Substring(0, 6))"
$distRoot = Join-Path $projectRoot 'dist'
$packageRoot = Join-Path $distRoot $packageName
New-Item -ItemType Directory -Path $packageRoot -Force | Out-Null
Copy-Item -LiteralPath $appExecutable -Destination $packageRoot
Copy-Item -LiteralPath $runtimeRoot -Destination (Join-Path $packageRoot 'tts') -Recurse
$webViewLoader = Join-Path $buildRoot 'WebView2Loader.dll'
if (Test-Path -LiteralPath $webViewLoader) {
    Copy-Item -LiteralPath $webViewLoader -Destination $packageRoot
}
$portablePython = Join-Path $packageRoot 'tts/python/python.exe'
& $portablePython -c 'import edge_tts; print(edge_tts.__version__)'
if ($LASTEXITCODE -ne 0) { throw 'Packaged Python/edge-tts validation failed; package is incomplete.' }
& (Join-Path $packageRoot 'tts/mpv/mpv.exe') --version
if ($LASTEXITCODE -ne 0) { throw 'Packaged mpv validation failed; package is incomplete.' }
$readme = @'
DeskPet Windows portable package

1. Extract the ENTIRE ZIP to a local folder, then run deskpet-app.exe.
2. Keep the tts folder next to the EXE. Do not move only the EXE.
3. Python and mpv are included; no separate installation is required.
4. Windows x64 and an installed Microsoft Edge WebView2 Runtime are required.
5. Edge TTS needs Internet access. A development PC's localhost proxy is not
   available on another PC; use that PC's working network/proxy settings.
6. User configuration and database stay in %APPDATA%\deskpet, separately from
   this package. No developer configuration, API keys or database are included.

Debug packages are for testing; use a Release package for distribution.
'@
Set-Content -LiteralPath (Join-Path $packageRoot 'README.txt') -Value $readme -Encoding UTF8
$zipPath = "$packageRoot.zip"
Add-Type -AssemblyName System.IO.Compression.FileSystem
[System.IO.Compression.ZipFile]::CreateFromDirectory(
    $packageRoot, $zipPath, [System.IO.Compression.CompressionLevel]::Optimal, $true
)
Write-Output "Portable folder: $packageRoot"
Write-Output "Portable ZIP: $zipPath"
