# Generates herdup's icon set.
#
# Committed so the icon is reproducible rather than an opaque binary nobody can
# regenerate. Run from the repo root:
#
#   pwsh assets/make-icons.ps1
#
# Design: a herdr workspace as herdup actually builds one — a wide coordinator
# pane on the left and the team stacked to its right. It is the Squad layout,
# literally. Two shapes and one accent colour, which is all that survives at
# 16 px in a taskbar.

param(
  [string]$OutDir = "app/src-tauri/icons"
)

Add-Type -AssemblyName System.Drawing

# A mid-tone tile, not near-black. The first attempt used #171A21 and vanished
# against a dark taskbar — the panes floated with no edge. This reads against
# both a white window and a dark dock.
$BG        = [System.Drawing.Color]::FromArgb(255, 38, 45, 68)    # indigo slate
$RIM       = [System.Drawing.Color]::FromArgb(46, 255, 255, 255)  # faint edge for dark grounds
$ACCENT    = [System.Drawing.Color]::FromArgb(255, 130, 170, 255) # coordinator
$TEAM      = [System.Drawing.Color]::FromArgb(255, 96, 112, 158)  # the rest

function New-RoundedPath([single]$x, [single]$y, [single]$w, [single]$h, [single]$r) {
  $p = New-Object System.Drawing.Drawing2D.GraphicsPath
  $d = $r * 2
  if ($d -le 0) { $p.AddRectangle((New-Object System.Drawing.RectangleF($x, $y, $w, $h))); return $p }
  $p.AddArc($x,           $y,           $d, $d, 180, 90)
  $p.AddArc($x + $w - $d, $y,           $d, $d, 270, 90)
  $p.AddArc($x + $w - $d, $y + $h - $d, $d, $d,   0, 90)
  $p.AddArc($x,           $y + $h - $d, $d, $d,  90, 90)
  $p.CloseFigure()
  return $p
}

function New-Icon([int]$size) {
  $bmp = New-Object System.Drawing.Bitmap($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.SmoothingMode     = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
  $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
  $g.Clear([System.Drawing.Color]::Transparent)

  $u = $size / 32.0          # design on a 32-unit grid
  $radius = [single](6 * $u) # ~19% — a squircle, not a circle

  # Tile, inset half a pixel so the rim is not clipped.
  $tile = New-RoundedPath 0.5 0.5 ([single]($size-1)) ([single]($size-1)) $radius
  $bgBrush = New-Object System.Drawing.SolidBrush($BG)
  $g.FillPath($bgBrush, $tile)
  $rimPen = New-Object System.Drawing.Pen($RIM, [single][Math]::Max(1.0, $u))
  $g.DrawPath($rimPen, $tile)

  # Panes. Chunkier than the first attempt, which went spindly at 16 px.
  $paneR = [single][Math]::Max(1.0, 2 * $u)
  $accentBrush = New-Object System.Drawing.SolidBrush($ACCENT)
  $teamBrush   = New-Object System.Drawing.SolidBrush($TEAM)

  # coordinator: wide, full height
  $c = New-RoundedPath ([single](5*$u)) ([single](5*$u)) ([single](8*$u)) ([single](22*$u)) $paneR
  $g.FillPath($accentBrush, $c)

  # team: two stacked to the right
  $t1 = New-RoundedPath ([single](15*$u)) ([single](5*$u))  ([single](12*$u)) ([single](10*$u)) $paneR
  $t2 = New-RoundedPath ([single](15*$u)) ([single](17*$u)) ([single](12*$u)) ([single](10*$u)) $paneR
  $g.FillPath($teamBrush, $t1)
  $g.FillPath($teamBrush, $t2)

  foreach ($d in @($tile, $c, $t1, $t2, $bgBrush, $rimPen, $accentBrush, $teamBrush, $g)) { $d.Dispose() }
  return $bmp
}

function Save-Png([System.Drawing.Bitmap]$bmp, [string]$path) {
  $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
}

# --- PNGs Tauri expects ------------------------------------------------------
New-Item -ItemType Directory -Force $OutDir | Out-Null
$targets = @{
  "32x32.png"      = 32
  "128x128.png"    = 128
  "128x128@2x.png" = 256
  "icon.png"       = 512
}
foreach ($name in $targets.Keys) {
  $b = New-Icon $targets[$name]
  Save-Png $b (Join-Path $OutDir $name)
  $b.Dispose()
}

# --- Multi-resolution .ico ---------------------------------------------------
# Windows picks a size per context: 16 in title bars, 32 in the taskbar, 256 in
# large-icon views. A single-size ico gets scaled and looks muddy.
$icoSizes = @(16, 24, 32, 48, 64, 128, 256)
$pngs = @()
foreach ($s in $icoSizes) {
  $b = New-Icon $s
  $ms = New-Object System.IO.MemoryStream
  $b.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
  $pngs += , @{ Size = $s; Bytes = $ms.ToArray() }
  $ms.Dispose(); $b.Dispose()
}

$icoPath = Join-Path $OutDir "icon.ico"
$fs = [System.IO.File]::Create($icoPath)
$bw = New-Object System.IO.BinaryWriter($fs)
$bw.Write([uint16]0)                 # reserved
$bw.Write([uint16]1)                 # type: icon
$bw.Write([uint16]$pngs.Count)
$offset = 6 + (16 * $pngs.Count)
foreach ($p in $pngs) {
  # 256 is encoded as 0 in the directory entry.
  $dim = if ($p.Size -ge 256) { 0 } else { $p.Size }
  $bw.Write([byte]$dim); $bw.Write([byte]$dim)
  $bw.Write([byte]0);    $bw.Write([byte]0)      # palette, reserved
  $bw.Write([uint16]1);  $bw.Write([uint16]32)   # planes, bpp
  $bw.Write([uint32]$p.Bytes.Length)
  $bw.Write([uint32]$offset)
  $offset += $p.Bytes.Length
}
foreach ($p in $pngs) { $bw.Write($p.Bytes) }
$bw.Flush(); $bw.Dispose(); $fs.Dispose()

Get-ChildItem $OutDir | Select-Object Name, Length | Format-Table -AutoSize
Write-Host "icon.ico contains sizes: $($icoSizes -join ', ')"
