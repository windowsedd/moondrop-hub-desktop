use crate::error::HidError;
use hidapi::{HidApi, HidDevice};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::CString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tauri::{AppHandle, Manager, WebviewWindow};

/// Moondrop USB Vendor ID (`0x35d8` / `13784`).
pub const MOONDROP_VENDOR_ID: u16 = 0x35d8;
/// Moondrop Rays USB Product ID (`0x011b` / `283`).
pub const MOONDROP_RAYS_PRODUCT_ID: u16 = 0x011b;
/// Firmware upgrade bootloader (`USB20`) Vendor ID (`0x0666` / `1638`).
pub const UPGRADE_VENDOR_ID: u16 = 0x0666;
/// Firmware upgrade bootloader (`USB20`) Product ID (`0x0888` / `2184`).
pub const UPGRADE_PRODUCT_ID: u16 = 0x0888;
/// Default HID Report ID used by Moondrop `0x35d8` devices (`75` / `0x4b`).
pub const MOONDROP_REPORT_ID: u8 = 75;
/// Standard payload length for Moondrop HID output reports (63 bytes + 1 byte report ID = 64 bytes).
pub const STANDARD_REPORT_LEN: usize = 63;

/// Summary of an interface's HID report descriptor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DescriptorSummary {
    pub input_report_ids: Vec<u8>,
    pub output_report_ids: Vec<u8>,
}

impl DescriptorSummary {
    pub fn supports_report_pair(&self, report_id: u8) -> bool {
        self.input_report_ids.contains(&report_id) && self.output_report_ids.contains(&report_id)
    }

    pub fn has_any_output_report(&self) -> bool {
        !self.output_report_ids.is_empty()
    }
}

/// Parses a raw USB-HID Report Descriptor and extracts all Input (`0x80`) and Output (`0x90`)
/// report IDs (`0x84` Global Report ID item), mirroring Moondrop Hub's `selectBestDevice` (`ig`).
pub fn parse_report_descriptor(desc: &[u8]) -> DescriptorSummary {
    let mut summary = DescriptorSummary::default();
    let mut current_report_id: u8 = 0;
    let mut idx = 0;

    while idx < desc.len() {
        let prefix = desc[idx];
        if prefix == 0xfe {
            // Long item: [0xFE, data_len, long_tag, ...data]
            if idx + 2 >= desc.len() {
                break;
            }
            let data_len = desc[idx + 1] as usize;
            idx += 3 + data_len;
            continue;
        }

        let size_code = prefix & 0x03;
        let data_len = match size_code {
            0 => 0,
            1 => 1,
            2 => 2,
            3 => 4,
            _ => 0,
        };
        let tag_and_type = prefix & 0xfc;

        if idx + 1 + data_len > desc.len() {
            break;
        }

        match tag_and_type {
            0x84 if data_len >= 1 => {
                // Global item: Report ID (0x85 when size=1)
                current_report_id = desc[idx + 1];
            }
            0x80 if !summary.input_report_ids.contains(&current_report_id) => {
                // Main item: Input (0x81 when size=1)
                summary.input_report_ids.push(current_report_id);
            }
            0x90 if !summary.output_report_ids.contains(&current_report_id) => {
                // Main item: Output (0x91 when size=1)
                summary.output_report_ids.push(current_report_id);
            }
            _ => {}
        }

        idx += 1 + data_len;
    }

    summary
}

/// Serializable DTO representing a discovered Moondrop or USB20 HID device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HidDeviceDto {
    pub device_key: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub product_name: String,
    pub usage_page: u16,
    pub usage: u16,
    pub interface_number: i32,
    pub report_id: u8,
    pub opened: bool,
}

/// Internal candidate discovered during `hidapi` enumeration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HidInterfaceCandidate {
    pub path: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub product_name: String,
    pub usage_page: u16,
    pub usage: u16,
    pub interface_number: i32,
    pub descriptor: DescriptorSummary,
}

/// Serializable quick-status DTO returned to the system tray popover.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayStatusDto {
    pub connected: bool,
    pub device_key: Option<String>,
    pub product_name: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub firmware_version: String,
    pub active_eq_index: u8,
    pub global_gain_db: f32,
}

/// Encodes a floating-point dB gain value into Moondrop's fixed-point `i16` (`round(db * 256.0)`).
pub fn encode_fixed_gain_q8(db: f32) -> i16 {
    let clamped = db.clamp(-18.0, 12.0);
    (clamped * 256.0).round() as i16
}

