/**
 * PC GamePak — main.js
 *
 * Reads the drive path from the query string (?drive=D%3A%5C), asks the Rust
 * backend what is on the cartridge, and wires up Play and Eject.
 *
 * Backend contract (src-tauri/src/main.rs):
 *   parse_cartridge({ drivePath })            -> { title, cover, cover_path, executable, drive_path }
 *   launch_game({ executable, drivePath })    -> ()
 *   eject_drive({ drivePath })                -> ()
 *   can_eject({ drivePath })                  -> bool
 *   cartridge_health({ drivePath })           -> { link, transport, usedPercent, warnings[] }
 *
 * `cover` arrives as a data URI already. There is no command that takes a path
 * to read, so the webview cannot ask the backend for arbitrary files.
 *
 * All cartridge-supplied text is written with textContent. The title and paths
 * come off an untrusted volume and must never be able to inject markup.
 */

import { connect as connectGamepad } from "./gamepad.js";

const tauri = window.__TAURI__;
const invoke = tauri?.core?.invoke ?? demoInvoke;

const el = {
  card: document.getElementById("card"),
  cover: document.getElementById("cover-img"),
  placeholder: document.getElementById("cover-placeholder"),
  eyebrow: document.getElementById("eyebrow-text"),
  title: document.getElementById("game-title"),
  notice: document.getElementById("notice"),
  bundleHint: document.getElementById("bundle-hint"),
  play: document.getElementById("btn-play"),
  eject: document.getElementById("btn-eject"),
  close: document.getElementById("btn-close"),
  details: document.getElementById("btn-details"),
  sheet: document.getElementById("sheet"),
  sheetClose: document.getElementById("btn-sheet-close"),
  specs: document.getElementById("specs"),
  health: document.getElementById("health"),
  toast: document.getElementById("toast"),
  gameList: document.getElementById("game-list"),
  bundleEjectRow: document.getElementById("bundle-eject-row"),
  bundleEject: document.getElementById("btn-bundle-eject"),
};

let cartridge = null;
/** The connection is only looked at once, and only if the sheet is opened. */
let healthAsked = false;

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
  const accentLum = luminance(...hslToRgb(h, s, l));
  const darkContrast = contrast(accentLum, luminance(...hslToRgb(h, 0.22, 0.11)));
  const lightContrast = contrast(accentLum, luminance(...hslToRgb(h, 0.12, 0.97)));
  const useDark = darkContrast >= lightContrast;

  if (Math.max(darkContrast, lightContrast) < 4.5) {
    root.setProperty("--accent", "oklch(0.76 0.15 85)");
    root.setProperty("--accent-ink", "oklch(0.16 0.02 65)");
    return;
  }

  root.setProperty("--accent", `hsl(${h.toFixed(0)} ${(s * 100).toFixed(0)}% ${(l * 100).toFixed(0)}%)`);
  root.setProperty("--accent-ink", useDark ? `hsl(${h.toFixed(0)} 22% 11%)` : `hsl(${h.toFixed(0)} 12% 97%)`);
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
  const rows = [specRow("Title", info.title || "—", !info.title)];
  const games = info.games ?? [];

  if (games.length > 1) {
    // One row per game, so the sheet says exactly what each button starts.
    rows.push(specRow("Games", String(games.length)));
    for (const game of games) rows.push(specRow(game.title, game.executable));
  } else {
    rows.push(specRow("Launch", info.executable || "nothing configured", !info.executable));
  }

  rows.push(
    specRow("Drive", info.drive_path || "—"),
    specRow("Cover", info.cover_path || "none found", !info.cover_path),
  );
  el.specs.replaceChildren(...rows);
}

/**
 * How the cartridge is connected, filled in after the sheet is open.
 *
 * Three things decide whether a cartridge performs like the drive inside it —
 * the negotiated link, UASP or BOT, and how full it is — and none of them are
 * visible anywhere else. Asked for lazily: on Windows this shells out, and the
 * launcher opening late is worse than the sheet filling in late.
 */
async function renderHealth() {
  if (!cartridge?.drive_path || healthAsked) return;
  healthAsked = true;

  let health;
  try {
    health = await invoke("cartridge_health", { drivePath: cartridge.drive_path });
  } catch {
    return; // The sheet is still useful without it.
  }

  const rows = [];
  if (health.link) rows.push(specRow("Link", health.link));
  if (health.transport) rows.push(specRow("Transport", health.transport));
  if (health.totalBytes > 0) {
    rows.push(
      specRow("Space", `${formatBytes(health.freeBytes)} free · ${health.usedPercent}% full`),
    );
  }
  el.specs.append(...rows);

  // Anything worth saying gets said in full, under the table.
  el.health.replaceChildren(
    ...(health.warnings ?? []).map((text) => {
      const p = document.createElement("p");
      p.className = "health__note";
      p.textContent = text;
      return p;
    }),
  );
  el.health.hidden = (health.warnings ?? []).length === 0;
}

