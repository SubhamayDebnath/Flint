<p align="center"><img src="assets/logo.png" width="104" alt="Flint"></p>

<h1 align="center">⚡ Flint</h1>
<p align="center"><b>The instant, offline, keyboard-first launcher for Windows.</b><br>Press <kbd>Alt</kbd> + <kbd>Space</kbd> → type → <kbd>Enter</kbd>. Done.</p>

<p align="center">
  <img alt="license" src="https://img.shields.io/badge/license-MIT-E5484D?style=for-the-badge">
  <img alt="platform" src="https://img.shields.io/badge/Windows-10%20%7C%2011-0078D4?style=for-the-badge&logo=windows11&logoColor=white">
  <img alt="tauri" src="https://img.shields.io/badge/Tauri-2-FFC131?style=for-the-badge&logo=tauri&logoColor=black">
  <img alt="rust" src="https://img.shields.io/badge/Rust-core-CE422B?style=for-the-badge&logo=rust&logoColor=white">
  <img alt="react" src="https://img.shields.io/badge/React-19-61DAFB?style=for-the-badge&logo=react&logoColor=black">
  <img alt="zustand" src="https://img.shields.io/badge/state-Zustand-A0522D?style=for-the-badge">
  <img alt="offline" src="https://img.shields.io/badge/100%25-offline-2EA043?style=for-the-badge">
  <img alt="by" src="https://img.shields.io/badge/by-unpaidbot-8A2BE2?style=for-the-badge">
</p>

---

## 📚 Contents

