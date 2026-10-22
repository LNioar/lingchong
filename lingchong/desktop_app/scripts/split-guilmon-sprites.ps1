param(
    [string]$SourcePath,
    [string]$OutputDirectory = (Join-Path $PSScriptRoot '../crates/deskpet-app/frontend/sprites')
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
if (-not $SourcePath) {
    # ASCII source keeps Windows PowerShell 5.1 from misreading a UTF-8 filename.
    $sourceName = [regex]::Unescape('\u57fa\u5c14\u517d\u8868\u60c5\u50cf\u7d20\u52a8\u753b\u56fe\u96c6.png')
    $SourcePath = Join-Path $OutputDirectory $sourceName
}

# The presentation sheet includes labels and a header. These coordinates refer
# to the actual 1448 x 1086 PNG, not the 128 px frame size printed in its header.
$rows = @(
    @{ Mood = 'idle';    Y = 110 },
    @{ Mood = 'happy';   Y = 270 },
    @{ Mood = 'curious'; Y = 430 },
    @{ Mood = 'excited'; Y = 590 },
    @{ Mood = 'sleepy';  Y = 752 },
    @{ Mood = 'bored';   Y = 914 }
)
$resolvedSource = (Resolve-Path -LiteralPath $SourcePath -ErrorAction Stop).ProviderPath
$source = [System.Drawing.Bitmap]::new([string]$resolvedSource)
try {
    if ($source.Width -ne 1448 -or $source.Height -ne 1086) {
        throw 'Expected the original 1448 x 1086 sprite presentation sheet.'
    }
    New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
    foreach ($row in $rows) {
        $strip = [System.Drawing.Bitmap]::new(1024, 128, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
        $graphics = [System.Drawing.Graphics]::FromImage($strip)
        try {
            $graphics.Clear([System.Drawing.Color]::Transparent)
            $graphics.CompositingMode = [System.Drawing.Drawing2D.CompositingMode]::SourceCopy
            $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::NearestNeighbor
            $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::Half
            for ($frame = 0; $frame -lt 8; $frame++) {
                $crop = [System.Drawing.Rectangle]::new((153 + $frame * 160), $row.Y, 160, 160)
                $destination = [System.Drawing.Rectangle]::new(($frame * 128), 0, 128, 128)
                $graphics.DrawImage($source, $destination, $crop, [System.Drawing.GraphicsUnit]::Pixel)
            }
            $destinationPath = Join-Path $OutputDirectory ($row.Mood + '.png')
            $strip.Save($destinationPath, [System.Drawing.Imaging.ImageFormat]::Png)
            Write-Output ($row.Mood + ': 1024 x 128, 8 frames')
        } finally {
            $graphics.Dispose()
            $strip.Dispose()
        }
    }
} finally {
    $source.Dispose()
}
