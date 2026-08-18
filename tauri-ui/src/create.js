/**
 * Create-cartridge wizard.
 *
 * Backend contract (src-tauri/src/create.rs):
 *   list_games()                     -> [{ id, name, library, source, sizeOnDisk,
 *                                          hasCover, executable, canCopy }]
 *   game_cover({ library, id })      -> "data:image/…" | ""
 *   sgdb_search_games({ query })      -> [{ id, name }]
 *   sgdb_get_artwork({ gameId, artType }) -> [{ id, url, thumb, width, height }]
 *   sgdb_download_artwork({ url, cacheKey, gameKey? }) -> "/abs/path/image.jpg"
 *   sgdb_last_used_artwork({ gameKey }) -> { path, dataUri } | null
 *   list_target_drives()             -> [{ path, label, totalBytes, freeBytes, hasCartridge }]
 *   format_plan({ drivePath })       -> { path, currentLabel, device, totalBytes, warning }
 *   executable_choices({ playniteId }) -> [{ relative, name, score }]  best first
 *   create_cartridge({ request })    -> { confPath, coverWritten, autorunWritten, icon,
 *                                          formatted, formattedFilesystem,
 *                                          gameCopied, bytesCopied,
 *                                          registeredWithSteam, gameFolder, warnings }
 *
 * The backend re-derives the list of writable drives and re-checks the format
 * confirmation itself, so nothing here can talk it into writing to the wrong
 * place.
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
  playniteLocate: document.getElementById("playnite-locate"),
  playniteRoot: document.getElementById("playnite-root"),
  btnPlayniteLocate: document.getElementById("btn-playnite-locate"),
  previewArt: document.getElementById("preview-art"),
  previewImg: document.getElementById("preview-img"),
  previewTitle: document.getElementById("preview-title"),
  previewSub: document.getElementById("preview-sub"),
  previewSource: document.getElementById("preview-source"),
  btnSgdb: document.getElementById("btn-sgdb"),
  drives: document.getElementById("drives"),
  drivesEmpty: document.getElementById("drives-empty"),
  driveSpace: document.getElementById("drive-space"),
  optCopy: document.getElementById("opt-copy"),
  optCopyHint: document.getElementById("opt-copy-hint"),
  exePick: document.getElementById("exe-pick"),
  exeChoices: document.getElementById("exe-choices"),
  exeHint: document.getElementById("exe-hint"),
  optFormat: document.getElementById("opt-format"),
  formatFields: document.getElementById("format-fields"),
  formatFilesystem: document.getElementById("format-filesystem"),
  formatLabel: document.getElementById("format-label"),
  formatConfirm: document.getElementById("format-confirm"),
  formatWarning: document.getElementById("format-warning"),
  confirmLabel: document.getElementById("confirm-label"),
  create: document.getElementById("btn-create"),
  rescan: document.getElementById("btn-rescan"),
  unregister: document.getElementById("btn-unregister"),
  close: document.getElementById("btn-close"),
  progress: document.getElementById("progress"),
  progressFill: document.getElementById("progress-fill"),
  progressText: document.getElementById("progress-text"),
  status: document.getElementById("status"),
  sgdbDialog: document.getElementById("sgdb-dialog"),
  sgdbSearch: document.getElementById("sgdb-search"),
  sgdbType: document.getElementById("sgdb-type"),
  sgdbStatus: document.getElementById("sgdb-status"),
  sgdbResults: document.getElementById("sgdb-results"),
  sgdbManualUrl: document.getElementById("sgdb-manual-url"),
  sgdbUseManual: document.getElementById("sgdb-use-manual"),
  // Bundle UI
  bundlePanel: document.getElementById("bundle-panel"),
  bundleList: document.getElementById("bundle-list"),
  bundleSpace: document.getElementById("bundle-space"),
  collectionMeta: document.getElementById("collection-meta"),
  collectionTitle: document.getElementById("collection-title"),
};

let games = [];
let drives = [];
let selectedGame = null;
let selectedDrive = null;
let manualMode = false;
/** What the backend says formatting the chosen drive would destroy. */
let formatPlan = null;
let building = false;
/** Candidates for what Play should start, when copying a non-Steam game. */
let exeCandidates = [];
let selectedCoverSource = null;
let sgdbSearchTimer = null;
let sgdbResultsFor = [];
let sgdbSelectedGameId = null;
/** Games added to the bundle (Map of game.id → game object). Order preserved. */
let bundleGames = new Map();

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
    ? games.filter(
        (g) =>
          g.name.toLowerCase().includes(query) ||
          g.source.toLowerCase().includes(query),
      )
    : games;

  el.games.replaceChildren();

  for (const game of matches) {
    const li = document.createElement("li");
    const row = document.createElement("button");
    row.type = "button";
    row.className = "row";
    row.setAttribute("role", "option");
    const inBundle = bundleGames.has(game.id);
    row.setAttribute(
      "aria-selected",
      String(!manualMode && (selectedGame?.id === game.id || inBundle)),
    );
    if (inBundle) row.classList.add("in-bundle");

    const name = document.createElement("span");
    name.className = "row__name";
    name.textContent = game.name;

    const meta = document.createElement("span");
    meta.className = "row__meta";
    // The source is the useful column when the list spans several launchers.
    const bits = [game.source || "", game.sizeOnDisk ? formatBytes(game.sizeOnDisk) : ""];
    meta.textContent = bits.filter(Boolean).join(" · ");

    // Bundle toggle: "+" to add, "✓ Added" to remove.
    const bundleBtn = document.createElement("button");
    bundleBtn.type = "button";
    bundleBtn.className = `row__bundle-btn${inBundle ? " is-added" : ""}`;
    bundleBtn.title = inBundle ? "Remove from bundle" : "Add to bundle";
    bundleBtn.setAttribute("aria-label", inBundle ? `Remove ${game.name} from bundle` : `Add ${game.name} to bundle`);
    bundleBtn.textContent = inBundle ? "✓" : "+";
    bundleBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      toggleBundleGame(game);
    });

    row.append(name, meta);
    row.addEventListener("click", () => selectGame(game));
    li.append(row, bundleBtn);
    el.games.append(li);
  }

  const nothing = matches.length === 0;
  el.gamesEmpty.hidden = !nothing;
  if (nothing && query) {
    el.gamesEmpty.textContent = `Nothing matches “${el.search.value.trim()}”.`;
  }
}

