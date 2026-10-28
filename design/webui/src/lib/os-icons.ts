// Operating system glyphs for machine tiles, on the Zerus 24 px grid, drawn as
// outlines with the family's 1.6 stroke and round caps and joins (no fill).
//
// macos and windows are the outline paths of Tabler Icons 3.50.0 `brand-apple`
// and `brand-windows` (MIT, copyright Paweł Kuna; licenses/tabler-icons-LICENSE.txt).
// linux is original Zerus artwork: a plain penguin outline with eyes, beak, belly
// and feet. These are monochrome identification glyphs, not colour vendor logos.

export type OsKind = "linux" | "macos" | "windows";

export const OS_GLYPH: Record<OsKind, string> = {
  macos: `<path d="M8.286 7.008c-3.216 0 -4.286 3.23 -4.286 5.92c0 3.229 2.143 8.072 4.286 8.072c1.165 -.05 1.799 -.538 3.214 -.538c1.406 0 1.607 .538 3.214 .538s4.286 -3.229 4.286 -5.381c-.03 -.011 -2.649 -.434 -2.679 -3.23c-.02 -2.335 2.589 -3.179 2.679 -3.228c-1.096 -1.606 -3.162 -2.113 -3.75 -2.153c-1.535 -.12 -3.032 1.077 -3.75 1.077c-.729 0 -2.036 -1.077 -3.214 -1.077"/><path d="M12 4a2 2 0 0 0 2 -2a2 2 0 0 0 -2 2"/>`,
  windows: `<path d="M17.8 20l-12 -1.5c-1 -.1 -1.8 -.9 -1.8 -1.9v-9.2c0 -1 .8 -1.8 1.8 -1.9l12 -1.5c1.2 -.1 2.2 .8 2.2 1.9v12.1c0 1.2 -1.1 2.1 -2.2 1.9l0 .1"/><path d="M12 5l0 14"/><path d="M4 12l16 0"/>`,
  linux: `<path d="M12 2.5C9.8 2.5 8.7 4.2 8.7 6.6C8.7 8.2 8.2 9.2 7.2 10.6C6 12.3 5.2 14.2 5.6 16.2C5.9 17.8 7.2 19 9 19H15C16.8 19 18.1 17.8 18.4 16.2C18.8 14.2 18 12.3 16.8 10.6C15.8 9.2 15.3 8.2 15.3 6.6C15.3 4.2 14.2 2.5 12 2.5Z"/><path d="M9.6 11.8C8.9 13 8.6 14.3 8.9 15.5C9.2 16.6 10.4 17.2 12 17.2S14.8 16.6 15.1 15.5C15.4 14.3 15.1 13 14.4 11.8"/><path d="M10.6 6.3V6.31M13.4 6.3V6.31"/><path d="M10.8 8.4L12 9.5L13.2 8.4"/><path d="M8.4 19C7 19.5 6.2 20.3 6.6 20.9C7 21.4 8.7 21.2 10.3 20.5M15.6 19C17 19.5 17.8 20.3 17.4 20.9C17 21.4 15.3 21.2 13.7 20.5"/>`,
};

export const OS_NAME: Record<OsKind, string> = { linux: "Linux", macos: "macOS", windows: "Windows" };
