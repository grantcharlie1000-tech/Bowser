# Crops a region of a PNG and scales it up with nearest-neighbour sampling, for pixel inspection.
#   powershell -File zoom-crop.ps1 -In shot.png -Out zoom.png -X 330 -Y 0 -W 120 -H 60 [-Scale 6]
param([string]$In, [string]$Out, [int]$X, [int]$Y, [int]$W, [int]$H, [int]$Scale = 6)
Add-Type -AssemblyName System.Drawing
$src = [Drawing.Bitmap]::FromFile((Resolve-Path $In).Path)
$dst = New-Object Drawing.Bitmap ($W * $Scale), ($H * $Scale)
$g = [Drawing.Graphics]::FromImage($dst)
$g.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::NearestNeighbor
$g.PixelOffsetMode = [Drawing.Drawing2D.PixelOffsetMode]::Half
$g.DrawImage($src, (New-Object Drawing.Rectangle 0, 0, ($W * $Scale), ($H * $Scale)), (New-Object Drawing.Rectangle $X, $Y, $W, $H), [Drawing.GraphicsUnit]::Pixel)
$g.Dispose(); $dst.Save($Out, [Drawing.Imaging.ImageFormat]::Png); $dst.Dispose(); $src.Dispose()
