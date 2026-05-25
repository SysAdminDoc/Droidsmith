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
      },
    },
  },
  plugins: [],
} satisfies Config;
