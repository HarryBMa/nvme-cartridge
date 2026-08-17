/**
 * Create-cartridge wizard.
 *
 * Backend contract (src-tauri/src/create.rs):
 *   list_steam_games()                  -> [{ appId, name, sizeOnDisk, hasCover }]
 *   steam_cover({ appId })              -> "data:image/…" | ""
 *   list_target_drives()                -> [{ path, label, totalBytes, freeBytes, hasCartridge }]
 *   create_cartridge({ request })       -> { confPath, coverWritten, warnings }
 *
 * The backend re-checks the chosen drive against its own list of removable
 * targets, so a wrong path here cannot make it write somewhere it shouldn't.
 */

const tauri = window.__TAURI__;
const invoke = tauri?.core?.invoke ?? demoInvoke;

const el = {
  search: document.getElementById("search"),
  games: document.getElementById("games"),
  gamesEmpty: document.getElementById("games-empty"),
  custom: document.getElementById("custom"),
  customTitle: document.getElementById("custom-title"),
  customExec: document.getElementById("custom-exec"),
  btnCustom: document.getElementById("btn-custom"),
  previewArt: document.getElementById("preview-art"),
  previewImg: document.getElementById("preview-img"),
  previewTitle: document.getElementById("preview-title"),
  previewSub: document.getElementById("preview-sub"),
  drives: document.getElementById("drives"),
  drivesEmpty: document.getElementById("drives-empty"),
  create: document.getElementById("btn-create"),
  rescan: document.getElementById("btn-rescan"),
  close: document.getElementById("btn-close"),
  status: document.getElementById("status"),
};

/** All installed games, unfiltered. */
let games = [];
/** Drives currently offered. */
let drives = [];
/** The chosen game, or null while in manual mode. */
let selectedGame = null;
/** The chosen drive path. */
let selectedDrive = null;
/** True when the manual title/executable fields are in use. */
let manualMode = false;

/* ========================================================================== */

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

function status(message, kind = "") {
  el.status.textContent = message;
  el.status.className = kind ? `is-${kind}` : "";
}

/* ------------------------------------------------------------------ games */

function renderGames() {
  const query = el.search.value.trim().toLowerCase();
  const matches = query
    ? games.filter((g) => g.name.toLowerCase().includes(query))
    : games;

  el.games.replaceChildren();

  for (const game of matches) {
    const li = document.createElement("li");
    const row = document.createElement("button");
    row.type = "button";
    row.className = "row";
    row.setAttribute("role", "option");
    row.setAttribute(
      "aria-selected",
      String(!manualMode && selectedGame?.appId === game.appId),
    );

    const name = document.createElement("span");
    name.className = "row__name";
    name.textContent = game.name;

    const meta = document.createElement("span");
    meta.className = "row__meta";
    meta.textContent = game.sizeOnDisk ? formatBytes(game.sizeOnDisk) : "";

    row.append(name, meta);
    row.addEventListener("click", () => selectGame(game));
    li.append(row);
    el.games.append(li);
  }

  const nothing = matches.length === 0;
  el.gamesEmpty.hidden = !nothing;
  if (nothing) {
    el.gamesEmpty.textContent = query
      ? `Nothing in your library matches “${el.search.value.trim()}”.`
      : "No installed Steam games found.";
  }
}

async function selectGame(game) {
  manualMode = false;
  el.custom.hidden = true;
  selectedGame = game;

  el.previewTitle.textContent = game.name;
  el.previewSub.textContent = `steam://rungameid/${game.appId}`;
  setPreviewArt("");
  renderGames();
  refreshCreateButton();

  // One cover at a time: base64ing a whole library would be tens of megabytes.
  if (game.hasCover) {
    try {
      const uri = await invoke("steam_cover", { appId: game.appId });
      // Ignore a cover that arrives after the user moved on.
      if (uri && selectedGame?.appId === game.appId) setPreviewArt(uri);
    } catch {
      // No art is not an error; the placeholder stays.
    }
  }
}

function setPreviewArt(src) {
  if (src) {
    el.previewImg.src = src;
    el.previewArt.classList.add("has-art");
  } else {
    el.previewImg.removeAttribute("src");
    el.previewArt.classList.remove("has-art");
  }
}

function enterManualMode() {
  manualMode = true;
  selectedGame = null;
  el.custom.hidden = false;
  setPreviewArt("");
  el.previewTitle.textContent = "By hand";
  el.previewSub.textContent = "No cover art will be copied.";
  renderGames();
  refreshCreateButton();
  el.customTitle.focus();
}

/* ----------------------------------------------------------------- drives */

function renderDrives() {
  el.drives.replaceChildren();

  for (const drive of drives) {
    const li = document.createElement("li");
    const row = document.createElement("button");
    row.type = "button";
    row.className = "row";
    row.setAttribute("role", "option");
    row.setAttribute("aria-selected", String(selectedDrive === drive.path));

    const name = document.createElement("span");
    name.className = "row__name";
    name.textContent = drive.label;

    const meta = document.createElement("span");
    meta.className = "row__meta";
    if (drive.hasCartridge) {
      meta.classList.add("row__warn");
      meta.textContent = `${formatBytes(drive.freeBytes)} free · has a cartridge`;
    } else {
      meta.textContent = `${formatBytes(drive.freeBytes)} free of ${formatBytes(drive.totalBytes)}`;
    }

    row.append(name, meta);
    row.addEventListener("click", () => {
      selectedDrive = drive.path;
      renderDrives();
      refreshCreateButton();
      if (drive.hasCartridge) {
        status(`${drive.label} already holds a cartridge. Writing will replace it.`);
      } else {
        status("");
      }
    });
    li.append(row);
    el.drives.append(li);
  }

  const nothing = drives.length === 0;
  el.drivesEmpty.hidden = !nothing;
  if (nothing) {
    el.drivesEmpty.textContent =
      "No removable drives found. Plug the cartridge in, wait for it to mount, then Rescan.";
  }
}

