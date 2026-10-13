# Stacks the same crop of several PNGs into one image (top to bottom), labelled with file names.
#   powershell -File contact-sheet.ps1 -Out sheet.png -X 330 -Y 0 -W 650 -H 190 a.png b.png ...
param([string]$Out, [int]$X, [int]$Y, [int]$W, [int]$H, [Parameter(ValueFromRemainingArguments = $true)][string[]]$Files)
Add-Type -AssemblyName System.Drawing
$label = 16
$sheet = New-Object Drawing.Bitmap $W, (($H + $label) * $Files.Count)
$g = [Drawing.Graphics]::FromImage($sheet); $g.Clear([Drawing.Color]::Black)
$font = New-Object Drawing.Font 'Segoe UI', 9
$i = 0
foreach ($f in $Files) {
  $src = [Drawing.Bitmap]::FromFile((Resolve-Path $f).Path)
  $top = $i * ($H + $label)
  $g.DrawString([IO.Path]::GetFileNameWithoutExtension($f), $font, [Drawing.Brushes]::Yellow, 2, $top)
  $g.DrawImage($src, (New-Object Drawing.Rectangle 0, ($top + $label), $W, $H), (New-Object Drawing.Rectangle $X, $Y, $W, $H), [Drawing.GraphicsUnit]::Pixel)
  $src.Dispose(); $i++
}
$g.Dispose(); $sheet.Save($Out, [Drawing.Imaging.ImageFormat]::Png); $sheet.Dispose()
