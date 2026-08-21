---
target: launcher
total_score: 30
max_score: 40
na_heuristics: 
p0_count: 0
p1_count: 3
timestamp: 2026-08-21T18-18-39Z
slug: tauri-ui-app-index-html
---
## Design Health Score

| # | Heuristic | Score | Key Issue |
|---|---|---:|---|
| 1 | Visibility of System Status | 3 | Toasts confirm actions, but loading and safety transitions are understated. |
| 2 | Match System / Real World | 4 | Cartridge, cover, Play, and Eject form a highly coherent physical metaphor. |
| 3 | User Control and Freedom | 3 | Escape and close work well; launch has no cancellation path once started. |
| 4 | Consistency and Standards | 3 | Interaction styling is consistent, but the gear icon suggests Settings more than Details. |
| 5 | Error Prevention | 4 | Disabled Play, backend eject checks, and the held-game warning are strong safeguards. |
| 6 | Recognition Rather Than Recall | 2 | Bundle shortcuts and details are discoverable mainly through prior knowledge. |
| 7 | Flexibility and Efficiency | 4 | Keyboard, number-key, and gamepad paths are excellent for repeat users. |
| 8 | Aesthetic and Minimalist Design | 4 | The cover, title, and primary actions receive disciplined visual priority. |
| 9 | Error Recovery | 2 | Failures identify the problem but rarely offer a concrete next action. |
| 10 | Help and Documentation | 1 | There is no visible shortcut legend or controller guidance. |
| **Total** | | **30/40** | **Good** |

## Design Specificity Verdict

The launcher feels authored for PC GamePak rather than interchangeable with a generic media launcher. The fixed cover-first composition, physical cartridge vocabulary, sampled accent, and `#read-line` arrival animation carry a clear product point of view.

The detector found one `broken-image` warning at tauri-ui/app/index.html#L13: the cover image has no initial `src`. This is a likely false positive because the element is hidden and receives its data URI dynamically in tauri-ui/app/src/main.js#L466. The detector ran in regex fallback mode because its HTML parser modules were unavailable. Browser inspection showed the normal, no-executable, and bundle states rendering correctly. Details-sheet inspection was inconclusive because the browser click timed out during animation.

## Overall Impression

This is a focused launcher with a strong emotional sequence: insert, recognize, play. Single-game mode is especially successful. The biggest opportunity is to make secondary capability and failure recovery legible without adding dashboard-like clutter.

## What’s Working

- `#cover-img`, `#scrim-bottom`, and `#game-title` make the artwork the interface while keeping Play unmistakable.
- Keyboard and gamepad behavior provide strong parity for television-distance use.
- `#sheet` keeps technical cartridge and drive information behind progressive disclosure.

## Priority Issues

### [P1] Bundle shortcuts are invisible

Rows have no visible number mapping even though `1–9` can launch collection entries. Add visible row numbers or keycaps and a compact “Use 1–9 to play” hint.

### [P1] Details has weak information scent

The 16px gear icon conventionally means Settings, so users may never discover health and cartridge metadata. Use an info icon or compact icon-plus-label treatment with a television-distance hit target.

### [P1] Failure states stop at diagnosis

“Unreadable” plus a raw backend error does not tell users whether to reconnect the drive, inspect `cartridge.conf`, retry, or close. Map common backend failures to plain-language causes and one recovery action.

### [P2] Accent contrast has a fragile fallback

If sampling fails or produces no usable pixels, the default accent is retained without a final contrast validation. Validate the final Play/background contrast and fall back to a known-safe accent or neutral primary treatment.

### [P2] Loading and Eject safety states are under-explained

Reading, the animated pip, and the transient second-press Eject toast may look decorative or broken from across a room. Give loading, blocked, and confirmation states a stable status treatment in the primary surface.

## Persona Red Flags

- Alex, power user: collection rows hide their `1–9` mapping; large collections require scrolling; shortcut support is undocumented; details do not expose an obvious integrity status.
- Jordan, first-timer: the gear icon reads as Settings; unreadable errors provide no recovery path; the second Eject press can feel like a failed first press; bundle navigation assumes prior knowledge.

## Minor Observations

- The fixed 240px bundle list can become difficult to scan.
- Toasts are transient and easy to miss at television distance.
- Executable identifiers are technical for non-technical users.
- The blinking pip has no visible explanation.
- “No cover art” does not distinguish absent art from still-loading art.

## Questions to Consider

- Which should come first: bundle shortcut discoverability, details discoverability, or actionable error recovery?
- Should the quiet mechanical tone stay intact, or should failure states become warmer and more explicit?
- Should the next pass address all five issues or only the three P1 issues?
