(function () {
  "use strict";

  const MOONDROP_VID = 0x35d8; // 13784
  const UPGRADE_VID = 0x0666; // 1638
  const UPGRADE_PID = 0x0888; // 2184

  function hasTauriIpc() {
    return Boolean(
      typeof window !== "undefined" &&
        window.__TAURI_INTERNALS__ &&
        typeof window.__TAURI_INTERNALS__.invoke === "function"
    );
  }

  async function tauriInvoke(cmd, args = {}) {
    if (!hasTauriIpc()) {
      throw new Error(`Tauri IPC unavailable for command: ${cmd}`);
    }
    return window.__TAURI_INTERNALS__.invoke(cmd, args);
  }

  function toUint8Array(data) {
    if (data instanceof Uint8Array) {
      return data;
    }
    if (ArrayBuffer.isView(data)) {
      return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
    }
    if (data instanceof ArrayBuffer) {
      return new Uint8Array(data);
    }
    if (Array.isArray(data)) {
      return new Uint8Array(data);
    }
    return new Uint8Array(0);
  }

  function buildCollectionsForDevice(dto) {
    const usagePage = dto.usagePage || 0xff00;
    const usage = dto.usage || 0x0001;
    if (dto.vendorId === UPGRADE_VID && dto.productId === UPGRADE_PID) {
      return [
        {
          usagePage,
          usage,
          type: 0,
          children: [],
          inputReports: [{ reportId: 2, items: [] }],
          outputReports: [{ reportId: 1, items: [] }],
          featureReports: [
            { reportId: 1, items: [] },
            { reportId: 2, items: [] },
          ],
        },
      ];
    }

    const reportId = dto.reportId || 75;
    return [
      {
        usagePage,
        usage,
        type: 0,
        children: [],
        inputReports: [{ reportId, items: [] }],
        outputReports: [{ reportId, items: [] }],
        featureReports: [{ reportId, items: [] }],
      },
    ];
  }

  class NativeHIDInputReportEvent extends Event {
    constructor(type, init) {
      super(type);
      this.device = init.device;
      this.reportId = init.reportId;
      this.data = init.data;
    }
  }

  class NativeHIDConnectionEvent extends Event {
    constructor(type, init) {
      super(type);
      this.device = init.device;
    }
  }

  class NativeHIDDevice extends EventTarget {
    constructor(dto) {
      super();
      this._oninputreport = null;
      this._updateFromDto(dto);
    }

    _updateFromDto(dto) {
      this.deviceKey = dto.deviceKey;
      this.vendorId = dto.vendorId;
      this.productId = dto.productId;
      this.productName = dto.productName || "MOONDROP USB Audio";
      this.opened = Boolean(dto.opened);
      this.collections = buildCollectionsForDevice(dto);
    }

    get oninputreport() {
      return this._oninputreport;
    }

    set oninputreport(fn) {
      if (this._oninputreport) {
        this.removeEventListener("inputreport", this._oninputreport);
      }
      this._oninputreport = typeof fn === "function" ? fn : null;
      if (this._oninputreport) {
        this.addEventListener("inputreport", this._oninputreport);
      }
    }

    async open() {
      if (this.opened) return;
      await tauriInvoke("hid_open_device", { deviceKey: this.deviceKey });
      this.opened = true;
    }

    async close() {
      if (!this.opened) return;
      try {
        await tauriInvoke("hid_close_device", { deviceKey: this.deviceKey });
      } finally {
        this.opened = false;
      }
    }

    async forget() {
      await this.close();
    }

    async sendReport(reportId, data) {
      if (!this.opened) {
        await this.open();
      }
      const bytes = Array.from(toUint8Array(data));
      await tauriInvoke("hid_send_report", {
        deviceKey: this.deviceKey,
        reportId: Number(reportId) & 0xff,
        data: bytes,
      });
    }

    async sendFeatureReport(reportId, data) {
      if (!this.opened) {
        await this.open();
      }
      const bytes = Array.from(toUint8Array(data));
      await tauriInvoke("hid_send_feature_report", {
        deviceKey: this.deviceKey,
        reportId: Number(reportId) & 0xff,
        data: bytes,
      });
    }

    async receiveFeatureReport(reportId) {
      if (!this.opened) {
        await this.open();
      }
      const bytes = await tauriInvoke("hid_receive_feature_report", {
        deviceKey: this.deviceKey,
        reportId: Number(reportId) & 0xff,
      });
      const u8 = new Uint8Array(bytes || []);
      return new DataView(u8.buffer, u8.byteOffset, u8.byteLength);
    }

    _dispatchInputReport(reportId, byteArray) {
      const u8 = toUint8Array(byteArray);
      const view = new DataView(u8.buffer, u8.byteOffset, u8.byteLength);
      const evt = new NativeHIDInputReportEvent("inputreport", {
        device: this,
        reportId: Number(reportId) & 0xff,
        data: view,
      });
      this.dispatchEvent(evt);
    }
  }

  class NativeHID extends EventTarget {
    constructor() {
      super();
      this._devices = new Map();
      this._onconnect = null;
      this._ondisconnect = null;
      this._pollingTimer = null;
      this._initEvents();
    }

    get onconnect() {
      return this._onconnect;
    }

    set onconnect(fn) {
      if (this._onconnect) {
        this.removeEventListener("connect", this._onconnect);
      }
      this._onconnect = typeof fn === "function" ? fn : null;
      if (this._onconnect) {
        this.addEventListener("connect", this._onconnect);
      }
    }

    get ondisconnect() {
      return this._ondisconnect;
    }

    set ondisconnect(fn) {
      if (this._ondisconnect) {
        this.removeEventListener("disconnect", this._ondisconnect);
      }
      this._ondisconnect = typeof fn === "function" ? fn : null;
      if (this._ondisconnect) {
        this.addEventListener("disconnect", this._ondisconnect);
      }
    }

    _upsertDevice(dto) {
      let dev = this._devices.get(dto.deviceKey);
      if (!dev) {
        dev = new NativeHIDDevice(dto);
        this._devices.set(dto.deviceKey, dev);
      } else {
        const wasOpened = dev.opened;
        dev._updateFromDto(dto);
        if (wasOpened) dev.opened = true;
      }
      return dev;
    }

    async _syncDevices(emitHotplug = false) {
      if (!hasTauriIpc()) {
        return [];
      }
      const list = (await tauriInvoke("hid_list_devices")) || [];
      const seenKeys = new Set();

      for (const dto of list) {
        seenKeys.add(dto.deviceKey);
        const existed = this._devices.has(dto.deviceKey);
        const dev = this._upsertDevice(dto);
        if (emitHotplug && !existed) {
          this.dispatchEvent(
            new NativeHIDConnectionEvent("connect", { device: dev })
          );
        }
      }

      for (const [key, dev] of Array.from(this._devices.entries())) {
        if (!seenKeys.has(key)) {
          dev.opened = false;
          this._devices.delete(key);
          if (emitHotplug) {
            this.dispatchEvent(
              new NativeHIDConnectionEvent("disconnect", { device: dev })
            );
          }
        }
      }

      return Array.from(this._devices.values());
    }

    async getDevices() {
      return this._syncDevices(false);
    }

    async requestDevice(options = {}) {
      const allDevices = await this._syncDevices(false);
      const filters = Array.isArray(options?.filters) ? options.filters : [];
      if (filters.length === 0) {
        return allDevices;
      }

      return allDevices.filter((device) =>
        filters.some((f) => {
          if (f.vendorId !== undefined && f.vendorId !== device.vendorId) {
            return false;
          }
          if (f.productId !== undefined && f.productId !== device.productId) {
            return false;
          }
          if (f.usagePage !== undefined) {
            const matchUsagePage = device.collections.some(
              (c) => c.usagePage === f.usagePage
            );
            if (!matchUsagePage) return false;
          }
          if (f.usage !== undefined) {
            const matchUsage = device.collections.some(
              (c) => c.usage === f.usage
            );
            if (!matchUsage) return false;
          }
          return true;
        })
      );
    }

    _handleBackendInputReport(payload) {
      if (!payload || !payload.deviceKey) return;
      const dev = this._devices.get(payload.deviceKey);
      if (dev) {
        dev._dispatchInputReport(payload.reportId, payload.data || []);
      }
    }

    _initEvents() {
      if (typeof window === "undefined") return;
      window.__MOONDROP_HID_DISPATCH_INPUT__ = (payload) => {
        this._handleBackendInputReport(payload);
      };
      window.__MOONDROP_HID_TRIGGER_HOTPLUG__ = () => {
        this._syncDevices(true).catch(() => {});
      };

      // Poll hotplug every 1.2s when running inside Tauri
      if (hasTauriIpc() && typeof setTimeout === "function") {
        const poll = async () => {
          try {
            await this._syncDevices(true);
          } catch (_) {
            // ignore transient poll errors
          }
          this._pollingTimer = setTimeout(poll, 1200);
          if (this._pollingTimer && typeof this._pollingTimer.unref === "function") {
            this._pollingTimer.unref();
          }
        };
        this._pollingTimer = setTimeout(poll, 1200);
        if (this._pollingTimer && typeof this._pollingTimer.unref === "function") {
          this._pollingTimer.unref();
        }
      }
    }
  }

  const nativeHidInstance = new NativeHID();

  // Install on navigator.hid if inside Tauri or if WebHID is missing
  if (typeof navigator !== "undefined" && (hasTauriIpc() || !navigator.hid)) {
    try {
      Object.defineProperty(navigator, "hid", {
        value: nativeHidInstance,
        configurable: true,
        writable: false,
      });
    } catch (_) {
      navigator.hid = nativeHidInstance;
    }
  }

  if (typeof window !== "undefined") {
    window.__MOONDROP_HID_BRIDGE__ = {
      hid: nativeHidInstance,
      NativeHID,
      NativeHIDDevice,
      MOONDROP_VID,
      UPGRADE_VID,
      UPGRADE_PID,
    };
  }
})();
