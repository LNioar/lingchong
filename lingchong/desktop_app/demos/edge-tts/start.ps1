param([int]$Port = 8765, [switch]$InstallDependencies)
$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath $PSScriptRoot
$demoPython = Join-Path $PSScriptRoot '.venv/Scripts/python.exe'
if (-not (Test-Path -LiteralPath $demoPython)) {
    if (Get-Command py -ErrorAction SilentlyContinue) {
        & py -3 -m venv .venv
    } else {
        & python -m venv .venv
    }
    if ($LASTEXITCODE -ne 0) { throw 'Could not create .venv. Install standard Windows Python 3.10+ and try again.' }
    $InstallDependencies = $true
}
if ($InstallDependencies) {
    & $demoPython -m pip install -r requirements.txt
    if ($LASTEXITCODE -ne 0) { throw 'Dependency installation failed. Check the network and retry with -InstallDependencies.' }
}
& $demoPython server.py --port $Port
exit $LASTEXITCODE
