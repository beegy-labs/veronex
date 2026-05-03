/**
 * Design Token Module — type-safe CSS custom property references.
 *
 * Rule: Never write `var(--vds-theme-*)` magic strings in component files.
 *       Import from this module for all inline-style `style={{}}` usage.
 *
 * For className strings, use the verodesign utility classes
 * (`vds-bg-success`, `vds-text-primary`, etc).
 *
 * SSOT: @verobee/design themes + globals.css overrides.
 */

export const tokens = {
  bg: {
    page:     'var(--vds-theme-bg-page)',
    card:     'var(--vds-theme-bg-card)',
    elevated: 'var(--vds-theme-bg-elevated)',
    hover:    'var(--vds-theme-bg-hover)',
    muted:    'var(--vds-theme-bg-muted)',
    code:     'var(--vds-theme-bg-code)',
    inverse:  'var(--vds-theme-bg-inverse)',
  },

  text: {
    primary:   'var(--vds-theme-text-primary)',
    secondary: 'var(--vds-theme-text-secondary)',
    bright:    'var(--vds-theme-text-bright)',
    dim:       'var(--vds-theme-text-dim)',
    faint:     'var(--vds-theme-text-faint)',
    inverse:   'var(--vds-theme-text-inverse)',
  },

  border: {
    base:    'var(--vds-theme-border-default)',
    subtle:  'var(--vds-theme-border-subtle)',
    default: 'var(--vds-theme-border-default)',
    strong:  'var(--vds-theme-border-strong)',
    focus:   'var(--vds-theme-border-focus)',
  },

  brand: {
    primary:    'var(--vds-theme-primary)',
    foreground: 'var(--vds-theme-primary-fg)',
    ring:       'var(--vds-theme-primary-ring)',
    focusRing:  'var(--vds-theme-border-focus)',
  },

  status: {
    success:   'var(--vds-theme-success)',
    error:     'var(--vds-theme-error)',
    warning:   'var(--vds-theme-warning)',
    info:      'var(--vds-theme-info)',
    cancelled: 'var(--vds-theme-cancelled)',
  },

  statusFg: {
    success: 'var(--vds-theme-success-fg)',
    error:   'var(--vds-theme-error-fg)',
    warning: 'var(--vds-theme-warning-fg)',
    info:    'var(--vds-theme-info-fg)',
  },

  accent: {
    gpu:   'var(--vds-theme-accent-2)',
    power: 'var(--vds-theme-warning)',
    brand: 'var(--vds-theme-primary)',
  },

  chart: {
    c1: 'var(--vds-theme-chart-1)',
    c2: 'var(--vds-theme-chart-2)',
    c3: 'var(--vds-theme-chart-3)',
    c4: 'var(--vds-theme-chart-4)',
    c5: 'var(--vds-theme-chart-5)',
  },

  logo: {
    start: 'var(--vds-theme-logo-start)',
    end:   'var(--vds-theme-logo-end)',
    inner: 'var(--vds-theme-logo-inner)',
  },
} as const

export type ThemeToken = (typeof tokens)[keyof typeof tokens][keyof (typeof tokens)[keyof typeof tokens]]