async function selectGame(game) {
  manualMode = false;
  el.custom.hidden = true;
  selectedGame = game;
  selectedCoverSource = null;

  el.previewTitle.textContent = game.name;
  el.previewSub.textContent = game.executable;
  setPreviewArt("", "");
  renderGames();
  refreshOptions();
  refreshCreateButton();

  const gameKey = `${game.library}:${game.id}`;
  try {
    const cached = await invoke("sgdb_last_used_artwork", { gameKey });
    if (cached?.dataUri && selectedGame?.id === game.id) {
      selectedCoverSource = cached.path;
      setPreviewArt(cached.dataUri, "From SteamGridDB");
      return;
    }
  } catch {
    // Cache miss or unavailable cache index.
  }

  if (game.hasCover) {
    try {
      const uri = await invoke("game_cover", { library: game.library, id: game.id });
      if (uri && selectedGame?.id === game.id) setPreviewArt(uri, "");
    } catch {
      // No art is not an error.
    }
  }
}

function safePreviewSrc(src) {
  const value = String(src || "").trim();
  if (!value) return "";
  if (value.startsWith("data:image/")) return value;
  try {
    const parsed = new URL(value, window.location.href);
    if (parsed.protocol === "http:" || parsed.protocol === "https:") return parsed.href;
    if (parsed.protocol === "file:") return parsed.href;
  } catch {
    return "";
  }
  return "";
}

function setPreviewArt(src, source = "") {
  const safeSrc = safePreviewSrc(src);
  if (safeSrc) {
    el.previewImg.src = safeSrc;
    el.previewArt.classList.add("has-art");
  } else {
    el.previewImg.removeAttribute("src");
    el.previewArt.classList.remove("has-art");
  }
  el.previewSource.hidden = !source;
  el.previewSource.textContent = source || "";
}

function enterManualMode() {
  manualMode = true;
  selectedGame = null;
  selectedCoverSource = null;
  el.custom.hidden = false;
  setPreviewArt("", "");
  el.previewTitle.textContent = "By hand";
  el.previewSub.textContent = "No cover art will be copied.";
  renderGames();
  refreshOptions();
  refreshCreateButton();
  el.customTitle.focus();
}

/* ----------------------------------------------------------------- bundle */

function toggleBundleGame(game) {
  if (bundleGames.has(game.id)) {
    bundleGames.delete(game.id);
  } else {
    bundleGames.set(game.id, game);
  }
  renderGames();
  renderBundlePanel();
  refreshCreateButton();
}

