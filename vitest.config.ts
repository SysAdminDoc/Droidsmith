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
      provider: "v8",
      reporter: ["text", "json", "html"],
      // Route components are intentionally covered by the invisible rendered
      // route gate. Keep this focused gate on renderer helpers/state that can
      // be exercised deterministically in Vitest.
      include: ["src/lib/**/*.{ts,tsx}"],
      exclude: [
        "src/**/*.test.{ts,tsx}",
        "src/lib/bindings.ts",
        "src/main.tsx",
        "src/vite-env.d.ts",
      ],
      thresholds: {
        statements: 45,
        branches: 40,
        functions: 35,
        lines: 45,
      },
    },
  },
});
