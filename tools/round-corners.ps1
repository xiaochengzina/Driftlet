param(
    [string]$Source = "icon/Logo.png",
    [string]$Output = "icon/Logo_rounded.png",
    [double]$RadiusRatio = 0.22
)

Add-Type -AssemblyName System.Drawing

$src = [System.Drawing.Image]::FromFile((Resolve-Path $Source))
$w = $src.Width
$h = $src.Height
$radius = [int]([Math]::Min($w, $h) * $RadiusRatio)
$d = $radius * 2

$bmp = New-Object System.Drawing.Bitmap($w, $h, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality

$path = New-Object System.Drawing.Drawing2D.GraphicsPath
$path.AddArc(0, 0, $d, $d, 180, 90)
$path.AddArc($w - $d, 0, $d, $d, 270, 90)
$path.AddArc($w - $d, $h - $d, $d, $d, 0, 90)
$path.AddArc(0, $h - $d, $d, $d, 90, 90)
$path.CloseFigure()

$brush = New-Object System.Drawing.TextureBrush($src)
$g.FillPath($brush, $path)

$bmp.Save((Join-Path (Get-Location) $Output), [System.Drawing.Imaging.ImageFormat]::Png)

$g.Dispose()
$brush.Dispose()
$path.Dispose()
$bmp.Dispose()
$src.Dispose()

Write-Output "OK: $Output ($w x $h, radius $radius)"
