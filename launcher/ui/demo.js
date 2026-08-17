/* Sample cartridge data.
 *
 * Loaded only when the page is opened outside Tauri, so the window can be
 * designed, reviewed and screenshotted without physically inserting a drive:
 *
 *   npx http-server launcher/ui   →  http://localhost:8080
 *
 * Append ?state=blocked or ?state=steam to inspect the other states.
 */

const BASE = {
  id: "/run/media/harry/CINDER",
  title: "Cinder & Salt",
  subtitle: "Longwave Industries",
  edition: "Deluxe",
  year: 2026,
  serial: "LW-0117-A",
  accent: null, // sampled from the cover art
  artwork: "assets/demo-cover.jpg",
  logo: null,

  mount: "/run/media/harry/CINDER",
  drive: "CINDER",
  volumeLabel: "CINDER",
  device: "/dev/sdb1",
  fileSystem: "exfat",
  totalBytes: 512_110_190_592,
  availableBytes: 96_320_000_000,

  launchKind: "script",
  launchSummary: "launch.sh",
  trust: {
    state: "verified",
    digest: "3f9c1e7a5d0b48c2e6f7a91b8c4d2e05f3a7b6c9d8e1f0a2b3c4d5e6f7a8b9c0",
  },
  canPlay: true,
  autolaunch: false,
};

const VARIANTS = {
  // A cartridge whose script is not on the trust list: Play is refused and the
  // window explains what to do about it.
  blocked: {
    ...BASE,
    trust: {
      state: "untrusted",
      digest: "9b2d4f6a8c0e1357bd9f0a2c4e68a1b3d5f70928cae4c6081a3b5d7f9012e4a6",
    },
    canPlay: false,
  },
  // The common case: a Steam hand-off, which needs no trust entry.
  steam: {
    ...BASE,
    launchKind: "steam",
    launchSummary: "steam://rungameid/367520",
    trust: { state: "notRequired" },
  },
};

const requested = new URLSearchParams(location.search).get("state");

export const demoCartridge = VARIANTS[requested] ?? BASE;
