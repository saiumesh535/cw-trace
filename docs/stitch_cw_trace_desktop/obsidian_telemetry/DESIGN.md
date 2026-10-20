---
name: Obsidian Telemetry
colors:
  surface: '#0f131c'
  surface-dim: '#0f131c'
  surface-bright: '#353942'
  surface-container-lowest: '#0a0e16'
  surface-container-low: '#181c24'
  surface-container: '#1c2028'
  surface-container-high: '#262a33'
  surface-container-highest: '#31353e'
  on-surface: '#dfe2ee'
  on-surface-variant: '#e0c0b1'
  inverse-surface: '#dfe2ee'
  inverse-on-surface: '#2c3039'
  outline: '#a78b7d'
  outline-variant: '#584237'
  surface-tint: '#ffb690'
  primary: '#ffb690'
  on-primary: '#552100'
  primary-container: '#f97316'
  on-primary-container: '#582200'
  inverse-primary: '#9d4300'
  secondary: '#7bd0ff'
  on-secondary: '#00354a'
  secondary-container: '#00a6e0'
  on-secondary-container: '#00374d'
  tertiary: '#d0bcff'
  on-tertiary: '#3c0091'
  tertiary-container: '#a985ff'
  on-tertiary-container: '#3e0096'
  error: '#ffb4ab'
  on-error: '#690005'
  error-container: '#93000a'
  on-error-container: '#ffdad6'
  primary-fixed: '#ffdbca'
  primary-fixed-dim: '#ffb690'
  on-primary-fixed: '#341100'
  on-primary-fixed-variant: '#783200'
  secondary-fixed: '#c4e7ff'
  secondary-fixed-dim: '#7bd0ff'
  on-secondary-fixed: '#001e2c'
  on-secondary-fixed-variant: '#004c69'
  tertiary-fixed: '#e9ddff'
  tertiary-fixed-dim: '#d0bcff'
  on-tertiary-fixed: '#23005c'
  on-tertiary-fixed-variant: '#5516be'
  background: '#0f131c'
  on-background: '#dfe2ee'
  surface-variant: '#31353e'
typography:
  headline-xl:
    fontFamily: Geist
    fontSize: 24px
    fontWeight: '600'
    lineHeight: 32px
    letterSpacing: -0.02em
  headline-lg:
    fontFamily: Geist
    fontSize: 18px
    fontWeight: '600'
    lineHeight: 24px
    letterSpacing: -0.015em
  headline-md:
    fontFamily: Geist
    fontSize: 15px
    fontWeight: '500'
    lineHeight: 20px
    letterSpacing: -0.01em
  body-lg:
    fontFamily: Geist
    fontSize: 14px
    fontWeight: '400'
    lineHeight: 20px
  body-md:
    fontFamily: Geist
    fontSize: 13px
    fontWeight: '400'
    lineHeight: 18px
  body-sm:
    fontFamily: Geist
    fontSize: 12px
    fontWeight: '400'
    lineHeight: 16px
  code-lg:
    fontFamily: JetBrains Mono
    fontSize: 13px
    fontWeight: '500'
    lineHeight: 18px
  code-md:
    fontFamily: JetBrains Mono
    fontSize: 12px
    fontWeight: '400'
    lineHeight: 16px
  code-sm:
    fontFamily: JetBrains Mono
    fontSize: 11px
    fontWeight: '400'
    lineHeight: 14px
    letterSpacing: -0.01em
  label-md:
    fontFamily: JetBrains Mono
    fontSize: 11px
    fontWeight: '500'
    lineHeight: 14px
    letterSpacing: 0.04em
  label-sm:
    fontFamily: JetBrains Mono
    fontSize: 10px
    fontWeight: '600'
    lineHeight: 12px
    letterSpacing: 0.06em
rounded:
  sm: 0.125rem
  DEFAULT: 0.25rem
  md: 0.375rem
  lg: 0.5rem
  xl: 0.75rem
  full: 9999px
spacing:
  gutter: 0.75rem
  gutter-sm: 0.5rem
  gutter-lg: 1rem
  margin: 1rem
  margin-sm: 0.5rem
  margin-lg: 1.5rem
  space-xs: 0.25rem
  space-sm: 0.5rem
  space-md: 0.75rem
  space-lg: 1rem
  space-xl: 1.5rem
---

## Brand & Style
The design system establishes a high-density, terminal-adjacent developer control plane built for local-first execution and live cloud telemetry debugging. Its aesthetic derives from the convergence of mechanical studio equipment, Unix CLI interfaces, and modern engineering workbenches like Linear, Raycast, and the SST console. 

The visual personality is technical, silent, and razor-sharp. It purposefully omits consumer SaaS paradigms—such as collaborative team avatars, enterprise workspace switchers, and marketing-driven onboarding cards—in favor of raw operational efficiency, immediate keystroke response, and maximum vertical data density. The interface treats logs, distributed spans, execution timing, and resource graphs not as passive dashboards, but as tactile, real-time diagnostic instruments.

