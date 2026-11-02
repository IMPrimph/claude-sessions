---
name: Evertusk
description: A live departure board for your AI coding agent sessions.
colors:
  bg-app: "#0c0c0d"
  bg-sidebar: "#111112"
  bg-panel: "#111112"
  bg-elevated: "#1b1b1d"
  bg-hover: "#1f1f22"
  bg-selected: "#242427"
  border: "#252528"
  border-strong: "#38383c"
  text-primary: "#ededee"
  text-secondary: "#bcbcc2"
  text-muted: "#94949b"
  text-faint: "#8d8d94"
  accent: "#f59e0b"
  accent-hover: "#fbbf24"
  accent-bright: "#fcd34d"
  accent-text: "#fbbf24"
  accent-soft: "rgba(245, 158, 11, 0.13)"
  on-accent: "#1a1000"
  danger: "#f87171"
  danger-soft: "rgba(248, 113, 113, 0.11)"
  ok: "#4ade80"
  interject-bg: "#241f16"
  bubble-user: "#2a2a2f"
  overlay-scrim: "rgba(0, 0, 0, 0.62)"
  code-bg: "#18181a"
  code-header-bg: "#222225"
  code-text: "#d4d4d8"
  code-muted: "#9a9aa1"
  light-bg-app: "#f4f4f5"
  light-bg-sidebar: "#ebebed"
  light-bg-panel: "#ffffff"
  light-bg-elevated: "#ffffff"
  light-bg-hover: "#e6e6e9"
  light-bg-selected: "#dddde1"
  light-border: "#dfdfe3"
  light-border-strong: "#c6c6cc"
  light-text-primary: "#18181b"
  light-text-secondary: "#4a4a52"
  light-text-muted: "#606068"
  light-text-faint: "#606068"
  light-accent: "#b45f00"
  light-accent-hover: "#954e00"
  light-accent-bright: "#954e00"
  light-accent-text: "#8a4800"
  light-accent-soft: "rgba(180, 95, 0, 0.12)"
  light-on-accent: "#ffffff"
  light-danger: "#b91c1c"
  light-danger-soft: "rgba(185, 28, 28, 0.08)"
  light-ok: "#116a32"
  light-interject-bg: "#fff7e6"
  light-bubble-user: "#e1e1e7"
  bright-bg-app: "#ffffff"
  bright-bg-sidebar: "#ffffff"
  bright-bg-panel: "#ffffff"
  bright-bg-elevated: "#ffffff"
  bright-bg-hover: "#f0f0f0"
  bright-bg-selected: "#e8e8e8"
  bright-border: "#e2e2e2"
  bright-border-strong: "#bdbdbd"
  bright-text-primary: "#000000"
  bright-text-secondary: "#333333"
  bright-text-muted: "#555555"
  bright-text-faint: "#656565"
  bright-accent: "#a85400"
  bright-accent-hover: "#8a4500"
  bright-accent-bright: "#8a4500"
  bright-accent-text: "#7a3d00"
  bright-accent-soft: "rgba(168, 84, 0, 0.11)"
  bright-on-accent: "#ffffff"
  bright-danger: "#b00020"
  bright-danger-soft: "rgba(176, 0, 32, 0.07)"
  bright-ok: "#0f6b33"
  bright-interject-bg: "#fffbeb"
  bright-bubble-user: "#eaeaef"
