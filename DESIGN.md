---
version: alpha
name: Raycast-design-analysis
essence: |
  A dark-canvas developer-tools system that treats the marketing page like an extended product screenshot — pure-near-black background, command-palette mockups as the hero, Inter typography with the ss03 stylistic set turned on, and a single white CTA pill that doesn't break the inky atmosphere. The chrome reads like Raycast's own command-palette UI scaled up to a marketing page: monochrome dark surfaces with a faint surface ladder (#07080a → #0d0d0d → #101111), tight 6–10px radius on cards, hairline 1px borders in #242728, and rare splashes of saturated accent (Hacker News yellow, Slack red, Mac green, info blue) reserved for product-tile category illustrations. The signature visual moment is a red gradient hero wordmark — three diagonal red stripes laid across the very top of the home page like a launch-banner — paired with full-bleed product UI screenshots that show Raycast's actual command palette, store, and AI chat surfaces.
description: |
  Raycast's marketing system reads like an extended product screenshot. The chrome IS the in-product chrome at marketing scale: pure-near-black canvas, hairline 1px borders, command-palette-style cards, Inter typography with the ss03 stylistic set enabled site-wide, white CTA pill, and a small set of saturated category accent colors (yellow / red / green / blue) reserved for extension and feature illustrations. Section rhythm is generous (~96px) but the page never breaks tonal continuity — the whole site sits in one continuous dark mode.

colors:
  background: "#101112"
  sidebar: "#171819"
  panel: "#1D1F21"
  elevated: "#27292C"
  balloon: "#34373B"
  divider: "#34363A"
  primary: "#F4F4F6"
  secondary: "#A4A5A8"
  muted: "#92959A"
  accent: "#D3D3D4"
  marketing-primary: "#ffffff"
  primary-pressed: "#e8e8e8"
  on-primary: "#000000"
  ink: "#f4f4f6"
  body: "#cdcdcd"
  charcoal: "#d3d3d4"
  mute: "#9c9c9d"
  ash: "#6a6b6c"
  stone: "#434345"
  on-dark: "#ffffff"
  on-dark-mute: "rgba(255,255,255,0.72)"
  canvas: "#07080a"
  surface: "#0d0d0d"
  surface-elevated: "#101111"
  surface-card: "#121212"
  button-fg: "#18191a"
  hairline: "#242728"
  hairline-soft: "rgba(255,255,255,0.08)"
  hairline-strong: "rgba(255,255,255,0.16)"
  accent-blue: "#57c1ff"
  accent-blue-soft: "#182831"
  accent-red: "#ff6161"
  accent-red-soft: "rgba(255,97,97,0.15)"
  accent-green: "#59d499"
  accent-green-soft: "rgba(89,212,153,0.15)"
  accent-yellow: "#ffc533"
  accent-yellow-soft: "rgba(255,197,51,0.15)"
  hero-stripe-start: "#ff5757"
  hero-stripe-end: "#a1131a"
  key-bg-start: "#121212"
  key-bg-end: "#0d0d0d"

typography:
  micro:
    fontFamily: Inter
    fontSize: 9px
    fontWeight: 400
    lineHeight: 1.4
    fontFeature: '"ss03"'
  caption:
    fontFamily: Inter
    fontSize: 10px
    fontWeight: 400
    lineHeight: 1.4
    fontFeature: '"ss03"'
  body:
    fontFamily: Inter
    fontSize: 11px
    fontWeight: 400
    lineHeight: 1.4
    fontFeature: '"ss03"'
  subhead:
    fontFamily: Inter
    fontSize: 12px
    fontWeight: 400
    lineHeight: 1.4
    fontFeature: '"ss03"'
  title:
    fontFamily: Inter
    fontSize: 13px
    fontWeight: 400
    lineHeight: 1.4
    fontFeature: '"ss03"'
  headline:
    fontFamily: Inter
    fontSize: 17px
    fontWeight: 400
    lineHeight: 1.4
    fontFeature: '"ss03"'
  display:
    fontFamily: Inter
    fontSize: 30px
    fontWeight: 400
    lineHeight: 1.4
    fontFeature: '"ss03"'
  display-xl:
    fontFamily: Inter
    fontSize: 64px
    fontWeight: 600
    lineHeight: 1.1
    letterSpacing: 0
    fontFeature: '"calt", "kern", "liga", "ss03"'
  display-lg:
    fontFamily: Inter
    fontSize: 56px
    fontWeight: 500
    lineHeight: 1.17
    letterSpacing: 0.2px
    fontFeature: '"calt", "kern", "liga", "ss03"'
  heading-xl:
    fontFamily: Inter
    fontSize: 24px
    fontWeight: 500
    lineHeight: 1.6
    letterSpacing: 0.2px
    fontFeature: '"calt", "kern", "liga", "ss03"'
  heading-lg:
    fontFamily: Inter
    fontSize: 22px
    fontWeight: 500
    lineHeight: 1.15
    letterSpacing: 0
    fontFeature: '"calt", "kern", "liga", "ss03"'
  heading-md:
    fontFamily: Inter
    fontSize: 20px
    fontWeight: 500
    lineHeight: 1.4
    letterSpacing: 0.2px
    fontFeature: '"calt", "kern", "liga", "ss03"'
  heading-sm:
    fontFamily: Inter
    fontSize: 18px
    fontWeight: 500
    lineHeight: 1.4
    letterSpacing: 0.2px
    fontFeature: '"calt", "kern", "liga", "ss03"'
  body-lg:
    fontFamily: Inter
    fontSize: 18px
    fontWeight: 400
    lineHeight: 1.6
    letterSpacing: 0
    fontFeature: '"calt", "kern", "liga", "ss03"'
  body-md:
    fontFamily: Inter
    fontSize: 16px
    fontWeight: 400
    lineHeight: 1.6
    letterSpacing: 0
    fontFeature: '"calt", "kern", "liga", "ss03"'
  body-strong:
    fontFamily: Inter
    fontSize: 16px
    fontWeight: 500
    lineHeight: 1.4
    letterSpacing: 0.2px
    fontFeature: '"calt", "kern", "liga", "ss03"'
  body-sm:
    fontFamily: Inter
    fontSize: 14px
    fontWeight: 400
    lineHeight: 1.6
    letterSpacing: 0
    fontFeature: '"calt", "kern", "liga", "ss03"'
  body-sm-strong:
    fontFamily: Inter
    fontSize: 14px
    fontWeight: 500
    lineHeight: 1.6
    letterSpacing: 0.2px
    fontFeature: '"calt", "kern", "liga", "ss03"'
  caption-md:
    fontFamily: Inter
    fontSize: 13px
    fontWeight: 400
    lineHeight: 1.4
    letterSpacing: 0.1px
    fontFeature: '"calt", "kern", "liga", "ss03"'
  caption-sm:
    fontFamily: Inter
    fontSize: 12px
    fontWeight: 400
    lineHeight: 1.5
    letterSpacing: 0.4px
    fontFeature: '"calt", "kern", "liga", "ss03"'
  link-md:
    fontFamily: Inter
    fontSize: 16px
    fontWeight: 500
    lineHeight: 1.4
    letterSpacing: 0.3px
    fontFeature: '"calt", "kern", "liga", "ss03"'
  button-md:
    fontFamily: Inter
    fontSize: 14px
    fontWeight: 500
    lineHeight: 1.6
    letterSpacing: 0.2px
    fontFeature: '"calt", "kern", "liga", "ss03"'

rounded:
  radiusExtraSmall: 4px
  radiusSmall: 6px
  radiusMedium: 8px
  radiusLarge: 10px
  radiusExtraLarge: 16px
  none: 0px
  xs: 4px
  sm: 6px
  md: 8px
  lg: 10px
  xl: 16px
  full: 9999px

spacing:
  spacingNone: 0px
  spacingXXS: 2px
  spacingXS: 4px
  spacingSM: 8px
  spacingMD: 12px
  spacingLG: 16px
  spacingXL: 24px
  spacingXXL: 32px
  spacingXXXL: 40px
  xxs: 2px
  xs: 4px
  sm: 8px
  md: 12px
  lg: 16px
  xl: 24px
  xxl: 32px
  section: 96px

components:
  shell-hairline:
    backgroundColor: "{colors.divider}"
  marketing-card-hairline:
    backgroundColor: "{colors.hairline}"
  marketing-soft-hairline:
    backgroundColor: "{colors.hairline-soft}"
  marketing-focus-hairline:
    backgroundColor: "{colors.hairline-strong}"
  marketing-inner-card:
    backgroundColor: "{colors.button-fg}"
  marketing-illustration-outline:
    backgroundColor: "{colors.stone}"
  marketing-red-illustration:
    backgroundColor: "{colors.accent-red}"
  marketing-red-wash:
    backgroundColor: "{colors.accent-red-soft}"
  marketing-green-illustration:
    backgroundColor: "{colors.accent-green}"
  marketing-green-wash:
    backgroundColor: "{colors.accent-green-soft}"
  marketing-yellow-illustration:
    backgroundColor: "{colors.accent-yellow}"
  marketing-yellow-wash:
    backgroundColor: "{colors.accent-yellow-soft}"
  hero-stripe-light-layer:
    backgroundColor: "{colors.hero-stripe-start}"
  hero-stripe-dark-layer:
    backgroundColor: "{colors.hero-stripe-end}"
  keycap-top-layer:
    backgroundColor: "{colors.key-bg-start}"
  keycap-bottom-layer:
    backgroundColor: "{colors.key-bg-end}"
  shell-metadata:
    textColor: "{colors.muted}"
  shell-supporting-copy:
    textColor: "{colors.secondary}"
  marketing-disabled-icon:
    textColor: "{colors.ash}"

  shell-sidebar:
    backgroundColor: "{colors.sidebar}"
    textColor: "{colors.primary}"
    typography: "{typography.body}"
    rounded: "{rounded.radiusSmall}"
  shell-panel:
    backgroundColor: "{colors.panel}"
    textColor: "{colors.primary}"
  shell-keycap:
    backgroundColor: "{colors.elevated}"
    textColor: "{colors.primary}"
    typography: "{typography.micro}"
    rounded: "{rounded.radiusSmall}"
    height: 18px
    padding: "{spacing.spacingXS}"
  shell-tooltip:
    backgroundColor: "{colors.balloon}"
    textColor: "{colors.primary}"
    typography: "{typography.subhead}"
    rounded: "{rounded.radiusMedium}"
  shell-primary-action:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.background}"
    typography: "{typography.body}"
    rounded: "{rounded.radiusSmall}"
  marketing-heading:
    textColor: "{colors.ink}"
    typography: "{typography.heading-xl}"
  marketing-supporting-copy:
    textColor: "{colors.charcoal}"
    typography: "{typography.body-md}"
  marketing-category-illustration:
    backgroundColor: "{colors.surface-card}"
  button-primary:
    backgroundColor: "{colors.marketing-primary}"
    textColor: "{colors.on-primary}"
    typography: "{typography.button-md}"
    rounded: "{rounded.md}"
    padding: 8px 16px
    height: 36px
  button-primary-pressed:
    backgroundColor: "{colors.primary-pressed}"
    textColor: "{colors.on-primary}"
    typography: "{typography.button-md}"
    rounded: "{rounded.md}"
  button-secondary:
    backgroundColor: "transparent"
    textColor: "{colors.on-dark}"
    typography: "{typography.button-md}"
    rounded: "{rounded.md}"
    padding: 8px 16px
    height: 36px
  button-tertiary:
    backgroundColor: "{colors.surface-elevated}"
    textColor: "{colors.on-dark}"
    typography: "{typography.button-md}"
    rounded: "{rounded.md}"
    padding: 8px 16px
    height: 36px
  button-disabled:
    backgroundColor: "{colors.surface-elevated}"
    textColor: "{colors.mute}"
    rounded: "{rounded.md}"
  install-button:
    backgroundColor: "transparent"
    textColor: "{colors.on-dark}"
    typography: "{typography.button-md}"
    rounded: "{rounded.md}"
    padding: 6px 14px
  text-input:
    backgroundColor: "{colors.surface-elevated}"
    textColor: "{colors.on-dark}"
    typography: "{typography.body-md}"
    rounded: "{rounded.md}"
    padding: 8px 12px
    height: 36px
  text-input-focused:
    backgroundColor: "{colors.surface-elevated}"
    textColor: "{colors.on-dark}"
    rounded: "{rounded.md}"
  store-search-bar:
    backgroundColor: "{colors.surface-elevated}"
    textColor: "{colors.on-dark}"
    typography: "{typography.body-md}"
    rounded: "{rounded.md}"
    padding: 10px 16px
    height: 44px
  command-palette-row:
    backgroundColor: "transparent"
    textColor: "{colors.on-dark}"
    typography: "{typography.body-md}"
    rounded: "{rounded.sm}"
    padding: 6px 10px
  command-palette-row-active:
    backgroundColor: "{colors.surface-card}"
    textColor: "{colors.on-dark}"
    typography: "{typography.body-md}"
    rounded: "{rounded.sm}"
  pill-tab:
    backgroundColor: "transparent"
    textColor: "{colors.body}"
    typography: "{typography.body-sm}"
    rounded: "{rounded.full}"
    padding: 4px 10px
  pill-tab-active:
    backgroundColor: "{colors.surface-elevated}"
    textColor: "{colors.on-dark}"
    typography: "{typography.body-sm}"
    rounded: "{rounded.full}"
  badge-pro:
    backgroundColor: "{colors.surface-elevated}"
    textColor: "{colors.on-dark-mute}"
    typography: "{typography.caption-sm}"
    rounded: "{rounded.xs}"
    padding: 2px 6px
  badge-info-soft:
    backgroundColor: "{colors.accent-blue-soft}"
    textColor: "{colors.accent-blue}"
    typography: "{typography.caption-sm}"
    rounded: "{rounded.xs}"
    padding: 2px 8px
  keycap:
    backgroundColor: "{colors.surface-card}"
    textColor: "{colors.body}"
    typography: "{typography.caption-md}"
    rounded: "{rounded.xs}"
    padding: 1px 6px
    height: 20px
  command-palette-card:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.on-dark}"
    typography: "{typography.body-md}"
    rounded: "{rounded.lg}"
    padding: 0px
  feature-card-dark:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.on-dark}"
    typography: "{typography.body-md}"
    rounded: "{rounded.lg}"
    padding: 24px
  feature-card-elevated:
    backgroundColor: "{colors.surface-elevated}"
    textColor: "{colors.on-dark}"
    typography: "{typography.body-md}"
    rounded: "{rounded.lg}"
    padding: 24px
  store-extension-card:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.on-dark}"
    typography: "{typography.body-md}"
    rounded: "{rounded.md}"
    padding: 16px
  pricing-tier-card:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.on-dark}"
    typography: "{typography.body-md}"
    rounded: "{rounded.lg}"
    padding: 24px
  pricing-tier-card-featured:
    backgroundColor: "{colors.surface-elevated}"
    textColor: "{colors.on-dark}"
    typography: "{typography.body-md}"
    rounded: "{rounded.lg}"
    padding: 24px
  hero-stripe-band:
    backgroundColor: "{colors.canvas}"
    textColor: "{colors.on-dark}"
    typography: "{typography.display-xl}"
    rounded: "{rounded.none}"
    padding: 96px 48px
  app-icon-tile:
    backgroundColor: "{colors.surface-card}"
    rounded: "{rounded.md}"
    size: 48px
  app-icon-tile-large:
    backgroundColor: "{colors.surface-card}"
    rounded: "{rounded.md}"
    size: 64px
  primary-nav:
    backgroundColor: "{colors.canvas}"
    textColor: "{colors.on-dark}"
    typography: "{typography.body-sm-strong}"
    rounded: "{rounded.none}"
    height: 56px
  footer-section:
    backgroundColor: "{colors.canvas}"
    textColor: "{colors.body}"
    typography: "{typography.body-sm}"
    rounded: "{rounded.none}"
    padding: 64px 48px
  link-inline:
    textColor: "{colors.on-dark}"
    typography: "{typography.link-md}"
