import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { prepareFrontend } from "../scripts/prepare-frontend.mjs";

test("prepareFrontend strips Cloudflare Rocket Loader, copies WASM/assets, and injects tauri-hid-bridge.js", async () => {
  const result = await prepareFrontend();
  assert.equal(result.assetCount, 90, `Expected 90 assets in dump/assets, got ${result.assetCount}`);

  const indexHtml = fs.readFileSync(path.resolve("dist/index.html"), "utf8");
  assert.equal(indexHtml.includes("rocket-loader"), false, "Rocket Loader should be stripped");
  assert.equal(indexHtml.includes("-module"), false, "Cloudflare rewritten module type should be stripped");
  assert.equal(indexHtml.includes("-text/javascript"), false, "Cloudflare rewritten inline script type should be stripped");
  assert.equal(
    indexHtml.includes('<script src="/tauri-hid-bridge.js"></script>'),
    true,
    "tauri-hid-bridge.js should be injected before main bundle"
  );
  assert.equal(
    indexHtml.includes('<script type="module" crossorigin src="/assets/index-CIZ2xpvU.js"></script>'),
    true,
    "Main module script should be restored"
  );
  assert.equal(
    fs.existsSync(path.resolve("dist/tray.html")),
    true,
    "dist/tray.html should be copied for system tray popover"
  );
  const connectBundle = fs.readFileSync(path.resolve("dist/assets/index-BsKbV-8h.js"), "utf8");
  assert.equal(
    connectBundle.includes("pages.connect.please-use-this-feature-in-browsers"),
    false,
    "Browser-only Chrome/Edge tip should be removed from ConnectionTipsCard"
  );

  for (const wasm of [
    "tuning_response-DMc1_tCy.wasm",
    "tuning_autoeq-BCH8TLkw.wasm",
    "tuning_calibration-CDpvrBru.wasm",
  ]) {
    assert.equal(
      fs.existsSync(path.resolve("dist/assets", wasm)),
      true,
      `Missing WASM file ${wasm} in dist/assets`
    );
  }
});

test("tauri-hid-bridge.js synthesizes HIDDevice collections matching Moondrop Hub selectBestDevice(ig)", async () => {
  const bridgeCode = fs.readFileSync(path.resolve("dist/tauri-hid-bridge.js"), "utf8");
  const sentReports = [];
  const context = {
    window: null,
    navigator: {},
    EventTarget,
    Event,
    CustomEvent,
    DataView,
    Uint8Array,
    ArrayBuffer,
    console,
    setTimeout,
    clearTimeout,
  };
  context.window = context;
  context.globalThis = context;
  context.__TAURI_INTERNALS__ = {
    invoke: async (cmd, args) => {
      if (cmd === "hid_list_devices") {
        return [
          {
            deviceKey: "moondrop-rays-0x35d8-0x011b",
            vendorId: 0x35d8,
            productId: 0x011b,
            productName: "MOONDROP Rays",
            usagePage: 0xff00,
            usage: 0x0001,
            interfaceNumber: 3,
            reportId: 75,
            opened: false,
          },
          {
            deviceKey: "usb20-upgrade-0x0666-0x0888",
            vendorId: 0x0666,
            productId: 0x0888,
            productName: "USB20",
            usagePage: 0xff00,
            usage: 0x0001,
            interfaceNumber: 0,
            reportId: 1,
            opened: false,
          },
        ];
      }
      if (cmd === "hid_open_device") {
        return { opened: true };
      }
      if (cmd === "hid_send_report") {
        sentReports.push({ type: "output", ...args });
        return null;
      }
      if (cmd === "hid_send_feature_report") {
        sentReports.push({ type: "feature", ...args });
        return null;
      }
      return null;
    },
  };

  vm.createContext(context);
  vm.runInContext(bridgeCode, context);

  assert.ok(context.navigator.hid, "navigator.hid should be defined");
  const raysDevices = await context.navigator.hid.requestDevice({
    filters: [{ vendorId: 13784 }],
  });
  assert.equal(raysDevices.length, 1);
  const rays = raysDevices[0];
  assert.equal(rays.vendorId, 13784);
  assert.equal(rays.productId, 283);

  const hasReport75 = rays.collections.some(
    (c) =>
      c.inputReports?.some((r) => r.reportId === 75) &&
      c.outputReports?.some((r) => r.reportId === 75)
  );
  assert.equal(hasReport75, true, "Rays HIDDevice must expose reportId 75 in inputReports and outputReports");

  await rays.open();
  assert.equal(rays.opened, true);

  await rays.sendReport(75, new Uint8Array([0x80, 0x0c, 0x00]));
  assert.equal(sentReports.length, 1);
  assert.equal(sentReports[0].reportId, 75);
  assert.deepEqual(Array.from(sentReports[0].data), [0x80, 0x0c, 0x00]);
});
