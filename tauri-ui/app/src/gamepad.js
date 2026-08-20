/**
 * Gamepad navigation for the launcher.
 *
 * A cartridge on a shelf next to a TV wants a controller, not a mouse. The
 * webview already exposes the Gamepad API, so this needs nothing from the Rust
 * side and nothing resident: the polling loop only exists while a pad is
 * connected and the window is open, which is a few seconds per insert.
 *
 * The mapping is the obvious one, using the standard layout every modern pad
 * reports:
 *
 *   D-pad / left stick   move between the buttons on screen
 *   A (south)            press the focused button
 *   B (east)             back out of details, or dismiss the window
 *   Y (north)            details
 *   Start                Play, or the first game of a collection
 *
 * Nothing here launches anything by itself — it moves focus and clicks, exactly
 * as the keyboard does, so a controller cannot do more than a person at the
 * keyboard could.
 */

/** Standard-layout indices, named so the mapping below reads as English. */
const BUTTON = { SOUTH: 0, EAST: 1, WEST: 2, NORTH: 3, START: 9, UP: 12, DOWN: 13, LEFT: 14, RIGHT: 15 };

/** Sticks rest slightly off centre; below this a pad is being held, not moved. */
const DEADZONE = 0.55;

/** How long a held direction waits before repeating, then how fast. */
const REPEAT_DELAY = 420;
const REPEAT_RATE = 140;

/**
 * Which controls a person can reach right now, in the order they appear.
 *
 * Read from the DOM each time rather than cached: the launcher rebuilds its
 * buttons when it learns what is on the cartridge, and a collection has a
 * different set from a single game.
 */
export function focusable(root = document) {
  // Queried one selector at a time and concatenated, because a combined
  // querySelectorAll returns document order — which would put the window's
  // chrome first and land the cursor on the close button instead of the game.
  const order = [
    "#game-list .game-row__play",
    "#btn-play",
    "#btn-bundle-eject",
    "#btn-eject",
    "#btn-details",
    "#btn-close",
  ];

  const reachable = [];
  for (const selector of order) {
    for (const el of root.querySelectorAll(selector)) {
      if (el.disabled) continue;
      // offsetParent is null for anything display:none, which is how the
      // single-game row and the collection list hide each other.
      if (el.offsetParent === null) continue;
      if (!reachable.includes(el)) reachable.push(el);
    }
  }
  return reachable;
}

/** Where the cursor should go, given where it is and which way was pressed. */
export function nextIndex(current, direction, count) {
  if (count === 0) return -1;
  if (current < 0) return direction > 0 ? 0 : count - 1;
  // Wraps deliberately: on a two-button window, down from Eject should be Play
  // rather than nothing at all.
  return (current + direction + count) % count;
}

/**
 * Turn a snapshot of a pad into the buttons that went down this frame.
 *
 * Edge detection, not level: holding A must not press the button sixty times a
 * second. Directions repeat on a timer instead, which is what a menu should do.
 */
export function pressed(previous, current) {
  const down = [];
  for (let i = 0; i < current.length; i += 1) {
    if (current[i] && !previous[i]) down.push(i);
  }
  return down;
}

/** A pad's buttons and sticks, flattened into one array of booleans. */
export function snapshot(pad) {
  const state = pad.buttons.map((b) => b.pressed || b.value > 0.5);
  const [x = 0, y = 0] = pad.axes;
  // The stick is folded into the d-pad, so the rest of the code has one thing
  // to think about.
  state[BUTTON.UP] = state[BUTTON.UP] || y < -DEADZONE;
  state[BUTTON.DOWN] = state[BUTTON.DOWN] || y > DEADZONE;
  state[BUTTON.LEFT] = state[BUTTON.LEFT] || x < -DEADZONE;
  state[BUTTON.RIGHT] = state[BUTTON.RIGHT] || x > DEADZONE;
  return state;
}

/**
 * Start listening for controllers.
 *
 * `actions` supplies what the launcher wants done, so this file stays about
 * pads and knows nothing about cartridges.
 */
export function connect(actions) {
  let previous = [];
  let running = false;
  let held = null;
  let heldSince = 0;
  let lastRepeat = 0;

  function move(direction) {
    const buttons = focusable();
    if (buttons.length === 0) return;
    const current = buttons.indexOf(document.activeElement);
    const next = buttons[nextIndex(current, direction, buttons.length)];
    next?.focus({ preventScroll: true });
    next?.scrollIntoView({ block: "nearest" });
  }

  function press() {
    const buttons = focusable();
    const target = buttons.includes(document.activeElement)
      ? document.activeElement
      : buttons[0];
    target?.click();
  }

  function frame(now) {
    if (!running) return;

    const pad = [...(navigator.getGamepads?.() ?? [])].find(Boolean);
    if (pad) {
      const state = snapshot(pad);

      for (const button of pressed(previous, state)) {
        switch (button) {
          case BUTTON.SOUTH:
            press();
            break;
          case BUTTON.EAST:
            actions.back();
            break;
          case BUTTON.NORTH:
            actions.details();
            break;
          case BUTTON.START:
            actions.play();
            break;
          case BUTTON.UP:
          case BUTTON.LEFT:
            move(-1);
            held = -1;
            heldSince = now;
            lastRepeat = now;
            break;
          case BUTTON.DOWN:
          case BUTTON.RIGHT:
            move(1);
            held = 1;
            heldSince = now;
            lastRepeat = now;
            break;
        }
      }

      // A held direction keeps moving, after a pause long enough that a single
      // press never repeats by accident.
      const stillHeld =
        (held === -1 && (state[BUTTON.UP] || state[BUTTON.LEFT])) ||
        (held === 1 && (state[BUTTON.DOWN] || state[BUTTON.RIGHT]));
      if (!stillHeld) {
        held = null;
      } else if (now - heldSince > REPEAT_DELAY && now - lastRepeat > REPEAT_RATE) {
        move(held);
        lastRepeat = now;
      }

      previous = state;
    }

    requestAnimationFrame(frame);
  }

  function start() {
    if (running) return;
    running = true;
    // The focus ring is normally only drawn for keyboard users; with a pad in
    // hand it is the only way to see where you are.
    document.body.classList.add("is-gamepad");
    requestAnimationFrame(frame);
  }

  /**
   * Put the cursor on the first thing worth pressing.
   *
   * Called by the launcher once it knows what is on the cartridge, not when the
   * pad connects: at connect time the window is still empty, and the cursor
   * would land on the close button and stay there.
   */
  function focusFirst() {
    if (!running) return;
    const buttons = focusable();
    if (!buttons.includes(document.activeElement)) {
      buttons[0]?.focus({ preventScroll: true });
    }
  }

  function stop() {
    running = false;
    previous = [];
    held = null;
    document.body.classList.remove("is-gamepad");
  }

  window.addEventListener("gamepadconnected", start);
  window.addEventListener("gamepaddisconnected", () => {
    // Only stop once the last pad has gone.
    const any = [...(navigator.getGamepads?.() ?? [])].some(Boolean);
    if (!any) stop();
  });

  // A pad that was already connected when the window opened raises no event
  // until a button is pressed, so look once at startup.
  if ([...(navigator.getGamepads?.() ?? [])].some(Boolean)) start();

  return { start, stop, focusFirst };
}
