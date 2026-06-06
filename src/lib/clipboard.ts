import { writeText as tauriWriteText } from "@tauri-apps/plugin-clipboard-manager";

// Copy via the native Tauri clipboard, falling back to the webview API.
// navigator.clipboard.writeText can truncate/silently fail on very long strings
// inside WKWebView, which is what cuts off copies of long messages.
export async function copyToClipboard(text: string): Promise<void> {
  try {
    await tauriWriteText(text);
  } catch {
    await navigator.clipboard.writeText(text);
  }
}
