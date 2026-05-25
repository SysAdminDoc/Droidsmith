import { defineConfig } from "vitest/config";

// No @vitejs/plugin-react here — esbuild (which Vitest runs internally)
// has built-in JSX transformation, and we don't render components in
// tests. Adding the plugin caused a duplicate-Plugin-type clash between
// the project's vite 6 and vitest's bundled vite 5.
export default defineConfig({
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
