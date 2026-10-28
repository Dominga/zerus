// The Zerus icon family, ported from the Qt GUI so both clients draw the same glyphs.
//
// Source of truth: tray/src/WorkspaceIcons.h `workspaceIcon(name, color)`, which paints
// every glyph with QPainter on a 24x24 grid with a 1.6 pen, round caps and joins and no
// fill. Translation rules: drawLine -> M/L, drawRoundedRect -> <rect rx>, drawEllipse ->
// <circle>, drawArc -> an SVG arc (Qt angles run counter-clockwise from 3 o'clock with
// y down, so a positive Qt span is sweep-flag 0), drawPoint -> a zero-length round-cap
// line, translate/rotate -> transform. Consecutive lines that share an end point are
// merged into one polyline; with round caps and joins the painted region is identical.
// `github` and `zerus` reuse tray/resources/icons/github-mark.svg and
// hgs-zerus-symbolic.svg, filled with the icon colour like the Qt SourceIn fill.
//
// A few small glyphs painted outside WorkspaceIcons.h are ported as named entries on
// their own grid (`view`): tray/src/speech/VoicePlayerImage.h (voice-transcript*, 32;
// delivery-*, 18), tray/src/SessionCardDelegate.h children counter (agents*, 16 and 8)
// and tray/src/SessionStatusBadge.h (status-*, 12, centred at 6,6).
// Agent glyphs (codex, claude, kimi, opencode, dsh) live in icons.tsx.

export interface QtGlyph {
  /** SVG markup inside a viewBox of `0 0 view view`. */
  body: string;
  /** Pen width in grid units; 1.6 when omitted. */
  stroke?: number;
  /** A filled shape without a stroke (the SVG resources). */
  fill?: boolean;
  /** Grid size; 24 when omitted. */
  view?: number;
}

export const ZERUS_PETAL =
  "M39 9 C54 3 70 13 77 29 C72 25 69 24 72 32 C81 44 77 58 65 65 C70 51 65 36 52 28 C40 21 30 25 22 34 C25 21 38 15 51 19 C57 21 56 16 51 13 C47 11 43 10 39 9 Z";

const GITHUB_MARK =
  "M6.766 11.328c-2.063-.25-3.516-1.734-3.516-3.656 0-.781.281-1.625.75-2.188-.203-.515-.172-1.609.063-2.062.625-.078 1.468.25 1.968.703.594-.187 1.219-.281 1.985-.281.765 0 1.39.094 1.953.265.484-.437 1.344-.765 1.969-.687.218.422.25 1.515.046 2.047.5.593.766 1.39.766 2.203 0 1.922-1.453 3.375-3.547 3.64.531.344.89 1.094.89 1.954v1.625c0 .468.391.734.86.547C13.781 14.359 16 11.53 16 8.03 16 3.61 12.406 0 7.984 0 3.563 0 0 3.61 0 8.031a7.88 7.88 0 0 0 5.172 7.422c.422.156.828-.125.828-.547v-1.25c-.219.094-.5.156-.75.156-1.031 0-1.64-.562-2.078-1.609-.172-.422-.36-.672-.719-.719-.187-.015-.25-.093-.25-.187 0-.188.313-.328.625-.328.453 0 .844.281 1.25.86.313.452.64.655 1.031.655s.641-.14 1-.5c.266-.265.47-.5.657-.656";

/** drawPoint with a round-cap pen: a dot as wide as the pen. */
const dot = (x: number, y: number) => `M${x} ${y}h.01`;

const r3 = (v: number) => String(Math.round(v * 1000) / 1000);

/** settings: for a in 0..315 step 45: translate(12,12) rotate(a) drawLine(0,7,0,9). */
const SETTINGS_TEETH = Array.from({ length: 8 }, (_, i) => {
  const a = (i * 45 * Math.PI) / 180;
  const at = (r: number) => `${r3(12 - r * Math.sin(a))} ${r3(12 + r * Math.cos(a))}`;
  return `M${at(7)}L${at(9)}`;
}).join("");

const FOLDER = `<path d="M3 8V5H10L12 8H21V19H3Z"/>`;
const PIN_HEAD = "M8 3H16L15 9L19 13H5L9 9Z";
const CLOUD = `<path d="M6 19C0 19 0 10 6 10C5 2 17 1 19 9C25 9 25 19 19 19Z"/>`;
const QUESTION_MARK = `<path d="M10 10C10 7 15 7 15 10C15 12 12 11 12 14${dot(12, 17)}"/>`;
const BELL = `<path d="M6 10C6 3 18 3 18 10V15L20 18H4L6 15Z"/><path d="M10 20A2 2 0 0 0 14 20"/>`;
const CHAINS = `<path d="M10 7H7C1 7 1 17 7 17H10M14 7H17C23 7 23 17 17 17H14"/>`;
const GIT_TRAY = `<path d="M4 15V19Q4 21 6 21H18Q20 21 20 19V15"/>`;
const DELIVERY_CIRCLE = `<circle cx="9" cy="9" r="6.5"/>`;

