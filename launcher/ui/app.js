/* Cartridge Launcher — window logic.
 *
 * Everything on screen comes from a `Cartridge` payload produced in Rust. All
 * text is written with textContent rather than innerHTML: the title, serial and
 * paths come off an untrusted volume, and a cartridge must not be able to inject
 * markup into the launcher.
 */

const el = {
  chassis: document.getElementById("chassis"),
  plate: document.querySelector(".plate"),
  eyebrow: document.getElementById("eyebrow"),
  eyebrowText: document.getElementById("eyebrow-text"),
  slot: document.getElementById("slot"),
  title: document.getElementById("title"),
  byline: document.getElementById("byline"),
  specs: document.getElementById("specs"),
  trust: document.getElementById("trust"),
  art: document.getElementById("art"),
  artImg: document.getElementById("art-img"),
  play: document.getElementById("play"),
  eject: document.getElementById("eject"),
  dismiss: document.getElementById("dismiss"),
  toast: document.getElementById("toast"),
};

const tauri = window.__TAURI__;
const invoke = tauri ? tauri.core.invoke : null;

/** The cartridge currently on screen. */
let current = null;

/* ==========================================================================
   Formatting
   ========================================================================== */

/** Drive-vendor decimal units, so the number matches the label on the SSD. */
function formatBytes(bytes) {
  if (!Number.isFinite(bytes) || bytes <= 0) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let value = bytes;
  while (value >= 1000 && i < units.length - 1) {
    value /= 1000;
    i += 1;
  }
  // Whole numbers above 100 do not need a decimal to be informative.
  const decimals = value >= 100 || i === 0 ? 0 : 1;
  return `${value.toFixed(decimals)} ${units[i]}`;
}

function shortDigest(digest) {
  return `${digest.slice(0, 8)}…${digest.slice(-8)}`;
}

/* ==========================================================================
   Accent colour
   ========================================================================== */

/** WCAG relative luminance from 8-bit sRGB. */
function luminance(r, g, b) {
  const lin = [r, g, b].map((c) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2];
}

function contrast(lumA, lumB) {
  const [hi, lo] = lumA > lumB ? [lumA, lumB] : [lumB, lumA];
  return (hi + 0.05) / (lo + 0.05);
}

function hslToRgb(h, s, l) {
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const hp = (((h % 360) + 360) % 360) / 60;
  const x = c * (1 - Math.abs((hp % 2) - 1));
  const [r1, g1, b1] =
    hp < 1 ? [c, x, 0]
    : hp < 2 ? [x, c, 0]
    : hp < 3 ? [0, c, x]
    : hp < 4 ? [0, x, c]
    : hp < 5 ? [x, 0, c]
    : [c, 0, x];
  const m = l - c / 2;
  return [(r1 + m) * 255, (g1 + m) * 255, (b1 + m) * 255];
}

/**
 * Apply an accent, and pick the ink that sits on top of it.
 *
 * The Play button is the only filled surface in the interface, so its label has
 * to stay readable whatever colour the artwork happens to be. Dark ink wins on
 * ambers and greens; white wins on deep blues and reds.
 */
function setAccent(h, s, l) {
  const root = document.documentElement.style;
  root.setProperty("--accent", `hsl(${h.toFixed(0)} ${(s * 100).toFixed(0)}% ${(l * 100).toFixed(0)}%)`);

  const [r, g, b] = hslToRgb(h, s, l);
  const accentLum = luminance(r, g, b);
  // Candidate inks, both tinted to the accent's hue rather than pure black/white.
  const darkInk = hslToRgb(h, 0.22, 0.11);
  const lightInk = hslToRgb(h, 0.14, 0.97);
  const darkContrast = contrast(accentLum, luminance(...darkInk));
  const lightContrast = contrast(accentLum, luminance(...lightInk));

  root.setProperty(
    "--accent-ink",
    darkContrast >= lightContrast
      ? `hsl(${h.toFixed(0)} 22% 11%)`
      : `hsl(${h.toFixed(0)} 14% 97%)`,
  );
}

/** Parse `#rgb` / `#rrggbb` into HSL and apply it. */
function setAccentFromHex(hex) {
  let body = hex.slice(1);
  if (body.length === 3) body = body.split("").map((c) => c + c).join("");
  const r = parseInt(body.slice(0, 2), 16) / 255;
  const g = parseInt(body.slice(2, 4), 16) / 255;
  const b = parseInt(body.slice(4, 6), 16) / 255;

  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  const d = max - min;
  let h = 0;
  if (d !== 0) {
    if (max === r) h = 60 * (((g - b) / d) % 6);
    else if (max === g) h = 60 * ((b - r) / d + 2);
    else h = 60 * ((r - g) / d + 4);
  }
  const s = d === 0 ? 0 : d / (1 - Math.abs(2 * l - 1));
  // Honour the author's hue, but keep lightness in a band where the chassis
  // hairlines and the Play button both still work.
  setAccent(h, Math.max(s, 0.4), Math.min(Math.max(l, 0.52), 0.72));
}

