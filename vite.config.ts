import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],

  build: {
    rolldownOptions: {
      output: {
        manualChunks(id) {
          if (!id.includes("node_modules")) return undefined;
          if (id.includes("/react/") || id.includes("/react-dom/")) return "vendor-react";
          if (id.includes("/@xterm/xterm/")) return "vendor-xterm";
          if (id.includes("/@xterm/addon-")) return "vendor-xterm-addons";
          if (id.includes("/@uiw/react-codemirror") || id.includes("/@uiw/codemirror-theme-")) {
            return "vendor-editor-ui";
          }
          if (id.includes("/@codemirror/view/")) return "vendor-codemirror-view";
          if (id.includes("/@codemirror/state/")) return "vendor-codemirror-state";
          if (id.includes("/@codemirror/commands/")) return "vendor-codemirror-commands";
          if (id.includes("/@codemirror/language/")) return "vendor-codemirror-language";
          if (
            id.includes("/@lezer/common/") ||
            id.includes("/@lezer/highlight/") ||
            id.includes("/@lezer/lr/")
          ) {
            return "vendor-lezer-core";
          }
          if (id.includes("/@tauri-apps/")) return "vendor-tauri";
          if (id.includes("/@radix-ui/")) return "vendor-radix";
          return undefined;
        },
      },
    },
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
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
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
