# UX cleanup 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Selected dependencies shown in light blue, dependency picking in an Add-dependencies modal with the main pane listing only the selection, and one-line shaded text fields.

**Architecture:** `App` gains `picker: Option<Picker>` holding a draft selection, the search text and a cursor. While it's open, keys go to the picker. OK copies the draft into the request; Cancel drops it. `ui.rs` draws the modal over the main screen and replaces the Dependencies pane with the selected list. All changes are in `src/app.rs` and `src/ui.rs`.

**Tech Stack:** Rust, ratatui 0.30 (crossterm), nucleo-matcher, insta.

**Spec:** `docs/superpowers/specs/2026-11-20-ux-cleanup-design.md` (builds on `2026-11-20-start-spring-tui-design.md`)

## Global Constraints

- Commit with `jj commit -m "<conventional commit>"`; push later with `scripts/jj-push` only when asked.
- Before each commit: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` all clean.
- Selection color: `Color::LightBlue`. Text-field background: `Color::DarkGray`, with edges `▕` and `▏`.
- Modal title ` Add dependencies `; empty-pane text `No dependencies — press a to add`.
- Keyboard only; no mouse capture.

## Review Focus

1. Boot changed after selecting → incompatible deps removed (existing behaviour) and `sel_cursor` stays in range. Test in Task 1.
2. `d` on the last row, or on an empty list → no panic; cursor clamps. Test in Task 1.
3. Picker open in a 40×10 terminal with a long list → renders, cursor visible. Test in Task 2.
4. Typing `q`, `g`, `X`, `a` or `d` in the picker search → text, not commands. Test in Task 1.
5. Very long description in the main pane → truncated to one line, no wrap. Test in Task 2.

---

### Task 1: Picker state and keys

**Files:**
- Modify: `src/app.rs`

**Interfaces:**
- Produces:
  - `pub struct Picker { pub draft: Vec<String>, pub filter: String, pub cursor: usize }`
  - `App` fields: remove `filter`, `filtering` and `dep_cursor`; add `pub picker: Option<Picker>` and `pub sel_cursor: usize`.
  - `App::visible_dependencies(&self) -> Vec<(&str, &Dependency, bool)>`: the same as today, but the query is `picker.filter` (empty when there's no picker). Compatibility checks still use `req.boot_version`.
  - `App::selected_dependencies(&self) -> Vec<(&str, &Dependency)>`: (group name, dependency) in `req.dependencies` order. Ids missing from the metadata are skipped.
  - `App::is_selected(&self, id: &str) -> bool`: checks `picker.draft` while the picker is open, otherwise `req.dependencies`.

Keys:
- **Picker open:** Ctrl-C quits. Space toggles the dependency under the cursor in `draft` if it's compatible; otherwise it sets `status` the way the current code does. Any other character is appended to `filter` and resets `cursor` to 0. Backspace removes the last filter character. ↑/↓ move the cursor, clamped. Enter sets `req.dependencies = draft` and closes the picker. Esc closes it without changes.
- **Picker closed, not editing:** `a` or `/` opens the picker with `draft = req.dependencies.clone()`, an empty filter and the cursor at 0. Enter on the Dependencies pane also opens it.
- **Dependencies pane:** ↑/↓ move `sel_cursor` over `req.dependencies`. `d` or Delete removes the entry at `sel_cursor`, then clamps it to `len - 1` (or 0 when empty). `X` clears all. Space no longer toggles here.
- **After a Boot change removes dependencies:** clamp `sel_cursor`.

- [ ] **Step 1: Update the tests for the new model.** Rewrite the existing tests that use `filter`, `filtering` or `dep_cursor` to go through the picker:
  - `fuzzy_filter_*` and `empty_filter_is_grouped` set `app.picker = Some(Picker { draft: vec![], filter: "sweb".into(), cursor: 0 })` and so on.
  - The `cursor_to` helper opens the picker with `a` and sets `picker.cursor`.
  - `incompatible_dep_cannot_be_toggled` uses the picker.
  - `boot_change_drops_incompatible` selects `vaadin` through the picker (`a`, cursor, Space, Enter) before changing Boot.
  - Remove `filter_typing_is_text_and_esc_clears`. Its replacement is `picker_typing_is_search` below.
- [ ] **Step 2: Write the new failing tests:**
  - `a_opens_picker_with_current_selection`: with deps `["web"]`, `a` gives a picker with `draft == ["web"]`, an empty filter and cursor 0.
  - `slash_and_enter_open_picker`: `/` opens it. With the Dependencies pane focused, Enter opens it.
  - `picker_typing_is_search`: with the picker open, typing `qgXad` gives filter `"qgXad"`, `quit == false`, and `req.dependencies` unchanged.
  - `picker_space_toggles_draft_only`: on `web`, Space gives a draft containing `"web"` while `req.dependencies` stays empty.
  - `picker_enter_commits`: toggle `web`, then Enter. Now `req.dependencies == ["web"]` and the picker is `None`.
  - `picker_esc_discards`: start from deps `["lombok"]`, open, toggle `web`, Esc. Now `req.dependencies == ["lombok"]`.
  - `d_removes_highlighted_and_clamps`: with deps `["web","lombok","devtools"]`, the Dependencies pane focused and `sel_cursor = 2`, `d` gives `["web","lombok"]` and `sel_cursor == 1`. Delete twice more leaves the list empty with `sel_cursor == 0`. One more `d` does not panic.
  - `selected_dependencies_in_selection_order`: deps `["lombok","web"]` give ids `["lombok","web"]` with groups `"Developer Tools"` and `"Web"`.
  - `boot_change_clamps_sel_cursor`: at Boot 4.1.3 with `["web","vaadin"]` and `sel_cursor = 1`, switching to 4.2.0 gives `["web"]` and `sel_cursor == 0`.
- [ ] **Step 3:** Run `cargo test --lib app::`. Expected: fails to compile or fails, because `Picker` and the new fields don't exist yet.
- [ ] **Step 4:** Implement in `src/app.rs`. Move the current `filter_key` and Space logic into a `picker_key(&mut self, code: KeyCode)`. `ui.rs` uses the removed fields, so make the smallest edits there that keep it compiling: read the filter from `app.picker` and keep the current layout. The real UI work is Task 2.
- [ ] **Step 5:** Run `cargo test`. Expected: PASS. The UI snapshot may still pass, since its layout hasn't changed.
- [ ] **Step 6:** `jj commit -m "feat: add dependency picker state with draft selection"`

### Task 2: Modal, selected list, colors and text fields

**Files:**
- Modify: `src/ui.rs`

**Interfaces:**
- Consumes: `App::picker`, `App::sel_cursor`, `App::selected_dependencies()`, `App::visible_dependencies()`, `App::is_selected()`.

Rendering:
- **Main Dependencies pane:**
  - Title: ` Dependencies ({n}) · a: add `.
  - Each selected entry takes two lines. Line 1 is the name in LightBlue, then the category right-aligned, then ` ✕`. Line 2 is two spaces of indent and the description, dimmed (`Color::DarkGray` foreground) and cut to the pane width with a trailing `…` when it's too long.
  - The row at `sel_cursor` is reversed when the pane has focus. Use a `List` with `ListState` so the cursor stays in view.
  - With no selection, the pane shows `No dependencies — press a to add`.
- **Modal (picker open):**
  - A centered popup, 80% of the main area's width and height (at least 20×6, at most the main area), drawn on top of a `Clear`. Title ` Add dependencies `.
  - First line: `Search: {filter}▏`.
  - Then today's list, rendered from `visible_dependencies()`: group headers when the filter is empty, the category column when filtering, incompatible entries dimmed. Selected entries (`is_selected`) show `[x]` and a LightBlue name.
  - Last line: `Space toggle · Enter OK · Esc cancel`.
  - The highlighted row is reversed and kept in view.
- **Text fields:** the value goes in a well that fills the rest of the line after the label. The edges are `▕`/`▏` and the inside is padded with spaces to the well width, on a `Color::DarkGray` background. The focused field is still reversed. While editing, the cursor `▏` is shown inside the well.
- **Help text:** `Tab pane · ↑↓ move · ←→ choose · Enter edit · a add · d remove · X clear · g generate · ? about · q quit`.

- [ ] **Step 1: Write failing tests** in `ui.rs`:
  - `main_pane_lists_selected_with_remove_marker`: with deps `["web","lombok"]`, the 100×24 screen contains `Spring Web`, `Lombok`, `✕` and `Dependencies (2)`, and does not contain `Spring Reactive Web`.
  - `main_pane_empty_message`: with no deps, the screen contains `No dependencies — press a to add`.
  - `selected_name_is_light_blue`: with deps `["web"]`, the buffer cell where `Spring Web` starts has `fg == Color::LightBlue`. Find it by searching the buffer rows.
  - `long_description_is_truncated`: the description line of `web` ends with `…` inside the pane, and no row has text past the border.
  - `picker_modal_shows_search_and_list`: with the picker open and filter `web`, the screen contains `Add dependencies`, `Search: web` and `Esc cancel`.
  - `picker_selected_is_marked`: with the picker open and draft `["web"]`, the screen contains `[x] Spring Web`.
  - `empty_text_field_has_well`: on the Description row, the cell just after the label column has `bg == Color::DarkGray`.
  - `tiny_terminal_with_picker_does_not_panic`: 40×10, picker open, cursor on the last visible item.
  - Snapshots: update `default_screen_snapshot` (review the diff), and add `picker_snapshot` (100×24, picker open, filter `"sweb"`, draft `["web"]`).
- [ ] **Step 2:** Run `cargo test --lib ui::`. Expected: FAIL for the new assertions, and the snapshot pending.
- [ ] **Step 3:** Implement in `src/ui.rs`. Put `draw_picker(frame, app, area)` and `draw_selected(frame, app, area)` in place of today's `draw_dependencies`, and put the text-field well in `draw_project`.
- [ ] **Step 4:** Run `cargo test`. Expected: PASS once you've reviewed the snapshots. Read every `.snap.new` before accepting it.
- [ ] **Step 5:** Manual check in tmux (110×30):
  - `a` opens the modal. Type `web`, Space, Enter: the main pane lists Spring Web in light blue with `✕`.
  - `d` removes it.
  - Open the modal, select something, press Esc: nothing changes.
  - An empty Description shows a shaded well.
- [ ] **Step 6:** Update the README key table: `a` add, `d`/Delete remove, and `/` and Space now work inside the picker. Then `jj commit -m "feat: add dependencies modal, selected list, highlight color and text field wells"`
