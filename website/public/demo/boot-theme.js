// Runs before anything paints, so the startup screen is already in the chosen theme.
try {
  const saved = JSON.parse(localStorage.getItem("claude-sessions-prefs") || "{}");
  if (saved.theme) document.documentElement.dataset.theme = saved.theme;
} catch {
  // The default dark theme is fine.
}
