$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$size = 1024
# Cat drawn from the same path as CatLogo.tsx (viewBox 0 0 100 86).
# Fill ~78% of the tile so the mark reads at 24-32 px instead of ~55%.
$targetH = [double]$size * 0.78
$scale = $targetH / 86.0
$tx = ([double]$size - (100.0 * $scale)) / 2.0
$ty = ([double]$size - (86.0 * $scale)) / 2.0

function Convert-SvgPath([string]$d) {
  $g = New-Object System.Drawing.Drawing2D.GraphicsPath
  $tokens = ($d -replace '\s+', ' ').Trim() -split ' '
  $i = 0; $cx = 0.0; $cy = 0.0
  while ($i -lt $tokens.Count) {
    $cmd = $tokens[$i]; $i++
    if ($cmd -eq 'M') {
      $cx = [double]$tokens[$i]; $cy = [double]$tokens[$i + 1]; $i += 2
    } elseif ($cmd -eq 'C') {
      $x1 = [double]$tokens[$i]; $y1 = [double]$tokens[$i + 1]
      $x2 = [double]$tokens[$i + 2]; $y2 = [double]$tokens[$i + 3]
      $x = [double]$tokens[$i + 4]; $y = [double]$tokens[$i + 5]; $i += 6
      $g.AddBezier([single]$cx, [single]$cy, [single]$x1, [single]$y1, [single]$x2, [single]$y2, [single]$x, [single]$y)
      $cx = $x; $cy = $y
    } elseif ($cmd -eq 'Z') {
      $g.CloseFigure()
    } else {
      throw "unsupported path command: $cmd"
    }
  }
  return $g
}

$head = 'M 27 22 C 25 17 21 13 18 13 C 16 13 15 16 16 20 C 18 30 19 38 20 43 C 14 50 14 62 21 72 C 29 82 43 85 50 85 C 57 85 71 82 79 72 C 86 62 86 50 80 43 C 81 38 82 30 84 20 C 85 16 84 13 82 13 C 79 13 75 17 73 22 C 67 30 58 33 50 33 C 42 33 33 30 27 22 Z'
$mouth = 'M 43.5 63 C 43.5 67 47 68 50 64 C 53 68 56.5 67 56.5 63'

$ink = [System.Drawing.ColorTranslator]::FromHtml('#14100e')
$accent = [System.Drawing.ColorTranslator]::FromHtml('#a8613f')
$plate = [System.Drawing.Color]::White

$bmp = New-Object System.Drawing.Bitmap($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
$gfx = [System.Drawing.Graphics]::FromImage($bmp)
$gfx.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$gfx.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$gfx.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
$gfx.Clear([System.Drawing.Color]::Transparent)

# Rounded white plate, radius 22.4% like the previous icon.
$radius = [single]($size * 0.224)
$platePath = New-Object System.Drawing.Drawing2D.GraphicsPath
$d = [single]($radius * 2)
$platePath.AddArc(0, 0, $d, $d, 180, 90)
$platePath.AddArc([single]($size - $d), 0, $d, $d, 270, 90)
$platePath.AddArc([single]($size - $d), [single]($size - $d), $d, $d, 0, 90)
$platePath.AddArc(0, [single]($size - $d), $d, $d, 90, 90)
$platePath.CloseFigure()
$plateBrush = New-Object System.Drawing.SolidBrush($plate)
$gfx.FillPath($plateBrush, $platePath)
$plateBrush.Dispose()
$platePath.Dispose()

# Draw the mark in viewBox units.
$gfx.TranslateTransform([single]$tx, [single]$ty)
$gfx.ScaleTransform([single]$scale, [single]$scale)

$inkBrush = New-Object System.Drawing.SolidBrush($ink)
$headPath = Convert-SvgPath $head
$pen = New-Object System.Drawing.Pen($ink, [single]2.0)
$pen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
$pen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
$pen.LineJoin = [System.Drawing.Drawing2D.LineJoin]::Round
$gfx.DrawPath($pen, $headPath)

$gfx.FillEllipse($inkBrush, [single](37 - 3.2), [single](53 - 3.2), [single]6.4, [single]6.4)
$gfx.FillEllipse($inkBrush, [single](63 - 3.2), [single](53 - 3.2), [single]6.4, [single]6.4)

$accentBrush = New-Object System.Drawing.SolidBrush($accent)
$gfx.FillEllipse($accentBrush, [single](50 - 2.2), [single](60 - 1.6), [single]4.4, [single]3.2)

$mouthPen = New-Object System.Drawing.Pen($ink, [single]1.8)
$mouthPen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
$mouthPen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
$mouthPen.LineJoin = [System.Drawing.Drawing2D.LineJoin]::Round
$mouthPath = Convert-SvgPath $mouth
$gfx.DrawPath($mouthPen, $mouthPath)

$out = Join-Path $PSScriptRoot 'icon-source.png'
$bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)

$mouthPen.Dispose(); $pen.Dispose(); $mouthPath.Dispose(); $headPath.Dispose()
$inkBrush.Dispose(); $accentBrush.Dispose(); $gfx.Dispose(); $bmp.Dispose()

"wrote $out ({0}x{0}) scale={1:N3} tx={2:N1} ty={3:N1}" -f $size, $scale, $tx, $ty