typography:
  title:
    fontFamily: "-apple-system, BlinkMacSystemFont, SF Pro Text, Helvetica Neue, sans-serif"
    fontSize: "14px"
    fontWeight: 600
  session-title:
    fontFamily: "-apple-system, BlinkMacSystemFont, SF Pro Text, Helvetica Neue, sans-serif"
    fontSize: "14px"
    fontWeight: 500
  body:
    fontFamily: "-apple-system, BlinkMacSystemFont, SF Pro Text, Helvetica Neue, sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.6
  row-title:
    fontFamily: "-apple-system, BlinkMacSystemFont, SF Pro Text, Helvetica Neue, sans-serif"
    fontSize: "13px"
    fontWeight: 500
    lineHeight: 1.35
  control:
    fontFamily: "-apple-system, BlinkMacSystemFont, SF Pro Text, Helvetica Neue, sans-serif"
    fontSize: "12px"
    fontWeight: 400
  meta:
    fontFamily: "-apple-system, BlinkMacSystemFont, SF Pro Text, Helvetica Neue, sans-serif"
    fontSize: "12px"
    fontWeight: 400
    fontFeature: "tnum"
  section-label:
    fontFamily: "-apple-system, BlinkMacSystemFont, SF Pro Text, Helvetica Neue, sans-serif"
    fontSize: "12px"
    fontWeight: 600
  mono:
    fontFamily: "SF Mono, Fira Code, monospace"
    fontSize: "12px"
    fontWeight: 400
    lineHeight: 1.5
rounded:
  mark: "2px"
  chip: "4px"
  control: "5px"
  field: "6px"
  block: "8px"
  dialog: "12px"
  round: "50%"
spacing:
  hairline: "1px"
  gap-tight: "6px"
  gap: "8px"
  row-y: "9px"
  row-x: "14px"
  tab-gap: "14px"
  summary-gap: "10px"
  pane-x: "24px"
  titlebar: "46px"
  rail: "224px"
  board: "384px"
  chat-column: "880px"
components:
  board-row:
    backgroundColor: "transparent"
    textColor: "{colors.text-secondary}"
    typography: "{typography.row-title}"
    padding: "9px 14px"
  board-row-hover:
    backgroundColor: "{colors.bg-hover}"
  board-row-selected:
    backgroundColor: "{colors.bg-selected}"
  rail-row:
    backgroundColor: "transparent"
    textColor: "{colors.text-secondary}"
    rounded: "{rounded.control}"
    padding: "6px 8px"
  rail-row-active:
    backgroundColor: "{colors.bg-selected}"
    textColor: "{colors.text-primary}"
  input-search:
    backgroundColor: "{colors.bg-elevated}"
    textColor: "{colors.text-primary}"
    rounded: "{rounded.field}"
    padding: "7px 9px"
  segment:
    backgroundColor: "{colors.bg-elevated}"
    rounded: "{rounded.field}"
    padding: "2px"
  segment-item:
    backgroundColor: "transparent"
    textColor: "{colors.text-muted}"
    rounded: "{rounded.chip}"
    padding: "4px 6px"
  segment-item-active:
    backgroundColor: "{colors.bg-selected}"
    textColor: "{colors.text-primary}"
  button-header:
    backgroundColor: "transparent"
    textColor: "{colors.text-secondary}"
    typography: "{typography.control}"
    rounded: "{rounded.control}"
    padding: "4px 10px"
    height: "26px"
  button-header-hover:
    backgroundColor: "{colors.bg-hover}"
    textColor: "{colors.text-primary}"
  button-saved:
    backgroundColor: "{colors.accent-soft}"
    textColor: "{colors.accent-text}"
    rounded: "{rounded.control}"
  user-bubble:
    backgroundColor: "{colors.bubble-user}"
    textColor: "{colors.text-primary}"
    typography: "{typography.body}"
    rounded: "14px 14px 4px 14px"
    padding: "10px 14px"
  code-block:
    backgroundColor: "{colors.code-bg}"
    textColor: "{colors.code-text}"
    rounded: "{rounded.block}"
    padding: "14px 16px"
  file-block:
    backgroundColor: "{colors.bg-app}"
    rounded: "{rounded.block}"
---

# Design System: Evertusk

## Overview

**Creative North Star: "The Departure Board"**

Evertusk reads like a board in a quiet terminal: times in a narrow column, one lit lamp for what is running, and rows ruled by hairlines. It sits beside an editor, often at night, so dark is the default and the near-black neutrals stay out of the way. Light and bright themes keep the same roles with new values.

