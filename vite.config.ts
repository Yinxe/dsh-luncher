import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri expects a fixed port, fail if that port is not available
export default defineConfig({
  plugins: [react()],
  envPrefix: ["VITE_", "TAURI_ENV_"],
  server: {
    port: 1420,
    strictPort: true,
    host: "localhost",
  },
  // Env variables starting with TAURI_ENV_* are exposed to the frontend
  clearScreen: false,
  build: {
    target: "chrome105",
    minify: "esbuild",
    sourcemap: false,
  },
});