function renderBundlePanel() {
  const list = [...bundleGames.values()];
  el.bundlePanel.hidden = list.length === 0;
  el.collectionMeta.hidden = list.length < 2;

  el.bundleList.replaceChildren();
  for (const game of list) {
    const li = document.createElement("li");
    li.className = "bundle-item";

    const name = document.createElement("span");
    name.className = "bundle-item__name";
    name.textContent = game.name;

    const size = document.createElement("span");
    size.className = "bundle-item__size";
    size.textContent = game.sizeOnDisk ? formatBytes(game.sizeOnDisk) : "";

    const removeBtn = document.createElement("button");
    removeBtn.type = "button";
    removeBtn.className = "bundle-item__remove";
    removeBtn.setAttribute("aria-label", `Remove ${game.name}`);
    removeBtn.textContent = "✕";
    removeBtn.addEventListener("click", () => toggleBundleGame(game));

    li.append(name, size, removeBtn);
    el.bundleList.append(li);
  }

  // Space summary.
  const drive = drives.find((d) => d.path === selectedDrive);
  if (list.length > 0) {
    const totalSize = list.reduce((sum, g) => sum + (g.sizeOnDisk || 0), 0);
    const freeBytes = drive?.freeBytes ?? null;
    let msg = `Total: ${formatBytes(totalSize)}`;
    if (freeBytes !== null) {
      msg += ` · ${formatBytes(freeBytes)} available`;
      if (totalSize > freeBytes) {
        msg += " — ⚠ Not enough space";
        el.bundleSpace.classList.add("is-error");
      } else {
        el.bundleSpace.classList.remove("is-error");
      }
    }
    el.bundleSpace.textContent = msg;
    el.bundleSpace.hidden = false;
  } else {
    el.bundleSpace.hidden = true;
    el.bundleSpace.classList.remove("is-error");
  }

  // Auto-suggest collection title from game names.
  if (list.length >= 2 && !el.collectionTitle.value) {
    const commonWord = findCommonWord(list.map((g) => g.name));
    if (commonWord) {
      el.collectionTitle.placeholder = `${commonWord} Collection`;
    }
  }
}

/** Find a common word across game names for auto-suggesting a collection title. */
function findCommonWord(names) {
  if (names.length === 0) return "";
  const wordSets = names.map((n) =>
    new Set(n.toLowerCase().split(/\s+/).filter((w) => w.length > 3)),
  );
  for (const word of wordSets[0]) {
    if (wordSets.every((s) => s.has(word))) {
      return word.charAt(0).toUpperCase() + word.slice(1);
    }
  }
  return "";
}

