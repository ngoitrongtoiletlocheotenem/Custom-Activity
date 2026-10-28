param([string]$SourcePath = (Join-Path $PSScriptRoot '../assets/app-icon.png'))
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$iconDirectory = Join-Path $PSScriptRoot '../src-tauri/icons'
[IO.Directory]::CreateDirectory($iconDirectory) | Out-Null
$source = [Drawing.Image]::FromFile((Resolve-Path -LiteralPath $SourcePath).Path)
$frames = [Collections.Generic.List[byte[]]]::new()
$sizes = @(16, 24, 32, 48, 64, 128, 256)
try {
    # Windows application and installer icons use a static image.
    foreach ($size in $sizes) {
        $bitmap = [Drawing.Bitmap]::new($size, $size, [Drawing.Imaging.PixelFormat]::Format32bppArgb)
        $graphics = [Drawing.Graphics]::FromImage($bitmap)
        $attributes = [Drawing.Imaging.ImageAttributes]::new()
        $stream = [IO.MemoryStream]::new()
        try {
            $graphics.Clear([Drawing.Color]::Transparent)
            $graphics.CompositingMode = [Drawing.Drawing2D.CompositingMode]::SourceCopy
            $graphics.CompositingQuality = [Drawing.Drawing2D.CompositingQuality]::HighQuality
            $graphics.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
            $graphics.PixelOffsetMode = [Drawing.Drawing2D.PixelOffsetMode]::HighQuality
            $attributes.SetWrapMode([Drawing.Drawing2D.WrapMode]::TileFlipXY)
            $scale = [Math]::Min($size / [double]$source.Width, $size / [double]$source.Height)
            $width = [Math]::Max(1, [int][Math]::Round($source.Width * $scale))
            $height = [Math]::Max(1, [int][Math]::Round($source.Height * $scale))
            $rectangle = [Drawing.Rectangle]::new([int](($size - $width) / 2), [int](($size - $height) / 2), $width, $height)
            $graphics.DrawImage($source, $rectangle, 0, 0, $source.Width, $source.Height, [Drawing.GraphicsUnit]::Pixel, $attributes)
            $bitmap.Save($stream, [Drawing.Imaging.ImageFormat]::Png)
            $png = $stream.ToArray()
            $frames.Add($png)
            if ($size -eq 256) { [IO.File]::WriteAllBytes((Join-Path $iconDirectory 'icon.png'), $png) }
        } finally {
            $stream.Dispose()
            $attributes.Dispose()
            $graphics.Dispose()
            $bitmap.Dispose()
        }
    }
    $output = [IO.File]::Create((Join-Path $iconDirectory 'icon.ico'))
    $writer = [IO.BinaryWriter]::new($output)
    try {
        $writer.Write([uint16]0)
        $writer.Write([uint16]1)
        $writer.Write([uint16]$sizes.Count)
        $offset = 6 + 16 * $sizes.Count
        for ($index = 0; $index -lt $sizes.Count; $index++) {
            $dimension = if ($sizes[$index] -eq 256) { 0 } else { $sizes[$index] }
            $writer.Write([byte]$dimension)
            $writer.Write([byte]$dimension)
            $writer.Write([byte]0)
            $writer.Write([byte]0)
            $writer.Write([uint16]1)
            $writer.Write([uint16]32)
            $writer.Write([uint32]$frames[$index].Length)
            $writer.Write([uint32]$offset)
            $offset += $frames[$index].Length
        }
        foreach ($frame in $frames) { $writer.Write($frame) }
    } finally { $writer.Dispose() }
} finally { $source.Dispose() }
Write-Output ('Generated static Windows icon sizes: ' + ($sizes -join ', '))
