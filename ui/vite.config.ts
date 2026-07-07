import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri wraps this Vite app. No sidecar — the Rust Tauri commands ARE the
// backend (invoke'd from the webview). Port 1422 to avoid clashing with
// shelf (1421) and emb-reader (1420) dev servers.
export default defineConfig({
  plugins: [react()],
  server: { port: 1422, strictPort: true },
  build: { target: "chrome105", emptyOutDir: true },
});
