export type DateFormat = "relative" | "absolute";
export type SearchScope = "all" | "user" | "assistant";

interface Preferences {
  dateFormat: DateFormat;
  defaultSearchScope: SearchScope;
}

const STORAGE_KEY = "claude-sessions-prefs";
const defaults: Preferences = {
  dateFormat: "relative",
  defaultSearchScope: "all",
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

export function toggleDateFormat() {
  preferences.dateFormat =
    preferences.dateFormat === "relative" ? "absolute" : "relative";
  persistPreferences();
}

export function setDateFormat(value: DateFormat) {
  preferences.dateFormat = value;
  persistPreferences();
}

export function setDefaultSearchScope(value: SearchScope) {
  preferences.defaultSearchScope = value;
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
