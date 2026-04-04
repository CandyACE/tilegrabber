# 御图 MSI 快速重新链接脚本（仅 light.exe 步骤）
# 在 `npm run tauri:build` 已运行过（wixobj 文件已存在）后使用
#
# 背景：Tauri 2.10.1 CLI 调用 light.exe 时未传 -ext WixUIExtension 导致失败，
#       此脚本直接以正确参数运行 light.exe。

$WIX_DIR = "$env:LOCALAPPDATA\tauri\WixTools314"
$WORK_DIR = "$PSScriptRoot\src-tauri\target\release\wix\x64"
$OUT_DIR  = "$PSScriptRoot\src-tauri\target\release\bundle\msi"

if (-not (Test-Path "$WORK_DIR\main.wixobj")) {
    Write-Host "main.wixobj not found. Running full build first..." -ForegroundColor Yellow
    Set-Location $PSScriptRoot
    # 运行完整构建（Tauri 会在 light.exe 失败，但 wixobj 已生成，这是正常的）
    npm run tauri:build 2>&1 | Write-Host
}

New-Item -ItemType Directory -Force $OUT_DIR | Out-Null
$version = (Get-Content "$PSScriptRoot\src-tauri\tauri.conf.json" | ConvertFrom-Json).version

Write-Host "Linking MSI with WixUIExtension..." -ForegroundColor Cyan
& "$WIX_DIR\light.exe" `
    -nologo `
    -cultures:zh-cn `
    -ext "$WIX_DIR\WixUIExtension.dll" `
    -ext "$WIX_DIR\WixUtilExtension.dll" `
    -loc "$WORK_DIR\locale.wxl" `
    "$WORK_DIR\main.wixobj" `
    -o "$OUT_DIR\御图_${version}_x64.msi" `
    -spdb

if ($LASTEXITCODE -eq 0) {
    $f = Get-Item "$OUT_DIR\御图_${version}_x64.msi"
    Write-Host "SUCCESS: $($f.FullName) ($([math]::Round($f.Length/1MB,1)) MB)" -ForegroundColor Green
} else {
    Write-Error "light.exe failed (exit $LASTEXITCODE)"
}