/**
 * Sample an accent from the cover art.
 *
 * A flat average of a cover is always mud, so this weights each pixel by its
 * own saturation: the colour a person would name when describing the art wins
 * over the large dark areas around it.
 */
function sampleAccentFromArtwork(img) {
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
    // A tainted canvas just means we keep the default amber.
    return;
  }

  // Average hue as a vector sum, so hues either side of 0° do not cancel out.
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
    if (d < 0.06) continue; // near-grey pixels carry no hue information
    // Ignore the extremes: near-black and blown-out pixels have unreliable hue.
    if (l < 0.08 || l > 0.95) continue;

    const s = d / (1 - Math.abs(2 * l - 1));
    let h = 0;
    if (max === r) h = 60 * (((g - b) / d) % 6);
    else if (max === g) h = 60 * ((b - r) / d + 2);
    else h = 60 * ((r - g) / d + 4);

    // Saturation-squared lets the vivid pixels decide, and the lightness term
    // breaks ties toward the lit parts of the image — the colour someone would
    // actually name — rather than the deep shadows that share their hue.
    const w = s * s * (0.3 + l);
    const rad = (h * Math.PI) / 180;
    x += Math.cos(rad) * w;
    y += Math.sin(rad) * w;
    satSum += s * w;
    weight += w;
  }

  if (weight === 0) return; // a greyscale cover keeps the default accent

  const hue = (Math.atan2(y, x) * 180) / Math.PI;
  // Floor the saturation and keep the lightness below the pastel range, so a
  // sampled accent always has enough body to carry the Play button.
  const saturation = Math.min(Math.max(satSum / weight, 0.72), 0.9);
  setAccent(hue, saturation, 0.575);
}

/* ==========================================================================
   Rendering
   ========================================================================== */

function specRow(label, valueNode) {
  const row = document.createElement("div");
  row.className = "specs__row";
  const dt = document.createElement("dt");
  dt.textContent = label;
  const dd = document.createElement("dd");
  if (typeof valueNode === "string") dd.textContent = valueNode;
  else dd.append(valueNode);
  row.append(dt, dd);
  return row;
}

/** Join non-empty parts with a dimmed middot. */
function joined(parts) {
  const frag = document.createDocumentFragment();
  parts.filter(Boolean).forEach((part, i) => {
    if (i > 0) {
      const sep = document.createElement("em");
      sep.textContent = " · ";
      frag.append(sep);
    }
    frag.append(document.createTextNode(part));
  });
  return frag;
}

function renderSpecs(cart) {
  el.specs.replaceChildren();

  el.specs.append(specRow("Mount", cart.mount));
  // The head already shows the drive designation; on Linux that is the volume
  // label, so repeating it here would say the same thing twice.
  const label = cart.volumeLabel === cart.drive ? null : cart.volumeLabel;
  el.specs.append(
    specRow("Volume", joined([label, cart.fileSystem, cart.device])),
  );

  const capacity = document.createElement("div");
  const line = document.createElement("div");
  const total = document.createElement("em");
  total.textContent = ` of ${formatBytes(cart.totalBytes)}`;
  line.append(`${formatBytes(cart.availableBytes)} free`, total);
  const meter = document.createElement("div");
  meter.className = "meter";
  const fill = document.createElement("div");
  fill.className = "meter__fill";
  const used = cart.totalBytes > 0
    ? Math.min(Math.max((cart.totalBytes - cart.availableBytes) / cart.totalBytes, 0), 1)
    : 0;
  fill.style.width = `${(used * 100).toFixed(1)}%`;
  meter.append(fill);
  capacity.append(line, meter);
  el.specs.append(specRow("Capacity", capacity));

  el.specs.append(specRow("Launch", cart.launchSummary));

  const digest = cart.trust.digest;
  el.specs.append(
    specRow(
      "Verify",
      digest
        ? `sha256 ${shortDigest(digest)}`
        : "not required · protocol hand-off",
    ),
  );
}

const TRUST_COPY = {
  untrusted: {
    badge: "Not on trust list",
    note: "This cartridge runs a script that has not been approved on this machine. Trust it once and its SHA-256 is remembered; edit the script later and it will need approving again.",
    action: "Trust this cartridge",
  },
  unreadable: {
    badge: "Cannot verify",
    note: "The file this cartridge wants to run could not be read, so there is nothing to check against the trust list.",
  },
};

function renderTrust(cart) {
  el.trust.replaceChildren();
  const state = cart.trust.state;
  const blocked = !cart.canPlay;
  el.trust.classList.toggle("trust--blocked", blocked);
  el.plate.classList.toggle("plate--blocked", blocked);

  const badge = document.createElement("p");
  badge.className = "trust__badge";
  badge.style.margin = "0";

  if (state === "verified") {
    badge.textContent = "Trusted · SHA-256 verified";
    el.trust.append(badge);
    return;
  }
  if (state === "notRequired") {
    badge.textContent = "Steam hand-off · no script to trust";
    el.trust.append(badge);
    return;
  }

  const copy = TRUST_COPY[state] ?? TRUST_COPY.unreadable;
  badge.textContent = copy.badge;
  const note = document.createElement("p");
  note.className = "trust__note";
  note.textContent = copy.note;
  el.trust.append(badge, note);

  if (copy.action && cart.trust.digest) {
    const button = document.createElement("button");
    button.className = "trust__link";
    button.type = "button";
    button.textContent = copy.action;
    button.addEventListener("click", () => trustCartridge());
    // Sits inside the note so it reads as the end of that sentence.
    note.append(document.createTextNode(" "), button);
  }
}

