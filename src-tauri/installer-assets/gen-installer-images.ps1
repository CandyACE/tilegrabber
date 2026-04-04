Add-Type -AssemblyName System.Drawing

$Out   = Split-Path -Parent $MyInvocation.MyCommand.Path
$IconPath = Join-Path $Out "..\icons\icon.png"
$icon  = [System.Drawing.Image]::FromFile((Resolve-Path $IconPath).Path)

# ─────────────────────────────────────────────────────────────────────────────
# 侧边栏 164×314  (Welcome / Finish 页)
# ─────────────────────────────────────────────────────────────────────────────
$W = 164; $H = 314
$bmp = New-Object System.Drawing.Bitmap($W, $H)
$g   = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode      = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$g.TextRenderingHint  = [System.Drawing.Text.TextRenderingHint]::AntiAlias
$g.InterpolationMode  = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic

# ── 背景渐变（深夜蓝 → 深海蓝）────────────────────────────────────────────────
$bgBrush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
    [System.Drawing.PointF]::new(0, 0), [System.Drawing.PointF]::new(0, $H),
    [System.Drawing.Color]::FromArgb(255,  8, 17, 38),   # #081126
    [System.Drawing.Color]::FromArgb(255, 10, 47, 93)    # #0a2f5d
)
$g.FillRectangle($bgBrush, 0, 0, $W, $H)
$bgBrush.Dispose()

# ── 对角深色光带（动感斜切）──────────────────────────────────────────────────
$stripBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(18, 255, 255, 255))
$pts1 = @(
    [System.Drawing.PointF]::new(-20, 0),
    [System.Drawing.PointF]::new(80, 0),
    [System.Drawing.PointF]::new($W, $H),
    [System.Drawing.PointF]::new($W - 100, $H)
)
$g.FillPolygon($stripBrush, $pts1)
$pts2 = @(
    [System.Drawing.PointF]::new(60, 0),
    [System.Drawing.PointF]::new(120, 0),
    [System.Drawing.PointF]::new($W, 100),
    [System.Drawing.PointF]::new($W - 60, 100)
)
$g.FillPolygon($stripBrush, $pts2)
$stripBrush.Dispose()

# ── 地图瓦片网格覆盖 ──────────────────────────────────────────────────────────
$gridPen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(22, 100, 180, 255), 0.7)
for ($x = 0; $x -lt $W; $x += 16) { $g.DrawLine($gridPen, $x, 0, $x, $H) }
for ($y = 0; $y -lt $H; $y += 16) { $g.DrawLine($gridPen, 0, $y, $W, $y) }
$gridPen.Dispose()

# ── Logo 光晕（径向模拟）──────────────────────────────────────────────────────
$cx = 82; $cy = 115; $r = 60
$haloPath = New-Object System.Drawing.Drawing2D.GraphicsPath
$haloPath.AddEllipse($cx - $r, $cy - $r, $r * 2, $r * 2)
$haloBrush = New-Object System.Drawing.Drawing2D.PathGradientBrush($haloPath)
$haloBrush.CenterColor    = [System.Drawing.Color]::FromArgb(70, 56, 182, 255)
$haloBrush.SurroundColors = @([System.Drawing.Color]::FromArgb(0, 0, 0, 0))
$g.FillEllipse($haloBrush, $cx - $r, $cy - $r, $r * 2, $r * 2)
$haloBrush.Dispose()
$haloPath.Dispose()

# ── Logo 图标（居中 64×64）───────────────────────────────────────────────────
$logoSz = 64
$logoX = [int](($W - $logoSz) / 2)
$logoY = 83
$g.DrawImage($icon, [System.Drawing.Rectangle]::new($logoX, $logoY, $logoSz, $logoSz))

# ── 主标题「御图」────────────────────────────────────────────────────────────
$fTitle = New-Object System.Drawing.Font("Microsoft YaHei", 26, [System.Drawing.FontStyle]::Bold, [System.Drawing.GraphicsUnit]::Pixel)
$titleText = "御图"
$titleSz   = $g.MeasureString($titleText, $fTitle)
$g.DrawString($titleText, $fTitle, [System.Drawing.Brushes]::White,
    [float](($W - $titleSz.Width) / 2), 158.0)
$fTitle.Dispose()

