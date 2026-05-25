import type { Config } from "tailwindcss";

export default {
  darkMode: "class",
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
      colors: {
        anvil: {
          50: "#f6f7f9",
          100: "#ecedf2",
          200: "#d5d7e0",
          300: "#b1b5c4",
          400: "#888da3",
          500: "#6c7287",
          600: "#565b6e",
          700: "#464a59",
          800: "#3c3f4c",
          900: "#1e2027",
          950: "#121317",
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
      },
      boxShadow: {
        glow: "0 0 0 1px rgba(34, 211, 238, 0.08), 0 18px 55px rgba(0, 0, 0, 0.35)",
      },
    },
  },
  plugins: [],
} satisfies Config;
