# Settings window redesign (proposal for 2.6)

Status: option C (sidebar, scrolling, hints, reset per section, editor tiles, light and dark themes, search) implemented for 3.0, together with the file name and tray history rows.

## Where we are (2.5.4)

- One 400×760 window (720 on macOS/Linux), four tabs: General, Shortcuts, Features, Editor.
- The General tab carries 15 rows that have little to do with each other: language, autostart,
  updates, color format, Snipping Tool takeover, folder, format, copy mode and five switches.
- The window has no scrolling, so every new setting made it taller (600 → 640 → 720 → 760).
  That is the real limit: 2.6 cannot add anything to General without the window outgrowing
  a 768 px laptop screen.
- Switches have no explanations. "Clicking a window opens the editor" only matters when
  "Copy right after selecting" is on, and nothing says so.
- Changes already apply instantly (no Apply button), so that part stays as it is.
- Dark only, neutral grey palette (`#1C1C1F`), while the site uses a violet-tinted palette with
  light and dark themes.

Technical base: the window is not egui. It is our own small immediate-mode UI in
`src/settings_window.rs` (winit + softbuffer + tiny-skia + bundled Inter), and every frame lays out
the controls, draws them and keeps their rectangles for hit tests. This matters for estimates:
anything is possible, but each widget (scroll view, text field, tooltip) is ours to write.
The good part is that these widgets are small, and the layout tests in every language
(`every_language_lays_out_inside_the_window`) plus the snapshot test catch regressions.

## 1. Grouping

Replace the four top tabs with a sidebar of sections, each with an icon. Every section is short
and gets a subtitle line explaining what it is for.

| Section | What goes there |
|---|---|
| **General** | Language, theme (new: System / Light / Dark), start at login |
| **Capture** | Copy right after selecting, clicking a window opens the editor (indented under it, disabled while the first is off), color format HEX/RGB for the picker |
| **Copy & save** | Copy as Image/File · Folder + Open, format PNG/JPEG, save without asking, also save copied shots, open the file after saving. Optional new rows: file name template and history size (both already exist in `settings.ini`, just hidden) |
| **Shortcuts** | Same list, split into two groups: "Anywhere" (screenshot, full screen, repeat area, pin last, color picker) and "In the editor" (recognize, scroll, record, pin, copy, save). "Reset all" at the bottom |
| **Tools** | Today's Features page: recognize text, scrolling capture, recording, pin, search, print, color picker. Each row gets an icon and one line of description |
| **Editor** | Drawing tools shown as a grid of tiles (toolbar icon + name), click toggles a tool on/off. Reads at a glance instead of 13 identical switches |
| **Windows** *(Windows only)* | Replace system screenshots, "If Windows takes it back: Ask/Replace", the PrtSc explanation and "Restore Windows defaults". Hidden on macOS/Linux |
| **About** | Version, update status, "Check for updates automatically", "Check now" / "Install", website, license, "More from the author" (Leveler, Video Glow, same as the site) |

The footer with the version moves into About, which frees the bottom of every page.

### Layout sketch

```
┌──────────────────────────────────────────────────────────────┐
│ [logo] Snapt              [ Search settings…            ]   │
├────────────────┬─────────────────────────────────────────────┤
│ ⚙ General      │ Copy & save                                 │
│ ⬚ Capture      │ Where screenshots go and how they are copied│
│ ⧉ Copy & save ◀│                                             │
│ ⌨ Shortcuts    │ Copy as                    [Image | File ]  │
│ ✦ Tools        │   File lets you paste the shot into a folder│
│ ✎ Editor       │ ─────────────────────────────────────────── │
│ ⊞ Windows      │ Folder   [C:\…\Pictures\Snapt]   [Open]     │
│                │ Format                     [ PNG | JPEG ]   │
│                │ Save without asking                    (●)  │
│                │ Also save copied shots                 ( )  │
│                │ Open the file after saving             ( )  │
│ ℹ About        │                                             │
│  Snapt 2.6.0   │                         Reset this section  │
└────────────────┴─────────────────────────────────────────────┘
```