# ── 副标题 ────────────────────────────────────────────────────────────────────
$fSub = New-Object System.Drawing.Font("Microsoft YaHei", 11, [System.Drawing.FontStyle]::Regular, [System.Drawing.GraphicsUnit]::Pixel)
$subBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(200, 140, 195, 255))
$subText  = "地图瓦片下载工具"
$subSz    = $g.MeasureString($subText, $fSub)
$g.DrawString($subText, $fSub, $subBrush, [float](($W - $subSz.Width) / 2), 190.0)
$fSub.Dispose()
$subBrush.Dispose()

# ── 分割线（渐变青色）────────────────────────────────────────────────────────
$sepBrush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
    [System.Drawing.PointF]::new(16, 0), [System.Drawing.PointF]::new($W - 16, 0),
    [System.Drawing.Color]::FromArgb(0, 56, 182, 255),
    [System.Drawing.Color]::FromArgb(220, 56, 182, 255)
)
$sepPen = New-Object System.Drawing.Pen($sepBrush, 1.5)
$g.DrawLine($sepPen, 16, 215, $W - 16, 215)
$sepPen.Dispose()
$sepBrush.Dispose()

# 再画一条右边亮、向左渐隐（形成双线动感）
$sepBrush2 = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
    [System.Drawing.PointF]::new(16, 0), [System.Drawing.PointF]::new($W - 16, 0),
    [System.Drawing.Color]::FromArgb(0, 255, 255, 255),
    [System.Drawing.Color]::FromArgb(80, 255, 255, 255)
)
$sepPen2 = New-Object System.Drawing.Pen($sepBrush2, 0.7)
$g.DrawLine($sepPen2, 16, 218, $W - 16, 218)
$sepPen2.Dispose()
$sepBrush2.Dispose()

# ── 小标签（版本信息区域标注）────────────────────────────────────────────────
$fTag = New-Object System.Drawing.Font("Microsoft YaHei", 9, [System.Drawing.FontStyle]::Regular, [System.Drawing.GraphicsUnit]::Pixel)
$tagBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(120, 160, 210, 255))

$tagLine1 = "御览天地，图行万里"
$tagSz1   = $g.MeasureString($tagLine1, $fTag)
$g.DrawString($tagLine1, $fTag, $tagBrush, [float](($W - $tagSz1.Width) / 2), 230.0)

$tagLine2 = "emapgis.com"
$tagSz2   = $g.MeasureString($tagLine2, $fTag)
$tagBrushLight = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(80, 140, 185, 255))
$g.DrawString($tagLine2, $fTag, $tagBrushLight, [float](($W - $tagSz2.Width) / 2), 246.0)
$fTag.Dispose()
$tagBrush.Dispose()
$tagBrushLight.Dispose()

# ── 底部三个发光圆点 ──────────────────────────────────────────────────────────
$dotColors = @(
    [System.Drawing.Color]::FromArgb(60,  56, 182, 255),
    [System.Drawing.Color]::FromArgb(130, 56, 182, 255),
    [System.Drawing.Color]::FromArgb(60,  56, 182, 255)
)
$dotXs = @(72, 80, 88)
for ($i = 0; $i -lt 3; $i++) {
    $db = New-Object System.Drawing.SolidBrush($dotColors[$i])
    $g.FillEllipse($db, $dotXs[$i], 290, 5, 5)
    $db.Dispose()
}

# ── 左上角装饰小方块（模拟瓦片标记）─────────────────────────────────────────
$tilePen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(50, 56, 182, 255), 1)
$g.DrawRectangle($tilePen, 8, 8, 12, 12)
$g.DrawRectangle($tilePen, 22, 8, 12, 12)
$g.DrawRectangle($tilePen, 8, 22, 12, 12)
$tilePen.Dispose()

# ── 右下角装饰 ────────────────────────────────────────────────────────────────
$rPen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(40, 56, 182, 255), 1)
$g.DrawRectangle($rPen, $W - 22, $H - 22, 12, 12)
$g.DrawRectangle($rPen, $W - 36, $H - 22, 12, 12)
$g.DrawRectangle($rPen, $W - 22, $H - 36, 12, 12)
$rPen.Dispose()