---

## Overview

Raycast's marketing site reads like an extended product screenshot. The chrome IS the in-product command palette at marketing scale: pure near-black canvas (`{colors.canvas}` — `#07080a`), hairline 1px borders (`{colors.hairline}` — `#242728`), command-palette-style cards with rounded corners between 6 and 16px, Inter typography with the **ss03 stylistic set enabled site-wide** (a single character — the alternate `g` — that gives Raycast's typography its signature subtle distinction), a single white CTA pill that anchors every primary action, and small splashes of saturated accent reserved for category illustrations.

The system has effectively one surface mode — dark — with a faint three-step surface ladder (`{colors.canvas}` → `{colors.surface}` → `{colors.surface-elevated}` → `{colors.surface-card}`) carrying cards, in-card panels, and key-cap glyph backgrounds. The signature decorative moment is a **red diagonal-stripe gradient band** across the very top of the home page hero, used as a launch-banner motif behind the headline (the only time saturated red appears on chrome). Beyond that single moment, color in the chrome is reserved for category accents inside extension and feature illustrations: Hacker News yellow, Slack red, Linear green, info blue.

The design philosophy is "the marketing page is the product." Section rhythm is generous (`{spacing.section}` 96px) but the page never breaks tonal continuity — the whole site sits in one continuous dark mode, full-bleed product UI screenshots show Raycast's actual command palette / store / AI chat surfaces, and the typography ligature settings (`ss03`) are inherited from the in-product app's text rendering.

