import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath, URL } from "node:url";

// When Tauri runs `tauri dev --host`, it sets TAURI_DEV_HOST so HMR uses
// the LAN address instead of localhost.
const host = process.env.TAURI_DEV_HOST;
const qrcodeGeneratorPath = fileURLToPath(
  new URL("./node_modules/qrcode-generator/dist/qrcode.js", import.meta.url),
);

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  resolve: {
    alias: {
      "qrcode-generator": qrcodeGeneratorPath,
    },
  },
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    // Tauri sets TAURI_ENV_PLATFORM in the build step. Windows WebView2
    // is current Chrome; Linux WebKitGTK / macOS WKWebView are Safari-ish.
    target:
      process.env.TAURI_ENV_PLATFORM === "windows" ? "chrome105" : "safari13",
    minify: process.env.TAURI_ENV_DEBUG ? false : "esbuild",
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
});
