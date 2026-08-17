/**
 * PC Cartridge Launcher — main.js
 *
 * Reads the drive path from the query string (?drive=D%3A%5C), asks the Rust
 * backend what is on the cartridge, and wires up Play and Eject.
 *
 * Backend contract (src-tauri/src/main.rs):
 *   parse_cartridge({ drivePath })            -> { title, cover, cover_path, executable, drive_path }
 *   launch_game({ executable, drivePath })    -> ()
 *   eject_drive({ drivePath })                -> ()
 *
 * `cover` arrives as a data URI already. There is no command that takes a path
 * to read, so the webview cannot ask the backend for arbitrary files.
 *
 * All cartridge-supplied text is written with textContent. The title and paths
 * come off an untrusted volume and must never be able to inject markup.
 */

const tauri = window.__TAURI__;
const invoke = tauri?.core?.invoke ?? demoInvoke;

const el = {
  card: document.getElementById("card"),
  cover: document.getElementById("cover-img"),
  placeholder: document.getElementById("cover-placeholder"),
  eyebrow: document.getElementById("eyebrow-text"),
  title: document.getElementById("game-title"),
  notice: document.getElementById("notice"),
  play: document.getElementById("btn-play"),
  eject: document.getElementById("btn-eject"),
  close: document.getElementById("btn-close"),
  details: document.getElementById("btn-details"),
  sheet: document.getElementById("sheet"),
  sheetClose: document.getElementById("btn-sheet-close"),
  specs: document.getElementById("specs"),
  toast: document.getElementById("toast"),
};

let cartridge = null;

/* ==========================================================================
   Accent colour, sampled from the cover art
   ========================================================================== */

/** WCAG relative luminance from 8-bit sRGB. */
function luminance(r, g, b) {
  const lin = [r, g, b].map((c) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2];
}

function contrast(a, b) {
  const [hi, lo] = a > b ? [a, b] : [b, a];
  return (hi + 0.05) / (lo + 0.05);
}

function hslToRgb(h, s, l) {
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const hp = (((h % 360) + 360) % 360) / 60;
  const x = c * (1 - Math.abs((hp % 2) - 1));
  const [r, g, b] =
    hp < 1 ? [c, x, 0]
    : hp < 2 ? [x, c, 0]
    : hp < 3 ? [0, c, x]
    : hp < 4 ? [0, x, c]
    : hp < 5 ? [x, 0, c]
    : [c, 0, x];
  const m = l - c / 2;
  return [(r + m) * 255, (g + m) * 255, (b + m) * 255];
}

/**
 * Apply an accent and pick the ink that sits on it.
 *
 * Play is the only filled surface in the window, so its label has to stay
 * readable whatever colour the artwork happens to be: dark ink wins on ambers
 * and greens, near-white on deep blues and reds.
 */
function setAccent(h, s, l) {
  const root = document.documentElement.style;
  root.setProperty("--accent", `hsl(${h.toFixed(0)} ${(s * 100).toFixed(0)}% ${(l * 100).toFixed(0)}%)`);

  const accentLum = luminance(...hslToRgb(h, s, l));
  const dark = contrast(accentLum, luminance(...hslToRgb(h, 0.22, 0.11)));
  const light = contrast(accentLum, luminance(...hslToRgb(h, 0.12, 0.97)));
  root.setProperty(
    "--accent-ink",
    dark >= light ? `hsl(${h.toFixed(0)} 22% 11%)` : `hsl(${h.toFixed(0)} 12% 97%)`,
  );
}

/**
 * Pick the colour a person would name if asked to describe the cover.
 *
 * A flat average of any cover is mud, so each pixel is weighted by its own
 * saturation squared, biased toward the lit areas. Hues are summed as vectors
 * so reds either side of 0° do not cancel out.
 */