function render(cart) {
  current = cart;

  // Accent first, so the reveal animation runs in the final colour.
  if (cart.accent) setAccentFromHex(cart.accent);

  el.title.textContent = cart.title;
  el.title.classList.toggle("title--long", cart.title.length > 22);

  el.byline.replaceChildren();
  const bylineParts = [cart.subtitle, cart.year ? String(cart.year) : null, cart.serial]
    .filter(Boolean);
  bylineParts.forEach((part, i) => {
    if (i > 0) {
      const sep = document.createElement("span");
      sep.className = "byline__sep";
      el.byline.append(sep);
    }
    el.byline.append(document.createTextNode(part));
  });
  if (cart.edition) {
    const edition = document.createElement("span");
    edition.className = "byline__edition";
    edition.textContent = cart.edition;
    el.byline.append(edition);
  }

  el.slot.textContent = cart.drive;
  el.eyebrowText.textContent = cart.canPlay
    ? "Cartridge detected"
    : "Cartridge blocked";

  renderSpecs(cart);
  renderTrust(cart);

  el.play.disabled = !cart.canPlay;
  el.play.querySelector(".btn__label").textContent = cart.autolaunch
    ? "Launching"
    : "Play";

  if (cart.artwork) {
    el.art.classList.remove("art--empty");
    el.artImg.hidden = false;
    el.artImg.src = cart.artwork;
    el.artImg.alt = `${cart.title} cover art`;
    // Only sample when the manifest did not name a colour itself.
    if (!cart.accent) {
      if (el.artImg.complete) sampleAccentFromArtwork(el.artImg);
      else el.artImg.addEventListener(
        "load",
        () => sampleAccentFromArtwork(el.artImg),
        { once: true },
      );
    }
  } else {
    el.art.classList.add("art--empty");
    el.artImg.hidden = true;
    el.artImg.removeAttribute("src");
  }

  // Replay the insert animation for a cartridge swapped in while the window is
  // already up.
  el.chassis.getAnimations({ subtree: true }).forEach((a) => {
    a.cancel();
    a.play();
  });
}

/* ==========================================================================
   Actions
   ========================================================================== */

let toastTimer;

function toast(message, isError = false) {
  el.toast.textContent = message;
  el.toast.classList.toggle("toast--error", isError);
  el.toast.classList.add("toast--on");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => el.toast.classList.remove("toast--on"), 3200);
}

/** Run a backend command, keeping the buttons disabled while it is in flight. */
async function act(command, pending) {
  if (!current || !invoke) return;
  const buttons = [el.play, el.eject];
  const wasDisabled = buttons.map((b) => b.disabled);
  buttons.forEach((b) => (b.disabled = true));
  if (pending) toast(pending);

  try {
    const result = await invoke(command, { id: current.id });
    toast(result.message, !result.ok);
    return result;
  } catch (error) {
    toast(String(error), true);
  } finally {
    buttons.forEach((b, i) => (b.disabled = wasDisabled[i]));
  }
}

async function play() {
  if (el.play.disabled) return;
  await act("play", "Launching…");
}

async function ejectCartridge() {
  await act("eject_cartridge", "Ejecting…");
}

async function trustCartridge() {
  const result = await act("trust_cartridge");
  if (result?.ok && invoke) {
    // Re-read so the badge, the digest row and the Play button all agree.
    const fresh = await invoke("current_cartridge");
    if (fresh) render(fresh);
  }
}

function dismiss() {
  if (invoke) invoke("dismiss");
  else toast("Dismissed");
}

el.play.addEventListener("click", play);
el.eject.addEventListener("click", ejectCartridge);
el.dismiss.addEventListener("click", dismiss);

document.addEventListener("keydown", (event) => {
  if (event.metaKey || event.ctrlKey || event.altKey) return;
  switch (event.key) {
    case "Enter":
      event.preventDefault();
      play();
      break;
    case "e":
    case "E":
      event.preventDefault();
      ejectCartridge();
      break;
    case "Escape":
      event.preventDefault();
      dismiss();
      break;
  }
});

/* ==========================================================================
   Wiring
   ========================================================================== */

async function start() {
  if (!tauri) {
    // Opened in a plain browser: render a sample cartridge so the window can be
    // designed and screenshotted without a physical drive.
    const { demoCartridge } = await import("./demo.js");
    render(demoCartridge);
    return;
  }

  const { listen } = tauri.event;
  listen("cartridge://inserted", (event) => render(event.payload));
  listen("cartridge://removed", (event) => {
    if (current && current.id === event.payload) current = null;
  });
  listen("cartridge://status", (event) => {
    toast(event.payload.message, !event.payload.ok);
  });

  // The window may have been shown before this script ran.
  const existing = await invoke("current_cartridge");
  if (existing) render(existing);
}

start();