/// Decodes Moondrop's fixed-point `i16` (`raw / 256.0`) into a 1-decimal floating-point dB value.
pub fn decode_fixed_gain_q8(raw: i16) -> f32 {
    ((raw as f32 / 256.0) * 10.0).round() / 10.0
}

/// Decodes a null-terminated ASCII firmware string starting at offset `3` of an input report.
pub fn decode_firmware_string(data: &[u8]) -> String {
    if data.len() <= 3 {
        return String::from("Unknown");
    }
    let slice = &data[3..];
    let end = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
    let s = String::from_utf8_lossy(&slice[..end]).trim().to_string();
    if s.is_empty() {
        String::from("Unknown")
    } else {
        s
    }
}

/// Payload emitted to the frontend when an input report arrives from the USB device.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputReportEventPayload {
    pub device_key: String,
    pub report_id: u8,
    pub data: Vec<u8>,
}

/// Returns `true` if `(vendor_id, product_id)` belongs to a Moondrop audio device (`0x35d8`)
/// or the Moondrop `USB20` firmware upgrade bootloader (`0x0666:0x0888`).
pub fn is_supported_moondrop_device(vendor_id: u16, product_id: u16) -> bool {
    vendor_id == MOONDROP_VENDOR_ID
        || (vendor_id == UPGRADE_VENDOR_ID && product_id == UPGRADE_PRODUCT_ID)
}

/// Returns the default HID report ID expected by Moondrop Hub for the given VID/PID.
pub fn default_report_id_for_device(vendor_id: u16, product_id: u16) -> u8 {
    if vendor_id == UPGRADE_VENDOR_ID && product_id == UPGRADE_PRODUCT_ID {
        1
    } else {
        MOONDROP_REPORT_ID
    }
}

/// Ranks HID interfaces on composite USB audio devices.
/// Mirrors Moondrop Hub's `selectBestDevice` (`ig`):
/// 1. Highest priority (+5000): Descriptor has BOTH Input and Output reports for `expected_report_id` (e.g. `0x4B` / `75`).
/// 2. Secondary priority (+1500): Descriptor has at least one Output report (`0x91`), meaning `WriteFile` is valid.
/// 3. Penalty (-2000): Input-only interface (e.g. `MI_04` consumer media keys with no Output report, where `WriteFile` fails with `0x00000001 Incorrect function`).
pub fn candidate_priority_score(cand: &HidInterfaceCandidate) -> i32 {
    let expected_report_id = default_report_id_for_device(cand.vendor_id, cand.product_id);
    let mut score = 0;

    if cand.descriptor.supports_report_pair(expected_report_id) {
        score += 5000;
    } else if cand.descriptor.has_any_output_report() {
        score += 1500;
    } else if !cand.descriptor.input_report_ids.is_empty() {
        // Known input-only interface (such as MI_04 volume buttons)
        score -= 2000;
    }

    if cand.usage_page >= 0xff00 {
        score += 1000;
    } else if cand.usage_page != 0x000c && cand.usage_page != 0x0001 {
        score += 200;
    }

    if cand.usage == 0x0001 {
        score += 50;
    }

    score
}

/// Legacy helper retained for unit testing usage-page-only heuristics when descriptor is empty.
pub fn interface_priority_score(usage_page: u16, usage: u16, _interface_number: i32) -> i32 {
    let dummy = HidInterfaceCandidate {
        path: String::new(),
        vendor_id: MOONDROP_VENDOR_ID,
        product_id: MOONDROP_RAYS_PRODUCT_ID,
        product_name: String::new(),
        usage_page,
        usage,
        interface_number: 0,
        descriptor: DescriptorSummary::default(),
    };
    candidate_priority_score(&dummy)
}

/// Selects the highest-priority HID interface candidate per `(vendor_id, product_id)` pair.
pub fn select_best_candidates(
    candidates: Vec<HidInterfaceCandidate>,
) -> Vec<HidInterfaceCandidate> {
    let mut best_by_vid_pid: HashMap<(u16, u16), HidInterfaceCandidate> = HashMap::new();

    for cand in candidates {
        if !is_supported_moondrop_device(cand.vendor_id, cand.product_id) {
            continue;
        }
        let key = (cand.vendor_id, cand.product_id);
        let cand_score = candidate_priority_score(&cand);

        match best_by_vid_pid.get(&key) {
            Some(existing) => {
                let existing_score = candidate_priority_score(existing);
                if cand_score > existing_score {
                    best_by_vid_pid.insert(key, cand);
                }
            }
            None => {
                best_by_vid_pid.insert(key, cand);
            }
        }
    }

    let mut result: Vec<HidInterfaceCandidate> = best_by_vid_pid.into_values().collect();
    result.sort_by_key(|c| (c.vendor_id, c.product_id));
    result
}