Window about 640×460, resizable in height, with pages that scroll when they do not fit.

## 2. Usability

| Idea | Value | Cost in our UI |
|---|---|---|
| Sidebar sections instead of tabs | High: room for 2.6+ settings, every page short | Medium: sidebar is a list of buttons; mostly moving code around |
| Scrolling pages (wheel + thin scrollbar, clip to the page area) | High: ends the "window keeps growing" problem | Small: one y-offset and a clip rect, wheel events in `window_event` |
| One-line hints under rows that need them (muted 12 px) | High: explains Copy as File, window click, Windows takeover | Small: `wrapped()` already exists; ~10 strings × 3 languages |
| Dependent rows indented and dimmed when they do nothing | Medium | Small: a `disabled` flag for `switch()` |
| Shorter labels ("Save without asking" stays; "Also save copied shots" instead of "Save copied too") | Medium | Small: i18n only |
| "Reset this section" link with an inline "Click again to confirm" | Medium | Small: per-section defaults from `Settings::default()` |
| Search across all sections, matching labels and keywords in EN/RU/UK; results show the row with its section name | Medium for now (≈35 settings), more later | Medium–large: we need a text field (caret, typing, IME events from winit, Ctrl+A/Backspace), plus a keyword table. 1–1.5 days |
| Tooltips on hover | Low (hints cover it) | Small, but skip for now |
| Keyboard navigation (Tab/arrows/Space) | Medium for accessibility | Medium: focus ring + order; can come later |
| No Apply button | Already done | — |

## 3. Look

- **One style with the site.** Use the site palette as tokens: violet brand `#7c3aed` (light) /
  `#a78bfa` (dark), tinted dark background `#0f0c1d` / surface `#17132b`, line `#2a2445`, muted
  `#a49dbd`; light theme `#ffffff` / `#f6f4fe` / `#e6e1f5`, text `#1c1530`. Same 14 px corner radius
  idea, scaled down for controls. Inter is already bundled, the site falls back to system UI,
  so the window keeps Inter.
- **Light and dark themes.** Today the colors are constants; they become a `Palette` struct with
  two instances. "System" reads `Window::theme()` and follows `WindowEvent::ThemeChanged`
  (works on Windows and macOS; on Linux winit often reports nothing, so we fall back to dark).
  The title bar follows the theme on Windows 11 and macOS automatically. Later the overlay
  toolbar can use the same palette, but that is out of scope.
- **Icons.** Drawn as tiny-skia paths, the same way the toolbar draws its icons
  (`overlay/toolbar.rs`, `fn icon`), so no image assets and no size growth. About 8 sidebar icons
  plus 7 for Tools; the Editor tiles reuse the toolbar icons directly.
- **Details.** Section title 18 px + muted subtitle, rows 44 px with more air, selected sidebar
  item as a soft violet pill, switches and segments unchanged in shape but themed,
  a small Snapt logo in the header.

## Options

**A. Polish in place (~1 day).** Keep 4 tabs and the 400 px window. Split General into labeled
groups, add hints, light theme, site palette. Cheap, but the height problem stays, and 2.6
settings still have nowhere to go.

**B. Sidebar redesign (~2.5–3 days)** — recommended. Everything in sections 1 and 3 plus
scrolling, hints, dependent rows and reset per section. Solves the space problem for good and
matches the site.

**C. B + search (~4 days).** Adds the search field. Worth it once there are more settings; it can
also be a follow-up release on top of B without rework.

Common to all: three languages for every new string, snapshot images updated
(`SNAPT_UPDATE_SNAPSHOTS=1`), the per-language layout tests extended to every section and both
themes, `settings.ini` stays compatible (only a new `theme` key).

## Questions for the decision

1. A, B or C?
2. Sidebar labels with icons, or icons only (narrower window)?
3. Theme: follow the system by default, or keep dark by default?
4. Show the hidden `file_name` and `history` settings in Copy & save?
