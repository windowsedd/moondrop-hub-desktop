import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";

function crc32(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i++) {
    c ^= buf[i];
    for (let k = 0; k < 8; k++) {
      c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    }
  }
  return (c ^ 0xffffffff) >>> 0;
}

function pngChunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length, 0);
  const typeBuf = Buffer.from(type, "ascii");
  const crcBuf = Buffer.alloc(4);
  crcBuf.writeUInt32BE(crc32(Buffer.concat([typeBuf, data])), 0);
  return Buffer.concat([len, typeBuf, data, crcBuf]);
}

export function createMoondropIconPng(size = 256) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8; // bit depth 8
  ihdr[9] = 6; // RGBA
  ihdr[10] = 0;
  ihdr[11] = 0;
  ihdr[12] = 0;

  const raw = Buffer.alloc(size * (size * 4 + 1));
  const cx = size / 2;
  const cy = size / 2;
  const rOuter = size * 0.44;
  const rInner = size * 0.30;

  for (let y = 0; y < size; y++) {
    const rowOffset = y * (size * 4 + 1);
    raw[rowOffset] = 0; // filter type 0
    for (let x = 0; x < size; x++) {
      const pxOffset = rowOffset + 1 + x * 4;
      const dx = x - cx;
      const dy = y - cy;
      const dist = Math.hypot(dx, dy);

      if (dist > rOuter + 1.5) {
        raw[pxOffset] = 0;
        raw[pxOffset + 1] = 0;
        raw[pxOffset + 2] = 0;
        raw[pxOffset + 3] = 0;
        continue;
      }

      // Smooth anti-aliased outer badge (#121318 dark slate with gold/cyan Moondrop crescent)
      const alpha = dist > rOuter ? Math.max(0, Math.min(255, Math.round((rOuter + 1.5 - dist) * 170))) : 255;

      // Crescent moon geometry
      const moonOffsetDist = Math.hypot(dx - size * 0.08, dy + size * 0.06);
      const isCrescent = dist <= rInner && moonOffsetDist > rInner * 0.72;

      if (isCrescent) {
        raw[pxOffset] = 245;
        raw[pxOffset + 1] = 158;
        raw[pxOffset + 2] = 11;
        raw[pxOffset + 3] = alpha;
      } else {
        raw[pxOffset] = 20;
        raw[pxOffset + 1] = 22;
        raw[pxOffset + 2] = 29;
        raw[pxOffset + 3] = alpha;
      }
    }
  }

  const idat = zlib.deflateSync(raw, { level: 9 });
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    pngChunk("IHDR", ihdr),
    pngChunk("IDAT", idat),
    pngChunk("IEND", Buffer.alloc(0)),
  ]);
}

export function createIcoFromPng(pngBuf, width = 256, height = 256) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2); // type 1 = ICO
  header.writeUInt16LE(1, 4); // 1 image

  const dir = Buffer.alloc(16);
  dir[0] = width >= 256 ? 0 : width;
  dir[1] = height >= 256 ? 0 : height;
  dir[2] = 0;
  dir[3] = 0;
  dir.writeUInt16LE(1, 4); // color planes
  dir.writeUInt16LE(32, 6); // bits per pixel
  dir.writeUInt32LE(pngBuf.length, 8);
  dir.writeUInt32LE(6 + 16, 12);

  return Buffer.concat([header, dir, pngBuf]);
}

