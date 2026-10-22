# Erzeugt die Programmsymbole aus dem Igel-Bild docs\brand\igel-icon.jpg neu:
# res\icon.png (Infobereich, 32 px), res\vorleser.ico (EXE, Installer) und docs\brand\igel.png (freigestellt).
# Aufruf im Projektordner:  powershell -ExecutionPolicy Bypass -File res\make-icon.ps1
param([string]$Src = "$PSScriptRoot\..\docs\brand\igel-icon.jpg", [string]$OutDir = $PSScriptRoot)
Add-Type -AssemblyName System.Drawing
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
public static class Igel {
    // Macht den weißen Hintergrund durchsichtig (nur zusammenhängend vom Rand aus, damit die
    // weiße Sprechblase bleibt) und schneidet auf ein Quadrat um die Figur zu.
    public static Bitmap Cutout(string path) {
        var src = new Bitmap(path);
        int w = src.Width, h = src.Height;
        var bmp = new Bitmap(w, h, PixelFormat.Format32bppArgb);
        using (var g = Graphics.FromImage(bmp)) g.DrawImage(src, 0, 0, w, h);
        var data = bmp.LockBits(new Rectangle(0, 0, w, h), ImageLockMode.ReadWrite, PixelFormat.Format32bppArgb);
        var px = new int[w * h];
        Marshal.Copy(data.Scan0, px, 0, px.Length);
        Func<int, int> white = c => Math.Min(Math.Min((c >> 16) & 255, (c >> 8) & 255), c & 255);
        var bg = new bool[w * h];
        var queue = new Queue<int>();
        for (int x = 0; x < w; x++) { queue.Enqueue(x); queue.Enqueue((h - 1) * w + x); }
        for (int y = 0; y < h; y++) { queue.Enqueue(y * w); queue.Enqueue(y * w + w - 1); }
        while (queue.Count > 0) {
            int i = queue.Dequeue();
            if (bg[i] || white(px[i]) < 225) continue;
            bg[i] = true;
            int x = i % w, y = i / w;
            if (x > 0) queue.Enqueue(i - 1);
            if (x < w - 1) queue.Enqueue(i + 1);
            if (y > 0) queue.Enqueue(i - w);
            if (y < h - 1) queue.Enqueue(i + w);
        }
        int minX = w, minY = h, maxX = 0, maxY = 0;
        for (int i = 0; i < px.Length; i++) {
            int x = i % w, y = i / w;
            if (bg[i]) {
                px[i] = 0;
                continue;
            }
            // Weicher Rand: helle Randpixel neben dem Hintergrund werden teilweise durchsichtig.
            bool edge = (x > 0 && bg[i - 1]) || (x < w - 1 && bg[i + 1]) || (y > 0 && bg[i - w]) || (y < h - 1 && bg[i + w]);
            if (edge) {
                int a = Math.Max(0, Math.Min(255, (255 - white(px[i])) * 255 / 60));
                px[i] = (px[i] & 0xFFFFFF) | (Math.Max(a, 40) << 24);
            }
            minX = Math.Min(minX, x); maxX = Math.Max(maxX, x); minY = Math.Min(minY, y); maxY = Math.Max(maxY, y);
        }
        Marshal.Copy(px, 0, data.Scan0, px.Length);
        bmp.UnlockBits(data);
        int side = Math.Max(maxX - minX, maxY - minY) + 1;
        var square = new Bitmap(side, side, PixelFormat.Format32bppArgb);
        using (var g = Graphics.FromImage(square)) {
            g.Clear(Color.Transparent);
            g.DrawImage(bmp, (side - (maxX - minX + 1)) / 2 - minX, (side - (maxY - minY + 1)) / 2 - minY);
        }
        return square;
    }
    public static Bitmap Scale(Bitmap img, int size) {
        var b = new Bitmap(size, size, PixelFormat.Format32bppArgb);
        using (var g = Graphics.FromImage(b)) {
            g.InterpolationMode = System.Drawing.Drawing2D.InterpolationMode.HighQualityBicubic;
            g.PixelOffsetMode = System.Drawing.Drawing2D.PixelOffsetMode.HighQuality;
            g.Clear(Color.Transparent);
            g.DrawImage(img, 0, 0, size, size);
        }
        return b;
    }
}
'@
$img = [Igel]::Cutout($Src)
$img.Save("$PSScriptRoot\..\docs\brand\igel.png",[System.Drawing.Imaging.ImageFormat]::Png)
$tray = [Igel]::Scale($img, 32); $tray.Save("$OutDir\icon.png", [System.Drawing.Imaging.ImageFormat]::Png)

# ICO mit PNG-Bildern je Größe (versteht Windows seit Vista).
$sizes = 16, 20, 24, 32, 40, 48, 64, 256
$pngs = foreach ($s in $sizes) {
    $ms = New-Object System.IO.MemoryStream
    ([Igel]::Scale($img, $s)).Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    , $ms.ToArray()
}
$out = New-Object System.IO.MemoryStream
$bw = New-Object System.IO.BinaryWriter $out
$bw.Write([uint16]0); $bw.Write([uint16]1); $bw.Write([uint16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $s = $sizes[$i]; $d = if ($s -ge 256) { 0 } else { $s }
    $bw.Write([byte]$d); $bw.Write([byte]$d); $bw.Write([byte]0); $bw.Write([byte]0)
    $bw.Write([uint16]1); $bw.Write([uint16]32); $bw.Write([uint32]$pngs[$i].Length); $bw.Write([uint32]$offset)
    $offset += $pngs[$i].Length
}
foreach ($p in $pngs) { $bw.Write($p) }
[IO.File]::WriteAllBytes("$OutDir\vorleser.ico", $out.ToArray())

"ok"
