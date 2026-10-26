param([string]$Proxy, [string]$MpvArchive)
$ErrorActionPreference = 'Stop'
$ttsRoot = Split-Path -Parent $PSScriptRoot
$ttsPython = Join-Path $ttsRoot 'demos/edge-tts/.venv/Scripts/python.exe'
if (-not (Test-Path -LiteralPath $ttsPython)) {
    throw 'First create the isolated Demo environment following demos/edge-tts/README.md.'
}
$ttsArguments = @((Join-Path $PSScriptRoot 'prepare-windows-tts.py'))
if ($Proxy) { $ttsArguments += @('--proxy', $Proxy) }
if ($MpvArchive) { $ttsArguments += @('--mpv-archive', $MpvArchive) }
& $ttsPython @ttsArguments
if ($LASTEXITCODE -ne 0) { throw 'Speech runtime preparation failed; see output above.' }
