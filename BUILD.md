# How to build the exe (Windows 10/11)

1. Install Rust: https://rustup.rs (default options, MSVC toolchain). If it asks, also install
   **Visual Studio Build Tools -> "Desktop development with C++"**.
2. Unzip the project, open the folder, double-click **`build.bat`** (about 3-6 minutes the first time).
3. Result: `dist\Flint-setup.exe`. Double-click it once:
   installs for your user, appears in Settings -> Apps, Start menu, starts with Windows, runs in the tray.
4. Press **Alt + Space**.

Command line instead: `cargo build --release` -> `target\release\flint.exe` (same self-installing exe).

Professional packages (two MSIs + portable ZIP + SHA256 file):
```
dotnet tool install --global wix --version 5.0.2
wix extension add -g WixToolset.Util.wixext/5.0.2
pwsh scripts\build-installers.ps1
```
Output is in `dist\`. For a public release, code-sign the exe/MSI to avoid the SmartScreen warning.

Uninstall: Settings -> Apps -> Installed apps -> Flint.