/** Whether we are in bundle mode (2+ games selected). */
function isBundleMode() {
  return bundleGames.size >= 2;
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
    row.addEventListener("click", () => selectDrive(drive));
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

async function selectDrive(drive) {
  selectedDrive = drive.path;
  renderDrives();

  // Show the available space for the chosen cartridge.
  if (drive.freeBytes > 0) {
    el.driveSpace.textContent = `${formatBytes(drive.freeBytes)} available on ${drive.label}`;
    el.driveSpace.hidden = false;
  } else {
    el.driveSpace.hidden = true;
  }

  // Update the bundle space display now we know the drive.
  if (bundleGames.size > 0) renderBundlePanel();

  // Ask the backend what erasing this drive would mean; the confirmation the
  // user types is checked against its answer, not against anything held here.
  formatPlan = null;
  try {
    formatPlan = await invoke("format_plan", { drivePath: drive.path });
  } catch {
    // Not formattable: the option stays available but will refuse on Write.
  }
  refreshFormatFields();
  refreshOptions();
  refreshCreateButton();

  // Offered only for a drive Steam currently knows about, since that is the only
  // case where there is anything to remove.
  try {
    el.unregister.hidden = !(await invoke("steam_registration", { drivePath: drive.path }));
  } catch {
    el.unregister.hidden = true;
  }

  status(
    drive.hasCartridge
      ? `${drive.label} already holds a cartridge. Writing will replace it.`
      : "",
  );
}

/* ---------------------------------------------------------------- options */

function refreshOptions() {
  // In bundle mode, the copy/exe options are hidden — each game's steam:// URI
  // handles launching without copying files.
  const bundle = isBundleMode();
  document.getElementById("options").hidden = bundle;

  if (bundle) return;

  const copyable = !manualMode && Boolean(selectedGame?.canCopy);
  el.optCopy.disabled = !copyable;
  if (!copyable) el.optCopy.checked = false;
  el.btnSgdb.disabled = !Boolean((selectedGame && !manualMode) || el.customTitle.value.trim());

  // The two routes differ enough to be worth saying which one applies.
  const isSteam = selectedGame?.library === "steam";
  let hint = copyable
    ? isSteam
      ? "Also registers the drive as a Steam library, so Steam plays from the cartridge instead of your internal copy."
      : "Copies the game's folder onto the cartridge and points Play at a file inside it. No launcher needed."
    : manualMode
      ? "Only available for a game picked from the list."
      : "Playnite does not record where this one is installed, so there is nothing to copy.";
  if (el.optCopy.checked && selectedGame && selectedDrive) {
    const drive = drives.find((d) => d.path === selectedDrive);
    if (drive) {
      const capacity = el.optFormat.checked ? drive.totalBytes : drive.freeBytes;
      if (selectedGame.sizeOnDisk > 0 && selectedGame.sizeOnDisk > capacity) {
        hint = `Not enough space: needs ${formatBytes(selectedGame.sizeOnDisk)}, has ${formatBytes(capacity)}.`;
      }
    }
  }
  el.optCopyHint.textContent = hint;

  refreshExePicker();
}

/**
 * A non-Steam copy needs to know which file to run, so the choice is made here
 * rather than guessed at silently. Steam copies do not need it: the app id
 * already identifies the game wherever the library lives.
 */
async function refreshExePicker() {
  const needed =
    el.optCopy.checked && !manualMode && selectedGame && selectedGame.library !== "steam";

  el.exePick.hidden = !needed;
  if (!needed) {
    exeCandidates = [];
    return;
  }

  el.exeChoices.replaceChildren();
  el.exeHint.textContent = "Looking for the game's program…";

  try {
    exeCandidates = await invoke("executable_choices", { playniteId: selectedGame.id });
  } catch (error) {
    exeCandidates = [];
    el.exeHint.textContent = String(error);
    refreshCreateButton();
    return;
  }

  if (exeCandidates.length === 0) {
    el.exeHint.textContent =
      "Nothing runnable found in the game's folder, so it cannot be copied.";
    refreshCreateButton();
    return;
  }

  for (const candidate of exeCandidates) {
    const option = document.createElement("option");
    option.value = candidate.relative;
    option.textContent = candidate.relative;
    el.exeChoices.append(option);
  }
  // The list arrives best-first, so the default is already the best guess.
  el.exeChoices.value = exeCandidates[0].relative;
  el.exeHint.textContent =
    exeCandidates.length === 1
      ? "One program found."
      : `Best guess of ${exeCandidates.length} found. Change it if that is the wrong one.`;

  refreshCreateButton();
}

function refreshFormatFields() {
  const on = el.optFormat.checked;
  el.formatFields.hidden = !on;
  if (!on) return;

  if (formatPlan) {
    el.confirmLabel.textContent = `Type “${formatPlan.currentLabel}” to confirm`;
    el.formatConfirm.placeholder = formatPlan.currentLabel;
    el.formatWarning.textContent = formatPlan.warning;
  } else {
    el.confirmLabel.textContent = "Type the current drive name to confirm";
    el.formatConfirm.placeholder = "";
    el.formatWarning.textContent = selectedDrive
      ? "This drive cannot be formatted by the wizard."
      : "Choose a drive first.";
  }

  if (!el.formatLabel.value) {
    el.formatLabel.value = defaultLabel(intent()?.title ?? "");
  }
}

function formatFilesystemLabel(filesystem) {
  return filesystem === "exfat" ? "exFAT" : "btrfs";
}

/** Mirrors create.rs's default_label so the field starts where it would. */
function defaultLabel(title) {
  const cleaned = title
    .replace(/[^A-Za-z0-9]+/g, " ")
    .trim()
    .slice(0, 11)
    .trim()
    .toUpperCase();
  return cleaned || "CARTRIDGE";
}

/* -------------------------------------------------------------- SteamGridDB */

function sgdbGameKey() {
  if (selectedGame && !manualMode) return `${selectedGame.library}:${selectedGame.id}`;
  const title = el.customTitle.value.trim();
  return title ? `manual:${title}` : "";
}

function openSgdbDialog() {
  const seed = selectedGame?.name || el.customTitle.value.trim();
  if (!seed) {
    status("Pick a game or type a title first.", "error");
    return;
  }
  el.sgdbSearch.value = seed;
  el.sgdbStatus.textContent = "Searching…";
  el.sgdbResults.replaceChildren();
  sgdbSelectedGameId = null;
  if (typeof el.sgdbDialog.showModal === "function") {
    el.sgdbDialog.showModal();
  }
  queueSgdbSearch();
}

function queueSgdbSearch() {
  clearTimeout(sgdbSearchTimer);
  sgdbSearchTimer = setTimeout(loadSgdbGamesAndArtwork, 220);
}

async function loadSgdbGamesAndArtwork() {
  const query = el.sgdbSearch.value.trim();
  if (!query) {
    el.sgdbStatus.textContent = "Type a game title.";
    el.sgdbResults.replaceChildren();
    return;
  }
  el.sgdbStatus.textContent = "Searching…";
  try {
    const gamesFound = await invoke("sgdb_search_games", { query });
    if (!Array.isArray(gamesFound) || gamesFound.length === 0) {
      sgdbResultsFor = [];
      el.sgdbResults.replaceChildren();
      el.sgdbStatus.textContent = "No SteamGridDB matches yet.";
      return;
    }
    sgdbSelectedGameId =
      gamesFound.find((g) => g.name?.toLowerCase() === query.toLowerCase())?.id || gamesFound[0].id;
    await loadSgdbArtwork(gamesFound[0].name || query);
  } catch (error) {
    el.sgdbStatus.textContent = `SteamGridDB unavailable. You can still paste a URL. (${String(error)})`;
  }
}

async function loadSgdbArtwork(matchName) {
  if (!sgdbSelectedGameId) return;
  el.sgdbStatus.textContent = "Loading artwork…";
  try {
    const artType = el.sgdbType.value;
    const list = await invoke("sgdb_get_artwork", { gameId: sgdbSelectedGameId, artType });
    sgdbResultsFor = Array.isArray(list) ? list : [];
    renderSgdbResults(matchName);
    el.sgdbStatus.textContent = sgdbResultsFor.length
      ? `Showing ${sgdbResultsFor.length} ${artType} image${sgdbResultsFor.length === 1 ? "" : "s"} for ${matchName}.`
      : "No artwork found for that type.";
  } catch (error) {
    el.sgdbStatus.textContent = `Artwork lookup failed: ${String(error)}`;
    sgdbResultsFor = [];
    renderSgdbResults(matchName);
  }
}

function renderSgdbResults(matchName) {
  el.sgdbResults.replaceChildren();
  for (const art of sgdbResultsFor) {
    const card = document.createElement("button");
    card.type = "button";
    card.className = "sgdb-card";

    const img = document.createElement("img");
    img.src = safePreviewSrc(art.thumb || art.url);
    img.alt = `${matchName} artwork`;

    const meta = document.createElement("span");
    meta.textContent = art.width && art.height ? `${art.width}×${art.height}` : "SteamGridDB";

    card.append(img, meta);
    card.addEventListener("click", () => chooseSgdbArtwork(art));
    el.sgdbResults.append(card);
  }
}

async function chooseSgdbArtwork(art) {
  const keyBase = (selectedGame?.name || el.customTitle.value.trim() || "manual").toLowerCase();
  const cacheKey = `${keyBase}-${el.sgdbType.value}-${art.id}`;
  const gameKey = sgdbGameKey();
  try {
    const cachedPath = await invoke("sgdb_download_artwork", {
      url: art.url,
      cacheKey,
      gameKey: gameKey || null,
    });
    selectedCoverSource = cachedPath;
    setPreviewArt(art.url, "From SteamGridDB");
    status("Selected artwork from SteamGridDB.", "good");
    if (el.sgdbDialog.open) el.sgdbDialog.close("selected");
  } catch (error) {
    status(`Could not download artwork: ${String(error)}`, "error");
  }
}

async function useManualSgdbUrl() {
  const url = el.sgdbManualUrl.value.trim();
  if (!url) return;
  const keyBase = (selectedGame?.name || el.customTitle.value.trim() || "manual").toLowerCase();
  const gameKey = sgdbGameKey();
  try {
    const cachedPath = await invoke("sgdb_download_artwork", {
      url,
      cacheKey: `${keyBase}-manual-url`,
      gameKey: gameKey || null,
    });
    selectedCoverSource = cachedPath;
    setPreviewArt(url, "From SteamGridDB");
    status("Selected artwork URL.", "good");
    if (el.sgdbDialog.open) el.sgdbDialog.close("selected");
  } catch (error) {
    status(`Could not fetch that URL: ${String(error)}`, "error");
  }
}

/* ----------------------------------------------------------------- create */

function intent() {
  // Bundle mode: the "intent" is the collection.
  if (isBundleMode()) {
    const title = el.collectionTitle.value.trim() || el.collectionTitle.placeholder || "Game Collection";
    return {
      title,
      executable: "",
      appId: null,
      playniteId: null,
      isBundle: true,
    };
  }
  if (manualMode) {
    return {
      title: el.customTitle.value.trim(),
      executable: el.customExec.value.trim(),
      appId: null,
      playniteId: null,
    };
  }
  if (selectedGame) {
    return {
      title: selectedGame.name,
      executable: selectedGame.executable,
      appId: selectedGame.library === "steam" ? selectedGame.id : null,
      playniteId: selectedGame.library === "playnite" ? selectedGame.id : null,
    };
  }
  return null;
}

function refreshCreateButton() {
  if (building) {
    el.create.disabled = true;
    return;
  }

  // Bundle: need 2+ games and a drive. Check for space overflow.
  if (isBundleMode()) {
    const hasTitle = Boolean(
      el.collectionTitle.value.trim() || el.collectionTitle.placeholder,
    );
    const drive = drives.find((d) => d.path === selectedDrive);
    const totalSize = [...bundleGames.values()].reduce(
      (sum, g) => sum + (g.sizeOnDisk || 0),
      0,
    );
    const noSpace = drive && totalSize > 0 && totalSize > drive.freeBytes;
    let ok = hasTitle && Boolean(selectedDrive) && !noSpace;
    if (ok && el.optFormat.checked) {
      const typed = el.formatConfirm.value.trim();
      ok = Boolean(formatPlan) && typed === formatPlan.currentLabel && Boolean(el.formatLabel.value.trim());
    }
    el.create.disabled = !ok;
    return;
  }

  const want = intent();
  let ok = Boolean(want && want.title && want.executable && selectedDrive);

  // A non-Steam copy has nothing to point Play at until a program is chosen.
  if (ok && !el.exePick.hidden && exeCandidates.length === 0) {
    ok = false;
  }

  if (ok && el.optCopy.checked && selectedGame && selectedDrive) {
    const drive = drives.find((d) => d.path === selectedDrive);
    if (drive) {
      const capacity = el.optFormat.checked ? drive.totalBytes : drive.freeBytes;
      if (selectedGame.sizeOnDisk > 0 && selectedGame.sizeOnDisk > capacity) {
        ok = false;
      }
    }
  }

  // Formatting must be confirmed before Write is even offered.
  if (ok && el.optFormat.checked) {
    const typed = el.formatConfirm.value.trim();
    ok = Boolean(formatPlan) && typed === formatPlan.currentLabel && Boolean(el.formatLabel.value.trim());
  }
  el.create.disabled = !ok;
}

function showProgress(on) {
  el.progress.hidden = !on;
  if (!on) {
    el.progressFill.style.width = "0";
    el.progressFill.classList.remove("is-indeterminate");
    el.progressText.textContent = "";
  }
}

function onProgress(p) {
  el.progress.hidden = false;
  if (p.totalBytes > 0) {
    el.progressFill.classList.remove("is-indeterminate");
    const pct = Math.min(100, (p.doneBytes / p.totalBytes) * 100);
    el.progressFill.style.width = `${pct.toFixed(1)}%`;
    el.progressText.textContent =
      `${p.message} ${formatBytes(p.doneBytes)} of ${formatBytes(p.totalBytes)}`;
  } else {
    // Steps without a byte count (format, autorun) still need to look alive.
    el.progressFill.classList.add("is-indeterminate");
    el.progressText.textContent = p.message;
  }
}

async function writeCartridge() {
  const want = intent();
  if (!want || !selectedDrive) return;

  building = true;
  el.create.disabled = true;
  el.rescan.disabled = true;
  showProgress(true);
  el.progressFill.classList.add("is-indeterminate");
  el.progressText.textContent = "Starting…";
  status("");

  try {
    let request;
    if (isBundleMode()) {
      // Build a bundle request.
      const bundleList = [...bundleGames.values()];
      request = {
        drivePath: selectedDrive,
        title: want.title,
        executable: "",
        formatDrive: el.optFormat.checked,
        formatFilesystem: el.formatFilesystem.value,
        formatLabel: el.formatLabel.value.trim() || null,
        formatConfirmation: el.formatConfirm.value.trim() || null,
        games: bundleList.map((g) => ({
          title: g.name,
          executable: g.executable,
          appId: g.library === "steam" ? g.id : null,
          playniteId: g.library === "playnite" ? g.id : null,
          coverSource: null,
        })),
      };
    } else {
      request = {
        drivePath: selectedDrive,
        title: want.title,
        executable: want.executable,
        appId: want.appId,
        playniteId: want.playniteId,
        coverSource: selectedCoverSource,
        formatDrive: el.optFormat.checked,
        formatFilesystem: el.formatFilesystem.value,
        formatLabel: el.formatLabel.value.trim() || null,
        formatConfirmation: el.formatConfirm.value.trim() || null,
        copyGame: el.optCopy.checked,
        copyExecutable: el.exePick.hidden ? null : el.exeChoices.value || null,
      };
    }

    const result = await invoke("create_cartridge", { request });

    const drive = drives.find((d) => d.path === selectedDrive);
    const parts = [`Cartridge written to ${drive ? drive.label : selectedDrive}.`];
    if (result.formatted) {
      parts.push(
        `Formatted to ${formatFilesystemLabel(result.formattedFilesystem || request.formatFilesystem)}.`,
      );
    }
    if (result.gameCopied) {
      parts.push(
        result.gameFolder
          ? `Copied ${formatBytes(result.bytesCopied)} to ${result.gameFolder}.`
          : `Copied ${formatBytes(result.bytesCopied)}.`,
      );
    }
    if (result.registeredWithSteam) parts.push("Registered with Steam.");
    if (result.coverWritten) parts.push("Cover art copied.");
    if (result.icon) parts.push("Drive icon set.");
    parts.push(...(result.warnings ?? []));

    status(parts.join(" "), result.warnings?.length ? "" : "good");
    showProgress(false);
    el.optFormat.checked = false;
    el.formatConfirm.value = "";
    refreshFormatFields();
    await loadDrives({ keepSelection: true, quiet: true });
  } catch (error) {
    status(String(error), "error");
    showProgress(false);
  } finally {
    building = false;
    el.rescan.disabled = false;
    refreshCreateButton();
  }
}

/* ------------------------------------------------------------------- load */

async function loadGames(playniteRoot = null) {
  try {
    games = await invoke("list_games", playniteRoot ? { playniteRoot } : {});
    renderGames();
    // If we loaded successfully with a custom path, keep the panel visible so
    // the user can change it, but dim the hint since it worked.
    if (playniteRoot) {
      el.playniteLocate.hidden = false;
    } else {
      el.playniteLocate.hidden = true;
    }
  } catch (error) {
    games = [];
    renderGames();
    el.gamesEmpty.hidden = false;
    el.gamesEmpty.textContent = `${error} You can still enter a game by hand.`;
    // Show the manual Playnite path field whenever auto-discovery fails so the
    // user can point the wizard at a non-standard installation.
    el.playniteLocate.hidden = false;
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
    selectedDrive = null;
    formatPlan = null;
    if (drives.length === 1) await selectDrive(drives[0]);
  }
  renderDrives();
  refreshCreateButton();
}

/* ---------------------------------------------------------------- wiring */

el.search.addEventListener("input", renderGames);
el.btnCustom.addEventListener("click", enterManualMode);
el.btnSgdb.addEventListener("click", openSgdbDialog);
el.sgdbSearch.addEventListener("input", queueSgdbSearch);
el.sgdbType.addEventListener("change", () => {
  if (sgdbSelectedGameId) loadSgdbArtwork(el.sgdbSearch.value.trim());
});
el.sgdbUseManual.addEventListener("click", useManualSgdbUrl);
el.btnPlayniteLocate.addEventListener("click", async () => {
  const path = el.playniteRoot.value.trim();
  if (!path) return;
  el.btnPlayniteLocate.disabled = true;
  status("Loading Playnite library…");
  await loadGames(path);
  status(games.length ? `Loaded ${games.length} game${games.length === 1 ? "" : "s"}.` : "");
  el.btnPlayniteLocate.disabled = false;
});
el.customTitle.addEventListener("input", () => {
  refreshCreateButton();
  refreshOptions();
  if (el.optFormat.checked && !el.formatLabel.value) refreshFormatFields();
});
el.customExec.addEventListener("input", refreshCreateButton);
el.collectionTitle.addEventListener("input", refreshCreateButton);
el.optCopy.addEventListener("change", () => {
  refreshExePicker();
  refreshCreateButton();
});
el.exeChoices.addEventListener("change", refreshCreateButton);
el.optFormat.addEventListener("change", () => {
  refreshFormatFields();
  refreshOptions();
  refreshCreateButton();
});
el.formatFilesystem.addEventListener("change", refreshCreateButton);
el.formatConfirm.addEventListener("input", refreshCreateButton);
el.formatLabel.addEventListener("input", refreshCreateButton);
el.create.addEventListener("click", writeCartridge);
el.rescan.addEventListener("click", async () => {
  status("Rescanning…");
  await Promise.all([loadGames(), loadDrives({ keepSelection: true, quiet: true })]);
  status("");
});
el.unregister.addEventListener("click", async () => {
  if (!selectedDrive) return;
  el.unregister.disabled = true;
  try {
    const removed = await invoke("unregister_from_steam", { drivePath: selectedDrive });
    status(
      removed
        ? "Removed from Steam's library list. Its files are untouched."
        : "That drive was not in Steam's library list.",
      removed ? "good" : "",
    );
    el.unregister.hidden = removed;
  } catch (error) {
    status(String(error), "error");
  } finally {
    el.unregister.disabled = false;
  }
});

el.close.addEventListener("click", closeWindow);

document.addEventListener("keydown", (event) => {
  // Never let Escape discard a half-written cartridge.
  if (event.key === "Escape" && !building) {
    event.preventDefault();
    closeWindow();
  }
});

async function closeWindow() {
  if (tauri?.window) await tauri.window.getCurrentWindow().close();
}

async function start() {
  if (tauri?.event) {
    tauri.event.listen("cartridge://progress", (event) => onProgress(event.payload));
  }
  await Promise.all([loadGames(), loadDrives()]);
  refreshOptions();
  if (tauri?.window) await tauri.window.getCurrentWindow().show();
}

/* ==========================================================================
   Browser preview — opened outside Tauri, so the wizard can be worked on
   without Playnite, Steam or a spare drive:
       npx http-server tauri-ui  →  http://localhost:8080/create.html
   ========================================================================== */

async function demoInvoke(command, args) {
  switch (command) {
    case "list_games":
      return [
        { id: "367520", name: "Hollow Knight", library: "steam", source: "Steam",
          sizeOnDisk: 9_106_886_656, hasCover: true,
          executable: "steam://rungameid/367520", canCopy: true },
        { id: "1145360", name: "Hades", library: "steam", source: "Steam",
          sizeOnDisk: 15_204_593_664, hasCover: false,
          executable: "steam://rungameid/1145360", canCopy: true },
        { id: "b7f3-tunic", name: "Tunic", library: "playnite", source: "GOG",
          sizeOnDisk: 3_221_225_472, hasCover: false,
          executable: "playnite://playnite/start/b7f3-tunic", canCopy: true },
        { id: "c8a1-outer", name: "Outer Wilds", library: "playnite", source: "Epic",
          sizeOnDisk: 0, hasCover: false,
          executable: "playnite://playnite/start/c8a1-outer", canCopy: false },
        { id: "d9b2-halo", name: "Halo Infinite", library: "playnite", source: "Xbox",
          sizeOnDisk: 0, hasCover: false,
          executable: "playnite://playnite/start/d9b2-halo", canCopy: false },
        { id: "e0c3-mario", name: "Super Mario 64", library: "playnite", source: "Nintendo 64",
          sizeOnDisk: 0, hasCover: false,
          executable: "playnite://playnite/start/e0c3-mario", canCopy: false },
        { id: "413150", name: "Stardew Valley", library: "steam", source: "Steam",
          sizeOnDisk: 1_006_632_960, hasCover: false,
          executable: "steam://rungameid/413150", canCopy: true },
      ];
    case "game_cover":
      return args.id === "367520" ? "src/demo/cover.jpg" : "";
    case "sgdb_last_used_artwork":
      return null;
    case "sgdb_search_games":
      return [
        { id: 1, name: args.query || "Hollow Knight" },
        { id: 2, name: "God of War" },
      ];
    case "sgdb_get_artwork":
      return [
        {
          id: 7001,
          url: "https://cdn.steamgriddb.com/grid/demo-7001.jpg",
          thumb: "src/demo/cover.jpg",
          width: 600,
          height: 900,
        },
        {
          id: 7002,
          url: "https://cdn.steamgriddb.com/grid/demo-7002.jpg",
          thumb: "src/demo/cover.jpg",
          width: 460,
          height: 215,
        },
      ];
    case "sgdb_download_artwork":
      return "/tmp/sgdb-cache/demo-cover.jpg";
    case "list_target_drives":
      return [
        { path: "/run/media/harry/CINDER", label: "CINDER",
          totalBytes: 128_035_676_160, freeBytes: 119_014_128_640, hasCartridge: false },
        { path: "/run/media/harry/HOLLOW", label: "HOLLOW",
          totalBytes: 128_035_676_160, freeBytes: 18_253_611_008, hasCartridge: true },
      ];
    case "executable_choices":
      return [
        { relative: "TUNIC.exe", name: "TUNIC.exe", score: 120 },
        { relative: "bin/launcher.exe", name: "launcher.exe", score: -40 },
        { relative: "unins000.exe", name: "unins000.exe", score: -400 },
      ];
    case "steam_registration":
      return args.drivePath.endsWith("HOLLOW");
    case "unregister_from_steam":
      return true;
    case "format_plan":
      return {
        path: args.drivePath,
        currentLabel: args.drivePath.split("/").pop(),
        device: "/dev/sdb1",
        totalBytes: 128_035_676_160,
        warning: `Everything on ${args.drivePath.split("/").pop()} (128 GB) will be erased.`,
      };
    case "create_cartridge":
      return {
        confPath: `${args.request.drivePath}/cartridge.conf`,
        coverWritten: Boolean(args.request.appId) || Boolean(args.request.games?.length),
        autorunWritten: true,
        icon: null,
        formatted: args.request.formatDrive,
        formattedFilesystem: args.request.formatFilesystem || "btrfs",
        gameCopied: args.request.copyGame,
        bytesCopied: args.request.copyGame ? 9_106_886_656 : 0,
        registeredWithSteam: args.request.copyGame && Boolean(args.request.appId),
        gameFolder: args.request.copyGame
          ? args.request.appId
            ? "steamapps/common"
            : "Games/Tunic"
          : null,
        warnings: [],
      };
    default:
      console.log("[preview]", command, args);
      return "";
  }
}

start();
