# Missing homepage assets (v0.1.3)

The landing page at [`index.html`](index.html) uses CSS placeholders until these files exist.
Replace each placeholder by adding the image at the path below, then update `index.html` to use `<img src="…" alt="…">` instead of the `.media-placeholder` div.

---

## logo.png

- **Path:** `homepage/logo.png`
- **Description:** Echo app icon or compact wordmark for the navigation bar. Should read clearly at small size on a dark background (`#0d1117`).
- **Dimensions:** 48 × 48 px (provide 96 × 96 px @2x for retina)
- **Aspect ratio:** 1:1 (square)
- **Notes:** PNG with transparency preferred. Used in the sticky nav beside “Echo.” Simple, minimal mark — no fine detail that disappears at 32 px display size.

---

## favicon.png

- **Path:** `homepage/favicon.png`
- **Description:** Browser tab favicon — same branding as `logo.png` or a simplified single-color variant.
- **Dimensions:** 32 × 32 px (64 × 64 px @2x optional)
- **Aspect ratio:** 1:1
- **Notes:** PNG. Already referenced in `index.html`; replace only if you want refreshed v0.1.3 branding.

---

## screenshots/hero-main.png

- **Path:** `homepage/screenshots/hero-main.png`
- **Description:** Full Echo main window — editor open with a sample note, search/results pane visible, default **dark** theme. Shows the core “distraction-free notebook” experience.
- **Dimensions:** 1280 × 820 px (match app default window ~1280×820)
- **Aspect ratio:** ~16:10
- **Notes:** PNG. Crop OS window chrome if possible; subtle shadow is fine. Hero section (large) and gallery tile #1.

---

## screenshots/themes-grid.png

- **Path:** `homepage/screenshots/themes-grid.png`
- **Description:** Composite showing all **10 themes**: dark, light, solarized, hacker, orange-hacker, vga-437, vga-blue, speccy, vt, mf-3270. Same note content in each panel for easy comparison.
- **Dimensions:** 1920 × 1080 px recommended (or 2560 × 1440 for sharper displays)
- **Aspect ratio:** 16:9
- **Notes:** PNG. Grid or filmstrip layout. Used in the Retro themes section and gallery tile #2.

---

## screenshots/focus-mode.png

- **Path:** `homepage/screenshots/focus-mode.png`
- **Description:** Focus mode active — only the editor visible, no side panes or status chrome. Retro theme (e.g. **mf-3270** or **vt**) looks striking here.
- **Dimensions:** 1280 × 820 px
- **Aspect ratio:** ~16:10
- **Notes:** PNG. Trigger with `Ctrl/Cmd+.`. Gallery tile #3.

---

## screenshots/quick-note.png

- **Path:** `homepage/screenshots/quick-note.png`
- **Description:** Immediately after **Quick note** (`Ctrl/Cmd+Q`) — new note with timestamped title, cursor in editor, search/creation flow visible if relevant.
- **Dimensions:** 1280 × 820 px
- **Aspect ratio:** ~16:10
- **Notes:** PNG. Gallery tile #4.

---

## screenshots/find-in-note.png

- **Path:** `homepage/screenshots/find-in-note.png`
- **Description:** In-editor find bar open (`Ctrl/Cmd+F`) with a search term entered and at least one match highlighted in the note body.
- **Dimensions:** 1280 × 820 px
- **Aspect ratio:** ~16:10
- **Notes:** PNG. Gallery tile #5.

---

## screenshots/fuzzy-search.png

- **Path:** `homepage/screenshots/fuzzy-search.png`
- **Description:** Global note search (`Ctrl/Cmd+L`) with a fuzzy query — multiple results showing title, path, and snippet; ideally a typo-tolerant match (e.g. query “meting” matching “Meeting”).
- **Dimensions:** 1280 × 820 px
- **Aspect ratio:** ~16:10
- **Notes:** PNG. Gallery tile #6.

---

## screenshots/keyboard-shortcuts.png

- **Path:** `homepage/screenshots/keyboard-shortcuts.png`
- **Description:** Keyboard shortcuts overlay/dialog (`Ctrl/Cmd+K`) fully visible with readable shortcut list.
- **Dimensions:** 1280 × 820 px
- **Aspect ratio:** ~16:10
- **Notes:** PNG. Gallery tile #7. Replaces older `keyboard-shortcuts.png` if you recapture for v0.1.3 (include Q and F shortcuts).

---

## screenshots/markdown-syntax.png

- **Path:** `homepage/screenshots/markdown-syntax.png`
- **Description:** Markdown syntax help panel (`Ctrl/Cmd+M`) open beside or over the editor.
- **Dimensions:** 1280 × 820 px
- **Aspect ratio:** ~16:10
- **Notes:** PNG. Gallery tile #8.

---

## Existing files (optional reuse)

These files already exist under `homepage/screenshots/` from an earlier release. You may **reuse** them temporarily or **replace** them when capturing v0.1.3 shots:

| File | Status |
|------|--------|
| `screenshots/echo.png` | Legacy main view — superseded by `hero-main.png` naming |
| `screenshots/themes.png` | Legacy themes shot — superseded by `themes-grid.png` |
| `screenshots/focus-green.png` | Legacy focus mode — superseded by `focus-mode.png` |
| `screenshots/keyboard-shortcuts.png` | May need retake for new shortcuts |
| `screenshots/markdown-syntax.png` | May still be valid |

After adding real images, wire them in `index.html` and optionally add a lightbox (click-to-zoom) for gallery items only.
