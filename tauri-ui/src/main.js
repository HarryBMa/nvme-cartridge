/**
 * PC Cartridge Launcher — main.js
 *
 * Reads the drive path from the URL query string (?drive=D%3A%5C),
 * calls parse_cartridge + read_image_as_data_uri on the Rust backend,
 * and wires up the Play and Eject buttons.
 *
 * Tauri 2.0 uses window.__TAURI__.core.invoke (via withGlobalTauri: true).
 */

const { invoke, isTauri } = window.__TAURI__?.core ?? (() => {
  // Dev fallback — not running inside Tauri
  return {
    invoke: async (cmd, args) => {
      console.log('[DEV invoke]', cmd, args);
      if (cmd === 'parse_cartridge') {
        return {
          title: 'Demo Game',
          cover_path: '',
          executable: 'steam://rungameid/1091500',
          drive_path: args.drivePath,
        };
      }
      return '';
    },
  };
})();

// ---- DOM references ----
const elTitle      = document.getElementById('game-title');
const elExec       = document.getElementById('game-executable');
const elCover      = document.getElementById('cover-img');
const elPlaceholder = document.getElementById('cover-placeholder');
const elStatus     = document.getElementById('status-text');
const elPlay       = document.getElementById('btn-play');
const elEject      = document.getElementById('btn-eject');
const elClose      = document.getElementById('btn-close');

// ---- Helpers ----

function setStatus(msg, type = '') {
  elStatus.textContent = msg;
  elStatus.className = type;
}

function setLoading(on) {
  elPlay.disabled  = on;
  elEject.disabled = on;
}

// ---- Drive path from query string ----

function getDrivePath() {
  const params = new URLSearchParams(window.location.search);
  return params.get('drive') ?? '';
}

// ---- Initialise ----

let cartridge = null;

async function init() {
  const drivePath = getDrivePath();

  if (!drivePath) {
    setStatus('No cartridge drive specified.', 'error');
    elTitle.textContent = 'No cartridge';
    return;
  }

  setStatus(`Reading cartridge at ${drivePath}…`);
  setLoading(true);

  try {
    cartridge = await invoke('parse_cartridge', { drivePath });

    elTitle.textContent    = cartridge.title || 'Unknown Game';
    elExec.textContent     = cartridge.executable || '';

    // Load cover image
    if (cartridge.cover_path) {
      setStatus('Loading cover art…');
      try {
        const dataUri = await invoke('read_image_as_data_uri', { path: cartridge.cover_path });
        if (dataUri) {
          elCover.src = dataUri;
          elCover.onload = () => {
            elCover.classList.add('loaded');
            elPlaceholder.classList.add('hidden');
          };
        }
      } catch (_) {
        // Cover failed — placeholder remains visible
      }
    }

    setStatus('Ready');
    setLoading(false);
    elPlay.disabled  = false;
    elEject.disabled = false;

    // Show the window now that we have data
    if (window.__TAURI__?.window) {
      const { getCurrentWindow } = window.__TAURI__.window;
      await getCurrentWindow().show();
    }

  } catch (err) {
    setStatus(`Error: ${err}`, 'error');
    elTitle.textContent = 'Cartridge error';
    setLoading(false);
  }
}

// ---- Button handlers ----

elPlay.addEventListener('click', async () => {
  if (!cartridge) return;
  setStatus('Launching…');
  setLoading(true);
  try {
    await invoke('launch_game', {
      executable: cartridge.executable,
      drivePath:  cartridge.drive_path,
    });
    setStatus('Game launched!', 'success');
  } catch (err) {
    setStatus(`Launch failed: ${err}`, 'error');
  }
  setLoading(false);
  elPlay.disabled  = false;
  elEject.disabled = false;
});

elEject.addEventListener('click', async () => {
  if (!cartridge) return;
  setStatus('Ejecting…');
  setLoading(true);
  try {
    await invoke('eject_drive', { drivePath: cartridge.drive_path });
    setStatus('Cartridge ejected. Safe to remove.', 'success');
    // Close window after a short delay
    setTimeout(async () => {
      if (window.__TAURI__?.window) {
        const { getCurrentWindow } = window.__TAURI__.window;
        await getCurrentWindow().close();
      }
    }, 1500);
  } catch (err) {
    setStatus(`Eject failed: ${err}`, 'error');
    setLoading(false);
    elPlay.disabled  = false;
    elEject.disabled = false;
  }
});

elClose.addEventListener('click', async () => {
  if (window.__TAURI__?.window) {
    const { getCurrentWindow } = window.__TAURI__.window;
    await getCurrentWindow().close();
  }
});

// ---- Boot ----
init();
