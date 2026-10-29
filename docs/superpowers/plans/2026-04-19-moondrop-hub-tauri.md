# Moondrop Hub Desktop (Rust + Tauri v2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a standalone Rust + Tauri v2 desktop application wrapping the complete offline dump of `https://hub.moondroplab.com/` with a native OS-level USB-HID backend (`hidapi`) and zero-popup `navigator.hid` bridge for Moondrop Rays and all other Moondrop USB-HID audio devices.

**Architecture:** A deterministic frontend preparation script (`scripts/prepare-frontend.mjs`) transforms `dump/` into `dist/`, stripping Cloudflare Rocket Loader, preserving all 91 JS/CSS chunks and 3 WASM DSP modules, and injecting a spec-compliant `navigator.hid` polyfill (`dist/tauri-hid-bridge.js`). In `src-tauri/`, a native Rust HID manager (`src-tauri/src/hid.rs`) built on `hidapi` enumerates Moondrop USB devices (`0x35d8` and `0x0666:0x0888`), prioritizes vendor-specific HID collections (`usage_page >= 0xFF00`), frames 65-byte Windows HID output/feature reports (`[report_id, ...64_bytes]`), streams incoming input reports via Tauri events, and watches USB hotplug events.

**Tech Stack:** Rust 1.98 (`edition = "2021"`), Tauri v2 (`tauri = "2"`), `hidapi = "2.6"`, `serde`, `thiserror`, Node.js (`prepare-frontend.mjs`).

**Spec:** `docs/superpowers/specs/2026-04-19-moondrop-hub-tauri-design.md`

## Global Constraints

- Target primary device: **Moondrop Rays** (`vendorId: 0x35d8` / `13784`, `productId: 0x011b` / `283`, `reportId: 75` / `0x4b`), plus all other Moondrop `0x35d8` devices and firmware upgrade bootloader `USB20` (`vendorId: 0x0666` / `1638`, `productId: 0x0888` / `2184`).
- No `unwrap()` or `expect()` in production Rust code outside `#[cfg(test)]`; use `thiserror` for structured error types (`HidError`).
- Minimum window size `1024x680` (default `1280x820`) so Moondrop Hub's `InvalidScreen` breakpoint (`md: 768px`, `lg: 1024px`) never blocks the UI.
- Offline-first: all 91 JS/CSS chunks and 3 `.wasm` DSP modules (`tuning_response`, `tuning_autoeq`, `tuning_calibration`) must load locally from `dist/assets/`.

## Review Focus

1. **Windows composite USB HID interface collision**: Moondrop USB-C earbuds expose both standard USB Audio consumer control (volume buttons, `usage_page == 0x000C`, locked by Windows `hidserv`) and vendor-defined HID (`usage_page >= 0xFF00`, report ID `75`). Selecting or opening the consumer-control path fails on Windows; `hid.rs` must rank and select vendor-defined interfaces (`usage_page >= 0xFF00`) first.
2. **Windows `WriteFile` HID report length requirement**: Windows `hidapi::write` requires the buffer to start with the 1-byte `report_id` followed by the exact output report byte length defined in the HID descriptor (64 bytes for Moondrop `0x35d8` devices -> 65 bytes total). Short unpadded buffers (`[0x4b, 0x80, 0x0c, 0x00]`) fail with `Invalid parameter` on Windows unless padded to 65 bytes.
3. **`selectBestDevice` (`ig`) collection inspection**: Moondrop Hub's JS bundle checks `device.collections` for `inputReports` and `outputReports` containing `reportId: 75`. Synthesized `HIDDevice` objects in `tauri-hid-bridge.js` must include matching `collections` metadata.
4. **Cloudflare Rocket Loader script blocking**: `dump/index.raw.html` has `type="166fb5642e6478c62f4868bc-module"` and `rocket-loader.min.js`. If not stripped cleanly, the Vue app never mounts offline.
5. **Input report stripping of leading Report ID byte**: When `hidapi::read_timeout` reads a report on Windows, `buf[0]` may or may not be `report_id` (`0x4b`) depending on descriptor numbering. `normalize_input_report` must strip a leading `expected_report_id` byte so `event.data` in WebHID starts at byte 0 (`OP`), matching Chromium's `HIDInputReportEvent.data`.

---

### Task 1: Frontend Asset Preparation & `navigator.hid` Bridge (`dist/`)

**Files:**
- Create: `scripts/prepare-frontend.mjs`
- Create: `src-bridge/tauri-hid-bridge.js`
- Create: `tests/prepare-frontend.test.mjs`
- Create: `package.json`

**Interfaces:**
- Produces:
  - `dist/index.html` (clean HTML without Rocket Loader, loading `/tauri-hid-bridge.js` before `/assets/index-CIZ2xpvU.js`)
  - `dist/tauri-hid-bridge.js` (defines `window.__MOONDROP_HID_BRIDGE__` and installs `navigator.hid` backed by Tauri IPC commands `hid_list_devices`, `hid_open_device`, `hid_close_device`, `hid_send_report`, `hid_send_feature_report` and events `hid:input-report`, `hid:connect`, `hid:disconnect`)
  - `dist/assets/*` (all 91 JS/CSS/image files and 3 `.wasm` files copied from `dump/assets/`)

- [ ] **Step 1: Write the failing Node test `tests/prepare-frontend.test.mjs`**

Test that `prepareFrontend()` generates `dist/index.html` without `rocket-loader`, with `<script src="/tauri-hid-bridge.js"></script>` and `<script type="module" crossorigin src="/assets/index-CIZ2xpvU.js"></script>`, copies all 3 `.wasm` files, and that `tauri-hid-bridge.js` synthesizes `HIDDevice` objects with `collections` containing `reportId: 75` for `0x35d8` and `reportId: 1, 2` for `0x0666`.