function formatBytes(bytes) {
  if (!Number.isFinite(bytes) || bytes <= 0) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let value = bytes;
  while (value >= 1000 && i < units.length - 1) {
    value /= 1000;
    i += 1;
  }
  return `${value.toFixed(value >= 100 || i === 0 ? 0 : 1)} ${units[i]}`;
}

function toggleSheet(open) {
  const next = open ?? !el.sheet.classList.contains("is-open");
  if (next) renderHealth();
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

/**
 * Whether this cartridge is on a drive at all.
 *
 * A tag is a cartridge that never moves: it resolves to a directory on this
 * machine, so there is nothing to unmount and no button to offer. The backend
 * refuses the command as well — this only keeps the window honest.
 */
let ejectable = true;

function setBusy(busy) {
  el.play.disabled = busy || !cartridge?.executable;
  el.eject.disabled = busy || !cartridge || !ejectable;
  if (el.bundleEject) el.bundleEject.disabled = busy || !cartridge || !ejectable;
  if (el.gameList && !el.gameList.hidden) {
    for (const btn of el.gameList.querySelectorAll(".game-row__play")) {
      btn.disabled = busy;
    }
  }
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
  el.notice.textContent = `${detail} Close this window, reconnect the cartridge, and try again.`;
  el.play.disabled = true;
  el.eject.disabled = !cartridge || !ejectable;
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

  // A tag has no drive behind it, so Eject goes away rather than failing when
  // pressed. If the backend cannot answer, assume there is a drive: an old
  // build that does not know the command should keep the button it had.
  try {
    ejectable = Boolean(await invoke("can_eject", { drivePath }));
  } catch {
    ejectable = true;
  }
  if (!ejectable) {
    el.eject.hidden = true;
    if (el.bundleEjectRow) el.bundleEjectRow.hidden = true;
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

  // Bundle mode: replace the single Play button with a per-game list.
  if (cartridge.isBundle && cartridge.games?.length > 0) {
    document.getElementById("button-row").hidden = true;
    el.gameList.hidden = false;
    el.bundleEjectRow.hidden = !ejectable;
    el.bundleEject.disabled = !ejectable;
    el.bundleHint.hidden = false;
    el.eyebrow.textContent = `Collection · ${cartridge.games.length} games`;
    renderGameList(cartridge.games);
  }

  setBusy(false);
  await showWindow();
  // Only now is there something to point at: with a pad connected the cursor
  // starts on the first Play button.
  gamepad.focusFirst();
}

/* ==========================================================================
   Bundle game list
   ========================================================================== */

/**
 * Render the per-game list for a multi-game bundle cartridge.
 * Each row has a small cover thumbnail, the game title, and a Play button.
 * Arrow keys move focus; Enter activates the focused play button.
 */
function renderGameList(games) {
  el.gameList.replaceChildren();
  let focusedIndex = 0;

  games.forEach((game, index) => {
    const li = document.createElement("li");
    li.className = "game-row";

    const img = document.createElement("img");
    img.className = "game-row__cover";
    img.alt = "";
    if (game.cover) img.src = game.cover;

    const titleEl = document.createElement("span");
    titleEl.className = "game-row__title";
    titleEl.textContent = game.title || "Unknown game";

    const key = document.createElement("kbd");
    key.className = "game-row__key";
    key.textContent = index < 9 ? String(index + 1) : "";
    key.setAttribute("aria-hidden", "true");

    const btn = document.createElement("button");
    btn.className = "game-row__play";
    btn.type = "button";
    btn.textContent = "Play";
    btn.setAttribute("aria-label", `Play ${game.title || "Unknown game"}`);
    btn.dataset.gameIndex = String(index);
    btn.addEventListener("click", async () => {
      if (!game.executable || btn.disabled) return;
      setBusy(true);
      toast("Launching…");
      try {
        await invoke("launch_game", {
          executable: game.executable,
          drivePath: cartridge.drive_path,
        });
        toast("Launched");
        setTimeout(closeWindow, 900);
      } catch (error) {
        toast(String(error), true);
        setBusy(false);
      }
    });

    li.append(img, key, titleEl, btn);
    el.gameList.append(li);
  });

  // Arrow-key navigation between game rows.
  el.gameList.addEventListener("keydown", (event) => {
    const btns = Array.from(el.gameList.querySelectorAll(".game-row__play"));
    if (event.key === "ArrowDown" || event.key === "ArrowRight") {
      event.preventDefault();
      focusedIndex = Math.min(focusedIndex + 1, btns.length - 1);
      btns[focusedIndex]?.focus();
    } else if (event.key === "ArrowUp" || event.key === "ArrowLeft") {
      event.preventDefault();
      focusedIndex = Math.max(focusedIndex - 1, 0);
      btns[focusedIndex]?.focus();
    }
  });
}

function showCover(src) {
  return new Promise((resolve) => {
    el.cover.hidden = false;
    el.cover.addEventListener(
      "load",
      () => {
        el.placeholder.classList.add("hidden");
        sampleAccent(el.cover);
        resolve();
      },
      { once: true },
    );
    el.cover.addEventListener("error", () => {
      el.cover.hidden = true;
      resolve();
    }, { once: true });
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

/** Set once the user has been warned that a game lives on this cartridge. */
let ejectConfirmed = false;

async function doEject(btnLabel) {
  if (!cartridge) return;
  if (cartridge.holds_game && !ejectConfirmed) {
    ejectConfirmed = true;
    if (btnLabel) btnLabel.textContent = "Eject anyway";
    el.notice.hidden = false;
    el.notice.textContent = "The game is installed on this cartridge. Quit it first, then press Eject anyway.";
    toast("Eject is waiting for confirmation", true);
    return;
  }
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
}

el.eject.addEventListener("click", async () => {
  if (!cartridge || el.eject.disabled) return;
  await doEject(el.eject.querySelector(".btn__label"));
});

el.bundleEject.addEventListener("click", async () => {
  if (!cartridge || el.bundleEject.disabled) return;
  await doEject(el.bundleEject.querySelector(".btn__label"));
});

// A controller reaches the same four things the keyboard does, and nothing
// more: it moves focus and clicks, so it cannot start anything a person at the
// keyboard could not.
const gamepad = connectGamepad({
  play: () => {
    if (cartridge?.isBundle && cartridge.games?.length > 0) {
      el.gameList.querySelector(".game-row__play")?.click();
    } else {
      el.play.click();
    }
  },
  details: () => toggleSheet(),
  back: () => {
    if (el.sheet.classList.contains("is-open")) toggleSheet(false);
    else closeWindow();
  },
});

el.close.addEventListener("click", closeWindow);
el.details.addEventListener("click", () => toggleSheet());
el.sheetClose.addEventListener("click", () => toggleSheet(false));

document.addEventListener("keydown", (event) => {
  if (event.metaKey || event.ctrlKey || event.altKey) return;
  const sheetOpen = el.sheet.classList.contains("is-open");

  // 1-9 start the nth game of a collection, so a cartridge you know can be
  // played without reaching for the mouse or arrowing down the list.
  if (!sheetOpen && cartridge?.isBundle && /^[1-9]$/.test(event.key)) {
    const button = el.gameList.querySelectorAll(".game-row__play")[Number(event.key) - 1];
    if (button) {
      event.preventDefault();
      button.click();
    }
    return;
  }

  switch (event.key) {
    case "Enter":
      if (sheetOpen) return;
      event.preventDefault();
      // For bundles, Enter plays the first game.
      if (cartridge?.isBundle && cartridge.games?.length > 0) {
        el.gameList.querySelector(".game-row__play")?.click();
      } else {
        el.play.click();
      }
      break;
    case "e":
    case "E":
      event.preventDefault();
      if (cartridge?.isBundle) el.bundleEject.click();
      else el.eject.click();
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
   Append &state=noexec to see the nothing-to-play case, &state=bundle for a
   collection, and &tag=1 for a cartridge that is a tag rather than a drive —
   which composes with the others.
   ========================================================================== */

async function demoInvoke(command, args) {
  const state = new URLSearchParams(location.search).get("state");
  switch (command) {
    case "parse_cartridge":
      if (state === "bundle") {
        return {
          title: "God of War Collection",
          cover: "src/demo/gow-collection.jpg",
          cover_path: "D:\\collection.jpg",
          executable: "steam://rungameid/310970",
          drive_path: args.drivePath,
          isBundle: true,
          holdGame: false,
          games: [
            {
              title: "God of War (2018)",
              executable: "steam://rungameid/310970",
              cover: "src/demo/gow-1.jpg",
              coverPath: "",
            },
            {
              title: "God of War: Ragnarök",
              executable: "steam://rungameid/1476670",
              cover: "src/demo/gow-2.jpg",
              coverPath: "",
            },
          ],
        };
      }
      return {
        title: "Cinder & Salt",
        // A plain path stands in for the data URI the backend would send; the
        // browser loads it directly.
        cover: "src/demo/cover.jpg",
        cover_path: "D:\\cover.jpg",
        executable: state === "noexec" ? "" : "steam://rungameid/367520",
        drive_path: args.drivePath,
        isBundle: false,
        games: [],
      };
    case "can_eject":
      // A tag resolves to a directory on this machine; there is no drive to
      // unmount and the button should not be there.
      return !new URLSearchParams(location.search).has("tag");
    case "cartridge_health":
      // The preview shows the case worth designing for: a link that is fine,
      // a transport that is not, and a drive with no room left.
      return {
        link: "10 Gbps",
        linkMbps: 10000,
        transport: "BOT",
        totalBytes: 128_035_676_160,
        freeBytes: 9_663_676_416,
        usedPercent: 92,
        warnings: [
          "Running in BOT mode, which sends one command at a time. UASP queues them and is worth roughly two to three times as much on the small random reads a game streams. Usually a different port or enclosure firmware fixes it.",
          "92% full. These drives have no DRAM of their own and cannot borrow host memory over USB, so the last 15% costs more than it looks like it should. Leaving some room back keeps random reads quick.",
        ],
      };
    default:
      console.log("[preview]", command, args);
      return "";
  }
}

init();
