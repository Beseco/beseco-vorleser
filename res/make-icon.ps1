# Erzeugt die Programmsymbole neu:  res\icon.png (Infobereich, 32 px) und res\vorleser.ico (EXE).
# Aufruf im Projektordner:  powershell -ExecutionPolicy Bypass -File res\make-icon.ps1
Add-Type -AssemblyName System.Drawing
$dir = $PSScriptRoot

function New-IconBitmap([int]$Size) {
    $bmp = New-Object System.Drawing.Bitmap $Size, $Size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'AntiAlias'
    $g.Clear([System.Drawing.Color]::Transparent)
    # Gezeichnet wird in einem 32er-Raster und dann skaliert.
    $g.ScaleTransform($Size / 32.0, $Size / 32.0)
    $blue = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 30, 110, 200))
    $white = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::White)
    $g.FillEllipse($blue, 0, 0, 31, 31)
    # Lautsprecher: Rechteck und Trichter
    $g.FillRectangle($white, 6, 12, 5, 8)
    $funnel = @((New-Object System.Drawing.Point 11, 12), (New-Object System.Drawing.Point 17, 6), (New-Object System.Drawing.Point 17, 26), (New-Object System.Drawing.Point 11, 20))
    $g.FillPolygon($white, [System.Drawing.Point[]]$funnel)
    # Schallwellen
    $pen = New-Object System.Drawing.Pen ([System.Drawing.Color]::White), 2
    $pen.StartCap = 'Round'; $pen.EndCap = 'Round'
    $g.DrawArc($pen, 14, 11, 8, 10, -55, 110)
    $g.DrawArc($pen, 13, 7, 14, 18, -55, 110)
    $g.Dispose()
    $bmp
}

$png = New-IconBitmap 32
$png.Save("$dir\icon.png", [System.Drawing.Imaging.ImageFormat]::Png)
$png.Dispose()

# ICO mit unkomprimierten 32-Bit-Bildern; das versteht jeder Ressourcen-Compiler.
$sizes = 16, 24, 32, 48, 64
$images = foreach ($size in $sizes) {
    $bmp = New-IconBitmap $size
    $ms = New-Object System.IO.MemoryStream
    $w = New-Object System.IO.BinaryWriter $ms
    $maskRow = [int]([math]::Floor(($size + 31) / 32)) * 4
    # BITMAPINFOHEADER; die Höhe zählt Bild und Maske zusammen.
    $w.Write([int]40); $w.Write([int]$size); $w.Write([int]($size * 2)); $w.Write([int16]1); $w.Write([int16]32)
    $w.Write([int]0); $w.Write([int]($size * $size * 4 + $maskRow * $size)); $w.Write([int]0); $w.Write([int]0); $w.Write([int]0); $w.Write([int]0)
    for ($y = $size - 1; $y -ge 0; $y--) {
        for ($x = 0; $x -lt $size; $x++) {
            $c = $bmp.GetPixel($x, $y)
            $w.Write([byte]$c.B); $w.Write([byte]$c.G); $w.Write([byte]$c.R); $w.Write([byte]$c.A)
        }
    }
    $w.Write((New-Object byte[] ($maskRow * $size)))
    $w.Flush(); $bmp.Dispose()
    , $ms.ToArray()
}

$out = New-Object System.IO.FileStream "$dir\vorleser.ico", ([System.IO.FileMode]::Create)
$w = New-Object System.IO.BinaryWriter $out
$w.Write([int16]0); $w.Write([int16]1); $w.Write([int16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $w.Write([byte]$sizes[$i]); $w.Write([byte]$sizes[$i]); $w.Write([byte]0); $w.Write([byte]0)
    $w.Write([int16]1); $w.Write([int16]32); $w.Write([int]$images[$i].Length); $w.Write([int]$offset)
    $offset += $images[$i].Length
}
foreach ($image in $images) { $w.Write($image) }
$w.Close()
"icon.png und vorleser.ico geschrieben ($((Get-Item "$dir\vorleser.ico").Length) Bytes)"