[✨ Features](#-features) · [📦 Install](#-install) · [🔍 Search syntax](#-search-syntax) · [⌨️ Shortcuts](#️-keyboard-shortcuts) · [🧰 Commands](#-quick-commands) · [🛠 Build](#-build-from-source) · [🏗 How it works](#-how-it-works) · [🪶 Performance](#-performance--low-resource-design) · [🛡 SmartScreen & antivirus](#-smartscreen--antivirus) · [🩺 Troubleshooting](#-troubleshooting) · [🤝 Contributing](#-contributing)

## ✨ Features

|     | Feature               | Details                                                                                                      |
| --- | --------------------- | ------------------------------------------------------------------------------------------------------------ |
| ⚡  | **Instant hotkey**    | `Alt + Space` toggles the launcher. The window is pre-rendered at login, so even the first press is instant. |
| 🗂   | **All-drive search**  | Files and folders on every fixed drive, indexed in parallel. System folders and hidden files are skipped.    |
| 🔎  | **Extension search**  | `.pdf`, `.rs`, or `report .pdf`.                                                                             |
| 🧰  | **36 quick commands** | Power, trash, task manager, settings pages, system tools.                                                    |
| 🪟  | **Smart activation**  | A running app is focused instead of started twice.                                                           |
| 🧮  | **Calculator**        | Type `2*(3+4)`, <kbd>Enter</kbd> copies the result.                                                          |
| 🔕  | **Silent & tiny**     | Tray icon, starts with Windows (once, you can switch it off), idles at near-zero CPU.                        |
| 🎨  | **Native look**       | Dark UI, Windows Acrylic blur, no borders. All colours are tokens in `src/index.css`.                        |
| 🔒  | **Private**           | No network, no telemetry, no accounts.                                                                       |

## 📦 Install

Two installers are produced by `npm run installer:all` (or download them from the Releases page):

| Installer                     | File                                | Admin / UAC | Installs to            | Best for                                                 |
| ----------------------------- | ----------------------------------- | :---------: | ---------------------- | -------------------------------------------------------- |
| 🟢 **Per-user** (recommended) | `Flint_x.y.z_x64_user-setup.exe`    |   ❌ none   | `%LOCALAPPDATA%\Flint` | Everyone, quiet install like Chrome / VS Code user setup |
| 🔵 **Per-machine**            | `Flint_x.y.z_x64_machine-setup.exe` |   ✅ once   | `Program Files\Flint`  | Shared PCs, IT deployment (all users)                    |

Both are silent-capable for deployment: `Flint_..._setup.exe /S`. The WebView2 runtime is bundled as a silent bootstrapper, so a clean machine needs nothing else.

## 🔍 Search syntax

| Query                          | Result                                                 |
| ------------------------------ | ------------------------------------------------------ |
| `chrome`                       | Mixed results: apps, files, folders, commands (max 10) |
| `:app code`                    | 🟣 Apps only                                           |
| `:file budget` · `:file .xlsx` | 🔵 Files only (name or extension)                      |
| `:folder projects`             | 🟡 Folders only                                        |
| `report .pdf`                  | PDFs whose name contains "report"                      |

## ⌨️ Keyboard shortcuts

| Shortcut                                         | Action                                        |
| ------------------------------------------------ | --------------------------------------------- |
| <kbd>Alt</kbd>+<kbd>Space</kbd>                  | Open / toggle Flint                           |
| <kbd>Tab</kbd> / <kbd>Shift</kbd>+<kbd>Tab</kbd> | Cycle category (All → Apps → Files → Folders) |
| <kbd>↑</kbd> / <kbd>↓</kbd>                      | Move through results                          |
| <kbd>Enter</kbd>                                 | Launch, or copy the calculation               |
| <kbd>Shift</kbd>+<kbd>Enter</kbd>                | Reveal in Explorer                            |
| <kbd>→</kbd>                                     | Accept inline completion                      |
| <kbd>Ctrl</kbd>+<kbd>R</kbd>                     | Re-index now                                  |
| <kbd>Esc</kbd>                                   | Hide                                          |

## 🧰 Quick commands

| Group            | Commands                                                                                                                                                                                 |
| ---------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 🔴 Power         | `lock` · `sleep` · `hibernate` · `sign out` · `restart` · `shutdown`                                                                                                                     |
| 🟠 Quick actions | `empty trash` · `trash` · `task manager` · `show desktop` · `screenshot` · `flush dns` · `reindex`                                                                                       |
| 🟡 Folders       | `this pc` · `downloads` · `documents`                                                                                                                                                    |
| 🟢 Settings      | `settings` · `wifi` · `bluetooth` · `display` · `sound` · `installed apps` · `startup apps` · `storage` · `windows update`                                                               |
| 🔵 System tools  | `control panel` · `device manager` · `disk management` · `services` · `event viewer` · `resource monitor` · `system information` · `registry editor` · `cmd` · `powershell` · `terminal` |

## 🛠 Build from source

**Requirements:** Windows 10/11 · [Rust](https://rustup.rs) (MSVC) · Visual Studio Build Tools with _Desktop development with C++_ · [Node.js 20+](https://nodejs.org).

```powershell
npm install
npm run tauri dev          # develop, then press Alt+Space
npm run installer:user     # per-user installer  -> release/
npm run installer:machine  # per-machine installer -> release/
npm run installer:all      # both
npm run typecheck          # TypeScript
npm run format             # Prettier (use `cargo fmt` for Rust)
```

Installers and their `.sha256` files land in `release/`. Icons: `npm run tauri icon app-icon.svg`.

## 🏗 How it works

```text
 keystroke ─► Zustand store ─► 60 ms debounce ─► Rust search (ranked top-k) ─► results
                  ▲                 │ cached? instant                              │
                  └──── no flicker: old results stay until new ones arrive ◄─────────┘
```

- **Index**: all fixed drives are walked in parallel (rayon + walkdir) into memory, cached in `%LOCALAPPDATA%\Flint\`, refreshed every 30 min in the background.
- **Search**: parallel top-k scan; name-prefix beats word-prefix beats substring. Small lists are scanned sequentially.
- **Icons**: pulled lazily from the Windows shell and kept in a bounded cache (300 entries).
- **State**: [Zustand](https://github.com/pmndrs/zustand) store (`src/store.ts`); search logic in `src/hooks.ts`.
- **Files**: `src/` React UI · `src-tauri/src/lib.rs` window, hotkey, tray · `index.rs` index and search · `sys.rs` Win32.

## 🪶 Performance & low-resource design

| Area               | What Flint does                                                                                                                                   |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| ⌨️ Fast typing     | 60 ms debounce, stale responses dropped, identical results don't re-render, so there is no flicker.                                               |
| 🚀 Boot            | Caches are read first, the window is pre-rendered off-screen, the first rescan waits 10 s.                                                        |
| 🧵 Background work | Indexing runs in Windows _background mode_ (low CPU **and** disk priority) on 2–6 threads (half your cores), Store apps refreshed every 2nd scan. |
| 💾 Memory          | Bounded caches (80 queries, 300 icons), vectors trimmed after each scan, depth-limited walk (junction-loop safe).                                 |
| 🧯 Reliability     | PowerShell listing has a 20 s timeout; a panic in one scan never stops the next; UI errors self-recover.                                          |
| 🔐 Hardening       | Strict CSP, built-in tools launched by absolute `System32` path, input length limited.                                                            |

## 🛡 SmartScreen & antivirus

A brand-new unsigned `.exe` can trigger a _"Windows protected your PC"_ warning. This is reputation-based and **only code signing removes it reliably** — no setting in the app can.

What Flint already does to stay quiet: per-user install without UAC, proper publisher / version metadata (`unpaidbot`), no packers, no network access, no admin rights at runtime.

To ship with **zero warnings** like Chrome or VS Code:

1. Get a code-signing certificate (an **EV** certificate gives instant SmartScreen reputation; **Azure Trusted Signing** is the cheapest route; standard OV builds reputation over time).
2. Add it to `src-tauri/tauri.conf.json` → `bundle.windows` (`certificateThumbprint`, or `signCommand` for cloud signing) and rebuild.
3. Optional: submit the installer to [Microsoft's file submission portal](https://www.microsoft.com/wdsi/filesubmission) to clear a false positive.

## 🩺 Troubleshooting

| Problem                              | Fix                                                           |
| ------------------------------------ | ------------------------------------------------------------- |
| `Alt + Space` does nothing           | Another tool owns the shortcut (PowerToys Run…). Close it.    |
| Files missing right after first boot | The first index builds in the background; wait a few seconds. |
| `icons/icon.ico not found`           | `npm run tauri icon app-icon.svg`                             |
| `link.exe not found`                 | Install the Visual Studio C++ build tools.                    |
| Don't want autostart                 | Turn Flint off in _Settings → Apps → Startup_. It stays off.  |

## 🤝 Contributing

Flint stays **fast, minimal, offline**: no network calls, telemetry or settings screens. Keep `cargo clippy`, `cargo fmt` and `npm run typecheck` clean, run `npm run format`, and keep search instant. Open an issue before big changes. Report security issues privately via the repository's security advisories.

## 📄 License

[MIT](LICENSE) © 2026 **unpaidbot**