/// Builds a Windows-compatible HID output report packet (`[report_id, ...payload_padded]`).
/// Windows `WriteFile` on HID devices requires the buffer to be at least `report_len + 1` bytes
/// (1 byte report ID + 64-byte report body = 65 bytes).
pub fn build_output_report_packet(report_id: u8, payload: &[u8], report_len: usize) -> Vec<u8> {
    let body_len = payload.len().max(report_len);
    let mut packet = vec![0u8; body_len + 1];
    packet[0] = report_id;
    packet[1..1 + payload.len()].copy_from_slice(payload);
    packet
}

/// Normalizes a raw buffer returned by `HidDevice::read_timeout`.
/// If `raw[0] == expected_report_id`, strips the leading report ID byte so `data` matches
/// WebHID's `HIDInputReportEvent.data` (which excludes the report ID prefix).
pub fn normalize_input_report(raw: &[u8], expected_report_id: u8) -> (u8, Vec<u8>) {
    if raw.is_empty() {
        return (expected_report_id, Vec::new());
    }
    if raw[0] == expected_report_id && raw.len() > 1 {
        (raw[0], raw[1..].to_vec())
    } else {
        (expected_report_id, raw.to_vec())
    }
}

struct OpenDeviceEntry {
    writer: Arc<Mutex<HidDevice>>,
    stop_flag: Arc<AtomicBool>,
    reader_join: Option<JoinHandle<()>>,
}

impl Drop for OpenDeviceEntry {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::SeqCst);
        if let Some(handle) = self.reader_join.take() {
            let _ = handle.join();
        }
    }
}

/// Manages OS-level USB-HID enumeration, open handles, and background input report streaming.
pub struct HidManager {
    api: HidApi,
    descriptor_cache: HashMap<String, DescriptorSummary>,
    path_by_key: HashMap<String, HidInterfaceCandidate>,
    open_devices: HashMap<String, OpenDeviceEntry>,
}

impl HidManager {
    pub fn new() -> Result<Self, HidError> {
        let api = HidApi::new()?;
        Ok(Self {
            api,
            descriptor_cache: HashMap::new(),
            path_by_key: HashMap::new(),
            open_devices: HashMap::new(),
        })
    }

    pub fn list_devices(&mut self) -> Result<Vec<HidDeviceDto>, HidError> {
        self.api.refresh_devices()?;

        let mut raw_candidates = Vec::new();
        for info in self
            .api
            .device_list()
            .filter(|info| is_supported_moondrop_device(info.vendor_id(), info.product_id()))
        {
            let path_str = info.path().to_string_lossy().into_owned();
            let descriptor = if let Some(cached) = self.descriptor_cache.get(&path_str) {
                cached.clone()
            } else {
                let mut parsed = DescriptorSummary::default();
                if let Ok(dev) = info.open_device(&self.api) {
                    let mut buf = [0u8; 1024];
                    if let Ok(len) = dev.get_report_descriptor(&mut buf) {
                        parsed = parse_report_descriptor(&buf[..len]);
                    }
                }
                self.descriptor_cache.insert(path_str.clone(), parsed.clone());
                parsed
            };

            raw_candidates.push(HidInterfaceCandidate {
                path: path_str,
                vendor_id: info.vendor_id(),
                product_id: info.product_id(),
                product_name: info
                    .product_string()
                    .unwrap_or("MOONDROP USB Audio")
                    .to_string(),
                usage_page: info.usage_page(),
                usage: info.usage(),
                interface_number: info.interface_number(),
                descriptor,
            });
        }

        let selected = select_best_candidates(raw_candidates);
        let mut dtos = Vec::with_capacity(selected.len());
        let mut active_keys = HashMap::new();

        for cand in selected {
            let key = format!("moondrop-{:04x}-{:04x}", cand.vendor_id, cand.product_id);
            let opened = self.open_devices.contains_key(&key);
            let report_id = default_report_id_for_device(cand.vendor_id, cand.product_id);

            dtos.push(HidDeviceDto {
                device_key: key.clone(),
                vendor_id: cand.vendor_id,
                product_id: cand.product_id,
                product_name: cand.product_name.clone(),
                usage_page: cand.usage_page,
                usage: cand.usage,
                interface_number: cand.interface_number,
                report_id,
                opened,
            });
            active_keys.insert(key, cand);
        }

        // Clean up any open handles whose physical USB device was unplugged
        self.open_devices.retain(|k, _| active_keys.contains_key(k));
        self.path_by_key = active_keys;

        Ok(dtos)
    }

