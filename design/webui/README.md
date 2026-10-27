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
./scripts/publish.sh <folder>   # type check, build and copy for review
```

- `index.html`: the review page for the owner (Russian), with the Qt parity
  checklist (`review/parity.ts`).
- `app.html`: the mockup. URL parameters open a given state, for example
  `?page=settings`, `?dialog=new-session`, `?itab=tasks&tab=taskview`,
  `?select=mac:1`, `?filter=archived`, `?strip=1`, `?q=pagination`,
  `?demo=empty|loading|error` and `?skin=zerus-dark|zerus-light|fjord|paper`.
- `skins/*.json`: skin format v1 (tokens, component variants and layout
  variants; no CSS). `src/skin/skin.ts` validates, resolves `extends` and
  compiles a skin to CSS custom properties.
- `scripts/gen-icons.ts` regenerates `src/lib/icons.generated.ts` from the
  installed `lucide-solid` (ISC).

Session list geometry follows `tray/src/SessionCardDelegate.h`. The dashed
MOCKUP toolbar and the dashed outcome switches in some dialogs are review aids,
not product UI.
