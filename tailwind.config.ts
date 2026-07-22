import type { Config } from "tailwindcss";

export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      fontFamily: {
        sans: [
          "Inter",
          "ui-sans-serif",
          "system-ui",
          "-apple-system",
          "Segoe UI",
          "Roboto",
          "sans-serif",
        ],
        mono: [
          "JetBrains Mono",
          "ui-monospace",
          "Menlo",
          "Consolas",
          "monospace",
        ],
      },
      fontSize: {
        xs: ["0.875rem", { lineHeight: "1.25rem" }],
        sm: ["0.9375rem", { lineHeight: "1.375rem" }],
        base: ["1rem", { lineHeight: "1.5rem" }],
      },
      colors: {
        anvil: {
          50: "#f7f8fa",
          100: "#e8ebf0",
          200: "#d4d9e2",
          300: "#bac2cf",
          400: "#a7b0c0",
          500: "#98a2b4",
          600: "#8a95a8",
          700: "#39414d",
          800: "#252b34",
          900: "#171b22",
          950: "#0d1015",
        },
        circuit: {
          50: "#ecfeff",
          100: "#cffafe",
          200: "#a5f3fc",
          300: "#67e8f9",
          400: "#22d3ee",
          500: "#06b6d4",
          600: "#0891b2",
          700: "#0e7490",
          800: "#155e75",
          900: "#164e63",
          950: "#083344",
        },
        signal: {
          green: "#7dd3a8",
          amber: "#f4c86a",
          red: "#f48a8a",
        },
        surface: {
          // Elevated dialog/modal surface shared by every overlay card.
          dialog: "#121820",
          // Terminal/console scrollback background (Console, Logcat).
          terminal: "#0a0e13",
        },
      },
      boxShadow: {
        glow: "0 1px 2px rgba(0, 0, 0, 0.24), 0 14px 36px rgba(0, 0, 0, 0.16)",
        panel: "0 1px 0 rgba(255, 255, 255, 0.025) inset",
      },
    },
  },
  plugins: [],
} satisfies Config;
