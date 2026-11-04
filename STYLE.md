# `styling.md` — Freegal TUI Design System & Style Guide

This document establishes the canonical visual, spatial, and behavioral rules for all TUI rendering in `freegal`. Every screen, modal, and widget must adhere to these conventions.

---

## 1. Core Design Axioms

1. **80×24 is Law:** Every screen must be fully functional, aesthetically balanced, and free of scrollbars or clipping on an 80-column by 24-row terminal.
2. **Never Waste Vertical Lines:** On 80×24, vertical space is the most constrained resource. Zero arbitrary blank lines; every line must serve content, structure, or breathing room.
3. **Dual-Layer Visual Cues (Color + Glyph):** Visual state (such as the active tab, focused pane, or selected item) must **never rely solely on terminal color**. It must remain legible in monochrome, `NO_COLOR`, and `insta` test snapshots.
4. **Strict Overlay Isolation:** Overlays and modals must never leak background characters, lines, or ghost fragments.

---

## 2. Spatial Budget (80×24 Layout Ladder)

Every 80×24 screen is partitioned into four mandatory vertical zones:

```text
┌──────────────────────────────────────────────────────────────────────────────┐ Row 0: Header & Tabs (1 line)
│ ♪ freegal  [● Home] │ My Music │ Queue │ History │ Downloads   not signed in │
├──────────────────────────────────────────────────────────────────────────────┤ Rows 1–20: Content Canvas
│ (Search strip + Main panes + Sidebars)                                       │ (20 lines total)
│                                                                              │
├──────────────────────────────────────────────────────────────────────────────┤ Row 21: Context / Hints (1 line)
│ Enter: open · a: queue · y: dl · /: search · 1-5: tabs · ?: help             │
├──────────────────────────────────────────────────────────────────────────────┤ Rows 22–23: Player & Status
│ · IDLE nothing playing — select a track and press Enter    queue 0 · Vol 80% │ (2 lines)
│ ············································································ │
└──────────────────────────────────────────────────────────────────────────────┘
```

### Height Invariants

| Zone | Rows | Condition / Rules |
| :--- | :--- | :--- |
| **Zone 1: Header / Tabs** | **1** | Always 1 line. Logo on left, tabs center/left, status chip right. |
| **Zone 2: Content Canvas** | **18–20** | Depends on whether the player/hint rungs are active. |
| **Zone 3: Key Hint Strip** | **1** | Always 1 line. Contextual keys left, persistent navigation right. |
| **Zone 4: Player Bar** | **2** | Row 1: Track title, status pill, queue, volume. Row 2: Progress gauge. |

> **Single Footer Rule:** Never stack two hint strips. Do not render a screen-level key row directly above the global footer row. Merge them into Zone 3.

---

## 3. Borders & Box-Drawing Architecture

Borders must strictly match the active theme.

### Theme Border Tokens

| Token | Tokyo Night (Default) | Nord Theme | Notes |
| :--- | :--- | :--- | :--- |
| **Corners** | Rounded (`╭ ╮ ╯ ╰`) | Square (`┌ ┐ ┘ └`) | Set via `BorderType::Rounded` / `Plain` |
| **Horizontal** | `─` | `─` | Standard box line |
| **Vertical** | `│` | `│` | Standard box line |
| **Dividers** | `·` or `│` | `·` or `│` | Muted foreground color |

### Border Usage Rules

1. **Top & Main Canvas:** Use `Borders::ALL` with the theme-specific corner style.
2. **Sidebars:** Use `Borders::RIGHT | Borders::TOP` or full borders, but ensure adjoining borders share edges rather than drawing double parallel lines.
3. **Section Dividers inside Panes:** Use bold unicode horizontal marks (`━━ Section Name`) with **no surrounding box** to conserve vertical space.

---

## 4. Tab Bar Conventions

Tabs must always fit within 80 columns beside the title and user status chip.

### Tab Syntax
* **Active Tab:** Must use brackets and the bullet glyph `[● TabName]` (renders as high-contrast bold in color, explicit in snapshots).
* **Inactive Tabs:** Plain text `TabName` separated by muted `│`.
* **Hotkeys:** Numeric digits (`1-5`) are documented in the hint bar, not inside the tab label, to preserve horizontal space.

```text
♪ freegal  [● Home] │ My Music │ Queue │ History │ Downloads       not signed in
```

When switching tabs:
```text
♪ freegal  Home │ [● My Music] │ Queue │ History │ Downloads       not signed in
```

---

## 5. Input Fields & Search Strips

### When to show the Search Strip:
* **`Home` Tab:** Shows full-width search input at the top of the canvas (3 lines tall with border).
* **`History` & `Downloads` Tabs:** Local filter input (1 line inline or 3 lines bounded).
* **Placeholder Text:** Always include the activation key (`/`):
  ```text
  ╭─ 🔍 Search songs, artists, albums — press / to type ─────────────────────────╮
  ╰──────────────────────────────────────────────────────────────────────────────╯
  ```
* **Empty / Inactive State:** Never render a collapsed empty box (`╭──╮\n╰──╯`). It must always contain either the query or the placeholder.

---