    pub fn open_device(
        &mut self,
        device_key: &str,
        webview: WebviewWindow,
    ) -> Result<HidDeviceDto, HidError> {
        if !self.path_by_key.contains_key(device_key) {
            let _ = self.list_devices()?;
        }

        let cand = self
            .path_by_key
            .get(device_key)
            .cloned()
            .ok_or_else(|| HidError::DeviceNotFound(device_key.to_string()))?;

        let report_id = default_report_id_for_device(cand.vendor_id, cand.product_id);

        if self.open_devices.contains_key(device_key) {
            return Ok(HidDeviceDto {
                device_key: device_key.to_string(),
                vendor_id: cand.vendor_id,
                product_id: cand.product_id,
                product_name: cand.product_name,
                usage_page: cand.usage_page,
                usage: cand.usage,
                interface_number: cand.interface_number,
                report_id,
                opened: true,
            });
        }

        let c_path =
            CString::new(cand.path.clone()).map_err(|_| HidError::DeviceNotFound(cand.path.clone()))?;

        // Open a single OS handle protected by a Mutex so writes and reads are serialized
        // and never collide or split input reports across duplicate Windows HID handles.
        let device_handle = self.api.open_path(c_path.as_c_str())?;
        let writer = Arc::new(Mutex::new(device_handle));
        let reader = Arc::clone(&writer);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop_flag);
        let key_owned = device_key.to_string();

        let reader_join = thread::spawn(move || {
            let mut buf = [0u8; 256];
            while !stop_clone.load(Ordering::Relaxed) {
                let read_res = if let Ok(guard) = reader.lock() {
                    guard.read_timeout(&mut buf, 10)
                } else {
                    break;
                };
                match read_res {
                    Ok(n) if n > 0 => {
                        let (actual_report_id, data) =
                            normalize_input_report(&buf[..n], report_id);
                        let payload = InputReportEventPayload {
                            device_key: key_owned.clone(),
                            report_id: actual_report_id,
                            data,
                        };
                        if let Ok(json) = serde_json::to_string(&payload) {
                            let js = format!(
                                "window.__MOONDROP_HID_DISPATCH_INPUT__ && window.__MOONDROP_HID_DISPATCH_INPUT__({});",
                                json
                            );
                            let _ = webview.eval(&js);
                        }
                    }
                    Ok(_) => {}
                    Err(_) => {
                        // Device disconnected or read error; exit reader loop cleanly
                        break;
                    }
                }
                thread::sleep(Duration::from_millis(2));
            }
        });

        self.open_devices.insert(
            device_key.to_string(),
            OpenDeviceEntry {
                writer,
                stop_flag,
                reader_join: Some(reader_join),
            },
        );

