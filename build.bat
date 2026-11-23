@echo off
setlocal
cd /d "%~dp0"
echo === Flint build ===
where cargo >nul 2>nul || (
  echo Rust is not installed. Install it from https://rustup.rs  ^(choose the default MSVC option^),
  echo install "Visual Studio Build Tools" with "Desktop development with C++" if asked, then run this again.
  pause & exit /b 1
)
cargo build --release --locked || ( echo BUILD FAILED & pause & exit /b 1 )
if not exist dist mkdir dist
copy /y target\release\flint.exe dist\Flint-setup.exe >nul
echo.
echo DONE:  %cd%\dist\Flint-setup.exe
echo Double-click it ONCE: it installs, starts in the tray, and starts with Windows.
echo Press Alt+Space to open.
pause
