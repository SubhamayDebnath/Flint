<p align="center"><img src="assets/logo.png" width="104" alt="Flint"></p>

<h1 align="center">⚡ Flint</h1>
<p align="center"><b>The instant, offline, keyboard-first launcher for Windows — 100 % native Rust.</b><br>
<kbd>Alt</kbd> + <kbd>Space</kbd> → type → <kbd>Enter</kbd>.</p>

No browser, no WebView2, no React, no JavaScript. The UI is drawn natively with
[egui](https://github.com/emilk/egui) (OpenGL), the system integration is plain Win32.

## 📦 Install

| Package | File | Admin | Installs to |
| --- | --- | :---: | --- |
| ⚡ One-click setup | `Flint-x.y.z-x64-setup.exe` (or `flint.exe` from `cargo build --release`) | none | `%LOCALAPPDATA%\Programs\Flint` |
| 🟢 MSI, per-user | `Flint-x.y.z-x64-per-user.msi` | none | `%LOCALAPPDATA%\Programs\Flint` |
| 🔵 MSI, per-machine | `Flint-x.y.z-x64-per-machine.msi` | once | `C:\Program Files\Flint` |
| 📁 Portable ZIP | `Flint-x.y.z-x64-portable.zip` | none | wherever you unzip it |

**One-click setup:** double-click the exe once. It installs itself, appears in *Settings → Apps → Installed apps* and the Start menu, starts with Windows, and stays in the tray. Running a newer exe updates the install. Uninstall from Settings → Apps. (Debug builds, `--no-install` and the portable ZIP skip installing.)

Silent install: `msiexec /i Flint-x.y.z-x64-per-user.msi /qn`.
Both MSIs upgrade in place, close a running Flint first, and add a Start-menu shortcut.
The portable ZIP contains `portable.flag`, which keeps Flint from enabling *start with Windows*
by itself (you can still tick it in the tray menu).

## ⌨️ Use

| Key | Action |
| --- | --- |
| `Alt`+`Space` | open / close |
| `Tab` / `Shift`+`Tab` | All → Apps → Files → Folders |
| `↑` `↓` | move · `→` accept inline completion |
| `Enter` | launch (or copy a calculation) · `Shift`+`Enter` reveal in Explorer |
| `Ctrl`+`R` | re-index · `Esc` hide |

Search syntax: `:app code`, `:file budget`, `:folder projects`, `report .pdf`, `2*(3+4)`.

**Safe commands.** Restart, shut down, sign out and hibernate need a *second* `Enter`
("Press Enter again to confirm"). Registry editor, Disk management, Device manager and
Services were removed from the quick commands because they can delete data or break Windows.

**Position.** Same as the original Flint: horizontally centred, 14 % down from the top of the monitor under the mouse. Results grow downward; if a full list would reach the taskbar, the panel moves up just enough to stay clear of it.

## 🛠 Build

```powershell
rustup default stable-x86_64-pc-windows-msvc
cargo run --release                  # run it
cargo test                           # calculator tests
dotnet tool install --global wix --version 5.0.2
wix extension add -g WixToolset.Util.wixext/5.0.2
pwsh scripts/build-installers.ps1    # -> dist\*.msi + portable zip + SHA256SUMS.txt
```

## 🏗 Layout

| File | Role |
| --- | --- |
| `src/main.rs` | start-up, tray icon, single instance, hotkey |
| `src/ui.rs` | the panel (egui): input, rows, icons, debounce, keyboard |
| `src/win.rs` | window state shared with hotkey / tray threads, show / hide / toggle |
| `src/index.rs` | arena file index, app index, ranked parallel search, disk cache |
| `src/sys.rs` | Win32: icons, window placement, quick commands, autostart |
| `src/calc.rs` | safe calculator |
| `wix/Flint.wxs` | one WiX source → per-user and per-machine MSI |

## 🪶 Performance notes

* Hidden = zero work: no frames are drawn while the panel is hidden.
* File index: two string arenas plus a 16-byte record per file instead of two heap strings per
  file; search runs across all cores and returns the best 10 without sorting everything.
* The cache file is rewritten only when the file list actually changed.
* Searches run on a worker thread; only the newest keystroke is searched.
* Debounce is 120 ms, and 400 ms after a "no results" answer. When a query found nothing, typing
  more characters onto it is answered instantly without searching (it cannot find anything).
* Shell icons are loaded on a background thread, cached by extension, bounded to 300 entries.
* Launching runs on its own thread, so a slow network drive can never freeze the panel.

## 🐞 Fixed in 0.3

* `ShellExecute` ran on threads without COM initialised (some file types failed to open).
* Re-index rewrote the whole index cache even if nothing changed.
* The window was placed at 14 % of the full screen height (could overlap the taskbar).
* Index memory overhead (see above); duplicate lower-cased copies of every path.
* Rendering errors could leave a dead window (a web-view crash recovery hack) — gone with the web view.
* Ghost completion used UTF-16 offsets; now it is Unicode-correct.

## 🛡 SmartScreen

Unsigned builds trigger a "Windows protected your PC" prompt: *More info → Run anyway*,
or sign the MSI / EXE with your certificate.

## Known limits

* Text uses Segoe UI + Nirmala UI (Bengali, Hindi, Tamil, …). Chinese / Japanese / Korean file
  names are not covered by the bundled fallback fonts (that would add ~20 MB of RAM).
* Uninstall leaves the index cache in `%LOCALAPPDATA%\Flint` (delete it by hand).

MIT © subhamay
