# 御图 发行包打包脚本
# 构建完成后运行，将 NSIS 安装包 + 一键安装脚本打包到 dist-release\ 目录
#
# 使用方式：
#   1. 先运行 `npm run tauri:build` 完成构建
#   2. 再运行 `.\scripts\pack-release.ps1`
#   3. 分发 dist-release\ 目录中的两个文件

$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot)

# ---- 读取版本号 ----
$conf = Get-Content "src-tauri\tauri.conf.json" -Raw | ConvertFrom-Json
$version = $conf.version
$productName = $conf.productName   # 御图

Write-Host "产品：$productName  版本：$version" -ForegroundColor Cyan

# ---- 找安装包 ----
$nsisDir = "src-tauri\target\release\bundle\nsis"
$setupFile = Get-ChildItem -Path $nsisDir -Filter "*-setup.exe" -ErrorAction SilentlyContinue |
             Sort-Object LastWriteTime -Descending |
             Select-Object -First 1

if (-not $setupFile) {
    Write-Error "未在 $nsisDir 找到安装包，请先运行 `npm run tauri:build`"
    exit 1
}
Write-Host "找到安装包：$($setupFile.Name)" -ForegroundColor Green

# ---- 准备输出目录 ----
$outDir = "dist-release"
Remove-Item -Recurse -Force $outDir -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $outDir | Out-Null

# ---- 复制安装包 ----
Copy-Item $setupFile.FullName "$outDir\$($setupFile.Name)"

# ---- 生成一键安装脚本 ----
$bat = @"
@echo off
chcp 65001 >nul 2>&1
setlocal

set "INSTALLER="
for %%f in ("%~dp0*-setup.exe") do set "INSTALLER=%%~ff"

if not defined INSTALLER (
    echo [错误] 未找到安装包文件，请确保本文件与安装包放在同一目录。
    pause
    exit /b 1
)

echo 正在安装 $productName，请稍候...
start /wait "" "%INSTALLER%" /S

if %errorlevel% neq 0 (
    echo [错误] 安装失败，错误码：%errorlevel%
    pause
    exit /b %errorlevel%
)

set "APP=%LOCALAPPDATA%\Programs\$productName\$productName.exe"
if exist "%APP%" (
    echo 安装完成，正在启动 $productName ...
    start "" "%APP%"
) else (
    echo 安装完成，请从桌面快捷方式启动 $productName 。
)
endlocal
"@

$bat | Set-Content -Encoding UTF8 "$outDir\一键安装.bat"

# ---- 汇报 ----
$items = Get-ChildItem $outDir
Write-Host ""
Write-Host "=== 发行包已生成：$outDir\ ===" -ForegroundColor Yellow
$items | Format-Table Name, @{L="大小";E={"{0:N1} MB" -f ($_.Length/1MB)}} -AutoSize

Write-Host "分发方式：将以上两个文件发给用户，双击「一键安装.bat」即可自动安装并启动。" -ForegroundColor Cyan