- [ ] **Step 2: Run the test to verify it fails**

Run: `node --test tests/prepare-frontend.test.mjs`
Expected: FAIL (`Cannot find module '../scripts/prepare-frontend.mjs'`)

- [ ] **Step 3: Implement `src-bridge/tauri-hid-bridge.js`, `scripts/prepare-frontend.mjs`, and `package.json`**

Implement `src-bridge/tauri-hid-bridge.js` with `NativeHIDDevice` and `NativeHID` (supporting both Tauri IPC `window.__TAURI_INTERNALS__` and fallback to native WebHID if run in a normal browser), and `scripts/prepare-frontend.mjs` to build `dist/` and generate PNG/ICO app icons from `dump/favicon.ico` / `dump/moondrop-dark.svg`.

- [ ] **Step 4: Run the test to verify it passes**

Run: `node --test tests/prepare-frontend.test.mjs`
Expected: PASS

---

### Task 2: Rust USB-HID Protocol & Framing Engine (`src-tauri/src/hid.rs`)

**Files:**
- Create: `src-tauri/Cargo.toml`
- Create: `src-tauri/build.rs`
- Create: `src-tauri/src/error.rs`
- Create: `src-tauri/src/hid.rs`

**Interfaces:**
- Produces:
  - `pub enum HidError` (`thiserror::Error`, serializable to string for Tauri IPC)
  - `pub struct HidDeviceDto { pub device_key: String, pub vendor_id: u16, pub product_id: u16, pub product_name: String, pub usage_page: u16, pub usage: u16, pub interface_number: i32, pub report_id: u8, pub opened: bool }`
  - `pub fn is_supported_moondrop_device(vendor_id: u16, product_id: u16) -> bool`
  - `pub fn interface_priority_score(usage_page: u16, usage: u16, interface_number: i32) -> i32`
  - `pub fn build_output_report_packet(report_id: u8, payload: &[u8], report_len: usize) -> Vec<u8>`
  - `pub fn normalize_input_report(raw: &[u8], expected_report_id: u8) -> (u8, Vec<u8>)`
  - `pub struct HidManager` with `list_devices`, `open_device`, `close_device`, `send_report`, `send_feature_report`

- [ ] **Step 1: Write unit tests in `src-tauri/src/hid.rs`**

Write Rust unit tests covering:
1. `is_supported_moondrop_device_recognizes_rays_and_usb20`: `0x35d8:0x011b` (Rays) -> `true`, `0x0666:0x0888` (USB20) -> `true`, `0x046d:0xc52b` (Logitech) -> `false`.
2. `interface_priority_score_prefers_vendor_defined_usage_page`: `usage_page = 0xFF00` scores higher than consumer control `0x000C` or keyboard `0x0001`.
3. `build_output_report_packet_pads_short_payload_to_65_bytes`: `report_id = 75`, `payload = &[0x80, 0x0c, 0x00]`, `report_len = 64` produces a 65-byte `Vec<u8>` starting with `[75, 0x80, 0x0c, 0x00, 0, ...]`.
4. `normalize_input_report_strips_leading_report_id_when_present`: `[75, 0x80, 0x0c, 0x05, ...]` -> `(75, vec![0x80, 0x0c, 0x05, ...])`, and `[0x80, 0x0c, 0x05, ...]` -> `(75, vec![0x80, 0x0c, 0x05, ...])`.
5. `deduplicate_candidates_picks_best_interface_per_vid_pid`: Given a composite device with `0x000C` (consumer control) and `0xFF00` (vendor HID), selects `0xFF00`.

- [ ] **Step 2: Implement `src-tauri/src/error.rs` and `src-tauri/src/hid.rs`**

Implement the pure framing/filtering functions and the `HidManager` with reader thread management and hotplug polling.

- [ ] **Step 3: Run `cargo test` to verify all Rust unit tests pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS (5 tests passing)

---

### Task 3: Tauri v2 Application Shell, Tray & IPC Wiring (`src-tauri/src/lib.rs`, `src-tauri/src/main.rs`)

**Files:**
- Create: `src-tauri/src/lib.rs`
- Create: `src-tauri/src/main.rs`
- Create: `src-tauri/tauri.conf.json`
- Create: `src-tauri/capabilities/default.json`
- Create: `src-tauri/icons/icon.ico` (and PNG icons)

**Interfaces:**
- Consumes: `HidManager`, `HidDeviceDto`, `HidError` from `crate::hid` and `crate::error`
- Produces:
  - Tauri commands: `hid_list_devices`, `hid_open_device`, `hid_close_device`, `hid_send_report`, `hid_send_feature_report`
  - System tray menu (`Show Moondrop Hub`, `Reconnect Device`, `Quit`)
  - Hotplug watcher thread emitting `hid:connect` and `hid:disconnect`
  - Compiled binary `src-tauri/target/release/moondrop-hub.exe`

- [ ] **Step 1: Implement `src-tauri/src/lib.rs`, `src-tauri/src/main.rs`, `src-tauri/tauri.conf.json`, and `src-tauri/capabilities/default.json`**

Wire the Tauri v2 builder, register `HidManager` state, register all 5 HID commands, set up the system tray and window configuration (`1280x820`, `minWidth: 1024`, `minHeight: 680`, title `"MOONDROP Hub Desktop"`).

- [ ] **Step 2: Run `cargo clippy` and `cargo test`**

Run: `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings && cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS with zero warnings.

- [ ] **Step 3: Build the release executable and verify the UI + simulated Moondrop Rays connection**

Run: `cargo build --manifest-path src-tauri/Cargo.toml --release`
Expected: Compiles `src-tauri/target/release/moondrop-hub.exe` cleanly.