        Ok(HidDeviceDto {
            device_key: device_key.to_string(),
            vendor_id: cand.vendor_id,
            product_id: cand.product_id,
            product_name: cand.product_name,
            usage_page: cand.usage_page,
            usage: cand.usage,
            interface_number: cand.interface_number,
            report_id,
            opened: true,
        })
    }

    pub fn close_device(&mut self, device_key: &str) -> Result<(), HidError> {
        if let Some(mut entry) = self.open_devices.remove(device_key) {
            entry.stop_flag.store(true, Ordering::SeqCst);
            if let Some(join) = entry.reader_join.take() {
                let _ = join.join();
            }
        }
        Ok(())
    }

    pub fn send_report(
        &self,
        device_key: &str,
        report_id: u8,
        data: &[u8],
        window: &WebviewWindow,
    ) -> Result<(), HidError> {
        let entry = self
            .open_devices
            .get(device_key)
            .ok_or_else(|| HidError::DeviceNotOpen(device_key.to_string()))?;

        let packet = build_output_report_packet(report_id, data, STANDARD_REPORT_LEN);
        let guard = entry
            .writer
            .lock()
            .map_err(|_| HidError::StateLockPoisoned)?;

        // Read queries in Moondrop's protocol start with OP_READ (0x80) or firmware upgrade
        // commands on Report ID 1. Perform an immediate read while holding the device lock,
        // retrying up to 3 times if the MCU was busy and dropped the packet.
        let expects_immediate_reply =
            data.first().copied() == Some(0x80) || report_id == 1;

        if expects_immediate_reply {
            let mut rbuf = [0u8; 256];
            let expected_read_rid = if report_id == 1 { 2 } else { report_id };
            for attempt in 0..3 {
                guard.write(&packet)?;
                match guard.read_timeout(&mut rbuf, 120) {
                    Ok(n) if n > 0 => {
                        let (actual_report_id, reply_data) =
                            normalize_input_report(&rbuf[..n], expected_read_rid);
                        let payload = InputReportEventPayload {
                            device_key: device_key.to_string(),
                            report_id: actual_report_id,
                            data: reply_data,
                        };
                        if let Ok(json) = serde_json::to_string(&payload) {
                            let js = format!(
                                "window.__MOONDROP_HID_DISPATCH_INPUT__ && window.__MOONDROP_HID_DISPATCH_INPUT__({});",
                                json
                            );
                            let _ = window.eval(&js);
                        }
                        break;
                    }
                    Ok(_) if attempt < 2 => {
                        thread::sleep(Duration::from_millis(15));
                    }
                    _ => break,
                }
            }
        } else {
            guard.write(&packet)?;
        }

        Ok(())
    }

    pub fn send_feature_report(
        &self,
        device_key: &str,
        report_id: u8,
        data: &[u8],
    ) -> Result<(), HidError> {
        let entry = self
            .open_devices
            .get(device_key)
            .ok_or_else(|| HidError::DeviceNotOpen(device_key.to_string()))?;

        let packet = build_output_report_packet(report_id, data, data.len());
        let guard = entry
            .writer
            .lock()
            .map_err(|_| HidError::StateLockPoisoned)?;
        guard.send_feature_report(&packet)?;
        Ok(())
    }

    pub fn receive_feature_report(
        &self,
        device_key: &str,
        report_id: u8,
    ) -> Result<Vec<u8>, HidError> {
        let entry = self
            .open_devices
            .get(device_key)
            .ok_or_else(|| HidError::DeviceNotOpen(device_key.to_string()))?;

        let mut buf = vec![0u8; STANDARD_REPORT_LEN + 1];
        buf[0] = report_id;
        let guard = entry
            .writer
            .lock()
            .map_err(|_| HidError::StateLockPoisoned)?;
        let n = guard.get_feature_report(&mut buf)?;
        let (_, payload) = normalize_input_report(&buf[..n], report_id);
        Ok(payload)
    }

    /// Ensures the first available Moondrop device is opened and returns its `(device_key, candidate)`.
    fn ensure_primary_device(
        &mut self,
        app: &AppHandle,
    ) -> Result<Option<(String, HidInterfaceCandidate)>, HidError> {
        let devices = self.list_devices()?;
        let Some(first) = devices.first() else {
            return Ok(None);
        };
        let key = first.device_key.clone();
        if !self.open_devices.contains_key(&key) {
            if let Some(main_win) = app.get_webview_window("main") {
                let _ = self.open_device(&key, main_win)?;
            }
        }
        Ok(self.path_by_key.get(&key).cloned().map(|c| (key, c)))
    }

    /// Performs a direct synchronous HID command/query on an open device handle.
    fn query_raw(&self, device_key: &str, report_id: u8, cmd: &[u8]) -> Result<Vec<u8>, HidError> {
        let entry = self
            .open_devices
            .get(device_key)
            .ok_or_else(|| HidError::DeviceNotOpen(device_key.to_string()))?;
        let packet = build_output_report_packet(report_id, cmd, STANDARD_REPORT_LEN);
        let guard = entry
            .writer
            .lock()
            .map_err(|_| HidError::StateLockPoisoned)?;

        let mut rbuf = [0u8; 256];
        for attempt in 0..3 {
            guard.write(&packet)?;
            match guard.read_timeout(&mut rbuf, 120) {
                Ok(n) if n > 0 => {
                    let (_, data) = normalize_input_report(&rbuf[..n], report_id);
                    return Ok(data);
                }
                Ok(_) if attempt < 2 => {
                    thread::sleep(Duration::from_millis(15));
                }
                _ => break,
            }
        }
        Ok(Vec::new())
    }

    /// Performs a direct synchronous HID write command on an open device handle.
    fn write_raw(&self, device_key: &str, report_id: u8, cmd: &[u8]) -> Result<(), HidError> {
        let entry = self
            .open_devices
            .get(device_key)
            .ok_or_else(|| HidError::DeviceNotOpen(device_key.to_string()))?;
        let packet = build_output_report_packet(report_id, cmd, STANDARD_REPORT_LEN);
        let guard = entry
            .writer
            .lock()
            .map_err(|_| HidError::StateLockPoisoned)?;
        guard.write(&packet)?;
        Ok(())
    }

    /// Queries the live status of the primary connected Moondrop device for the system tray toolbar.
    pub fn tray_get_status(&mut self, app: &AppHandle) -> Result<TrayStatusDto, HidError> {
        let Some((key, cand)) = self.ensure_primary_device(app)? else {
            return Ok(TrayStatusDto {
                connected: false,
                device_key: None,
                product_name: String::from("No Device Connected"),
                vendor_id: 0,
                product_id: 0,
                firmware_version: String::from("—"),
                active_eq_index: 0,
                global_gain_db: 0.0,
            });
        };

        let rid = default_report_id_for_device(cand.vendor_id, cand.product_id);

        // 1. Query FIRMWARE_VERSION ([0x80, 12, 0])
        let fw_reply = self.query_raw(&key, rid, &[0x80, 12, 0]).unwrap_or_default();
        let firmware_version = decode_firmware_string(&fw_reply);

        // 2. Query EQ_ACTIVE ([0x80, 15, 0])
        let eq_reply = self.query_raw(&key, rid, &[0x80, 15, 0]).unwrap_or_default();
        let active_eq_index = eq_reply.get(3).copied().unwrap_or(0);

        // 3. Query DAC_OFFSET / Global Gain ([0x80, 3, 0])
        let gain_reply = self.query_raw(&key, rid, &[0x80, 3, 0]).unwrap_or_default();
        let global_gain_db = if gain_reply.len() >= 5 {
            let raw_i16 = i16::from_le_bytes([gain_reply[3], gain_reply[4]]);
            decode_fixed_gain_q8(raw_i16)
        } else {
            0.0
        };

        Ok(TrayStatusDto {
            connected: true,
            device_key: Some(key),
            product_name: cand.product_name,
            vendor_id: cand.vendor_id,
            product_id: cand.product_id,
            firmware_version,
            active_eq_index,
            global_gain_db,
        })
    }

    /// Switches the active hardware EQ preset index (`0..=4` on Moondrop Rays) and persists to flash.
    pub fn tray_set_eq_preset(
        &mut self,
        app: &AppHandle,
        preset_index: u8,
    ) -> Result<TrayStatusDto, HidError> {
        let Some((key, cand)) = self.ensure_primary_device(app)? else {
            return Err(HidError::DeviceNotFound(String::from("No Moondrop device connected")));
        };
        let rid = default_report_id_for_device(cand.vendor_id, cand.product_id);

        // OP_WRITE (1), EQ_ACTIVE (15), len (1), preset_index
        self.write_raw(&key, rid, &[0x01, 15, 1, preset_index])?;
        thread::sleep(Duration::from_millis(150));
        // OP_WRITE (1), EQ_SAVE_FLASH (1), len (0)
        self.write_raw(&key, rid, &[0x01, 1, 0])?;
        thread::sleep(Duration::from_millis(180));

        self.tray_get_status(app)
    }

    /// Sets the hardware DAC global gain (`-18.0` to `0.0` dB) and persists to flash.
    pub fn tray_set_global_gain(
        &mut self,
        app: &AppHandle,
        gain_db: f32,
    ) -> Result<TrayStatusDto, HidError> {
        let Some((key, cand)) = self.ensure_primary_device(app)? else {
            return Err(HidError::DeviceNotFound(String::from("No Moondrop device connected")));
        };
        let rid = default_report_id_for_device(cand.vendor_id, cand.product_id);

        let clamped = gain_db.clamp(-18.0, 0.0);
        let raw = encode_fixed_gain_q8(clamped).to_le_bytes();
        // OP_WRITE (1), DAC_OFFSET (3), len (2), i16_le
        self.write_raw(&key, rid, &[0x01, 3, 2, raw[0], raw[1]])?;
        thread::sleep(Duration::from_millis(50));
        // OP_WRITE (1), OFFSET_SAVE_FLASH (4), len (0)
        self.write_raw(&key, rid, &[0x01, 4, 0])?;
        thread::sleep(Duration::from_millis(150));

        self.tray_get_status(app)
    }
}

