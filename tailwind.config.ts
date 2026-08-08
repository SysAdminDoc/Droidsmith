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
          50: "rgb(var(--ds-anvil-50) / <alpha-value>)",
          100: "rgb(var(--ds-anvil-100) / <alpha-value>)",
          200: "rgb(var(--ds-anvil-200) / <alpha-value>)",
          300: "rgb(var(--ds-anvil-300) / <alpha-value>)",
          400: "rgb(var(--ds-anvil-400) / <alpha-value>)",
          500: "rgb(var(--ds-anvil-500) / <alpha-value>)",
          600: "rgb(var(--ds-anvil-600) / <alpha-value>)",
          700: "rgb(var(--ds-anvil-700) / <alpha-value>)",
          800: "rgb(var(--ds-anvil-800) / <alpha-value>)",
          900: "rgb(var(--ds-anvil-900) / <alpha-value>)",
          950: "rgb(var(--ds-anvil-950) / <alpha-value>)",
        },
        circuit: {
          50: "rgb(var(--ds-circuit-50) / <alpha-value>)",
          100: "rgb(var(--ds-circuit-100) / <alpha-value>)",
          200: "rgb(var(--ds-circuit-200) / <alpha-value>)",
          300: "rgb(var(--ds-circuit-300) / <alpha-value>)",
          400: "rgb(var(--ds-circuit-400) / <alpha-value>)",
          500: "rgb(var(--ds-circuit-500) / <alpha-value>)",
          600: "rgb(var(--ds-circuit-600) / <alpha-value>)",
          700: "rgb(var(--ds-circuit-700) / <alpha-value>)",
          800: "rgb(var(--ds-circuit-800) / <alpha-value>)",
          900: "rgb(var(--ds-circuit-900) / <alpha-value>)",
          950: "rgb(var(--ds-circuit-950) / <alpha-value>)",
        },
        signal: {
          green: "rgb(var(--ds-signal-green) / <alpha-value>)",
          amber: "rgb(var(--ds-signal-amber) / <alpha-value>)",
          red: "rgb(var(--ds-signal-red) / <alpha-value>)",
        },
        surface: {
          // Elevated dialog/modal surface shared by every overlay card.
          card: "rgb(var(--ds-surface-card) / <alpha-value>)",
          dialog: "rgb(var(--ds-surface-dialog) / <alpha-value>)",
          // Terminal/console scrollback background (Console, Logcat).
          terminal: "rgb(var(--ds-surface-terminal) / <alpha-value>)",
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