## Colors
The palette is rooted in an abyss-level deep slate canvas, avoiding uncalibrated true black (#000000) to maintain crisp subpixel anti-aliasing on code text.

### Surface Tiers
- **Canvas Base:** `#0B0F17` (App backdrop and root layout layer)
- **Surface Layer 1:** `#0F172A` (Panels, sidebar, trace view splits)
- **Surface Layer 2:** `#151E2E` (Selected cards, active stream rows, modal overlays)
- **Surface Accent / Hover:** `#1E293B` (Row hovers, toolbar inputs, tag fills)

### Accent & Diagnostic Identifiers
- **Primary Telemetry Amber:** `#F97316` (Active trace focus, warning states, primary execution trigger, SST invocation status).
- **Secondary Cyan:** `#38BDF8` (AWS resource tags, HTTP verbs, cold-start indicators).
- **Tertiary Violet:** `#8B5CF6` (Span IDs, correlation IDs, trace lineage).

### Status System
- **Nominal / 2xx:** `#10B981` (Healthy stages, zero-latency executions, success pills).
- **Error / 5xx:** `#EF4444` (Exceptions, invocation timeouts, memory breaches).
- **Borders & Dividers:** `rgba(255, 255, 255, 0.08)` on dark surfaces, reinforced to `#334155` for active panel dividers and active splitters.

## Typography
The system enforces a dual-type engine:
1. **Geist** handles spatial UI layouts, structural titles, status bar alerts, and component labels where human legibility at dense scales is imperative.
2. **JetBrains Mono** governs all raw computation primitives: ARN strings, payload inspectors, log timestamps, execution durations, memory gauges, and keyboard shortcuts.

All text styles below 13px employ tabular numeric figures (`tnum`) and slashed zeros to prevent horizontal jitter during rapid real-time telemetry streaming. Labels use uppercase tracking (`+0.04em` to `+0.06em`) strictly for technical metadata attributes (e.g., `INIT_DURATION`, `STATUS`, `REGION`).

## Layout & Spacing
The layout relies on an industrial, multi-pane docking model composed of fluid columns and resizable side rails.

- **Layout Grid & Split Views:** Default layout uses a fixed 240px collateral navigation/resource rail, an expandable multi-stage log and trace waterfall list (minimum 400px), and a collapsible right-hand telemetry inspector pane (minimum 480px).
- **Rhythm & Density:** Every element conforms strictly to an 8-point base grid with 4-point micro-steps (`space-xs`, `space-sm`). Log rows default to a strict 28px vertical line height, allowing up to 35 concurrent log frames on standard developer displays without scroll pagination.
- **Breakpoints:**
  - `Mobile / Compact (< 768px)`: Monolithic single-pane view with segment controller switching between Console, Traces, and Inspect.
  - `Tablet (768px - 1199px)`: Dual-pane layout (Resource list collapses into a command-palette style popover via `Cmd+K`).
  - `Desktop (>= 1200px)`: Full three-pane panoramic debugger with synchronous scroll lock.

## Elevation & Depth
Depth is produced via subtle tonal stepping and structural hairline borders rather than diffuse drop shadows.

- **Borders over Shadows:** Deep dark interfaces lose shadow definitions. Surfaces differentiate by shifting fill tones (`#0B0F17` -> `#0F172A` -> `#151E2E`) separated by 1px sub-borders (`rgba(255, 255, 255, 0.08)`).
- **Transient Depth:** Modals, fuzzy-search finders, and popovers utilize an outer border of `#334155` accompanied by an ultra-crisp directional glow: `0 8px 32px -4px rgba(0, 0, 0, 0.7), 0 0 0 1px rgba(255, 255, 255, 0.1)`.
- **Active State Keylines:** Active spans or focused log streams display a 2px left border keyline tinted in Electric Amber (`#F97316`) or status colors, with an inner bleed gradient: `linear-gradient(90deg, rgba(249, 115, 22, 0.08) 0%, transparent 100%)`.

## Shapes
The shape language uses soft engineering corners:
- Standard cards, split views, and inspector panels use `0.375rem` (6px) or `0.5rem` (8px).
- Badges, status pills, and monospaced keybinds use `0.25rem` (4px).
- Interactive buttons and inputs use `0.375rem` (6px).
- Full rounded pill shapes are strictly restricted to live connection indicators and active subscription stage dots.

## Components

### Buttons & Quick Actions
- **Primary:** Solid `#F97316` fill, `#0B0F17` bold typography, `0.375rem` border radius, micro-inset top highlight. State change on hover: brightness increase to `#FB923C`.
- **Secondary / Ghost:** Transparent background, `1px solid rgba(255, 255, 255, 0.08)`, text color `#94A3B8`. On hover: surface `#1E293B`, text `#F8FAFC`.
- **Size Scale:** Standard interactive height is 28px (`h-7`) for dense toolbars, 32px (`h-8`) for primary canvas actions.

### Telemetry Badges & Chips
- **Monospace Badges:** High-contrast, low-saturation fills. Example: Status 200 is `rgba(16, 185, 129, 0.1)` fill, `rgba(16, 185, 129, 0.3)` border, `#34D399` text.
- **Trace Spans:** JetBrains Mono, `11px`, with custom horizontal execution bars. Cold starts are badged with a `#38BDF8` snowflake glyph and a dotted outline.

### Input Fields & Filter Bars
- **Log Stream Filters:** Surface `#0F172A`, inner border `1px solid rgba(255, 255, 255, 0.08)`, focus ring `1px solid #F97316`. Monospaced text input with embedded keyboard navigation shortcut chip (`Esc`, `/`) right-aligned.

### Waterfall / Trace Trees
- Rows use alternate striping: `#0B0F17` and `#0E1420`.
- Left-indented hierarchical tree lines colored `#1E293B`.
- Timing bars rendered as micro-ranges (4px height) with rounded caps, colored conditionally: orange for invocation bottleneck, violet for AWS SDK client calls, and emerald for fast database queries.

### Inspection Cards
- Bordered panels containing structured JSON payloads, AWS Lambda request context, and environment variables. Key-value layouts align keys strictly to the right using `#64748B` and values to the left using `#F1F5F9` with automatic clipboard copy on click.