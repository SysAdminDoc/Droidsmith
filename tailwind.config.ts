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
          50: "#f3f7fa",
          100: "#e3eaf0",
          200: "#ced8e1",
          300: "#b6c2ce",
          400: "#91a2b2",
          500: "#8497a8",
          600: "#7d91a2",
          700: "#31404d",
          800: "#1a2631",
          900: "#0d1720",
          950: "#070d13",
        },
        circuit: {
          50: "#ecfeff",
          100: "#cffafe",
          200: "#a5f3fc",
          300: "#35d6ed",
          400: "#19c8e3",
          500: "#08adc9",
          600: "#0891b2",
          700: "#0e7490",
          800: "#155e75",
          900: "#164e63",
          950: "#083344",
        },
        signal: {
          green: "#54d990",
          amber: "#ffc657",
          red: "#ff756d",
        },
        surface: {
          // Elevated dialog/modal surface shared by every overlay card.
          dialog: "#0d1720",
          // Terminal/console scrollback background (Console, Logcat).
          terminal: "#050b10",
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