/* ----------------------------------------------------------------- create */

function intent() {
  if (manualMode) {
    return {
      title: el.customTitle.value.trim(),
      executable: el.customExec.value.trim(),
      appId: null,
    };
  }
  if (selectedGame) {
    return {
      title: selectedGame.name,
      executable: `steam://rungameid/${selectedGame.appId}`,
      appId: selectedGame.appId,
    };
  }
  return null;
}

function refreshCreateButton() {
  const want = intent();
  el.create.disabled = !(want && want.title && want.executable && selectedDrive);
}

async function writeCartridge() {
  const want = intent();
  if (!want || !selectedDrive) return;

  el.create.disabled = true;
  el.rescan.disabled = true;
  status("Writing…");

  try {
    const result = await invoke("create_cartridge", {
      request: {
        drivePath: selectedDrive,
        title: want.title,
        executable: want.executable,
        appId: want.appId,
        coverSource: null,
      },
    });

    const drive = drives.find((d) => d.path === selectedDrive);
    const where = drive ? drive.label : selectedDrive;
    const parts = [`Cartridge written to ${where}.`];
    if (result.coverWritten) parts.push("Cover art copied.");
    parts.push(...(result.warnings ?? []));
    status(parts.join(" "), result.warnings?.length ? "" : "good");

    // The drive now has a cartridge, so the list should say so.
    await loadDrives({ keepSelection: true, quiet: true });
  } catch (error) {
    status(String(error), "error");
  } finally {
    el.rescan.disabled = false;
    refreshCreateButton();
  }
}

/* ------------------------------------------------------------------- load */

async function loadGames() {
  try {
    games = await invoke("list_steam_games");
    renderGames();
  } catch (error) {
    games = [];
    renderGames();
    // Not fatal: manual entry still works, so say so rather than just failing.
    el.gamesEmpty.hidden = false;
    el.gamesEmpty.textContent = `${error} You can still enter a game by hand.`;
  }
}

async function loadDrives({ keepSelection = false, quiet = false } = {}) {
  try {
    drives = await invoke("list_target_drives");
  } catch (error) {
    drives = [];
    if (!quiet) status(String(error), "error");
  }
  if (!keepSelection || !drives.some((d) => d.path === selectedDrive)) {
    selectedDrive = drives.length === 1 ? drives[0].path : null;
  }
  renderDrives();
  refreshCreateButton();
}

/* ---------------------------------------------------------------- wiring */

el.search.addEventListener("input", renderGames);
el.btnCustom.addEventListener("click", enterManualMode);
el.customTitle.addEventListener("input", refreshCreateButton);
el.customExec.addEventListener("input", refreshCreateButton);
el.create.addEventListener("click", writeCartridge);
el.rescan.addEventListener("click", async () => {
  status("Rescanning…");
  await Promise.all([loadGames(), loadDrives({ keepSelection: true, quiet: true })]);
  status("");
});
el.close.addEventListener("click", closeWindow);

document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") {
    event.preventDefault();
    closeWindow();
  }
});

async function closeWindow() {
  if (tauri?.window) await tauri.window.getCurrentWindow().close();
}

async function start() {
  await Promise.all([loadGames(), loadDrives()]);
  if (tauri?.window) await tauri.window.getCurrentWindow().show();
}

/* ==========================================================================
   Browser preview — opened outside Tauri, so the wizard can be worked on
   without Steam or a spare drive:
       npx http-server tauri-ui  →  http://localhost:8080/create.html
   ========================================================================== */

async function demoInvoke(command, args) {
  switch (command) {
    case "list_steam_games":
      return [
        { appId: "367520", name: "Hollow Knight", sizeOnDisk: 9_106_886_656, hasCover: true },
        { appId: "1145360", name: "Hades", sizeOnDisk: 15_204_593_664, hasCover: false },
        { appId: "413150", name: "Stardew Valley", sizeOnDisk: 1_006_632_960, hasCover: false },
        { appId: "588650", name: "Dead Cells", sizeOnDisk: 1_395_864_371, hasCover: false },
        { appId: "1237970", name: "Titanfall 2", sizeOnDisk: 48_318_382_080, hasCover: false },
        { appId: "460950", name: "Katana ZERO", sizeOnDisk: 754_974_720, hasCover: false },
        { appId: "268910", name: "Cuphead", sizeOnDisk: 4_294_967_296, hasCover: false },
        { appId: "391540", name: "Undertale", sizeOnDisk: 178_257_920, hasCover: false },
      ];
    case "steam_cover":
      return args.appId === "367520" ? "src/demo/cover.jpg" : "";
    case "list_target_drives":
      return [
        {
          path: "/run/media/harry/CINDER",
          label: "CINDER",
          totalBytes: 128_035_676_160,
          freeBytes: 119_014_128_640,
          hasCartridge: false,
        },
        {
          path: "/run/media/harry/HOLLOW",
          label: "HOLLOW",
          totalBytes: 128_035_676_160,
          freeBytes: 18_253_611_008,
          hasCartridge: true,
        },
      ];
    case "create_cartridge":
      return {
        confPath: `${args.request.drivePath}/cartridge.conf`,
        coverWritten: Boolean(args.request.appId),
        warnings: [],
      };
    default:
      console.log("[preview]", command, args);
      return "";
  }
}

start();
