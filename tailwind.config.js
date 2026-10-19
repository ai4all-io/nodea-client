/** @type {import('tailwindcss').Config} */
export default {
  darkMode: ["class"],
  content: [
    './index.html',
    './src/**/*.{ts,tsx,js,jsx}',
  ],
  theme: {
    container: {
      center: true,
      padding: "2rem",
      screens: {
        "2xl": "1400px",
      },
    },
    extend: {
      colors: {
        border: "hsl(var(--border))",
        input: "hsl(var(--input))",
        ring: "hsl(var(--ring))",
        background: "hsl(var(--background))",
        foreground: "hsl(var(--foreground))",
        primary: {
          DEFAULT: "hsl(var(--primary))",
          foreground: "hsl(var(--primary-foreground))",
        },
        secondary: {
          DEFAULT: "hsl(var(--secondary))",
          foreground: "hsl(var(--secondary-foreground))",
        },
        destructive: {
          DEFAULT: "hsl(var(--destructive))",
          foreground: "hsl(var(--destructive-foreground))",
        },
        muted: {
          DEFAULT: "hsl(var(--muted))",
          foreground: "hsl(var(--muted-foreground))",
        },
        accent: {
          DEFAULT: "hsl(var(--accent))",
          foreground: "hsl(var(--accent-foreground))",
        },
        popover: {
          DEFAULT: "hsl(var(--popover))",
          foreground: "hsl(var(--popover-foreground))",
        },
        card: {
          DEFAULT: "hsl(var(--card))",
          foreground: "hsl(var(--card-foreground))",
        },
        n: {
          bg: "var(--n-bg)",
          sidebar: "var(--n-sidebar)",
          raised: "var(--n-raised)",
          line: "var(--n-line)",
          "line-strong": "var(--n-line-strong)",
          border: "var(--n-border)",
          text: "var(--n-text)",
          "text-2": "var(--n-text-2)",
          "text-3": "var(--n-text-3)",
          "text-4": "var(--n-text-4)",
          mute: "var(--n-mute)",
          "mute-2": "var(--n-mute-2)",
          accent: "var(--n-accent)",
          "accent-200": "var(--n-accent-200)",
          "accent-300": "var(--n-accent-300)",
          "accent-400": "var(--n-accent-400)",
          "accent-700": "var(--n-accent-700)",
          "accent-800": "var(--n-accent-800)",
          "accent-900": "var(--n-accent-900)",
        },
      },
      backgroundColor: {
        "n-tint": "var(--n-tint-hover)",
        "n-tint-active": "var(--n-tint-active)",
        "n-tint-selected": "var(--n-tint-selected)",
        "n-tint-row": "var(--n-tint-row)",
      },
      borderRadius: {
        lg: "var(--radius)",
        md: "calc(var(--radius) - 2px)",
        sm: "calc(var(--radius) - 4px)",
        window: "var(--n-radius-window)",
      },
      boxShadow: {
        "n-sm": "var(--n-shadow-sm)",
        "n-md": "var(--n-shadow-md)",
        "n-lg": "var(--n-shadow-lg)",
      },
      fontFamily: {
        sans: ["Inter", "system-ui", "-apple-system", "sans-serif"],
        mono: ["ui-monospace", "Menlo", "monospace"],
      },
      fontSize: {
        "2xs": ["9.5px", "1"],
        "xs-": ["10.5px", "1"],
        "xs+": ["11.5px", "1.3"],
        "sm-": ["12.5px", "1.4"],
      },
      keyframes: {
        "accordion-down": {
          from: { height: 0 },
          to: { height: "var(--radix-accordion-content-height)" },
        },
        "accordion-up": {
          from: { height: "var(--radix-accordion-content-height)" },
          to: { height: 0 },
        },
        "noc-pulse": {
          "0%,100%": { opacity: "1", transform: "scale(1)" },
          "50%": { opacity: "0.35", transform: "scale(0.82)" },
        },
        "noc-spin": { to: { transform: "rotate(360deg)" } },
        "noc-slide": {
          "0%": { transform: "translateX(-100%)" },
          "100%": { transform: "translateX(320%)" },
        },
      },
      animation: {
        "accordion-down": "accordion-down 0.2s ease-out",
        "accordion-up": "accordion-up 0.2s ease-out",
        "noc-pulse": "noc-pulse 2.4s ease-in-out infinite",
        "noc-spin": "noc-spin 1.1s linear infinite",
        "noc-slide": "noc-slide 1.3s ease-in-out infinite",
      },
    },
  },
  plugins: [],
}
