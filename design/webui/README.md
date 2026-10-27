# Web client design mockup

A clickable design for the planned Tauri 2 desktop client (epic `zerus-5wmh`),
with synthetic data only. It is a design artifact for owner review, not a runtime
dependency. The architecture is in
`docs/superpowers/specs/2026-10-27-tauri-web-ui-design.md`.

The mockup uses the stack proposed for the real client (SolidJS 1.9, TypeScript,
Vite), so approved components can move into the client package.

```sh
npm ci --ignore-scripts
npx vite            # dev server
npx tsc --noEmit    # type check
npm run skins:check # validate, resolve and compile the built-in skins
./scripts/publish.sh <folder>   # type check, build and copy for review
```

- `index.html`: the review page for the owner (Russian), with the Qt parity
  checklist (`review/parity.ts`).
- `app.html`: the mockup. URL parameters open a given state, for example
  `?page=settings`, `?dialog=new-session`, `?itab=tasks&tab=taskview`,
  `?select=mac:1`, `?filter=archived`, `?strip=1`, `?q=pagination`,
  `?demo=empty|loading|error`, `?skin=zerus-dark|zerus-light|fjord|paper`,
  `?logo=spin|turbine|qt` (Zerus mark motion) and `#item-<id>` (scroll
  Activity to an item, for example `#item-a3c` for the voice note and
  `#item-a3d` for the round video note).
- `skins/*.json`: skin format v1 (tokens, component variants and layout
  variants; no CSS). `src/skin/skin.ts` validates, resolves `extends` and
  compiles a skin to CSS custom properties.
- `src/lib/qt-icons.ts` holds Zerus's own glyphs ported from
  `tray/src/WorkspaceIcons.h`; `<Icon>` prefers them and falls back to Lucide.
  `scripts/gen-icons.ts` regenerates `src/lib/icons.generated.ts` from the
  installed `lucide-solid` (ISC).

Session list geometry follows `tray/src/SessionCardDelegate.h` at the compact
density (the only one), and the panel modes follow `SessionPanelDock.h`. Two
deliberate departures from Qt, both owner requests: the emphasis edge is 4 px
and sits closer to the card's top and bottom, and switching a card to its
compact row morphs in 220 ms instead of swapping. The composer height (76-720),
side panel width (default 400, at least 240), list width and Activity width are
remembered in browser storage. The
dashed MOCK UP button in the rail, its popover and the dashed outcome switches in
some dialogs are review aids, not product UI.
