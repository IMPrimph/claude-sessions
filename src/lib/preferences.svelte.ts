export type DateFormat = "relative" | "absolute";
export type SearchScope = "all" | "user" | "assistant";
export type Theme = "dark" | "light" | "bright";

export const THEMES: { value: Theme; label: string; description: string }[] = [
  { value: "dark", label: "Dark", description: "Neutral charcoal — easy on the eyes" },
  { value: "light", label: "Light", description: "Soft off-white background" },
  { value: "bright", label: "Bright", description: "Pure white, maximum contrast" },
];

interface Preferences {
  dateFormat: DateFormat;
  // Which side panels are hidden. Both hidden is the full-screen chat.
  railHidden: boolean;
  listHidden: boolean;
  theme: Theme;
  // Whether the user has seen the first-launch explainer about Claude Code's
  // 30-day session auto-delete and how saving protects against it.
  hasSeenWelcome: boolean;
}

const STORAGE_KEY = "claude-sessions-prefs";
const defaults: Preferences = {
  dateFormat: "relative",
  railHidden: false,
  listHidden: false,
  theme: "dark",
  hasSeenWelcome: false,
};

function load(): Preferences {
  if (typeof localStorage === "undefined") return { ...defaults };
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { ...defaults };
    return { ...defaults, ...JSON.parse(raw) };
  } catch {
    return { ...defaults };
  }
}

export const preferences = $state<Preferences>(load());

// Reflect the chosen theme onto <html data-theme> so the CSS token overrides apply.
export function applyTheme(theme: Theme) {
  if (typeof document !== "undefined") {
    document.documentElement.dataset.theme = theme;
  }
}

// Apply immediately on load so there's no flash of the default theme.
applyTheme(preferences.theme);

export function setTheme(value: Theme) {
  preferences.theme = value;
  applyTheme(value);
  persistPreferences();
}

export function setDateFormat(value: DateFormat) {
  preferences.dateFormat = value;
  persistPreferences();
}

export function setPanels(railHidden: boolean, listHidden: boolean) {
  preferences.railHidden = railHidden;
  preferences.listHidden = listHidden;
  persistPreferences();
}

export function dismissWelcome() {
  preferences.hasSeenWelcome = true;
  persistPreferences();
}

export function persistPreferences() {
  if (typeof localStorage === "undefined") return;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(preferences));
  } catch {
    // localStorage can throw in private mode — preferences just won't persist
  }
}