/// Spawns a lightweight background thread that notifies the webview when USB devices change.
pub fn spawn_hotplug_notifier(_app: AppHandle) {
    // Hotplug polling is driven cleanly by the bridge timer calling `hid_list_devices`
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_supported_moondrop_device_recognizes_rays_and_usb20() {
        assert!(is_supported_moondrop_device(0x35d8, 0x011b)); // Moondrop Rays
        assert!(is_supported_moondrop_device(0x35d8, 0x011e)); // AG Rays
        assert!(is_supported_moondrop_device(0x0666, 0x0888)); // USB20 bootloader
        assert!(!is_supported_moondrop_device(0x046d, 0xc52b)); // Unrelated mouse
    }

    #[test]
    fn interface_priority_score_prefers_vendor_defined_usage_page() {
        let vendor_score = interface_priority_score(0xff00, 0x0001, 3);
        let consumer_score = interface_priority_score(0x000c, 0x0001, 2);
        let keyboard_score = interface_priority_score(0x0001, 0x0006, 1);

        assert!(vendor_score > consumer_score);
        assert!(vendor_score > keyboard_score);
    }

    #[test]
    fn select_best_candidates_picks_rays_mi03_with_report_75_over_mi04_consumer_keys() {
        // Real HID descriptors captured from MOONDROP Rays (VID 0x35d8, PID 0x011b):
        // Both MI_03 and MI_04 have usage_page=0x000c, usage=0x0001!
        // MI_04 is input-only Report ID 0x03 (media volume buttons) -> WriteFile fails with 0x00000001
        let mi04_desc = [
            0x05, 0x0c, 0x09, 0x01, 0xa1, 0x01, 0x85, 0x03, 0x09, 0xe9, 0x09, 0xea, 0x09, 0xcd,
            0x09, 0xcf, 0x09, 0xb6, 0x09, 0xb5, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x06,
            0x81, 0x02, 0x75, 0x02, 0x95, 0x01, 0x81, 0x03, 0xc0,
        ];
        // MI_03 has Input (0x81) and Output (0x91) for Report ID 0x4b (75) and 0x54 (84)
        let mi03_desc = [
            0x05, 0x0c, 0x09, 0x01, 0xa1, 0x01, 0x85, 0x4b, 0x09, 0x01, 0x15, 0x00, 0x25, 0x00,
            0x75, 0x08, 0x95, 0x3f, 0x81, 0x03, 0x85, 0x54, 0x09, 0x03, 0x15, 0x00, 0x25, 0x00,
            0x75, 0x08, 0x95, 0x3f, 0x81, 0x03, 0x85, 0x4b, 0x09, 0x02, 0x15, 0x00, 0x25, 0x00,
            0x75, 0x08, 0x95, 0x3f, 0x91, 0x02, 0x85, 0x54, 0x09, 0x04, 0x15, 0x00, 0x25, 0x00,
            0x75, 0x08, 0x95, 0x3f, 0x91, 0x02, 0xc0,
        ];

        let parsed_mi04 = parse_report_descriptor(&mi04_desc);
        assert_eq!(parsed_mi04.input_report_ids, vec![3]);
        assert!(parsed_mi04.output_report_ids.is_empty());

        let parsed_mi03 = parse_report_descriptor(&mi03_desc);
        assert_eq!(parsed_mi03.input_report_ids, vec![75, 84]);
        assert_eq!(parsed_mi03.output_report_ids, vec![75, 84]);
        assert!(parsed_mi03.supports_report_pair(75));

        let candidates = vec![
            HidInterfaceCandidate {
                path: "\\\\?\\HID#VID_35D8&PID_011B&MI_04".to_string(),
                vendor_id: 0x35d8,
                product_id: 0x011b,
                product_name: "MOONDROP Rays".to_string(),
                usage_page: 0x000c,
                usage: 0x0001,
                interface_number: 4,
                descriptor: parsed_mi04,
            },
            HidInterfaceCandidate {
                path: "\\\\?\\HID#VID_35D8&PID_011B&MI_03".to_string(),
                vendor_id: 0x35d8,
                product_id: 0x011b,
                product_name: "MOONDROP Rays".to_string(),
                usage_page: 0x000c,
                usage: 0x0001,
                interface_number: 3,
                descriptor: parsed_mi03,
            },
        ];

        let selected = select_best_candidates(candidates);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].interface_number, 3);
        assert!(selected[0].path.contains("MI_03"));
    }

    #[test]
    fn build_output_report_packet_pads_short_payload_to_64_bytes() {
        let payload = [0x80, 0x0c, 0x00]; // OP_READ, FIRMWARE_VERSION, len 0
        let packet = build_output_report_packet(75, &payload, STANDARD_REPORT_LEN);

        assert_eq!(packet.len(), 64);
        assert_eq!(packet[0], 75);
        assert_eq!(&packet[1..4], &[0x80, 0x0c, 0x00]);
        assert!(packet[4..].iter().all(|&b| b == 0));
    }

    #[test]
    fn normalize_input_report_strips_leading_report_id_when_present() {
        let raw_with_id = [75u8, 0x80, 0x0c, 0x05, b'1', b'.', b'0', b'.', b'0'];
        let (rid1, data1) = normalize_input_report(&raw_with_id, 75);
        assert_eq!(rid1, 75);
        assert_eq!(data1, vec![0x80, 0x0c, 0x05, b'1', b'.', b'0', b'.', b'0']);

        let raw_without_id = [0x80u8, 0x0c, 0x05, b'1', b'.', b'0', b'.', b'0'];
        let (rid2, data2) = normalize_input_report(&raw_without_id, 75);
        assert_eq!(rid2, 75);
        assert_eq!(data2, vec![0x80, 0x0c, 0x05, b'1', b'.', b'0', b'.', b'0']);
    }

    #[test]
    fn encode_and_decode_fixed_gain_q8_roundtrips_cleanly() {
        assert_eq!(encode_fixed_gain_q8(0.0), 0);
        assert_eq!(encode_fixed_gain_q8(-3.5), -896);
        assert_eq!(decode_fixed_gain_q8(-896), -3.5);
        assert_eq!(decode_fixed_gain_q8(0), 0.0);
    }

    #[test]
    fn decode_firmware_string_extracts_clean_ascii_version() {
        let pkt = [0x80, 0x0c, 0x00, b'0', b'.', b'6', 0x00, 0x00];
        assert_eq!(decode_firmware_string(&pkt), "0.6");
    }

    #[test]
    #[ignore]
    fn test_single_handle_rays_sequence() {
        let mut mgr = HidManager::new().unwrap();
        let devices = mgr.list_devices().unwrap();
        assert!(!devices.is_empty(), "No Moondrop device found");
        let d = &devices[0];
        let cand = mgr.path_by_key.get(&d.device_key).unwrap();
        let c_path = CString::new(cand.path.clone()).unwrap();
        let handle = mgr.api.open_path(c_path.as_c_str()).unwrap();

        let queries: Vec<(&str, Vec<u8>)> = vec![
            ("FIRMWARE_VERSION", vec![0x80, 12, 0]),
            ("EQ_ACTIVE", vec![0x80, 15, 0]),
            ("EQ_UPDATE_0", vec![0x80, 9, 2, 0, 0]),
            ("DAC_OFFSET", vec![0x80, 3, 0]),
            ("PRE_GAIN", vec![0x80, 35, 0]),
        ];

        let mut total_sent = 0;
        let mut total_received = 0;

        for _ in 0..10 {
            for (name, cmd) in &queries {
                total_sent += 1;
                let pkt = build_output_report_packet(d.report_id, cmd, STANDARD_REPORT_LEN);
                let mut rbuf = [0u8; 256];
                let mut got = false;
                for attempt in 0..3 {
                    let _w = handle.write(&pkt).unwrap();
                    let r = handle.read_timeout(&mut rbuf, 120).unwrap();
                    if r > 0 {
                        got = true;
                        if attempt > 0 {
                            println!("Recovered {name} on retry #{attempt}");
                        }
                        break;
                    }
                }
                if got {
                    total_received += 1;
                } else {
                    println!("MISSED response for {name} after 3 attempts!");
                }
            }
        }
        println!("Single handle reliability: {total_received}/{total_sent}");
        assert_eq!(total_received, total_sent);
    }
}