const SESSIONS = `<rect x="4" y="4" width="16" height="7" rx="2"/><rect x="4" y="14" width="16" height="6" rx="2"/><path d="${dot(8, 7.5)}M12 7.5H16"/>`;
const PAUSE = `<path d="M8 5V19M16 5V19"/>`;
const BOLT = `<path d="M13 3L5 13H11L10 21L19 10H13Z"/>`;

export const QT_ICONS: Record<string, QtGlyph> = {
  sessions: { body: SESSIONS },
  all: { body: SESSIONS },
  calendar: {
    body: `<rect x="4" y="5" width="16" height="16" rx="2"/><path d="M4 10H20M8 3V7M16 3V7${dot(8, 14)}${dot(12, 14)}${dot(16, 14)}"/>`,
  },
  "chevron-left": { body: `<path d="M15 6L9 12L15 18"/>` },
  "chevron-right": { body: `<path d="M9 6L15 12L9 18"/>` },
  machines: { body: `<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M12 16V20M8 20H16"/>` },
  terminal: { body: `<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9L10 12L7 15M13 15H17"/>` },
  folder: { body: FOLDER },
  projects: { body: FOLDER },
  group: { body: `${FOLDER}<path d="M9 13H15M12 10V16"/>` },
  "folder-move": { body: `<path d="M3 8V5H10L12 8H21V20H3V17"/><path d="M2 13H13M9 9L13 13L9 17"/>` },
  pin: { body: `<path d="${PIN_HEAD}"/><path d="M12 13V21"/>` },
  // p.translate(12,12); p.rotate(-35); p.translate(-12,-12) before the whole glyph.
  unpinned: { body: `<g transform="rotate(-35 12 12)"><path d="${PIN_HEAD}"/><path d="M12 13V21"/></g>` },
  // The brush fills only the head; lines never take a fill.
  pinned: { body: `<path d="${PIN_HEAD}" fill="currentColor"/><path d="M12 13V21M8 19H16"/>` },
  "git-branch": {
    body: `<path d="M6 7.5V16.5M18 7.5V10C18 15 6 11 6 16.5"/><circle cx="6" cy="5" r="2.5"/><circle cx="6" cy="19" r="2.5"/><circle cx="18" cy="5" r="2.5"/>`,
  },
  "git-diff": { body: `<path d="M13 3H5V21H19V9Z"/><path d="M13 3V9H19M8 11H12M10 9V13M8 17H14"/>` },
  "git-push": { body: `${GIT_TRAY}<path d="M12 3V16M7 8L12 3L17 8"/>` },
  "git-pull": { body: `${GIT_TRAY}<path d="M12 3V16M7 11L12 16L17 11"/>` },
  "git-synced": { body: `${CLOUD}<path d="M8 13L11 16L16 11"/>` },
  "git-unknown": { body: `${CLOUD}${QUESTION_MARK}` },
  accounts: {
    body: `<circle cx="10" cy="7" r="3.5"/><path d="M3 20C3 11 17 11 17 20"/><circle cx="18" cy="12" r="2"/><path d="M18 14V20M18 18H21"/>`,
  },
  settings: { body: `<circle cx="12" cy="12" r="6"/><circle cx="12" cy="12" r="2.5"/><path d="${SETTINGS_TEETH}"/>` },
  "context-warning": { body: `<circle cx="12" cy="12" r="9"/><path d="M12 7V12${dot(12, 16)}"/>` },
  question: { body: `<circle cx="12" cy="12" r="9"/>${QUESTION_MARK}` },
  // drawArc(QRectF(10,18,4,4), 180*16, 180*16): the lower half, a clapper under the rim.
  attention: { body: BELL },
  mute: { body: `${BELL}<path d="M3 3L21 21"/>` },
  working: { body: BOLT },
  active: { body: BOLT },
  paused: { body: PAUSE },
  pause: { body: PAUSE },
  play: { body: `<path d="M8 5L19 12L8 19Z"/>` },
  archived: { body: `<rect x="3" y="4" width="18" height="4"/><rect x="5" y="8" width="14" height="12"/><path d="M10 12H14"/>` },
  trash: { body: `<path d="M4 6H20M9 3H15M6 6L7 21H17L18 6M10 10V17M14 10V17"/>` },
  // "stop" appears twice in the Qt chain; the first branch (an outlined square) wins.
  stop: { body: `<rect x="5" y="5" width="14" height="14" rx="2"/>` },
  "read-all": { body: `<path d="M3 12L7 16L16 7M11 15L13 17L22 8"/>` },
  add: { body: `<path d="M12 5V19M5 12H19"/>` },
  // drawArc(QRectF(5,5,14,14), 40*16, 290*16): counter-clockwise from 40 to 330 degrees.
  refresh: { body: `<path d="M17.3623 7.5005A7 7 0 1 0 18.0622 15.5"/><path d="M19 4V9H14"/>` },
  // drawArc(QRectF(5,5,14,14), 140*16, -290*16): clockwise from 140 to 210 degrees.
  reset: { body: `<path d="M6.6377 7.5005A7 7 0 1 1 5.9378 15.5"/><path d="M5 4V9H10"/>` },
  // An eraser on its baseline: translate(12,11), rotate(-45).
  clear: {
    body: `<g transform="translate(12 11) rotate(-45)"><rect x="-7" y="-4" width="14" height="8" rx="1.5"/><path d="M-1 -4V4"/></g><path d="M10 21H20"/>`,
  },
  connect: { body: `${CHAINS}<path d="M8 12H16"/>` },
  disconnect: { body: `${CHAINS}<path d="M11 3L13 6M11 18L13 21"/>` },
  video: { body: `<rect x="3" y="5" width="11" height="14" rx="2"/><path d="M14 9L21 5V19L14 15"/>` },
  // drawArc(QRectF(6,8,12,11), 180*16, 180*16): the lower half of the stand.
  microphone: {
    body: `<rect x="9" y="3" width="6" height="12" rx="3"/><path d="M6 13.5A6 5.5 0 0 0 18 13.5"/><path d="M12 19V22M8 22H16"/>`,
  },
  attachment: { body: `<path d="M9 16L16 9C19 6 15 2 12 5L5 12C-1 18 7 26 13 20L20 13C24 9 18 3 14 7L7 14"/>` },
  inspector: { body: `<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M14 4V20M17 8H18M17 12H18"/>` },
  "collapse-sessions": { body: `<path d="M4 4V20M19 12H9M14 7L9 12L14 17"/>` },
  "expand-sessions": { body: `<path d="M4 4V20M9 12H19M14 7L19 12L14 17"/>` },
  "expand-row": { body: `<path d="M5 4H19M5 20H19M12 8V16M8 11L12 7L16 11M8 13L12 17L16 13"/>` },
  "compact-row": { body: `<path d="M5 4H19M5 20H19M12 5V10M8 7L12 11L16 7M12 14V19M8 17L12 13L16 17"/>` },
  search: { body: `<circle cx="10.5" cy="10.5" r="6"/><path d="M15 15L20 20"/>` },
  "collapse-panel": { body: `<path d="M19 5V19M5 12H14M10 8L14 12L10 16"/>` },
  close: { body: `<path d="M6 6L18 18M6 18L18 6"/>` },
  copy: { body: `<rect x="8" y="8" width="12" height="13" rx="2"/><path d="M4 16V3H16"/>` },
  drafts: {
    body: `<path d="M11 21H4V3H13L18 8V10"/><path d="M13 3V8H18M7 11H12M7 15H9"/><path d="M12 17L19 10L22 13L15 20L11 21Z"/><path d="M17 12L20 15"/>`,
  },
  external: { body: `<path d="M12 4H20V12M20 4L10 14"/><path d="M8 5H4V20H19V16"/>` },
  // Composer actions sit beside semibold text; Qt draws them with a 2.2 pen.
  send: { body: `<path d="M12 19V5M6 11L12 5L18 11"/>`, stroke: 2.2 },
  "chevron-down": { body: `<path d="M6 9L12 15L18 9"/>`, stroke: 2.2 },
  "chevrons-down": { body: `<path d="M6 6L12 12L18 6M6 12L12 18L18 12"/>`, stroke: 2.2 },
  more: { body: `<path d="${dot(5, 12)}${dot(12, 12)}${dot(19, 12)}"/>` },
  // Project tasks (TasksView): a stage's icon is the icon of the action that moves a task there.
  "task-inbox": { body: `<path d="M4 13.5L6.5 6H17.5L20 13.5V19H4Z"/><path d="M4 13.5H8.5L10 16H14L15.5 13.5H20"/>` },
  "task-list": { body: `<path d="M4 6H20M4 11H20M4 16H14"/>` },
  "task-list-add": { body: `<path d="M4 6H16M4 11H16M4 16H11M17.5 14V21M14 17.5H21"/>` },
  "task-clock": { body: `<circle cx="12" cy="12" r="8.5"/><path d="M12 7.5V12L15 14"/>` },
  "task-check": { body: `<path d="M5 12L10 17L19 7"/>`, stroke: 2 },
  "task-funnel": { body: `<path d="M4 5H20L14 12.5V19L10 17V12.5Z"/>` },
  "task-edit": { body: `<path d="M4 20H8L19 9L15 5L4 16Z"/><path d="M13 7L17 11"/>` },
  "task-top": { body: `<path d="M7 12L12 7L17 12M7 18L12 13L17 18"/>` },
  // SVG resources, drawn to the full icon square and filled with the icon colour.
  zerus: {
    body: `<g transform="translate(50 50) scale(1.06) translate(-50 -50)">${[0, 120, 240]
      .map((d) => `<path transform="${d ? `rotate(${d} 50 50) ` : ""}translate(56 30) scale(.88) translate(-56 -30)" d="${ZERUS_PETAL}"/>`)
      .join("")}</g>`,
    fill: true,
    view: 100,
  },
  github: { body: `<path d="${GITHUB_MARK}"/>`, fill: true, view: 16 },

  // tray/src/speech/VoicePlayerImage.h transcriptIcon: 32x32, 1.6 pen. The accent
  // colour and the rounded 32px background (radius 6) belong to CSS.
  "voice-transcript": { body: `<path d="M10 8V19M6 15L10 19L14 15M16 24L20.5 12L25 24M17.5 20H23.5"/>`, view: 32 },
  "voice-transcript-open": { body: `<path d="M10 19L16 13L22 19"/>`, view: 32 },
  // VoicePlayerImage.h deliveryIcon: 18x18, 1.5 pen. Submitted is a check, cancelled
  // a cross, failed/uncertain/unknown states an issue (danger tone in CSS), and
  // uploading/queued/transcribing/ready/waiting_agent/delivering a pending clock.
  "delivery-submitted": { body: `<path d="M3 9L7 13L15 5"/>`, view: 18, stroke: 1.5 },
  "delivery-cancelled": { body: `<path d="M5 5L13 13M13 5L5 13"/>`, view: 18, stroke: 1.5 },
  "delivery-pending": { body: `${DELIVERY_CIRCLE}<path d="M9 5V9L12 10"/>`, view: 18, stroke: 1.5 },
  "delivery-issue": { body: `${DELIVERY_CIRCLE}<path d="M9 5V10${dot(9, 13)}"/>`, view: 18, stroke: 1.5 },

  // SessionCardDelegate.h children counter: two people drawn around (cx, cy) =
  // (counter.x + 10, counter centre) with QPen(ink, 1.2), i.e. square caps. On this
  // 16x16 grid cx = 5.5 and cy = 8.75, which centres the glyph's painted bounds.
  agents: {
    body: `<g stroke-linecap="square" stroke-linejoin="bevel"><circle cx="5.5" cy="5.75" r="2"/><circle cx="11.5" cy="6.75" r="2"/><path d="M9.5 11.75A4 3 0 0 0 1.5 11.75M14.5 12.25A3.5 2.5 0 0 0 7.5 12.25"/></g>`,
    view: 16,
    stroke: 1.2,
  },
  // The expandable counter's chevron: centre c = (counter.right - 9, cy) at (4,4) of
  // an 8x8 grid, pen 1.4. Collapsed points down (expand), expanded points up.
  "agents-expand": { body: `<path d="M1 3L4 6L7 3"/>`, view: 8, stroke: 1.4 },
  "agents-collapse": { body: `<path d="M1 5L4 2L7 5"/>`, view: 8, stroke: 1.4 },

  // SessionStatusBadge.h paint: 12x12 indicator grid centred at (6,6). The working
  // and background dots, their halo and the "!" of attention/error stay in CSS.
  "status-paused": { body: `<path d="M4 3V9M8 3V9"/>`, view: 12, stroke: 1.5 },
  "status-draft": { body: `<path d="M3 9L8.5 3.5M7.5 2.5L9.5 4.5M3 9L2.5 9.5"/>`, view: 12, stroke: 1.4 },
  "status-scheduled": { body: `<circle cx="6" cy="6" r="4.5"/><path d="M6 6V3.2M6 6H8.2"/>`, view: 12, stroke: 1.3 },
  // QPen(foreground, 1): square caps and bevel joins.
  "status-unread": {
    body: `<g stroke-linecap="square" stroke-linejoin="bevel"><rect x="2" y="3" width="8" height="6" rx="1"/><path d="M2 3L6 6M6 6L10 3"/></g>`,
    view: 12,
    stroke: 1,
  },
};