$g.Dispose()
$outSidebar = Join-Path $Out "sidebar.png"
$bmp.Save($outSidebar, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Host "✓ sidebar.png -> $outSidebar"

# ─────────────────────────────────────────────────────────────────────────────
# 页头 150×57  (安装过程内页)
# ─────────────────────────────────────────────────────────────────────────────
$HW = 150; $HH = 57
$hbmp = New-Object System.Drawing.Bitmap($HW, $HH)
$hg   = [System.Drawing.Graphics]::FromImage($hbmp)
$hg.SmoothingMode     = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$hg.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAlias
$hg.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic

# 背景
$hBg = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
    [System.Drawing.PointF]::new(0, 0), [System.Drawing.PointF]::new($HW, 0),
    [System.Drawing.Color]::FromArgb(255,  8, 17, 38),
    [System.Drawing.Color]::FromArgb(255, 12, 55, 110)
)
$hg.FillRectangle($hBg, 0, 0, $HW, $HH)
$hBg.Dispose()

# 斜切光带
$hStrip = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(15, 255, 255, 255))
$hPts = @(
    [System.Drawing.PointF]::new(0, 0),
    [System.Drawing.PointF]::new(60, 0),
    [System.Drawing.PointF]::new(30, $HH),
    [System.Drawing.PointF]::new(-30, $HH)
)
$hg.FillPolygon($hStrip, $hPts)
$hStrip.Dispose()

# 网格
$hGrid = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(18, 100, 180, 255), 0.5)
for ($x = 0; $x -lt $HW; $x += 16) { $hg.DrawLine($hGrid, $x, 0, $x, $HH) }
for ($y = 0; $y -lt $HH; $y += 16) { $hg.DrawLine($hGrid, 0, $y, $HW, $y) }
$hGrid.Dispose()

# Logo 光晕
$hcx = 29; $hcy = [int]($HH / 2); $hr = 24
$hHaloPath = New-Object System.Drawing.Drawing2D.GraphicsPath
$hHaloPath.AddEllipse($hcx - $hr, $hcy - $hr, $hr * 2, $hr * 2)
$hHaloBrush = New-Object System.Drawing.Drawing2D.PathGradientBrush($hHaloPath)
$hHaloBrush.CenterColor    = [System.Drawing.Color]::FromArgb(55, 56, 182, 255)
$hHaloBrush.SurroundColors = @([System.Drawing.Color]::FromArgb(0, 0, 0, 0))
$hg.FillEllipse($hHaloBrush, $hcx - $hr, $hcy - $hr, $hr * 2, $hr * 2)
$hHaloBrush.Dispose()
$hHaloPath.Dispose()

# Logo（32×32，垂直居中）
$hLogoSz = 32
$hLogoY  = [int](($HH - $hLogoSz) / 2)
$hg.DrawImage($icon, [System.Drawing.Rectangle]::new(13, $hLogoY, $hLogoSz, $hLogoSz))

# 标题「御图」
$hfTitle = New-Object System.Drawing.Font("Microsoft YaHei", 18, [System.Drawing.FontStyle]::Bold, [System.Drawing.GraphicsUnit]::Pixel)
$hTitleSz = $hg.MeasureString("御图", $hfTitle)
$hg.DrawString("御图", $hfTitle, [System.Drawing.Brushes]::White, 52.0, [float](($HH - $hTitleSz.Height) / 2) - 2)
$hfTitle.Dispose()

# 副标题
$hfSub = New-Object System.Drawing.Font("Microsoft YaHei", 9, [System.Drawing.FontStyle]::Regular, [System.Drawing.GraphicsUnit]::Pixel)
$hSubBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(170, 140, 195, 255))
$hSubSz = $hg.MeasureString("地图瓦片下载工具", $hfSub)
$hg.DrawString("地图瓦片下载工具", $hfSub, $hSubBrush, 53.0, [float](($HH + $hSubSz.Height) / 2) + 2)
$hfSub.Dispose()
$hSubBrush.Dispose()

# 底部青色线
$hLine1 = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
    [System.Drawing.PointF]::new(0, 0), [System.Drawing.PointF]::new($HW, 0),
    [System.Drawing.Color]::FromArgb(180, 30, 140, 255),
    [System.Drawing.Color]::FromArgb(80,  10,  80, 200)
)
$hLinePen = New-Object System.Drawing.Pen($hLine1, 2)
$hg.DrawLine($hLinePen, 0, $HH - 2, $HW, $HH - 2)
$hLinePen.Dispose()
$hLine1.Dispose()

$hg.Dispose()
$outHeader = Join-Path $Out "header.png"
$hbmp.Save($outHeader, [System.Drawing.Imaging.ImageFormat]::Png)
$hbmp.Dispose()
$icon.Dispose()
Write-Host "✓ header.png  -> $outHeader"
Write-Host ""
Write-Host "完成！请重新 tauri build 以应用新安装界面。"
