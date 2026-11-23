<p align="center"><img src="assets/logo.png" width="110" alt="Flint"></p>

<h1 align="center">⚡ Flint</h1>

<p align="center">
  <b>Press <kbd>Alt</kbd> + <kbd>Space</kbd>, type, press <kbd>Enter</kbd>. Done.</b><br>
  A fast, offline launcher for Windows that opens apps, files, folders, settings and
  system commands, and does quick maths, all from one box.
</p>

<p align="center">
  <img alt="Windows" src="https://img.shields.io/badge/Windows-10%20%7C%2011-0078D4?style=for-the-badge&logo=windows11&logoColor=white">
  <img alt="Rust" src="https://img.shields.io/badge/100%25-native%20Rust-CE422B?style=for-the-badge&logo=rust&logoColor=white">
  <img alt="Offline" src="https://img.shields.io/badge/works-offline-2EA043?style=for-the-badge">
  <img alt="License" src="https://img.shields.io/badge/license-MIT-E5484D?style=for-the-badge">
</p>

<p align="center"><sub>Made by <b>subhmay</b></sub></p>

---

## ✨ Why Flint

| | |
|---|---|
| ⚡ **Instant** | One hotkey from anywhere. The panel is ready the moment you press it. |
| 🪶 **Light** | A real native app: no browser engine, no background web processes. Hidden means idle. |
| 🔒 **Private** | Everything stays on your PC. No internet, no accounts, no tracking. |
| ⌨️ **Keyboard first** | Never touch the mouse. (You can click if you want to.) |
| 🎨 **Looks right** | Dark, borderless, blurred glass panel that fits Windows 11 and 10. |

---

## 🚀 Get started

1. Download the setup `.exe` and **double-click it once**.
2. Flint installs for you (no admin rights), appears in the tray and starts with Windows.
3. Press **Alt + Space** from anywhere.

That's all. You never have to start it again. Remove it any time from
**Settings → Apps → Installed apps → Flint**.

---

## 🔍 What you can do

### Find and open anything
Just start typing. Flint searches **apps, files, folders and commands** together and ranks the best match first.

| Type | You get |
|---|---|
| `chrome` | Chrome, plus matching files, folders and commands |
| `:app code` | Apps only |
| `:file budget` | Files only |
| `:folder projects` | Folders only |
| `report .pdf` | PDFs with "report" in the name |
| `.xlsx` | Every Excel file |

Press <kbd>Tab</kbd> to switch between **All → Apps → Files → Folders**.

### Smart app launching
If the app is **already running**, Flint brings it to the front instead of opening a second copy.

### Built-in calculator
Type `2*(3+4)`, `1920/3` or `2^10`. The answer appears at the top. Press <kbd>Enter</kbd> to copy it.
Supports `+ − × ÷ % ^` and brackets.

### Inline completion
Flint shows the rest of the name in grey as you type. Press <kbd>→</kbd> to accept it.

### 32 quick commands
Type a few letters (for example `wifi`, `lock`, `task`) and press <kbd>Enter</kbd>.

| Group | Commands |
|---|---|
| 🔴 **Power** | lock · sleep · hibernate · sign out · restart · shut down |
| 🟠 **Quick actions** | empty trash · open trash · task manager · show desktop · screenshot · flush DNS · re-index |
| 🟡 **Folders** | this PC · downloads · documents |
| 🟢 **Settings** | settings · Wi-Fi · Bluetooth · display · sound · installed apps · startup apps · storage · Windows Update |
| 🔵 **Tools** | control panel · event viewer · resource monitor · system information · command prompt · PowerShell · Windows Terminal |

> 🛡️ **Safe by design.** Restart, shut down, sign out and hibernate ask you to press
> <kbd>Enter</kbd> a **second time** ("Press Enter again to confirm"), so you can never lose
> work by accident. Tools that can delete data or break Windows (Registry editor, Disk management,
> Device manager, Services) are deliberately **not** included.

---

## ⌨️ Keyboard shortcuts

