/** @type {import('tailwindcss').Config} */
export default {
  content: [
    './index.html',
    './src/**/*.{ts,tsx}',
    '../desktop/src/**/*.{ts,tsx}',
  ],
  theme: {
    extend: {
      colors: {
        cs: {
          bg: "#0a0a0f",
          "bg-raised": "#111118",
          card: "#16161e",
          border: "#2a2a3a",
          hover: "#32324a",
          text: "#e8e8f0",
          muted: "#8888a0",
          accent: "#00FFB2",
          "accent-hover": "#00e6a0",
          "accent-dim": "#00FFB2",
          success: "#00FFB2",
          warning: "#FFB800",
          danger: "#FF4466",
        },
      },
      fontFamily: {
        mono: ['"JetBrains Mono"', '"Fira Code"', 'ui-monospace', 'monospace'],
      },
      keyframes: {
        'slide-in-right': {
          from: { transform: 'translateX(100%)' },
          to: { transform: 'translateX(0)' },
        },
      },
      animation: {
        'slide-in-right': 'slide-in-right 0.2s ease-out',
      },
    },
  },
  plugins: [],
};
