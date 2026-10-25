/** @type {import('tailwindcss').Config} */
export default {
  darkMode: "class",
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        base: "#0B0D10",
        surface: "#15181D",
        elevated: "#1E232B",
        hairline: "rgba(255,255,255,0.07)",
        ink: {
          DEFAULT: "#F5F7FA",
          muted: "#8E939B",
          faint: "#5A6068",
        },
        accent: {
          DEFAULT: "#0A84FF",
          soft: "rgba(10,132,255,0.14)",
        },
        ok: "#30D158",
        danger: "#FF453A",
        warn: "#FFD60A",
        info: "#64D2FF",
      },
      fontFamily: {
        sans: [
          "Source Han Sans SC",
          "Noto Sans SC",
          "Microsoft YaHei UI",
          "PingFang SC",
          "system-ui",
          "sans-serif",
        ],
        mono: ["JetBrains Mono", "Cascadia Mono", "Consolas", "monospace"],
      },
      fontSize: {
        title: ["26px", { lineHeight: "32px", fontWeight: "600" }],
        sub: ["16px", { lineHeight: "22px", fontWeight: "500" }],
        body: ["13px", { lineHeight: "20px" }],
      },
      borderRadius: {
        card: "14px",
      },
      boxShadow: {
        card: "0 1px 0 rgba(255,255,255,0.04) inset, 0 8px 24px rgba(0,0,0,0.35)",
        glow: "0 0 40px rgba(10,132,255,0.35)",
        "glow-ok": "0 0 40px rgba(48,209,88,0.32)",
      },
      transitionTimingFunction: {
        spring: "cubic-bezier(0.34, 1.56, 0.64, 1)",
      },
      keyframes: {
        breathe: {
          "0%, 100%": { transform: "scale(1)", opacity: "0.55" },
          "50%": { transform: "scale(1.06)", opacity: "0.9" },
        },
        "spin-slow": {
          from: { transform: "rotate(0deg)" },
          to: { transform: "rotate(360deg)" },
        },
        "fade-up": {
          from: { opacity: "0", transform: "translateY(6px)" },
          to: { opacity: "1", transform: "translateY(0)" },
        },
      },
      animation: {
        breathe: "breathe 3.6s ease-in-out infinite",
        "spin-slow": "spin-slow 9s linear infinite",
        "fade-up": "fade-up 240ms ease-out both",
      },
    },
  },
  plugins: [require("tailwindcss-animate")],
};