export async function prepareFrontend(rootDir = process.cwd()) {
  const dumpDir = path.resolve(rootDir, "dump");
  const distDir = path.resolve(rootDir, "dist");
  const distAssetsDir = path.resolve(distDir, "assets");
  const bridgeSrc = path.resolve(rootDir, "src-bridge/tauri-hid-bridge.js");
  const trayHtmlSrc = path.resolve(rootDir, "src-bridge/tray.html");
  const iconsDir = path.resolve(rootDir, "src-tauri/icons");

  fs.mkdirSync(distAssetsDir, { recursive: true });
  fs.mkdirSync(iconsDir, { recursive: true });

  // 1. Copy top-level static files
  for (const file of ["favicon.ico", "moondrop.svg", "moondrop-dark.svg"]) {
    const src = path.resolve(dumpDir, file);
    if (fs.existsSync(src)) {
      fs.copyFileSync(src, path.resolve(distDir, file));
    }
  }

  // 2. Copy all assets (JS, CSS, PNG, WASM)
  const dumpAssetsDir = path.resolve(dumpDir, "assets");
  const assetFiles = fs.readdirSync(dumpAssetsDir);
  for (const file of assetFiles) {
    const srcPath = path.resolve(dumpAssetsDir, file);
    const destPath = path.resolve(distAssetsDir, file);
    if (file === "index-BsKbV-8h.js") {
      // Remove the browser-only tip ("请在 Chrome 或 Edge 浏览器中使用此功能。") from ConnectionTipsCard in desktop app
      const jsContent = fs
        .readFileSync(srcPath, "utf8")
        .replace(
          /,v\("li",db,\[o\[2\]\|\|\(o\[2\]=v\("i",\{i:"lucide-info",class:"size-4 shrink-0 text-muted-foreground"\},null,-1\)\),_\(a,\{keypath:"pages\.connect\.please-use-this-feature-in-browsers",tag:"span",class:"text-sm"\},\{chrome:A\(\(\)=>\[v\("a",\{href:u\(lf\),target:"_blank",class:me\(u\(Es\)\)\},"Chrome",10,fb\)\]\),edge:A\(\(\)=>\[v\("a",\{href:u\(uf\),target:"_blank",class:me\(u\(Es\)\)\},"Edge",10,pb\)\]\),_:1\}\)\]\)/,
          ""
        );
      fs.writeFileSync(destPath, jsContent, "utf8");
    } else {
      fs.copyFileSync(srcPath, destPath);
    }
  }

  // 3. Copy tauri-hid-bridge.js and tray.html
  fs.copyFileSync(bridgeSrc, path.resolve(distDir, "tauri-hid-bridge.js"));
  if (fs.existsSync(trayHtmlSrc)) {
    fs.copyFileSync(trayHtmlSrc, path.resolve(distDir, "tray.html"));
  }

  // 4. Clean index.raw.html -> dist/index.html
  const rawHtml = fs.readFileSync(path.resolve(dumpDir, "index.raw.html"), "utf8");
  let cleanHtml = rawHtml
    // Restore standard module type stripped/rewritten by Cloudflare Rocket Loader
    .replace(
      /<script\s+type="[^"]*module"\s+crossorigin\s+src="\/assets\/(index-[^"]+\.js)"><\/script>/g,
      '<script src="/tauri-hid-bridge.js"></script>\n    <script type="module" crossorigin src="/assets/$1"></script>'
    )
    // Restore inline scripts rewritten to type="<cf-hash>-text/javascript"
    .replace(/<script\s+type="[^"]*-text\/javascript">/g, "<script>")
    // Remove Cloudflare rocket-loader script tag
    .replace(
      /<script[^>]+src="\/cdn-cgi\/scripts\/[^"]+rocket-loader\.min\.js"[^>]*><\/script>/g,
      ""
    );

  fs.writeFileSync(path.resolve(distDir, "index.html"), cleanHtml, "utf8");

  // 5. Generate Tauri icons if missing
  const png32 = createMoondropIconPng(32);
  const png128 = createMoondropIconPng(128);
  const png256 = createMoondropIconPng(256);
  const ico = createIcoFromPng(png256, 256, 256);

  fs.writeFileSync(path.resolve(iconsDir, "32x32.png"), png32);
  fs.writeFileSync(path.resolve(iconsDir, "128x128.png"), png128);
  fs.writeFileSync(path.resolve(iconsDir, "128x128@2x.png"), png256);
  fs.writeFileSync(path.resolve(iconsDir, "icon.png"), png256);
  fs.writeFileSync(path.resolve(iconsDir, "icon.ico"), ico);

  return {
    assetCount: assetFiles.length,
    distDir,
  };
}

if (import.meta.url === `file://${process.argv[1].replace(/\\/g, "/")}`) {
  prepareFrontend().then((res) => {
    console.log(`Prepared frontend in ${res.distDir} (${res.assetCount} assets).`);
  });
}
