import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async ({ mode }) => ({
  plugins: [
    svelte(),
    {
      // The website's demo: its own title, and kept out of search results.
      name: "demo-page",
      transformIndexHtml: (html: string) =>
        mode === "demo"
          ? html.replace(/<title>.*<\/title>/, "<title>Evertusk demo</title>").replace("</head>", '  <meta name="robots" content="noindex" />\n  </head>')
          : html,
    },
  ],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));