## 6. Layout Components & Content Density

### A. Quick Links (Inline Chip Rack)
Never stack quick links vertically line-by-line. Wrap them as inline chips:
```text
[→ Artists]  [→ Trending]  [→ New Arrivals]  [→ Albums]  [→ Genres]
```
*Saves up to 5 vertical lines on the Home dashboard.*

### B. Tables (Songs, History, Downloads)
* **Index Column (`#`):** Right-aligned, width `3` (e.g., `  1`, ` 12`).
* **Title / Artist / Album Columns:** Left-aligned, percentage-constrained with graceful truncation (`…`).
* **Duration / Year / Tracks:** Right-aligned, fixed width (e.g., `4:03`, `2024`).
* **Header Format:** Capitalized, muted text, 1 row tall:
  ```text
  #   Title                     Artist                Album                Len
  1   Golden Hour               Kacey Musgraves       Golden Hour         3:18
  ```

### C. Empty States
Every empty state must follow this 2-line standard (centered or left-padded 4 spaces):
1. **Line 1 (Heading):** Muted statement of reality (e.g., `The queue is empty`).
2. **Line 2 (Action):** Actionable remedy (e.g., `Search for a track and press 'a' to append`).

---

## 7. Overlays, Dialogs & Modals Protocol

Every popup dialog (Help, Error Log, Confirmations, Audio Picker) must strictly follow the **Anti-Bleed Layering Rule**.

### Anti-Bleed Rule (Mandatory in Render Code)
```rust
// 1. Calculate centered rect
let popup_area = centered_rect(60, 40, frame.area());

// 2. Clear the EXACT bounding box under the popup
frame.render_widget(Clear, popup_area);

// 3. Render the Block with background fill + borders
let block = Block::default()
    .borders(Borders::ALL)
    .border_type(active_theme.border_type())
    .title(" Confirmation ");
frame.render_widget(block, popup_area);
```

### Confirmation Modal Standard Format
* Dimensions: Width `50–60` cols, Height `7–9` rows.
* Buttons: Bracketed explicit keys. **Enter never confirms destructive actions.**
* Layout:
```text
╭─ Download Confirmation ────────────────────────────────────────╮
│                                                                │
│  Some Long Song Title That Truncates                           │
│  Abba · Arrival                                                │
│                                                                │
│  This claim will use 1 library credit.                         │
│                                                                │
│  [y] Confirm Download               [Esc / Enter] Cancel       │
╰────────────────────────────────────────────────────────────────╯
```

---

## 8. Typography & Color Semantics

Colors must map strictly to semantic tokens, never raw ANSI values in widget code.

| Semantic Token | Tokyo Night Target | Nord Target | Usage |
| :--- | :--- | :--- | :--- |
| `BgBase` | `#1a1b26` (Dark Navy) | `#2e3440` (Polar Night) | Background |
| `FgPrimary` | `#c0caf5` (Crisp Slate) | `#eceff4` (Snow Storm) | Song titles, active text |
| `FgMuted` | `#565f89` (Deep Slate) | `#4c566a` (Muted Grey) | Inactive tabs, column headers, separators |
| `BorderNormal` | `#3b4261` | `#3b4252` | Inactive pane & table borders |
| `BorderFocus` | `#7aa2f7` (Soft Blue) | `#88c0d0` (Frost Cyan) | Active/focused pane border |
| `Accent` | `#bb9af7` (Violet) | `#81a1c1` (Glacier) | Active tab chip `[●]`, play icon `▶` |
| `Success` | `#9ece6a` (Soft Green) | `#a3be8c` (Green) | Downloads complete, valid quotas |
| `Warning` | `#e0af68` (Warm Yellow)| `#ebcb8b` (Yellow) | Confirmations, credit warnings |
| `Error` | `#f7768e` (Red) | `#bf616a` (Red) | Network errors, failed downloads |

---

## 9. Key Hint Bar Guidelines

The key hint row at Row 21 must always follow this structure:
```text
[Contextual Actions] ·································· [Navigation & Globals]
```

### Rules:
1. **Action Verbs:** Use lowercase, direct verbs (`play`, `queue`, `dl`, `search`, `tabs`, `quit`).
2. **Key Brackets:** Contextual keys use no brackets or brackets consistently: `Enter: play · a: queue · y: dl · /: search · 1-5: tabs · ?: help`.
3. **No Overflow:** Key strings must be strictly clamped or abbreviated on narrow sizes (e.g., using `layout_ladder`).

---

## 10. Checklist for PRs and Snapshot Updates

Before running `INSTA_UPDATE=always cargo test`:
- [ ] **Tab Bar:** Is the active tab visibly marked with `[● Name]`?
- [ ] **Modal Clear:** Did you call `frame.render_widget(Clear, rect)` for all dialogs?
- [ ] **No Ghosting:** Are modal borders clean of chopped background characters?
- [ ] **No Double Footers:** Does the screen only render one hint bar row?
- [ ] **Screen Routing:** Does the snapshot match the tab name (no `Queue` text on `History`)?
- [ ] **80×24 Budget:** Does all content fit inside 24 rows without dropping below the viewport? 