function sampleAccent(img) {
  const SIZE = 48;
  const canvas = document.createElement("canvas");
  canvas.width = SIZE;
  canvas.height = SIZE;
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  if (!ctx) return;

  ctx.drawImage(img, 0, 0, SIZE, SIZE);
  let data;
  try {
    data = ctx.getImageData(0, 0, SIZE, SIZE).data;
  } catch {
    return; // tainted canvas: keep the default accent
  }

  let x = 0;
  let y = 0;
  let satSum = 0;
  let weight = 0;

  for (let i = 0; i < data.length; i += 4) {
    if (data[i + 3] < 200) continue;
    const r = data[i] / 255;
    const g = data[i + 1] / 255;
    const b = data[i + 2] / 255;
    const max = Math.max(r, g, b);
    const min = Math.min(r, g, b);
    const l = (max + min) / 2;
    const d = max - min;
    if (d < 0.06) continue; // greys carry no hue
    if (l < 0.08 || l > 0.95) continue; // crushed and blown pixels lie about hue

    const s = d / (1 - Math.abs(2 * l - 1));
    let h = 0;
    if (max === r) h = 60 * (((g - b) / d) % 6);
    else if (max === g) h = 60 * ((b - r) / d + 2);
    else h = 60 * ((r - g) / d + 4);

    const w = s * s * (0.3 + l);
    const rad = (h * Math.PI) / 180;
    x += Math.cos(rad) * w;
    y += Math.sin(rad) * w;
    satSum += s * w;
    weight += w;
  }

  if (weight === 0) return; // a greyscale cover keeps the default

  const hue = (Math.atan2(y, x) * 180) / Math.PI;
  // Floor the saturation and hold the lightness out of the pastel range so the
  // sampled accent always has enough body to carry a button.
  const saturation = Math.min(Math.max(satSum / weight, 0.72), 0.9);
  setAccent(hue, saturation, 0.575);
}

/* ==========================================================================
   Details sheet
   ========================================================================== */

function specRow(label, value, muted = false) {
  const row = document.createElement("div");
  const dt = document.createElement("dt");
  dt.textContent = label;
  const dd = document.createElement("dd");
  dd.textContent = value;
  if (muted) dd.classList.add("is-muted");
  row.append(dt, dd);
  return row;
}

function renderSpecs(info) {
  el.specs.replaceChildren(
    specRow("Title", info.title || "—", !info.title),
    specRow("Launch", info.executable || "nothing configured", !info.executable),
    specRow("Drive", info.drive_path || "—"),
    specRow("Cover", info.cover_path || "none found", !info.cover_path),
  );
}

function toggleSheet(open) {
  const next = open ?? !el.sheet.classList.contains("is-open");
  el.sheet.classList.toggle("is-open", next);
  el.sheet.hidden = !next;
  el.details.setAttribute("aria-expanded", String(next));
  // preventScroll matters here: #card has overflow:hidden, which makes it a
  // scroll container, and focusing the sheet while it is still translated
  // off-screen would scroll the whole card up to reveal it.
  if (next) el.sheetClose.focus({ preventScroll: true });
  else el.details.focus({ preventScroll: true });
  el.card.scrollTop = 0;
}

/* ==========================================================================
   Status
   ========================================================================== */

let toastTimer;

function toast(message, isError = false) {
  el.toast.textContent = message;
  el.toast.classList.toggle("is-error", isError);
  el.toast.classList.add("is-on");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => el.toast.classList.remove("is-on"), 3400);
}

function setBusy(busy) {
  el.play.disabled = busy || !cartridge?.executable;
  el.eject.disabled = busy || !cartridge;
}

async function closeWindow() {
  if (tauri?.window) await tauri.window.getCurrentWindow().close();
}

/* ==========================================================================
   Boot
   ========================================================================== */

/**
 * Which cartridge this window is for.
 *
 * Inside Tauri the answer comes from the backend, which has the `--drive`
 * argument it was started with. The query string is only for the browser
 * preview, where there is no backend to ask.
 */
async function resolveDrivePath() {
  if (tauri) {
    try {
      const fromArgs = await invoke("drive_path");
      if (fromArgs) return fromArgs;
    } catch {
      // fall through to the query string
    }
  }
  return new URLSearchParams(location.search).get("drive") ?? "";
}

function fail(headline, detail) {
  el.title.textContent = headline;
  el.title.classList.toggle("is-long", headline.length > 15);
  el.eyebrow.textContent = "Cartridge";
  el.card.classList.add("is-blocked");
  el.notice.hidden = false;
  el.notice.textContent = detail;
  el.play.disabled = true;
  el.eject.disabled = !cartridge;
}