Density is high and deliberate. A title bar sits over three panes: a rail, a board of same-height rows, and a session. The title bar's two toggles hide the rail and the board, so a session can take the whole window. Color carries meaning rather than decoration, and depth comes from small tonal steps and hairlines. Tool activity folds to a single line so prompts and answers read as the conversation.

The signature moment is small: when a running session changes, its time and prompt count turn over into place like a flap, beside a pulsing lamp. Under reduced motion the value simply updates and the lamp holds still.

**Key Characteristics:**
- Neutral near-black scale with one amber and one failure red.
- The system font for everything; tabular numerals wherever numbers line up.
- Hairline-ruled rows on fixed columns.
- Tonal layering; shadows only on floating dialogs.
- Compact 12px to 14px working type. Nothing goes below 12px.
- Your prompts sit in a bubble on the right; the agent's answers run plain on the left.

## Colors

A neutral, almost hueless scale, with amber and red reserved for signal. Tokens without a prefix are the dark default; `light-` and `bright-` tokens fill the same roles in the other two themes.

### Primary
- **Signal Amber** (#f59e0b): the lamp, the active tab underline, focused field borders and the focus ring. Its text form, **Lit Amber** (#fbbf24), colors running and selected times, the saved flag and the saved button. **Amber Wash** (accent-soft) tints search hits and the saved button.

### Secondary
- **Failure Red** (#f87171): failure counts, error events, failed tool runs and destructive hover. Never a decorative emphasis.
- **Passed Green** (#4ade80): small completion marks, added diff lines and the copied state. It stays at glyph and text scale.

### Neutral
- **Terminal Black** (#0c0c0d): the app ground and board.
- **Rail Black** (#111112): rail and panels, one step above the ground.
- **Raised Graphite** (#1b1b1d): fields, segmented controls and the scroll button.
- **Hover Graphite** (#1f1f22) and **Selected Graphite** (#242427): row and control states. Selection is a neutral fill; amber appears only on the row's time.
- **Hairline** (#252528) and **Strong Hairline** (#38383c): row rules and pane edges; header buttons, open file blocks and scrollbar thumbs.
- **Bright Ink** (#ededee), **Soft Ink** (#bcbcc2), **Muted Ink** (#94949b) and **Faint Ink** (#8d8d94): titles and prose; secondary labels; metadata and idle controls; the quietest annotations.
- **Prompt Graphite** (#1f1f22): the user's prompt bubble.
- **Code Graphite** (#18181a, header #222225): code surfaces, which stay dark in every theme.

### Named Rules
**The Three Meanings Rule.** Amber means selection, running and saved, and nothing else. Focus and search hits count as selection. The elephant mark is amber because it is the brand, not because it signals.

**The Red Is Failure Rule.** Red marks something that failed. A count of failures may be red; a merely large number may not.

**The Same Roles Rule.** Themes change values, never roles. Components read theme tokens, so new work holds up in dark, light and bright. Only code surfaces stay constant.

## Typography

**Font:** -apple-system (with BlinkMacSystemFont, SF Pro Text, Helvetica Neue, sans-serif), for everything but code
**Mono Font:** SF Mono (with Fira Code, monospace)

**Character:** The system face reads as native and calm for long transcripts. There is no display face and nothing is bundled. Numbers that line up use tabular figures in the same font, and weight carries emphasis.

### Hierarchy
- **Title** (600, 14px): pane headings and the brand name; empty-state headings step up to 15px.
- **Session title** (500, 14px): the open session's name, in the title bar.
- **Body** (400, 14px, 1.6): assistant prose. User prompts use 14px at 1.5.
- **Row title** (500, 13px, 1.35): session titles on the board. Rail rows and tabs also use 13px, at regular weight.
- **Control** (400, 12px): header buttons, segmented controls and session facts. Search fields and fold summaries use 12.5px.
- **Meta** (400, 12px, tabular where numeric): board times, tool name, prompt counts, tab counts, and the session totals, whose numbers step up to 600.
- **Section label** (600, 12px, sentence case): date groups and the Projects heading, with a regular-weight tabular count beside the date.
- **Mono** (400, 12px, 1.5): paths, shell output and tool summaries. Code inside rendered Markdown uses 13px.

### Named Rules
**The 12px Floor Rule.** No text goes below 12px, in any theme or at any window size.

**The One Face Rule.** Don't add a display or condensed face for numerals. Totals stand out through weight and colour, and they line up through tabular figures.

## Layout

A 46px title bar runs across the top, carrying the projects toggle (⌘1), the sessions toggle (⌘2) and the open session's name. Below it, three panes fill the window: a 224px rail, a 384px board, and a session pane that takes the rest. With both side panels hidden, the chat gets the whole window, and the search bar and summary stay out of the way until ⌘F asks for them. There is no page scroll; each pane scrolls on its own. The conversation sits in a centred column up to 880px wide. Board rows use fixed columns of 72px, 48px and a flexible remainder with a 6px gutter, so the eye runs straight down time and tool. Titles clamp at two lines. In a row's meta line the project name truncates first, so numbers and flags stay whole.

Spacing is tight and pragmatic rather than a strict scale: 14px insets on the board, 14px between tabs and 10px between summary facts.

Windows open at 1200×800 with an 800×600 minimum. At 1000px wide and below, the rail narrows to 190px and the board and session share one slot, with a back button in the title bar replacing the sessions toggle. At 700px tall and below, the session header tightens its summary and tab margins.

### Named Rules
**The Straight Column Rule.** Anything repeated on the board shares a column edge. A new datum goes in the meta line, not a new column that breaks the run.

## Elevation & Depth

The working surface is flat. Depth is tonal, stepping from ground to rail to raised graphite to hover and selected fills, with hairlines at pane and row edges. Sticky date labels and file headers rely on opaque fills rather than shadow. Shadows appear only on dialogs floating above a dimmed window.

### Shadow Vocabulary
- **Dialog** (`box-shadow: 0 20px 60px rgba(0, 0, 0, 0.6)`): every dialog (native `<dialog>`, shown as a modal so focus and Tab stay inside).
- **Tooltip** (`box-shadow: 0 4px 14px rgba(0, 0, 0, 0.3)`): the title bar's toggle tips, which float over the panes.
- **Update banner** (`box-shadow: 0 10px 40px rgba(0, 0, 0, 0.3)`): floats at the top of the window.

### Named Rules
**The Flat Board Rule.** Nothing resting in the three panes casts a shadow. If something needs lifting, step its tone.

## Shapes

Corners are small and quiet, and they grow with the container: chips at 4px, compact controls at 5px, the rail search and segmented control at 6px, content blocks at 8px, and dialogs at 12px. The prompt bubble is the one exception: 14px with a 4px corner at bottom right, pointing at its sender. Board rows have no radius because hairlines run edge to edge; rail rows are rounded because they float inside the rail. Circles are reserved for the lamp and the scroll button. Lines are 1px throughout; the only heavier stroke is the 2px amber underline on the active tab, plus the 2px focus ring. Disclosure uses one drawn chevron, rotated when open, in place of the native triangle.

## Components

Icons throughout are inline 15px line SVGs with a 2px stroke drawn in `currentColor`.

### Buttons
Quiet outlined controls, with no filled button in the working panes.
- **Shape:** gently rounded (5px), minimum 26px tall.
- **Header action:** transparent with a strong hairline and soft ink at 12px.
- **Hover / Focus:** hover fills with hover graphite and brightens the text; focus shows the global 2px amber ring at 1px offset.
- **Saved:** amber wash, amber border and lit amber text. **New activity:** amber border and text with a lamp.
- **Round scroll button:** raised graphite circle with a strong hairline.

### Chips
- **Filter chip:** transparent, 4px radius, 12px muted text. Active fills with selected graphite and bright ink, never amber.
- **Inline code:** a tint of its theme (`--inline-code-bg`, `--inline-code-text`), 4px radius. Only code blocks keep the dark surface.

### Cards / Containers
- **Corner Style:** 8px on code blocks and file blocks; 14px with a 4px corner at bottom right on prompt bubbles.
- **Background:** prompt graphite for prompts, code graphite for code, ground with a panel-colored header for files.
- **Shadow Strategy:** none; see Elevation & Depth.
- **Border:** hairlines, strengthening when a file block opens.
- **Internal Padding:** 10px 14px on prompts, 14px 16px on code, 9px 12px on file rows.
- **Prompt bubble:** sits on the right, sized to its text up to 82% of the column, with a strong hairline. The agent's answers have no container and run on the left.

### Inputs / Fields
- **Style:** raised graphite with a hairline, 5px to 6px radius, muted placeholder, 12.5px to 13px text.
- **Focus:** the border turns signal amber.
- **Error / Disabled:** errors appear as red text beneath the control; disabled buttons drop to 60% opacity.

### Navigation
- **Rail rows:** 13px soft ink, 6px 8px, 5px radius. Hover uses hover graphite; active uses selected graphite with bright ink. Each ends in a right-aligned tabular count, and a missing project dims to muted ink.
- **Tool filter:** a three-part segmented control on raised graphite, with a selected graphite thumb.
- **Title bar toggles:** 30px icon buttons with a tooltip naming the shortcut. Hidden panels dim the icon.
- **Tabs:** 13px muted text with 14px gaps; the active tab uses bright ink at 500 with a 2px amber underline that sits on the header's hairline.

### Board Row
The signature component. A time, a tool name and a body sit on fixed columns, with a hairline beneath. Hover and selection change the fill; selection also turns the time lit amber. Running rows replace the time with an amber lamp (6px, opacity pulsing on a 1.6s cycle) and lit amber text. Changed values turn over with a 320ms flap on `cubic-bezier(0.16, 1, 0.3, 1)`. Both animations stop under reduced motion. The saved flag is the only other amber on a row.

### Summary Strip
Session totals in the header, each a 600-weight tabular number in bright ink followed by a muted label, all at 12px on one line. Failure facts turn red, come first and link to the failure or to Stats.

### Tool Activity Fold
A single muted 12.5px line with a chevron that summarizes a run of tool calls. Hover fills and brightens it, failures add a red count, and opening it reveals the raw record. This is how transcripts stay readable without hiding the proof.

## Do's and Don'ts

### Do:
- **Do** read color from theme tokens, so every new surface works in dark, light and bright.
- **Do** keep amber to selection, running and saved, including focus and search hits as selection.
- **Do** set times and counts in the system font with tabular numerals, at 12px or above.
- **Do** rule board rows with a 1px hairline on the fixed 72px, 48px and flexible columns.
- **Do** show state with a neutral fill first: hover graphite, then selected graphite.
- **Do** fold tool activity to one line and keep the raw record one step away.
- **Do** pair animation with a plain update under `prefers-reduced-motion`.
- **Do** keep code surfaces dark in every theme.

### Don't:
- **Don't** use amber for links, callouts, decoration or emphasis.
- **Don't** use red for anything that hasn't failed.
- **Don't** add a display or condensed face, or bundle a font.
- **Don't** set any text below 12px.
- **Don't** add shadows to anything resting in the three panes.
- **Don't** convey tool or status by color alone; pair it with a label, count or lamp.
- **Don't** add a new accent hue to distinguish a block or tool.
- **Don't** add a board column or let rows vary in height outside search results and bookmarks.

## Loading

Nothing in the app sits blank while it works. `.progress` (`src/theme.css`) is the one working signal: a 2px amber line that moves, and stands still under reduced motion. It always comes with words saying what's happening. Rows use skeletons in the board's own columns, so nothing jumps when sessions arrive. An area never claims "no agents" or "nothing changed" until the read has finished.
