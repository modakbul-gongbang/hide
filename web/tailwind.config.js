/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        background: "var(--color-background)",
        sidebar: "var(--color-sidebar)",
        panel: "var(--color-panel)",
        elevated: "var(--color-elevated)",
        balloon: "var(--color-balloon)",
        divider: "var(--color-divider)",
        primary: "var(--color-primary)",
        secondary: "var(--color-secondary)",
        muted: "var(--color-muted)",
        accent: "var(--color-accent)",
        danger: "var(--color-danger)",
        warning: "var(--color-warning)",
        success: "var(--color-success)",
        "agent-working": "var(--color-agent-working)",
        "pr-open": "var(--color-pr-open)",
        "pr-merged": "var(--color-pr-merged)",
        "pr-closed": "var(--color-pr-closed)",
        "pr-draft": "var(--color-pr-draft)",
        "file-neutral": "var(--color-file-neutral)",
        "file-document": "var(--color-file-document)",
        "file-blue": "var(--color-file-blue)",
        "file-green": "var(--color-file-green)",
        "file-orange": "var(--color-file-orange)",
        "file-yellow": "var(--color-file-yellow)",
        "file-purple": "var(--color-file-purple)",
      },
      spacing: {
        none: "var(--spacing-none)",
        xxs: "var(--spacing-xxs)",
        xs: "var(--spacing-xs)",
        sm: "var(--spacing-sm)",
        md: "var(--spacing-md)",
        lg: "var(--spacing-lg)",
        xl: "var(--spacing-xl)",
      },
      borderRadius: {
        xs: "var(--radius-xs)",
        sm: "var(--radius-sm)",
        md: "var(--radius-md)",
        lg: "var(--radius-lg)",
      },
      // Interface text scales with Settings > Appearance > Interface font
      // (`--interface-scale`, set from the core's ui_state.font_size); the
      // terminal and editor sizes are separate tokens and do not scale here.
      fontSize: {
        micro: "calc(var(--text-micro) * var(--interface-scale, 1))",
        caption: "calc(var(--text-caption) * var(--interface-scale, 1))",
        body: "calc(var(--text-body) * var(--interface-scale, 1))",
        subhead: "calc(var(--text-subhead) * var(--interface-scale, 1))",
        title: "calc(var(--text-title) * var(--interface-scale, 1))",
        headline: "calc(var(--text-headline) * var(--interface-scale, 1))",
      },
    },
  },
  plugins: [],
};