**Key Characteristics:**
- Single dark surface mode with a 4-step surface ladder: `{colors.canvas}` (#07080a) → `{colors.surface}` (#0d0d0d) → `{colors.surface-elevated}` (#101111) → `{colors.surface-card}` (#121212)
- White CTA pill (`{colors.marketing-primary}` — #ffffff) is the universal primary action; everything else is monochrome dark
- Inter typography with `font-feature-settings: "calt", "kern", "liga", "ss03"` enabled site-wide — the ss03 alternate `g` is part of the brand voice
- Hairline 1px borders (`{colors.hairline}` — #242728) carry every card edge; there are no drop shadows in the system
- Multi-radius card vocabulary: `{rounded.sm}` (6px) for keycaps, `{rounded.md}` (8px) for buttons and small cards, `{rounded.lg}` (10px) for feature cards, `{rounded.xl}` (16px) for hero command-palette mockup containers
- Saturated category accents (`{colors.accent-yellow}` for Hacker News, `{colors.accent-red}` for Slack/Apple, `{colors.accent-green}` for productivity tools, `{colors.accent-blue}` for info) appear only inside extension tile imagery — never on chrome
- Signature red diagonal-stripe gradient band at the very top of the hero — three angled stripes in `{colors.hero-stripe-start}` → `{colors.hero-stripe-end}`, used once per page maximum

## Colors

> **Source pages:** `/` (home), `/store` (extension marketplace), `/core-features/ai` (feature page), `/pricing` (plan tiers), `/thomas/hacker-news` (single extension detail). The chrome palette is identical across all five pages — the dark surface ladder, hairline borders, white CTA, and ss03-enabled typography are the same on every page.

### Brand & Accent
- **White** (`{colors.marketing-primary}` — `#ffffff`): the universal primary CTA pill background. "Download" / "Install Extension" / "Get Pro" — every primary action carries it.
- **White Pressed** (`{colors.primary-pressed}` — `#e8e8e8`): pressed-state for the primary pill — a single notch dimmer.
- **On Primary** (`{colors.on-primary}` — `#000000`): pure black text on the white CTA — the only place black appears as text in the system.

### Surface
- **Canvas** (`{colors.canvas}` — `#07080a`): pure-near-black page background. The dominant surface across every page.
- **Surface** (`{colors.surface}` — `#0d0d0d`): card and elevated panel background — one notch lighter than canvas.
- **Surface Elevated** (`{colors.surface-elevated}` — `#101111`): button-tertiary fill, text-input fill, store-search-bar fill, pill-tab-active fill.
- **Surface Card** (`{colors.surface-card}` — `#121212`): app-icon-tile background, keycap fill, command-palette row hover.
- **Button FG (in-card)** (`{colors.button-fg}` — `#18191a`): rare deep-card variant used inside featured pricing tier card backgrounds.
- **Hairline** (`{colors.hairline}` — `#242728`): the universal 1px card border. Carries every card edge across every page.
- **Hairline Soft** (`{colors.hairline-soft}` — `rgba(255,255,255,0.08)`): even fainter border on translucent over-image overlays.
- **Hairline Strong** (`{colors.hairline-strong}` — `rgba(255,255,255,0.16)`): stronger 1px divider where a regular hairline reads as too soft.

### Text
- **Ink** (`{colors.ink}` — `#f4f4f6`): primary headlines on dark canvas. Slightly off-white for tonal coherence with the near-black background.
- **Body** (`{colors.body}` — `#cdcdcd`): default paragraph text and inline-link color.
- **Charcoal** (`{colors.charcoal}` — `#d3d3d4`): subtly brighter body where ink reads too soft.
- **Mute** (`{colors.mute}` — `#9c9c9d`): metadata, footer link text, secondary captions.
- **Ash** (`{colors.ash}` — `#6a6b6c`): disabled-state text, lowest-emphasis utility.
- **Stone** (`{colors.stone}` — `#434345`): least-emphasis caption text and disabled icon color.
- **On Dark** (`{colors.on-dark}` — `#ffffff`): interactive-state primary text (button label, focused tab).
- **On Dark Mute** (`{colors.on-dark-mute}` — `rgba(255,255,255,0.72)`): translucent secondary text on dark surfaces.

### Semantic
- **Accent Blue** (`{colors.accent-blue}` — `#57c1ff`) + **Soft** (`{colors.accent-blue-soft}` — `the 15% blue overlay composited on the dark surface (#182831)`): info and informational badge — used inside feature illustrations and the rare "New" pill.
- **Accent Red** (`{colors.accent-red}` — `#ff6161`) + **Soft** (`{colors.accent-red-soft}` — `rgba(255,97,97,0.15)`): destructive/error indicator + Slack/Apple category accent in extension illustrations.
- **Accent Green** (`{colors.accent-green}` — `#59d499`) + **Soft** (`{colors.accent-green-soft}` — `rgba(89,212,153,0.15)`): success state + productivity category accent in extension illustrations.
- **Accent Yellow** (`{colors.accent-yellow}` — `#ffc533`) + **Soft** (`{colors.accent-yellow-soft}` — `rgba(255,197,51,0.15)`): "warning" semantic + the Hacker News orange-yellow that appears as the most prominent accent illustration on the home page hero.

### Brand Gradient
- **Hero Stripe Gradient** — three diagonal red stripes layered across the very top of the home page hero, fading from `{colors.hero-stripe-start}` (`#ff5757`) to `{colors.hero-stripe-end}` (`#a1131a`). The system's only chromatic gradient on chrome — used once per page maximum and reserved for hero launch-banner moments.
- **Keycap Gradient** — the small key-glyph background uses a subtle linear-gradient from `{colors.key-bg-start}` (`#121212`) to `{colors.key-bg-end}` (`#0d0d0d`) that gives Raycast's keycap UI its slight 3D-key feel.

## Typography

### Font Family
**Inter** is the system's primary face, loaded with the `Inter Fallback` system fallback variant. Critically, Raycast enables `font-feature-settings: "calt", "kern", "liga", "ss03"` site-wide — the **ss03 stylistic set** swaps in Inter's alternate `g` glyph (single-story open `g`), which is the brand's signature typographic detail. Standard ligatures (`liga`), kerning (`kern`), and contextual alternates (`calt`) are also active. The display tier additionally enables `ss02` and `ss08` and disables standard `liga` to render the hero "Raycast Pro" wordmark with its distinctive geometric construction.

There is no monospace face used outside of inline `<code>` chips in documentation; the marketing pages use Inter for everything.

### Hierarchy

| Token | Size | Weight | Line Height | Letter Spacing | Use |
|---|---|---|---|---|---|
| `{typography.display-xl}` | 64px | 600 | 1.1 | 0 | Hero "Built for the perfect tools" / "The new way to..." headline (with `liga: 0`, `ss02`, `ss08`) |
| `{typography.display-lg}` | 56px | 500 | 1.17 | 0.2px | Section headline ("Explore", "Pricing", store hero "Store") |
| `{typography.heading-xl}` | 24px | 500 | 1.6 | 0.2px | Sub-section heading, pricing-tier name |
| `{typography.heading-lg}` | 22px | 500 | 1.15 | 0 | Mid-section feature heading |
| `{typography.heading-md}` | 20px | 500 | 1.4 | 0.2px | Card group title, in-card heading |
| `{typography.heading-sm}` | 18px | 500 | 1.4 | 0.2px | Small heading, extension card title |
| `{typography.body-lg}` | 18px | 400 | 1.6 | 0 | Pricing tier description, hero subtitle |
| `{typography.body-md}` | 16px | 400 | 1.6 | 0 | Default body, paragraph text |
| `{typography.body-strong}` | 16px | 500 | 1.4 | 0.2px | Inline emphasis, primary nav link |
| `{typography.body-sm}` | 14px | 400 | 1.6 | 0 | Card description, secondary copy |
| `{typography.body-sm-strong}` | 14px | 500 | 1.6 | 0.2px | In-card label, table-header text |
| `{typography.caption-md}` | 13px | 400 | 1.4 | 0.1px | Caption, metadata |
| `{typography.caption-sm}` | 12px | 400 | 1.5 | 0.4px | Smallest utility text, badge label |
| `{typography.link-md}` | 16px | 500 | 1.4 | 0.3px | Inline body anchor link |
| `{typography.button-md}` | 14px | 500 | 1.6 | 0.2px | Standard button label |

### Principles
The hierarchy works on a 1.6-line-height ladder for body and a 1.1–1.4 ladder for display/heading. Letter-spacing is consistently positive (0.1–0.4px) — slightly opening the type — which gives Raycast's chrome an airy quality at body sizes despite the dark canvas. The `ss03` stylistic set is the brand's most distinctive typographic detail; without it, the body face renders identically to plain Inter and loses Raycast's signature rendering.

### Note on Font Substitutes
Inter is open-source and Google-Fonts-hosted; load it directly. To preserve the brand's signature look, you must enable `font-feature-settings: "calt", "kern", "liga", "ss03"` on the body element. Without `ss03`, the typography is recognizably "Inter default" rather than "Raycast." On systems where Inter cannot be loaded, the documented fallback is `Inter Fallback` (a self-hosted variant) → `system-ui`. **JetBrains Mono** or **Geist Mono** are acceptable substitutes for inline code chips when needed, though Raycast's marketing chrome rarely uses code-styled text.

## Layout

### Spacing System
- **Base unit:** 8px (with 2/4/12px steps for tight inline gaps).
- **Tokens (front matter):** `{spacing.xxs}` (2px) · `{spacing.xs}` (4px) · `{spacing.sm}` (8px) · `{spacing.md}` (12px) · `{spacing.lg}` (16px) · `{spacing.xl}` (24px) · `{spacing.xxl}` (32px) · `{spacing.section}` (96px).
- **Universal section rhythm:** every page in the set uses `{spacing.section}` (96px) as the vertical gap between major content blocks. Card grids use `{spacing.lg}` (16px) gutters; in-card padding sits at `{spacing.xl}` (24px) for feature cards and `{spacing.lg}` (16px) for store extension cards.

### Grid & Container
- **Max width:** ~1240px content area at desktop with 24px gutters (~48px at ultrawide). Hero command-palette mockups run wider (~1080px) with the page background extending to full bleed.
- **Store extension grid:** 2-up at desktop with rows of 2 cards stacked, collapsing to 1-up at mobile. Each card is a horizontal layout with a large square app icon at the left and copy + Install button at the right.
- **Pricing tier grid:** 3-up at desktop (Free / Pro / Pro+Advanced AI), collapsing to 1-up stacked at mobile.
- **Featured extension card grid:** 3-up at desktop in the "Featured" row at the top of the store page.
- **Comparison table:** full-width on the pricing page below the tier cards — 5-column table (Free / Pro / Advanced AI / Custom for Teams / Enterprise) with feature rows.
- **Footer:** 6-column horizontal link grid at desktop, collapsing to 2-up at tablet and 1-up at mobile.

### Whitespace Philosophy
Whitespace is generous and the canvas is uninterrupted. Sections sit 96px apart with no decorative dividers between them — the dark canvas continues edge-to-edge from hero to footer. Inside a section, content is left-aligned in a tight column, with command-palette mockup imagery occupying the right 50–60% of the band on home-page feature rows. The signature decorative element — the red diagonal-stripe gradient band — only appears in the very first hero band; from the second section down, the page is monochrome dark.

## Elevation & Depth

| Level | Treatment | Use |
|---|---|---|
| 0 — Flat | No border, no shadow | Default for canvas-on-canvas blocks, hero text, footer body |
| 1 — Hairline border | 1px solid `{colors.hairline}` (#242728) | Every card on `{colors.surface}`, store extension card, pricing tier card |
| 2 — Hairline strong | 1px solid `{colors.hairline-strong}` | Stronger inline divider, table-row separator on the comparison table |
| 3 — Surface ladder elevation | `{colors.canvas}` → `{colors.surface}` → `{colors.surface-elevated}` → `{colors.surface-card}` | Multi-step background-color ladder used to create elevation without shadows |

The system has no drop-shadow elevation at all. Depth is built entirely from the surface-color ladder: each notch lighter on the dark scale reads as one step closer to the viewer.

### Decorative Depth
Depth comes from product imagery and a single stripe-gradient band:
- **Hero stripe gradient** — three diagonal red stripes (`{colors.hero-stripe-start}` → `{colors.hero-stripe-end}`) layered across the home-page hero band, evoking a launch-banner / motion-blur effect. The system's signature decorative moment.
- **Command-palette mockups** — full-fidelity Raycast in-product UI screenshots (the actual Spotlight-style overlay with rounded keycaps, command rows, and accent-color glyphs) sitting inside the home-page hero and feature rows. These ARE the brand decoration.
- **App icon tiles** — small 48–64px rounded-corner tiles displaying real app icons (Slack, Spotify, Figma, Notion, Linear, Hacker News) inside store and feature illustrations.
- **Keycap glyphs** — subtle gradient-filled rounded keycap glyphs used inline to indicate keyboard shortcuts (e.g., `⌘ K`), with a faint `{colors.key-bg-start}` → `{colors.key-bg-end}` linear gradient suggesting a physical key surface.

## Shapes

### Border Radius Scale

| Token | Value | Use |
|---|---|---|
| `{rounded.none}` | 0px | Hero band, primary nav, footer, full-bleed structural surfaces |
| `{rounded.xs}` | 4px | Keycap glyphs, badge-pro chips, small inline tags |
| `{rounded.sm}` | 6px | Command-palette row, inline buttons, micro chips |
| `{rounded.md}` | 8px | Standard buttons, text inputs, store search bar, app-icon tiles, store extension card |
| `{rounded.lg}` | 10px | Feature card, command-palette mockup card, pricing tier card |
| `{rounded.xl}` | 16px | Large hero command-palette mockup container, oversized feature panel |
| `{rounded.full}` | 9999px | Pill-tab chips, avatar circles |

The radius vocabulary clusters tightly between 4 and 16px, with most chrome at 6–10px. The system never goes flat (0px) on cards and never above 16px except for fully-rounded pills.

### Photography Geometry
There is no traditional photography. Visual elements are limited to:
- **Command-palette mockups** — full-fidelity Raycast UI screenshots at 16:9 or 4:3 aspect inside `{rounded.xl}` (16px) containers.
- **App icon tiles** — 48–64px square at `{rounded.md}` (8px), displaying real app icons.
- **Avatar circles** — 32–40px at `{rounded.full}` for in-extension author attribution.
- **Hero stripe gradient** — full-bleed wash with no aspect ratio.

## Components

> The marketing reference below covers Default and Active/Pressed.
> Native shell hover, tooltip, and held-key states are specified in In-Product Components.

### Buttons

**`button-primary`** — the universal Raycast CTA
- Background `{colors.marketing-primary}` (white), text `{colors.on-primary}` (black), type `{typography.button-md}`, padding `8px 16px`, height ~36px, rounded `{rounded.md}`.
- Used for "Download" (sticky top-nav CTA), "Get Pro", "Install" — every primary action across every surface.
- Pressed state lives in `button-primary-pressed` — background dims to `{colors.primary-pressed}`.

**`button-secondary`** — transparent text button
- Background transparent, text `{colors.on-dark}`, type `{typography.button-md}`, padding `8px 16px`, height ~36px, rounded `{rounded.md}`.
- Lower-emphasis action: "Sign in" (top nav), "Learn more →", "View on GitHub".

**`button-tertiary`** — soft surface button
- Background `{colors.surface-elevated}`, text `{colors.on-dark}`, type `{typography.button-md}`, padding `8px 16px`, height ~36px, rounded `{rounded.md}`.
- Mid-emphasis: "Watch demo", "View extension", "Manage" buttons inside cards.

**`button-disabled`**
- Background `{colors.surface-elevated}`, text `{colors.mute}`; the disabled icon retains `{colors.ash}`.

**`install-button`** — the store-page install pill
- Background transparent with 1px solid `{colors.hairline-strong}` border, text `{colors.on-dark}`, type `{typography.button-md}`, padding `6px 14px`, rounded `{rounded.md}`.
- Sits at the right edge of every store extension card with the label "Install Extension".

### Filter & Tab Chips

**`pill-tab`** + **`pill-tab-active`** — small filter chip strip
- Default: transparent background, text `{colors.body}`, type `{typography.body-sm}`, padding `4px 10px`, rounded `{rounded.full}`.
- Active: background flips to `{colors.surface-elevated}`, text `{colors.on-dark}` — the chip "lifts" by one surface notch.
- Used in the store filter row ("All Extensions", "Recently Added", "Most Popular") and similar segmented controls.

**`badge-pro`** — small Pro/Plan label
- Background `{colors.surface-elevated}`, text `{colors.on-dark-mute}`, type `{typography.caption-sm}`, padding `2px 6px`, rounded `{rounded.xs}`.
- Inline "Pro" / "Pro+" / "Free" tier indicators on pricing tier cards.

**`badge-info-soft`** — translucent info chip
- Background `{colors.accent-blue-soft}`, text `{colors.accent-blue}`, type `{typography.caption-sm}`, padding `2px 8px`, rounded `{rounded.xs}`.
- Rare "New" / "Beta" inline tag.

### Inputs & Forms

**`text-input`** + **`text-input-focused`**
- Default: background `{colors.surface-elevated}`, text `{colors.on-dark}`, 1px solid `{colors.hairline}`, type `{typography.body-md}`, padding `8px 12px`, height ~36px, rounded `{rounded.md}`.
- Focused: same surface; 1px border becomes `{colors.hairline-strong}` — a subtle brightening rather than a colored ring.

**`store-search-bar`** — the store-page search field
- Background `{colors.surface-elevated}`, text `{colors.on-dark}`, type `{typography.body-md}`, padding `10px 16px`, height ~44px, rounded `{rounded.md}`.
- Sits at the top of the store page hero with a magnifier icon at the left and "Search the store..." placeholder. Slightly taller than the standard `text-input`.

### Cards & Containers

**`command-palette-card`** — the home-page hero command-palette mockup
- Container: background `{colors.surface}`, 1px solid `{colors.hairline}`, padding 0 (the mockup contents fill the card), rounded `{rounded.lg}` or `{rounded.xl}` depending on hero size.
- Layout: top header strip with macOS traffic-light dots + a search input row, body with a vertical stack of `{component.command-palette-row}` items, bottom-right keycap hint cluster.

**`command-palette-row`** + **`command-palette-row-active`** — single row inside the command palette
- Default: transparent background, text `{colors.on-dark}` in `{typography.body-md}`, padding `6px 10px`, rounded `{rounded.sm}`.
- Active: background `{colors.surface-card}` (one notch lighter than the surrounding palette card) — the selection state.
- Each row contains a small app-icon tile + label + optional keycap shortcut at the right edge.

**`feature-card-dark`** — standard product feature card
- Container: background `{colors.surface}`, 1px solid `{colors.hairline}`, padding `{spacing.xl}` (24px), rounded `{rounded.lg}`.
- Used in 2- or 3-up grids on home and feature pages — pairs a small product mockup or app-icon row with body copy and a "Learn more →" `{component.button-secondary}`.

**`feature-card-elevated`** — slightly-elevated variant
- Same chrome as `feature-card-dark` but background flips to `{colors.surface-elevated}` — used to break visual rhythm in alternating feature rows.

**`store-extension-card`** — store-page extension card
- Container: background `{colors.surface}`, 1px solid `{colors.hairline}`, padding `{spacing.lg}` (16px), rounded `{rounded.md}`.
- Layout: 48px `{component.app-icon-tile}` at left, vertical stack of name + by-author metadata + 1-line description in the center, `{component.install-button}` at the right edge.

**`pricing-tier-card`** — pricing plan card (default tier)
- Container: background `{colors.surface}`, 1px solid `{colors.hairline}`, padding `{spacing.xl}` (24px), rounded `{rounded.lg}`.
- Layout: tier name in `{typography.heading-xl}` (24px), price in larger numeric in `{typography.display-lg}`, body description in `{typography.body-lg}`, CTA `{component.button-primary}` (or `{component.button-secondary}` for free tier), feature checklist with `✓` glyphs.

**`pricing-tier-card-featured`** — middle "Pro" featured tier
- Same chrome but background flips to `{colors.surface-elevated}` (one notch lighter) — the only visual cue distinguishing the featured tier from the surrounding cards.

**`hero-stripe-band`** — home-page hero with red stripe gradient
- Background `{colors.canvas}` with three diagonal red stripes layered across the top half (`{colors.hero-stripe-start}` → `{colors.hero-stripe-end}`).
- Padding `{spacing.section}` 96px vertical / 48px horizontal, rounded `{rounded.none}`.
- Carries the hero headline in `{typography.display-xl}` and a single `{component.button-primary}` "Download" CTA.

### Decorative

**`app-icon-tile`** — small 48px square app icon
- Background `{colors.surface-card}`, padding 0 (icon fills the tile), rounded `{rounded.md}`, size 48×48.
- Used in command-palette rows and store extension cards.

**`app-icon-tile-large`** — 64px feature variant
- Same but at 64×64. Used in featured store cards and home-page hero illustration rows.

**`keycap`** — keyboard shortcut glyph
- Background `{colors.surface-card}` with a subtle linear gradient `{colors.key-bg-start}` → `{colors.key-bg-end}`, text `{colors.body}` in `{typography.caption-md}`, padding `1px 6px`, height ~20px, rounded `{rounded.xs}`.
- Renders inline command-palette shortcut hints like `⌘ K`, `⏎`, `Esc`. The signature "physical-key" feel on a flat dark canvas.

### Navigation

**`primary-nav`**
- Background `{colors.canvas}`, text `{colors.on-dark}`, height ~56px, type `{typography.body-sm-strong}`, rounded `{rounded.none}`, with a 1px `{colors.hairline}` bottom rule.
- Layout (desktop): Raycast wordmark at left, centered nav cluster ("Pro · AI · Store · Manual · Changelog · Blog · Pricing"), right cluster (Sign in link + the always-white `{component.button-primary}` "Download" CTA pill).

**Top Nav (Mobile)**
- Hamburger menu icon at left, Raycast wordmark at center, "Download" white CTA pill at right. Primary nav collapses into a full-screen drawer that slides from the left.

### Footer

**`footer-section`**
- Background `{colors.canvas}`, text `{colors.body}` in `{typography.body-sm}`, padding `64px 48px`, with a 1px `{colors.hairline}` top rule.
- Layout: 6-column horizontal link grid (Product · Core Features · Top Extensions · Company · Community · By Raycast) with column headers in `{typography.body-sm-strong}` `{colors.on-dark}` and link lists in `{typography.body-sm}` `{colors.body}`.
- Bottom row: small Raycast wordmark + a subscribe newsletter input field with `{component.button-primary}` "Subscribe" at the right.
- The very top of the footer band has a faint red stripe-gradient repeat — a smaller echo of the hero's diagonal stripe motif.

### Inline

**`link-inline`** — body-prose anchor link
- `{colors.on-dark}` text with no underline by default; underlines on focus. Inline body links are full-white rather than a tinted accent color, which keeps the dark canvas tonally pure.

## Do's and Don'ts

### Do
- Render the entire site in one continuous dark mode. There is no light variant in the system.
- Use `{colors.marketing-primary}` (white pill) for every primary CTA. There is no second primary color — white IS the brand action.
- Build elevation from the surface-color ladder (`{colors.canvas}` → `{colors.surface}` → `{colors.surface-elevated}` → `{colors.surface-card}`), never from drop shadows.
- Enable `font-feature-settings: "calt", "kern", "liga", "ss03"` on the body element. The ss03 alternate `g` is part of the brand identity.
- Anchor a `{component.command-palette-card}` mockup as the hero's load-bearing visual. Real Raycast UI is the brand.
- Use `{component.keycap}` glyphs inline to indicate keyboard shortcuts. Subtle key-bg gradient (`{colors.key-bg-start}` → `{colors.key-bg-end}`) is the brand's only "depth" decoration.
- Reserve `{colors.hero-stripe-start}` → `{colors.hero-stripe-end}` red gradient for the hero band exactly once per page. Never repeat the stripe gradient deeper in the page.
- Use saturated category accents (`{colors.accent-yellow}`, `{colors.accent-red}`, `{colors.accent-green}`, `{colors.accent-blue}`) only inside extension and feature illustrations — never on chrome buttons or text.

### Don't

- Do not use native `.help()` tooltips in the main shell; use the shared command tooltip.
  The excluded Pet view retains its native tooltip.
- Don't introduce a light mode. The system is dark-only by design.
- Don't add drop shadows on cards. Elevation is built from the surface ladder, not from shadows.
- Don't replace `{colors.marketing-primary}` (white) with a tinted accent for the primary CTA. Pure white is the brand action color.
- Don't use the saturated accent colors (`{colors.accent-yellow}`, `{colors.accent-red}`, `{colors.accent-green}`, `{colors.accent-blue}`) on text, buttons, or chrome surfaces. They belong inside extension illustrations.
- Don't repeat the hero stripe gradient outside the top hero band. The one-band rule is the system's restraint.
- Don't use Inter without the `ss03` feature flag enabled. The chrome will lose its signature voice.
- Don't pad cards with 32px+ on all sides. The system runs tight at 16–24px in-card padding.

## Responsive Behavior

### Breakpoints

| Name | Width | Key Changes |
|---|---|---|
| ultrawide | 1920px+ | Content max-width holds at 1240px; outer gutters grow to ~80px |
| desktop-large | 1440px | Default — 3-up pricing grid, 2-up store extension grid |
| desktop | 1280px | Same with narrower outer gutters |
| desktop-small | 1024px | 3-up pricing collapses to 2+1; primary nav remains horizontal |
| tablet | 768px | Pricing → 1-up stacked; primary nav becomes hamburger drawer |
| mobile | 480px | Single-column everything; hero `{typography.display-xl}` scales 64px → ~36px |
| mobile-narrow | 320px | Section padding tightens to 48px |

### Touch Targets
All interactive elements meet WCAG AA at 36px+. `{component.button-primary}` and `{component.button-tertiary}` sit at 36px height with 16px padding. `{component.text-input}` sits at 36px. `{component.store-search-bar}` sits at 44px (above AAA). `{component.pill-tab}` is ~24–28px height with 10px padding extending to 36–40px tappable via inline padding (above AA but below AAA — intentional, the chips are compact). `{component.install-button}` sits at ~32px height with 14px padding.

### Collapsing Strategy
- **Primary nav:** desktop horizontal cluster → tablet hamburger drawer at 768px. The white "Download" CTA stays visible at every breakpoint.
- **Hero command-palette mockup:** desktop full-fidelity 2-column with copy at left + mockup at right → tablet stacks vertical with mockup below copy → mobile mockup scales down to ~80% width.
- **Store extension grid:** 2-up → 1-up at tablet.
- **Pricing tier grid:** 3-up → 2+1 at desktop-small → 1-up stacked at tablet.
- **Comparison table:** desktop full 5-column → tablet horizontal scroll → mobile vertical card stack with one tier per card.
- **Footer:** 6-up link columns → 3-up at tablet → 2-up at mobile-landscape → 1-up at mobile.
- **Section padding:** `{spacing.section}` (96px) desktop → 64px tablet → 48px mobile.
- **Hero headline:** `{typography.display-xl}` (64px) at desktop, scaling 56px / 44px / 36px down the breakpoint stack.

### Image Behavior
The only "imagery" in the system is in-product Raycast UI screenshots and small app-icon assets:
- **Command-palette mockups** scale fluidly with the container; the in-product UI itself is responsive and re-renders for each breakpoint.
- **App-icon tiles** stay at 48–64px fixed size at every breakpoint; they tile in flexible rows that wrap at narrower widths.
- **Hero stripe gradient** stays at the top of the hero band at every breakpoint with the stripe angle preserved.

## Iteration Guide

1. Focus on ONE component at a time. Pull its YAML entry and verify every property resolves.
2. Reference component names and tokens directly (`{colors.marketing-primary}`, `{component.button-primary-pressed}`, `{rounded.md}`) — do not paraphrase.
3. Run `npx @google/design.md lint DESIGN.md` after edits — `broken-ref`, `contrast-ratio`, and `orphaned-tokens` warnings flag issues automatically.
4. Add new variants as separate component entries (`-pressed`, `-disabled`, `-active`) — do not bury them inside prose.
5. Default body to `{typography.body-md}` (16px / 400 / 1.6); reach for `{typography.body-strong}` for emphasis; reserve `{typography.display-xl}` strictly for the hero band.
6. Keep `{colors.marketing-primary}` (white CTA pill) scarce per viewport — at most one solid white pill per fold.
7. When introducing a new component, ask whether it can be expressed with the existing surface-ladder + 8px-radius + ss03-Inter vocabulary before adding new tokens. The system's strength is that it almost never needs new ones.

## Known Gaps

- **Mobile screenshots not captured** — responsive behavior synthesizes Raycast's mobile pattern (hamburger drawer, single-column grid, hero downscale) from desktop evidence and the breakpoint stack.
- Native shell hover and held-key states are documented in In-Product Components; the marketing reference does not claim measured hover behavior.
- Hide native shell chrome is specified in In-Product Components below.
  Raycast launcher screenshots remain design references, not a separate implementation contract.
- **Dark mode is the only mode** — no light variant exists in the captured surfaces.
- **Form validation states** beyond the focused-input border treatment are not present in the captured surfaces.
- **Authenticated chrome** (account dashboard, billing settings, team management) not in the captured pages.

## Native Git and lineage tokens

`HideTheme.lineageIndent` is 12pt for the first two descendant levels, and `lineageDeepIndent` is 6pt per level from depth three onward.
`lineageChevronWidth` reserves 16pt on every project agent row, with a disclosure control for parents and hairline connectors for descendants.
Checkout titles use `HideTheme.Typography.subhead` and `checkoutRowHeight` (36pt), with primary text contrast even when no terminal is attached.
The branch is the title; the primary checkout carries a separate `primary` role badge.
The sidebar hierarchy is Project > Workspace > Agents; a workspace corresponds to one checkout path, including a plain folder.
Workspaces without a branch use their actual folder name, including missing paths.
Detached checkouts carry a separate `detached` badge; their tooltip retains the commit and path.
Workspaces with nested agent rows toggle disclosure across the whole row; the right-edge arrow only indicates expansion.
Workspaces without nested agent rows open when clicked and have no arrow.
Workspace disclosure persists across launches and hides only the nested agent rows, preserving selection, running panes, and raised attention rows.
Project-view number shortcuts skip agents hidden by workspace disclosure.
`agentMarkWidth` (12pt) and `checkoutIconWidth` (14pt) define the status and branch columns.
`compactAgentLeadingInset` derives the root agent status center from the Workspace branch center, accounting for the lineage chevron gutter.
Compact agent rows use `spacingXS` (4pt) between the status, provider icon, and title.
The Workspace status is shown once in a trailing chip with the representative provider and `+N` remaining agents; single agents omit the suffix, and empty Workspaces omit the chip.
The chip uses the toolbar height, `radiusMedium`, `spacingXS`, and the elevated surface; the disclosure chevron follows it at the far right.
The right-edge disclosure and fixed semantic status colors follow [the shared status contract](docs/status-model.md#shared-agent-and-workspace-status-contract).
`agentWorking` (`#61A6FF`) is the fixed blue semantic status token; workspace chrome and user accent choices do not recolor it.
Workspace agent counts use the trailing representative chip; uncommitted changes retain their separate Git indicator.
A PR icon appears before the chip for a known pull request, an active GitHub lookup, or a lookup failure.
Clicking it opens a 360pt details popover with PR number, title, state, CI rollup, branches, refresh, and an external GitHub action.
The PR control is a sibling of the full-row disclosure button, so opening details never folds the Workspace.
Workspace rows without agents reserve no disclosure slot.
Their PR control uses the same trailing 24pt column as populated rows' disclosure, keeping the icon centers and right inset aligned.
PR lifecycle is a semantic-color exception to monochrome chrome: Open `#3FB950`, Merged `#A371F7`, Closed `#F85149`, and Draft `#9198A1`.
`HideTheme.PullRequest` owns this GitHub-style dark palette and the 14pt glyph size, shared with the branch icon, inside the existing 24pt control.
Official MIT-licensed Octicons distinguish open, merged, closed, and draft by shape as well as color; the vector PDF resources and license ship in the bundle.
The sidebar control, popover header, and State badge use the same lifecycle color, including during hover and selection.
Review decisions and CI retain their own status meanings.
`HideIconButton` supports template image content with an explicit semantic color while keeping the shared hit area, interaction treatment, tooltip, and accessibility behavior.
Reference: [GitHub Primer state labels](https://primer.github.io/design/components/state-label/).
The primary branch mismatch keeps its migration action as a warning icon beside the role badge.
The context menu groups creation, branch configuration, path access, and guarded deletion with native separators.
New worktree uses stacked Branch name, Create from, and Start with fields, followed by Cancel and Create worktree.
`formControlHeight` is 36pt; compact settings retain `settingsFieldHeight` at 24pt.
`HideFormPicker` owns both compact and stacked menu presentations, and `HideSettingsField` owns text inputs.
Terminal tabs use the focused pane's existing header title precedence and agent status/provider marks, with `tabTitleMaxWidth` (200pt) bounding long summaries.
The tooltip retains the tab's stable name and full pane title; file and diff tabs retain their file names.
The sidebar runtime version stays on one line with middle truncation; its tooltip carries the complete value.
`worktreeDialogWidth` is 440pt for the consequence-first deletion confirmation.
`gitSectionIcon` uses `externaldrive.badge.checkmark`, and `gitPullRequestIcon` uses `arrow.triangle.pull`; status uses existing semantic colors and every icon has a tooltip.
`HideTheme.GitIcon` names refresh (`arrow.clockwise`), merged (`checkmark.circle`), unmerged (`circle`), dirty (`circle.fill`), clean (`checkmark`), merged PR (`arrow.triangle.merge`), closed PR (`xmark.circle`), unavailable (`exclamationmark.circle`), and absent PR (`minus.circle`).

## In-Product Components

This section is the native shell contract.
It applies to the sidebar, checkout cards, tab strip, terminal and browser headers, right panel, status bar, empty and unavailable states, Search, New Agent, Settings, file search, and editor overlays.
The earlier marketing analysis remains reference material; these native values govern the application.
The direction is compact Orca chrome expressed through the existing Hide components, with neutral controls and semantic state marks.
Pet windows, the menu bar dashboard, and native context menus retain their existing appearance.
The dashboard's active preservation tokens remain in the token definition file and its shared rows retain their existing system font.

### Surfaces and text

Frontmatter names below map directly to the same property on `HideTheme`.
Depth comes from the four surfaces and a hairline, without drop shadows.

| Token | Use |
| --- | --- |
| `{colors.background}` | Terminal surround, empty checkout and main canvas |
| `{colors.sidebar}` | Sidebar and navigation base |
| `{colors.panel}` | Pane headers, right panel, status bar and sheet containers |
| `{colors.elevated}` | Selected rows, compact controls, keycaps and input surfaces |
| `{colors.balloon}` | Tooltip surface, one step above elevated |
| `{colors.divider}` | One-point hairline and neutral focus outlines |
| `{colors.primary}` | Primary labels |
| `{colors.secondary}` | Supporting labels and inactive controls |
| `{colors.muted}` | Metadata and unheld search shortcut |
| `{colors.accent}` | Neutral primary action and control tint |

Semantic danger, warning, and success keep their existing state meanings.
Project and provider illustrations retain their category colors.
A selected agent retains the core's state mark and a panel fill; text remains readable on that fill.
No new state or copy is derived in the renderer.

### Typography scale

The bundled Inter variable font uses stylistic set ss03 for chrome.
Keycaps and numeric metadata use the system monospaced face.
Font scale continues to multiply these sizes; terminal and editor content retain their separate content-size tokens.
The font is bundled under its OFL license and does not require installation on the machine.

| Token | Size | Use |
| --- | --- | --- |
| `{typography.micro}` | 9px | Keycaps, small marks and numeric metadata |
| `{typography.caption}` | 10px | Supporting labels and badges |
| `{typography.body}` | 11px | Rows and control labels |
| `{typography.subhead}` | 12px | Tooltip text and explanatory text |
| `{typography.title}` | 13px | Section emphasis |
| `{typography.headline}` | 17px | Sheet headings and empty-state titles |
| `{typography.display}` | 30px | Large empty-state symbol or title |

### Spacing and radius

Frontmatter spacing and radius names map directly to `HideTheme`.
Fixed content geometry remains in its named Layout tokens rather than changing with text emphasis.

| Spacing token | Role |
| --- | --- |
| `{spacing.spacingNone}` | Flush structural stacks |
| `{spacing.spacingXXS}` | Tight label stacks |
| `{spacing.spacingXS}` | Keycap horizontal padding and small gaps |
| `{spacing.spacingSM}` | Control gaps and tooltip horizontal inset |
| `{spacing.spacingMD}` | Compact group padding |
| `{spacing.spacingLG}` | Sheet and panel content inset |
| `{spacing.spacingXL}` | Larger section and empty-state spacing |
| `{spacing.spacingXXL}` | Search empty state |
| `{spacing.spacingXXXL}` | Main empty-state surround |

| Radius token | Role |
| --- | --- |
| `{rounded.radiusExtraSmall}` | Micro shapes |
| `{rounded.radiusSmall}` | Keycaps, inline rows and buttons |
| `{rounded.radiusMedium}` | Tooltips, small cards and controls |
| `{rounded.radiusLarge}` | Search input and larger cards |
| `{rounded.radiusExtraLarge}` | Container vocabulary |

### Recent navigation in the native shell

Control+Tab and Control+Shift+Tab cycle all unified surfaces inside the selected project in recent-use order.
This includes terminal, Browser plugin, file/editor, and diff tabs across every checkout in that project.
Option+Tab and Option+Shift+Tab cycle projects globally and restore each project's last used surface.
Hold the chord's modifier to preview, release it to commit, or press Escape to keep the original selection.
Menu actions commit immediately.
Option+1 through Option+9 select sidebar agents; Command+1 through Command+9 retain direct strip selection.
Agent number hints follow the command registry: reveal only during an exact Option hold, ignoring Caps Lock, and clear on release or a suppressing sheet.
Numbered agent shortcuts are handled before native text interpretation, so terminal and editor responders cannot consume the Option chord.
With no other project or tab available, navigation keeps the current selection without a modal.
Selecting an empty project shows its existing empty state; closing an empty strip or reselecting a checkout whose terminal is starting requires no acknowledgement.
Automatic MRU pruning and concurrent selection recovery use structured diagnostics without a modal.
Workspace and device registration changes and connection-test requests use their existing list or status presentation.
Invalid runtime identities, unavailable devices, and failed operations remain visible, and destructive decisions retain their confirmations.

The project identity is `CoreWorkspaceSnapshot.id`, scoped by device, following the sidebar's Project > Workspace > Agents hierarchy.
Its checkouts are workspaces in that hierarchy, so two checkouts of one repository share a project cycle and a tab history.
Herdr workspaces contributing tabs to those checkouts do not create separate Hide projects.
The existing core catalog determines grouping; navigation does not infer it from display labels or directory names.

Both switchers use the same themed overlay and registry-derived keycaps, with at most nine rows around the highlight.
Project rows show the last surface and checkout; tab rows show their checkout and surface type.
History is session-local and retains only existing projects and surfaces.
A deleted highlight moves to the next surviving entry without reordering the held cycle and records the reconciliation in structured trace.
If none survives, cancel with a structured recovery trace and keep the core's current selection.
Empty projects show “No open tabs”; a project without an available checkout keeps the current selection and records the recovery.

### Keycaps, hint chips, and tooltips

`HideKeycap.swift` owns every shortcut glyph.
An 18-point high keycap uses `{typography.micro}`, medium monospaced weight, `{colors.elevated}`, `{rounded.radiusSmall}`, and a one-point `{colors.divider}` border.
Its horizontal inset is `{spacing.spacingXS}`.
Search keeps its keycap visible in `{colors.muted}` and emphasizes it with `{colors.primary}` during an exact Command hold.
Tab and agent number keycaps reserve their inline space so holding a modifier does not move labels.

`HideBalloon.swift` owns tooltip and floating-hint modes.
Hint mode renders the same keycap.
Tooltip mode uses `{typography.subhead}`, `{colors.balloon}`, `{rounded.radiusMedium}`, the same hairline, horizontal `{spacing.spacingSM}`, and vertical `{spacing.spacingXS}`.
Tooltip width is limited to 360 points.
There is no native tooltip layered underneath it.

Both take a command resolved from the menu registry, effective pane binding, direct selection number, or chordless label.
A chorded tooltip reads label followed by the registry chord in parentheses; chordless controls show only the label.
The identical formatter supplies the control's accessibility help.

`HideOverlay.swift` gathers control anchors into the content root.
The overlay does not take pointer events or add layout space.
A balloon sits four points above its control, flips below when needed, and stays eight points inside the window horizontally.
Tooltip hover delay is 400 milliseconds; exact modifier holds reveal hints after 150 milliseconds.
Release, app deactivation, and opening a sheet clear hints.
Pane focus, active tab, tab order, zoom state, and disappearing anchors update the exposure set.
Pointer exit, mouse down, scroll, key down, resign-key, and anchor removal dismiss tooltips.
Fades last 120 milliseconds, or zero with Reduce Motion enabled.
The event monitor observes and returns key events.

### Icon buttons and badges

`HideIconButton.swift` owns icon-only actions in the sidebar command bar, tab strip, pane headers, and browser toolbar.
Callers provide the symbol, help text, action, optional selection state, and a role; they do not add size, padding, foreground, background, or button-style overrides.
`standard` uses `HideTheme.IconButton.standardSize` (32×32pt) with an elevated resting surface.
`toolbar` uses `HideTheme.IconButton.toolbarSize` (24×24pt) with a transparent resting surface, fitting the 28pt pane header and 32pt tab strip.
Both use `radiusMedium`; icon typography is `body` for standard and `caption` for toolbar, independently of the hit area.
Hover raises foreground contrast and adds `Opacity.subtleFill`; selection uses `Opacity.selectedFill` and the accessibility selected trait.
Press uses `Opacity.secondary`; disabled uses `Opacity.disabled`, suppresses hover emphasis, and delegates activation blocking to the native Button.
Hover state stays local to each button; repeated identical hover events publish no state changes, and no runtime dispatch or new timer is added.
The shared command tooltip retains pane/tab targets, and the accessibility label defaults to help unless a more specific name is supplied.

```swift
HideIconButton(
    systemImage: "plus",
    help: "New Tab",
    variant: .toolbar,
    command: .menu(.newTab),
    action: model.addTab
)
```

Text buttons, menu triggers, title-bearing navigation rows, and the agent lineage renderer retain their own components and semantics.
Workspace disclosure uses its entire 36pt row, so its chevron is an indicator rather than an icon button.
`HideBadge.swift` owns compact labels, with `HideTheme.badgeHeight` (16pt); agent provider artwork remains in the existing agent badge.
A state keeps its symbol and semantic color when read, with reduced emphasis instead of a new word.

### Sheets, overlays, and abnormal states

Search, New Agent, Settings, and file search each host the same tooltip overlay.
Sheets use panel containers, headline titles, body or caption supporting text, and the same spacing scale.
The selected provider card uses an elevated fill and stronger neutral border; its status remains readable.
Disabled Start and Add controls retain their existing enablement conditions and use disabled emphasis.

The right panel's Explorer and Changes sections use the same panel and text ladder.
The project sidebar requests GitHub data once when a local Git project appears; repeated appearances reuse the same result.
The Git section retains its existing open/refresh trigger, while the sidebar popover and project menu can explicitly refresh one repository.
All triggers share the existing bounded background reader, authentication and cache.
Explorer and Changes do not independently start GitHub queries.
Loading, missing authentication, query failure and stale results remain explicit; an absent or unrecognized CI result never renders as passing.
Changes is a compact navigation list; activating a row opens a read-only diff as a central editor tab instead of dividing the panel vertically.
Diff tabs use the editor's monospaced content scale, fixed old and new line-number columns, semantic added and removed tints, and horizontal scrolling for long lines.
Their scroll canvas fills the editor viewport, with short diffs anchored at the top left and long diffs growing beyond it for scrolling.
Text file tabs use a fixed line-number ruler and preserve source whitespace through non-wrapping horizontal scrolling.
The ruler clips all drawing to its own bounds, and text loaded into an initially empty editor retains the editor's monospaced content font.
Syntax selection comes from the core's filename-aware language result, including extensionless configuration files and JSON-family extensions.
Loading and failed tree states, empty sidebar and checkout, missing pane projection, waiting pane size, browser connecting or disconnected, and editor conflict or stale banners use the same tokens as normal state.
Remote and browser idle, loading, ready, stale, unavailable, and failed phases preserve their existing labels and semantic status colors.
Content and accessibility identifiers are unchanged; only the visual hierarchy changes.

### Enforcement

Add a named token before using a new visual value.
`scripts/check-hide-theme-literals.sh` rejects inline styling, native tooltips, and shortcut glyph literals outside token definitions and Pet files.
`scripts/check-hide-components.sh` prevents duplicated component ownership and checks every migrated tooltip file.
The shell test parses this document's frontmatter and typography table against actual token values.
