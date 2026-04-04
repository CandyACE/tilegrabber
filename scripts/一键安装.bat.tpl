@echo off
chcp 65001 >nul 2>&1
setlocal

:: 找到同目录下的安装包（支持带版本号的文件名）
set "INSTALLER="
for %%f in ("%~dp0*-setup.exe") do set "INSTALLER=%%~ff"

if not defined INSTALLER (
    echo [错误] 未找到安装包文件，请确保本文件与安装包放在同一目录。
    pause
    exit /b 1
)

echo 正在安装御图，请稍候...
start /wait "" "%INSTALLER%" /S

if %errorlevel% neq 0 (
    echo [错误] 安装失败，错误码：%errorlevel%
    pause
    exit /b %errorlevel%
)

:: 安装完成，启动应用（NSIS currentUser 默认安装到 %LOCALAPPDATA%\Programs\ProductName）
set "APP=%LOCALAPPDATA%\Programs\御图\御图.exe"
if exist "%APP%" (
    echo 安装完成，正在启动御图...
    start "" "%APP%"
) else (
    echo 安装完成！请从桌面快捷方式启动御图。
)

endlocal
