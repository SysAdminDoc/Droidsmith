import { defineConfig } from "vitest/config";
import { fileURLToPath, URL } from "node:url";

// No @vitejs/plugin-react here — esbuild (which Vitest runs internally)
// has built-in JSX transformation, and we don't render components in
// tests. Adding the plugin caused a duplicate-Plugin-type clash between
// the project's vite 6 and vitest's bundled vite 5.
const qrcodeGeneratorPath = fileURLToPath(
  new URL("./node_modules/qrcode-generator/dist/qrcode.js", import.meta.url),
);

export default defineConfig({
  resolve: {
    alias: {
      "qrcode-generator": qrcodeGeneratorPath,
    },
  },
  esbuild: {
    jsx: "automatic",
  },
  test: {
    environment: "node",
    globals: false,
    include: ["src/**/*.test.{ts,tsx}"],
    coverage: {
      reporter: ["text", "json", "html"],
      include: ["src/**/*.{ts,tsx}"],
      exclude: ["src/**/*.test.{ts,tsx}", "src/main.tsx", "src/vite-env.d.ts"],
    },
  },
});