| Key | Action |
|---|---|
| <kbd>Alt</kbd> + <kbd>Space</kbd> | Open / close Flint |
| <kbd>↑</kbd> <kbd>↓</kbd> | Move through results |
| <kbd>Enter</kbd> | Open (or copy the calculator answer) |
| <kbd>Shift</kbd> + <kbd>Enter</kbd> | Show the file in Explorer |
| <kbd>Tab</kbd> / <kbd>Shift</kbd> + <kbd>Tab</kbd> | Next / previous category |
| <kbd>→</kbd> | Accept inline completion |
| <kbd>Ctrl</kbd> + <kbd>R</kbd> | Refresh the file list now |
| <kbd>Esc</kbd> | Hide Flint |

Clicking anywhere outside the panel also hides it.

---

## 🖥️ Tray icon

Flint lives in the system tray. Left-click opens it. Right-click for:

* **Open Flint**
* **Start with Windows** (tick or untick any time)
* **Quit**

---

## 🧠 How it works (for the curious)

* **Indexing.** Flint scans your fixed drives in the background at low priority, so it never slows
  down what you are doing. The list is cached, so search works **immediately after boot** and is
  refreshed every 30 minutes (or press <kbd>Ctrl</kbd> + <kbd>R</kbd>).
* **What is skipped.** Hidden and system files, `Windows`, `Program Files`, `ProgramData`, `AppData`,
  `node_modules` and similar noise, plus file types you never open (`.dll`, `.sys`, `.tmp`, `.log` …).
* **Apps.** Start Menu shortcuts and Microsoft Store apps.
* **Smooth typing.** Searching waits briefly after your last keystroke. If nothing was found, it waits
  longer, and typing *more* onto a search that found nothing answers instantly without searching again.
* **Where it appears.** Centred horizontally, a little above the middle of the screen, on the monitor
  where your mouse is. If a full list would reach the taskbar, the panel moves up just enough to stay clear.

---

## 🛠️ For developers

**Stack:** Rust · [egui / eframe](https://github.com/emilk/egui) (OpenGL) · Win32 · no WebView, no JavaScript.

```text
src/
├── main.rs      start-up, tray icon, single instance, global hotkey
├── ui.rs        the panel: input, rows, icons, debounce, keyboard
├── win.rs       window state shared with the hotkey and tray threads
├── index.rs     file + app index, ranked parallel search, disk cache
├── sys.rs       Win32: icons, placement, quick commands, autostart
├── install.rs   one-click self-install / uninstall
└── calc.rs      safe calculator (unit-tested)
wix/Flint.wxs    MSI definition (per-user and per-machine)
```

### Build

```powershell
# one time: install Rust from https://rustup.rs (MSVC toolchain)
cargo run --release      # run it
cargo test               # run the tests
build.bat                # build dist\Flint-setup.exe
```

### Build every package (MSI, setup exe, portable ZIP)

```powershell
dotnet tool install --global wix --version 5.0.2
wix extension add -g WixToolset.Util.wixext/5.0.2
pwsh scripts\build-installers.ps1
```

Output in `dist\`: `Flint-x.y.z-x64-setup.exe`, `…-per-user.msi`, `…-per-machine.msi`,
`…-portable.zip` and `SHA256SUMS.txt`.

### Command line flags

| Flag | Effect |
|---|---|
| `--no-install` | Run without installing (useful when testing a build) |
| `--uninstall-app` | Remove Flint completely (what Settings → Apps runs) |

### Data on disk

`%LOCALAPPDATA%\Flint\` holds only the index cache (`index.txt`, `apps.txt`). Delete it any time.

---

## 🩺 Troubleshooting

| Problem | Fix |
|---|---|
| **Alt + Space does nothing** | Another program may own the shortcut. Close it, or open Flint from the tray icon. |
| **A new file is not found** | Press <kbd>Ctrl</kbd> + <kbd>R</kbd> to refresh the list. |
| **Not starting with Windows** | Right-click the tray icon and tick **Start with Windows**. |
| **"Windows protected your PC"** | The app is not code-signed yet. Click **More info → Run anyway**. |
| **Some letters show as squares** | Flint supports Latin and Indian scripts (Bengali, Hindi, Tamil …). East-Asian file names are not covered. |

---

## 📄 License

MIT © subhmay