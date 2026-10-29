# MOONDROP Hub Desktop (Rust + Tauri v2)

A standalone, offline-first native desktop application for configuring **MOONDROP Rays** and all MOONDROP USB-HID audio devices (`0x35D8`) without requiring Chrome or Microsoft Edge.

Built with **Rust**, **Tauri v2**, and a native OS-level **USB-HID backend (`hidapi`)**, plus a **CC Switch-style System Tray Popover** featuring one-click hardware EQ switching, **Auto-Detect Game Mode**, DAC Global Gain control, and a live **Tray Icon Editor**.

---

## Key Features

- **Zero Browser / Zero WebHID Permission Popups**:
  - Replaces browser WebHID with a native Rust `hidapi` backend and a transparent `navigator.hid` bridge (`src-bridge/tauri-hid-bridge.js`).
  - Automatically parses Windows USB-HID Report Descriptors (`MI_03` vs `MI_04`) to select the bi-directional DSP/EQ control interface (`Report ID 75` / `0x4B`, 64-byte output packets) with 100% query reliability.
- **100% Offline & Standalone**:
  - All 90 frontend JS/CSS/image bundles and all 3 WebAssembly DSP engines (`tuning_response`, `tuning_autoeq`, `tuning_calibration`) are compiled directly into the native `.exe` binary.
  - Neither `dump/` nor `dist/` is needed at runtime.
- **CC Switch-Style System Tray Toolbar (`Left-Click` or `Right-Click` on Tray Icon)**:
  - **Presets Tab**: Instant hardware EQ preset switching saved directly to device flash (`Default Tuning`, `FPS Footsteps I`, `FPS Tactical II`, `Music & Rhythm`, `Triple-A Cinema`).
  - **Auto Game Tab**: Native Win32 process & foreground-window watcher (`Toolhelp32Snapshot` + `GetForegroundWindow`) that detects when a game launches (CS2, VALORANT, Apex Legends, Overwatch 2, osu!, Cyberpunk 2077, Elden Ring, Black Myth: Wukong, etc.), automatically switches your Moondrop Rays to the mapped EQ preset, and reverts to `Default Tuning` when the game closes. Supports adding custom `.exe` rules or picking from currently running processes.
  - **Gain Tab**: Live DAC Global Gain slider (`-18.0 dB` to `0.0 dB` in Q8 fixed-point format) with `-12 / -6 / -3 / 0 dB` quick buttons and live USB-HID device/firmware telemetry.
  - **Icon Editor Tab**: Live 64×64 canvas icon customizer (Circle/Squircle/Clear badge, IEM/Bolt/EQ/Text glyphs, accent color swatches, or custom PNG/SVG/ICO upload) that updates the Windows system tray and app icon in real time and persists across restarts.
  - **Start with Windows (Minimized)**: One-click registry autostart toggle (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run` with `--minimized`) so MOONDROP Hub Desktop and Auto-Detect Game Mode launch silently in the system tray at Windows boot.
  - **Lightweight Mode**: Keep MOONDROP Hub running silently in the system tray when closing the main window.

---

## Pre-Built Binaries & Installers

Located in the repository root:

| Artifact | File | Description |
| :--- | :--- | :--- |
| **Portable Executable** | `./MOONDROP-Hub-Desktop.exe` | Single-file standalone executable (no installation required) |
| **NSIS Setup Installer** | `./MOONDROP-Hub-Desktop_1.6.5_x64-setup.exe` | Standard Windows Setup wizard (`.exe`) |
| **Windows MSI Package** | `./MOONDROP-Hub-Desktop_1.6.5_x64_en-US.msi` | Windows Installer package (`.msi`) |

---

## Supported Hardware

- **Primary Device**: **MOONDROP Rays** (`VID: 0x35D8`, `PID: 0x011B`, `Report ID: 75` / `0x4B`)
- **All MOONDROP USB-HID Devices (`VID: 0x35D8`)**:
  - FreeDSP Pro, Dusk-SP, Marigold, Chu II DSP, Quark2, Echo-A, Echo-B, Dawn Pro 2, MoonRiver 3, Mays, Rays, Pokopoko, Pill, Old Fashioned, FreeDSP Mini, Nice Buds, Pavane, DarkSaber
- **Firmware Upgrade Bootloader**:
  - `USB20` (`VID: 0x0666`, `PID: 0x0888`, `Report ID: 1`)

---

## Project Structure

```text
├── MOONDROP-Hub-Desktop.exe                   # Standalone portable release executable
├── MOONDROP-Hub-Desktop_1.6.5_x64-setup.exe   # Windows NSIS installer
├── MOONDROP-Hub-Desktop_1.6.5_x64_en-US.msi   # Windows MSI installer
├── package.json                               # Build & test scripts
├── scripts/
│   └── prepare-frontend.mjs                   # Cleans dump/ -> dist/ & injects bridge + tray UI
├── src-bridge/
│   ├── tauri-hid-bridge.js                    # WebHID (navigator.hid) polyfill backed by Tauri IPC
│   └── tray.html                              # System tray popover UI (Presets, Auto Game, Gain, Icon Editor)
├── src-tauri/
│   ├── Cargo.toml                             # Rust dependencies (tauri, hidapi, windows-sys, serde, thiserror)
│   ├── tauri.conf.json                        # Tauri window, tray popover, and bundler configuration
│   └── src/
│       ├── main.rs                            # Binary entry point
│       ├── lib.rs                             # Tauri commands, tray popover manager, and app lifecycle
│       ├── hid.rs                             # USB-HID descriptor parser, framing engine, and hardware EQ/Gain API
│       ├── game_detector.rs                   # Native Windows foreground & process watcher for Auto Game Mode
│       ├── autostart.rs                       # Windows Registry Run key manager & --minimized boot support
│       └── error.rs                           # Structured HidError definitions
└── tests/
    └── prepare-frontend.test.mjs              # Automated tests for frontend preparation & WebHID bridge
```

---

## Development & Building from Source

### Prerequisites
- **Node.js** (v20+)
- **Rust & Cargo** (1.77.2+)
- **Windows 10/11 SDK** & WebView2 Runtime (pre-installed on Windows 10/11)

### Commands

```bash
# 1. Prepare frontend assets into dist/ and run JS bridge tests
npm test

# 2. Run Rust unit tests & Clippy lints
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings

# 3. Build release portable binary (src-tauri/target/release/moondrop-hub.exe)
npm run build

# 4. Build Windows NSIS (.exe) and MSI (.msi) installers
npm run build:installer
```

---

## User Configuration Storage

Custom settings configured in the System Tray Popover are persisted automatically under `%APPDATA%\com.moondroplab.hub\`:
- `game-mode-config.json` — Auto-Detect Game Mode state and custom `.exe` → EQ preset rules.
- `custom-tray-icon.rgba` — Custom 64×64 RGBA tray & window icon created in the Tray Icon Editor.
