# UX cleanup 1 — Design

Date: 2026-11-20
Status: approved
Builds on: `2026-11-20-start-spring-tui-design.md`

## Intent

Make the TUI easier to read and closer to start.spring.io: selected
dependencies stand out, dependency picking happens in a dialog while the main
pane shows just the selection, and editable text fields are visibly fields.

## Changes

1. **Selected dependencies are colored.** Wherever a dependency list is shown,
   a selected dependency's name is drawn in light blue (in addition to `[x]`
   in the picker). Light blue rather than blue for contrast on dark
   terminals.
2. **Add-dependencies modal; main pane lists the selection.**
   - `a` opens the modal from either pane (not while editing a text field).
     Enter on the Dependencies pane and `/` also open it.
   - The modal is a centered popup holding what the Dependencies pane shows
     today: a search line at the top that has focus as soon as the modal
     opens (fuzzy search, unchanged), the grouped list (unfiltered) or the
     ranked list with categories (filtered), and incompatible dependencies
     dimmed.
   - In the modal, ↑/↓ move, Space toggles, and typed characters go to the
     search (Backspace deletes). Enter is OK: it keeps the changes and closes.
     Esc is Cancel: it discards the changes and closes. Ctrl-C still quits.
   - The modal edits a draft copy of the selection, so Cancel restores
     exactly what was selected before it opened.
   - The main Dependencies pane lists only the selected dependencies, in
     selection order. Each entry shows its name (light blue), its category
     and a `✕` on the first line, and its description, dimmed and truncated,
     on the second. ↑/↓ move, `d` or Delete removes the highlighted
     dependency, and `X` clears all. With nothing selected the pane shows
     "No dependencies — press a to add".
   - Removing a dependency in the pane is keyboard only. No mouse capture,
     so terminal text selection keeps working.
3. **Visible text fields.** Group, Artifact, Name, Description and Package
   name draw their value in a one-line input area: a dim background spanning
   the rest of the pane width, with `▕`/`▏` edges, so empty fields are
   visible. The focused field keeps its reversed highlight. When editing,
   the cursor `▏` is shown as today.

The status-bar key help changes to mention `a add` and `d remove`.

## Approaches

- **Modal as App state with a draft selection (chosen).** `App` gains an
  `Option<Picker>` with the draft list, search text and cursor. While it's
  `Some`, keys go to the picker. OK copies the draft into the request;
  Cancel drops it. This is simple and testable, and it reuses the existing
  fuzzy search code.
- Edit the selection in place, with an undo snapshot taken on open. That's
  equivalent, but easier to get wrong when Boot changes interact. Rejected.
- A separate screen instead of a popup. Less like the website. Rejected.

## Decided by the user

- Removal is keyboard only (`d`/Delete), with `✕` shown as a marker.
- Text fields are one-line shaded inputs, not bordered boxes.
- `a` opens the modal. Ctrl+B is not used because it clashes with tmux.

## Builder's calls

- Enter (Dependencies pane) and `/` also open the modal, so the old habits
  keep working.
- The dependency search state lives in the picker. Each time the modal
  opens, its search is empty and its cursor is at the top.
- Light blue (`Color::LightBlue`) is the selection color.