async function init() {
  const drivePath = await resolveDrivePath();

  if (!drivePath) {
    fail("No cartridge", "The launcher was started without a --drive path.");
    await showWindow();
    return;
  }

  try {
    cartridge = await invoke("parse_cartridge", { drivePath });
  } catch (error) {
    fail("Unreadable", String(error));
    await showWindow();
    return;
  }

  el.title.textContent = cartridge.title || "Unknown game";
  // Past ~15 characters a single line no longer fits at the display size, so
  // the title narrows rather than shrinking away.
  el.title.classList.toggle("is-long", (cartridge.title || "").length > 15);
  renderSpecs(cartridge);

  if (!cartridge.executable) {
    el.card.classList.add("is-blocked");
    el.notice.hidden = false;
    el.notice.textContent =
      "No executable set in cartridge.conf, so there is nothing to play. Eject is still available.";
  }

  // No cover, an unreadable one or one over the size cap all arrive as "", and
  // the placeholder simply stays.
  if (cartridge.cover) await showCover(cartridge.cover);

  setBusy(false);
  await showWindow();
}

function showCover(src) {
  return new Promise((resolve) => {
    el.cover.addEventListener(
      "load",
      () => {
        el.placeholder.classList.add("hidden");
        sampleAccent(el.cover);
        resolve();
      },
      { once: true },
    );
    el.cover.addEventListener("error", () => resolve(), { once: true });
    el.cover.src = src;
  });
}

async function showWindow() {
  if (tauri?.window) await tauri.window.getCurrentWindow().show();
}

/* ==========================================================================
   Actions
   ========================================================================== */

el.play.addEventListener("click", async () => {
  if (!cartridge?.executable || el.play.disabled) return;
  setBusy(true);
  toast("Launching…");
  try {
    await invoke("launch_game", {
      executable: cartridge.executable,
      drivePath: cartridge.drive_path,
    });
    toast("Launched");
    // The game has the screen now; the launcher has nothing left to say.
    setTimeout(closeWindow, 900);
  } catch (error) {
    toast(String(error), true);
    setBusy(false);
  }
});

el.eject.addEventListener("click", async () => {
  if (!cartridge || el.eject.disabled) return;
  setBusy(true);
  toast("Ejecting…");
  try {
    await invoke("eject_drive", { drivePath: cartridge.drive_path });
    toast("Safe to remove");
    setTimeout(closeWindow, 1400);
  } catch (error) {
    toast(String(error), true);
    setBusy(false);
  }
});

el.close.addEventListener("click", closeWindow);
el.details.addEventListener("click", () => toggleSheet());
el.sheetClose.addEventListener("click", () => toggleSheet(false));

document.addEventListener("keydown", (event) => {
  if (event.metaKey || event.ctrlKey || event.altKey) return;
  const sheetOpen = el.sheet.classList.contains("is-open");

  switch (event.key) {
    case "Enter":
      if (sheetOpen) return;
      event.preventDefault();
      el.play.click();
      break;
    case "e":
    case "E":
      event.preventDefault();
      el.eject.click();
      break;
    case "i":
    case "I":
      event.preventDefault();
      toggleSheet();
      break;
    case "Escape":
      event.preventDefault();
      // Escape backs out of the sheet before it dismisses the window.
      if (sheetOpen) toggleSheet(false);
      else closeWindow();
      break;
  }
});

/* ==========================================================================
   Browser preview
   --------------------------------------------------------------------------
   Opened outside Tauri, the page serves a sample cartridge so the window can
   be designed and reviewed without a physical drive:
       npx http-server tauri-ui  →  http://localhost:8080/?drive=/demo
   Append &state=noexec to see the nothing-to-play case.
   ========================================================================== */

async function demoInvoke(command, args) {
  const state = new URLSearchParams(location.search).get("state");
  switch (command) {
    case "parse_cartridge":
      return {
        title: "Cinder & Salt",
        // A plain path stands in for the data URI the backend would send; the
        // browser loads it directly.
        cover: "src/demo/cover.jpg",
        cover_path: "D:\\cover.jpg",
        executable: state === "noexec" ? "" : "steam://rungameid/367520",
        drive_path: args.drivePath,
      };
    default:
      console.log("[preview]", command, args);
      return "";
  }
}

init();
