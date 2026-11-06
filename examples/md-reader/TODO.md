# md-reader: TODO

Legend: `[ ]` to do · `[~]` in progress · `[x]` done

## Done

- [x] Docs bundled at build time (`gen-docs.ts`): `docs/` and the top-level `.md` files, images
  copied into the game folder
- [x] Markdown parser: headings, paragraphs, emphasis, inline code, links, images, nested and
  task lists, quotes, GitHub tables, rules, code blocks; it scans with sticky patterns rather
  than copying the rest of each line (TODO.md: 293 ms to 36 ms in QuickJS)
- [x] File tree on the left, toggled from the toolbar (☰, Ctrl+B)
- [x] Back / forward (buttons, Alt+← / Alt+→, mouse side buttons), keeping each page's scroll
- [x] Home button (an icon)
- [x] Links between docs and to `#headings`; link targets in the status bar
- [x] Inline links take clicks and `:hover` (spans are hit-tested in `web_ui_dom`, `M3-EVT-07`)
- [x] Images (PNG, JPEG, SVG); videos get a placeholder
- [x] Path bar like Explorer's: each folder opens a listing of that folder, "Platypus" the root
- [x] Long pages load progressively: a screenful at once, then a chunk a frame (memoized chunk
  boxes, long lists cut between items); anchors and scroll restores mount what they need first
- [x] Engine work it prompted (`web_ui_dom`): an exact size cache beyond Taffy's nine entries
  (`M2-LAY-07`), drawing culled to what scroll containers show (`M2-DRW-09`), line breaking
  skipped when a block's width is unchanged. A scroll frame of TODO.md: ~21 ms to ~5 ms
  (release)
- [x] Syntax highlighting: TypeScript / JavaScript, Rust, WGSL, CSS, JSON, TOML, shell (One Dark)
- [x] Light / dark theme toggle on the right of the toolbar, remembered in `PlayerPrefs`
  (light code colours: One Light)

## Next

- [x] A jump in the first frame (as `--goto` will do) landed off: `platypus_app` laid out the first
  frame's UI at 1x1 before sizing it to the window. Fixed in `platypus_app`, no workaround here
- [ ] More languages for highlighting: Python, C / C++, C#, GLSL / HLSL, HTML / XML, YAML,
  Markdown, diff, PowerShell (its own rules rather than shell's), SQL
- [ ] Fuzzy search for files in the current folder (and below), keyboard-first (Ctrl+P)
- [ ] Find in page (Ctrl+F) with matches highlighted, next / previous, a match count
- [x] Command-line arguments after `--` (Platypus passes them as `Application.args`, `M5-APP-11`):
  - [x] `--goto "<doc>#<anchor>"`: a doc relative to the root, at a heading id or `#L<line>`
    (the block or list item holding the line); the target is highlighted, then fades. Links in
    the docs take the same anchors
  - [~] `--root <folder>`: parsed, but exits with an error (code 2) for now: reading a folder
    while running needs a decision on Platypus's file access (M6 plans only the game and data
    folders). A folder can be bundled when building instead (`MD_READER_ROOT` /
    `gen-docs.ts --root`)
  - [ ] Ranges: `#L10-L20` highlights the whole range (today it goes to line 10)
- [ ] An "on this page" outline, selectable / copyable text and code
- [ ] Open external links in the system browser (needs an app shell op)
- [ ] GIF images (Platypus decodes PNG, JPEG and SVG today; see the root TODO.md)
- [ ] Scrolling still lays out again on every wheel tick (two passes, ~55 ms a frame in a debug
  build): move the scrolled subtree's rects instead; consider building `web_ui_dom` with some
  optimisation in dev builds
