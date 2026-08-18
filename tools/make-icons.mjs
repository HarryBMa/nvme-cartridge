/**
 * Rasterise src-tauri/icons/icon.svg into the icon set Tauri bundles.
 *
 * The SVG is inlined into a page rather than loaded via <img>: an SVG in an
 * <img> is a sandboxed context that cannot fetch fonts, so the wordmark would
 * fall back to whatever the machine had. Inlining lets the page declare the
 * bundled Archivo face and keeps the output identical everywhere.
 *
 *   node tools/make-icons.mjs
 */
import { chromium } from "playwright";
import path from "node:path";
import fs from "node:fs/promises";

const ROOT = path.resolve(path.dirname(new URL(import.meta.url).pathname), "..");
const ICONS = path.join(ROOT, "tauri-ui/src-tauri/icons");
const FONT = path.join(ROOT, "tauri-ui/src/fonts/archivo-latin-var.woff2");

// The sizes Tauri's bundler expects, plus the ones that go into the .ico.
const PNGS = [
  ["32x32.png", 32],
  ["128x128.png", 128],
  ["128x128@2x.png", 256],
  ["icon.png", 512],
  ["Square150x150Logo.png", 150],
  ["Square44x44Logo.png", 44],
];
const ICO_SIZES = [16, 24, 32, 48, 64, 128, 256];

// Below this size the wordmark stops being readable and starts being a smear,
// so the simplified mark is used instead.
const SMALL_UPTO = 48;

const full = await fs.readFile(path.join(ICONS, "icon.svg"), "utf8");
const small = await fs.readFile(path.join(ICONS, "icon-small.svg"), "utf8");
const fontData = (await fs.readFile(FONT)).toString("base64");

const page = (size) => `<!doctype html><meta charset="utf-8">
<style>
  @font-face {
    font-family: "Archivo";
    src: url(data:font/woff2;base64,${fontData}) format("woff2-variations");
    font-weight: 400 800;
    font-stretch: 62% 125%;
  }
  html, body { margin: 0; background: transparent; }
  svg { display: block; width: ${size}px; height: ${size}px; }
</style>
${size <= SMALL_UPTO ? small : full}`;

const browser = await chromium.launch({
  executablePath: "/opt/pw-browsers/chromium",
  args: ["--no-sandbox", "--force-color-profile=srgb"],
});

async function render(size) {
  const tab = await browser.newPage({
    viewport: { width: size, height: size },
    deviceScaleFactor: 1,
  });
  await tab.setContent(page(size), { waitUntil: "load" });
  await tab.evaluate(() => document.fonts.ready);
  const png = await tab.screenshot({ omitBackground: true, type: "png" });
  await tab.close();
  return png;
}

const rendered = new Map();
for (const size of new Set([...PNGS.map(([, s]) => s), ...ICO_SIZES])) {
  rendered.set(size, await render(size));
}

for (const [name, size] of PNGS) {
  await fs.writeFile(path.join(ICONS, name), rendered.get(size));
  console.log(`${name.padEnd(22)} ${size}x${size}${size <= SMALL_UPTO ? "  (simplified)" : ""}`);
}

// Multi-resolution .ico. Each entry stores a PNG verbatim, which Windows has
// accepted since Vista and keeps the file small at 256px.
const entries = ICO_SIZES.map((size) => ({ size, png: rendered.get(size) }));
const header = Buffer.alloc(6 + entries.length * 16);
header.writeUInt16LE(0, 0);
header.writeUInt16LE(1, 2); // type: icon
header.writeUInt16LE(entries.length, 4);

let offset = header.length;
entries.forEach((entry, i) => {
  const at = 6 + i * 16;
  header.writeUInt8(entry.size === 256 ? 0 : entry.size, at); // 256 encodes as 0
  header.writeUInt8(entry.size === 256 ? 0 : entry.size, at + 1);
  header.writeUInt8(0, at + 2); // palette
  header.writeUInt8(0, at + 3); // reserved
  header.writeUInt16LE(1, at + 4); // colour planes
  header.writeUInt16LE(32, at + 6); // bits per pixel
  header.writeUInt32LE(entry.png.length, at + 8);
  header.writeUInt32LE(offset, at + 12);
  offset += entry.png.length;
});

const ico = Buffer.concat([header, ...entries.map((e) => e.png)]);
await fs.writeFile(path.join(ICONS, "icon.ico"), ico);
console.log(`${"icon.ico".padEnd(22)} ${ICO_SIZES.join(", ")} (${(ico.length / 1024).toFixed(0)} KB)`);

await browser.close();
