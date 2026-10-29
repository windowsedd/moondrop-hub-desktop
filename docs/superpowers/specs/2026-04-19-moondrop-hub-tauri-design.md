# Moondrop Hub Desktop (Rust + Tauri v2) — Design Spec

## 1. Overview & Goal
Package the dumped `https://hub.moondroplab.com/` web application (v1.6.5, build `26B1081`) into a standalone, native Rust + Tauri v2 desktop application with a native OS-level USB-HID backend (`hidapi`). Users can plug in their **Moondrop Rays** (or any other Moondrop USB-HID device supported by Moondrop Hub) and configure Parametric EQ, Gaming Presets, Pre-Gain / Global Gain, ANC / Control Panel features, and Firmware Upgrades directly from a desktop window without needing Microsoft Edge or Chrome's WebHID permission prompt.

## 2. Reverse-Engineered Moondrop Hub Architecture

### 2.1 Bundled Frontend Assets (`dump/`)
- **Entry HTML**: `dump/index.raw.html` (must have Cloudflare's `rocket-loader.min.js` script and rewritten `type="...-text/javascript"` attributes restored to standard `<script type="module" crossorigin src="./assets/index-CIZ2xpvU.js"></script>`).
- **JavaScript & CSS**: 91 chunks in `dump/assets/`, including `index-CIZ2xpvU.js`, `index-BsKbV-8h.js`, `tuning.worker-B6MZZOdl.js`, locale bundles (`en`, `ja`, `zh-CN`), and UI route chunks.
- **WebAssembly DSP Engines**:
  - `tuning_response-DMc1_tCy.wasm` (frequency response curve calculation)
  - `tuning_autoeq-BCH8TLkw.wasm` (AutoEQ filter optimization)
  - `tuning_calibration-CDpvrBru.wasm` (speaker/headphone calibration)
- **Icons & Branding**: `favicon.ico`, `moondrop.svg`, `moondrop-dark.svg`.

### 2.2 Device Registry & USB-HID Protocol
Moondrop Hub communicates exclusively via `navigator.hid` (WebHID):
- **Primary Moondrop Vendor ID**: `0x35d8` (`13784`).
- **Supported Audio Devices (`0x35d8`)**:
  - **Moondrop Rays**: `productId: 283` (`0x011b`), `reportId: 75` (`0x4b`), `peqIndex: 7`, `peqCount: 8`
  - **AG Rays**: `productId: 286` (`0x011e`), `reportId: 75` (`0x4b`)
  - **MM3A**: `productId: 256`, `reportId: 75`
  - **Echo-BP**: `productId: 263`, `reportId: 75`
  - **Gaming Maestro**: `productId: 265`, `reportId: 75`
  - **GM01 Pro**: `productId: 266`, `reportId: 75`
  - **Old Fashioned**: `productId: 267`, `reportId: 75`
  - **DAWN PRO2**: `productId: 279`, `reportId: 75`
  - **Deco Audio System**: `productId: 287`, `reportId: 75`
  - **DHA15**: `productId: 285`, `reportId: 75`
  - **E.S. combo**: `productId: 281`, `reportId: 75`
  - **FreeDSP Mini**: `productId: 282`, `reportId: 75`
  - **FreeDSP Pro**: `productId: 278`, `reportId: 75`
  - **INN Deco75-DH Audio**: `productId: 290`, `reportId: 75`
  - **Marigold**: `productId: 288`, `reportId: 75`
  - **MOONRIVER 3**: `productId: 289`, `reportId: 75`
  - **POSITION**: `productId: 291`, `reportId: 75`
  - **CM1 (Microphone)**: `productId: 304`, `reportId: 75`
- **Firmware Upgrade Bootloader (`USB20`)**:
  - `vendorId: 0x0666` (`1638`), `productId: 0x0888` (`2184`), `name: "USB20"`
  - Uses HID Feature Reports (`sendFeatureReport` with report ID `1`, input/feature report ID `2`).
- **HID Report Framing**:
  - Standard Moondrop devices use Report ID `75` (`0x4b`).
  - Packet format: `[OP, CMD, LEN, ...PAYLOAD]` (padded to standard HID report length, typically 64 bytes + 1 byte Report ID on `hidapi::write`).
  - `OP_WRITE = 0x01`, `OP_READ = 0x80` (`128`).
  - Commands for Moondrop Rays (`class Us`):
    - `0x01`: `EQ_SAVE_FLASH`
    - `0x02`: `ADC_OFFSET`
    - `0x03`: `DAC_OFFSET` (Global Gain)
    - `0x04`: `OFFSET_SAVE_FLASH`
    - `0x05`: `FLASH_CLEAR`
    - `0x09`: `EQ_UPDATE` (33-byte write with 5x int32 little-endian Q30 biquad IIR coefficients + frequency + Q + gain + filterType + peqIndex; 2-byte read)
    - `0x0A`: `EQ_COEFF` (enable PEQ bands)
    - `0x0C`: `FIRMWARE_VERSION`
    - `0x0F`: `EQ_ACTIVE` (preset / PEQ index)
    - `0x23`: `PRE_GAIN`
    - `0x24`: `DEVICE_VOLUME` (also emitted as unsolicited input report when physical buttons are pressed)
    - `0xFF`: `UPGRADE_ENTER`

### 2.3 Why `selectBestDevice` (`ig`) Matters for the Bridge
In `index-CIZ2xpvU.js`, Moondrop Hub's `selectBestDevice(devices)` (`ig`) inspects `device.collections` on each `HIDDevice`:
1. Prefers a collection whose `outputReports` AND `inputReports` both contain `reportId === 75` (`0x4b`).
2. Falls back to any collection with `outputReports` containing `reportId === 75`, or any non-empty `outputReports`.
Therefore, our native Rust `navigator.hid` bridge MUST populate `device.collections` on every synthesized `HIDDevice` with:
- For `vendorId === 0x35d8`: `[{ usagePage: 0xFF00, usage: 0x01, type: 0, children: [], inputReports: [{ reportId: 75, items: [] }], outputReports: [{ reportId: 75, items: [] }], featureReports: [] }]`
- For `vendorId === 0x0666` (`USB20`): `[{ usagePage: 0xFF00, usage: 0x01, type: 0, children: [], inputReports: [{ reportId: 2, items: [] }], outputReports: [{ reportId: 1, items: [] }], featureReports: [{ reportId: 1, items: [] }, { reportId: 2, items: [] }] }]`
Additionally, when `hidapi::HidApi::device_list()` enumerates multiple HID interfaces (collections/usages) for the same physical USB device (e.g. USB Audio Control consumer media keys vs. vendor-defined HID report `0x4b`), the Rust backend must prefer the vendor-specific interface (`usage_page >= 0xFF00`) or open/test the interface that accepts Report ID `0x4b`.

## 3. System Architecture

### 3.1 Frontend Preparation (`dist/`)
- A deterministic build/prepare script (`scripts/prepare-frontend.mjs`) copies `dump/` into `dist/`, cleans `index.html`:
  - Strips Cloudflare Rocket Loader script (`cdn-cgi/scripts/.../rocket-loader.min.js`) and restores `<script type="module" crossorigin src="/assets/index-CIZ2xpvU.js"></script>`.
  - Injects `/tauri-hid-bridge.js` BEFORE the main application module so `navigator.hid` is overridden with the native Rust HID bridge before Vue/Pinia initializes.
  - Adds a subtle desktop status/titlebar helper if needed while keeping 100% of Moondrop Hub's native UI intact.

### 3.2 Native Rust USB-HID Backend (`src-tauri/src/hid.rs`)
- Uses `hidapi` crate (`HidApi::new_without_enumerate()` + `refresh_devices()`) wrapped in thread-safe state (`Arc<Mutex<HidManager>>`).
- **Device Filtering & Interface Selection**:
  - Filters for Moondrop VID `0x35d8` (`13784`) and Upgrade VID `0x0666` (`1638`, PID `0x0888`).
  - Groups/deduplicates interfaces by `(vendor_id, product_id, usage_page, usage)` and prioritizes vendor-defined usage pages (`usage_page >= 0xFF00`, or the interface supporting report ID `75`) so Windows doesn't fail trying to open a locked keyboard/consumer-control interface.
- **Tauri Commands**:
  - `hid_list_devices() -> Result<Vec<HidDeviceDto>, HidError>`: Enumerates connected Moondrop / USB20 HID devices.
  - `hid_open_device(device_key: String) -> Result<HidDeviceDto, HidError>`: Opens the `HidDevice`, sets non-blocking or spawns a dedicated background reader thread for `inputreport` events, and stores the handle.
  - `hid_close_device(device_key: String) -> Result<(), HidError>`: Stops the reader thread and closes the handle.
  - `hid_send_report(device_key: String, report_id: u8, data: Vec<u8>) -> Result<(), HidError>`: Prepends `report_id` to `data` (padding to 65 bytes `[report_id, ...64 bytes]` if shorter than 64 bytes on Windows `WriteFile`) and writes to the HID device.
  - `hid_send_feature_report(device_key: String, report_id: u8, data: Vec<u8>) -> Result<(), HidError>`: Prepends `report_id` and calls `device.send_feature_report(&buf)`.
- **Background Input Report Reader & Hotplug Poller**:
  - When a device is opened, a dedicated Rust thread reads incoming HID reports (`device.read_timeout(&mut buf, 50)`) and emits Tauri events `hid:input-report` (`{ deviceKey, reportId, data }`) to the webview.
  - A background hotplug watcher polls `refresh_devices()` every 1000ms and emits `hid:connect` and `hid:disconnect` events so plugging/unplugging Moondrop Rays automatically triggers `navigator.hid` `connect` / `disconnect` events in the frontend.

### 3.3 JavaScript `navigator.hid` Bridge (`dist/tauri-hid-bridge.js`)
- Implements a spec-compliant `EventTarget`-based `HID` object and `HIDDevice` class backed by `window.__TAURI_INTERNALS__.invoke` and `listen`:
  - `navigator.hid.getDevices()`: Calls `hid_list_devices`, returns cached/updated `NativeHIDDevice` instances.
  - `navigator.hid.requestDevice({ filters })`: Calls `hid_list_devices`, filters matching `vendorId`/`productId`, and immediately returns matching `NativeHIDDevice` instances **without any browser popup**!
  - `NativeHIDDevice`:
    - Properties: `opened`, `vendorId`, `productId`, `productName`, `collections` (populated with Report ID `75` or `1`/`2` for upgrade device).
    - Methods: `open()`, `close()`, `sendReport(reportId, BufferSource)`, `sendFeatureReport(reportId, BufferSource)`, `receiveFeatureReport(reportId)`.
    - Events: `oninputreport` property + `addEventListener('inputreport', ...)` receiving `Event` with `{ device, reportId, data: DataView }`.

### 3.4 Desktop Integration (`src-tauri/src/lib.rs`)
- Window sized at `1280x820` with `minWidth: 1024, minHeight: 680` so Moondrop Hub's `InvalidScreen` breakpoint (`md: 768px`, `lg: 1024px`) never triggers accidentally.
- System Tray icon with menu ("Show Moondrop Hub", "Quit") so users can keep Moondrop Hub running in the background.
- Custom Moondrop icon generated from `dump/moondrop-dark.svg` / `dump/favicon.ico`.
