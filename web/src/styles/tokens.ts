/**
 * Design System Tokens
 * Single source of truth for all visual constants.
 * Import from here instead of hardcoding values in components.
 */

// ─── Color Palette ───────────────────────────────────────────────────────────

export const palette = {
  // Brand
  violet50:  '#f5f3ff',
  violet100: '#ede9fe',
  violet200: '#ddd6fe',
  violet300: '#c4b5fd',
  violet400: '#a78bfa',
  violet500: '#8b5cf6',
  violet600: '#7c3aed',
  violet700: '#6d28d9',
  violet800: '#5b21b6',
  violet900: '#4c1d95',

  // Neutral (dark-first)
  neutral0:   '#ffffff',
  neutral50:  '#f8fafc',
  neutral100: '#f1f5f9',
  neutral200: '#e2e8f0',
  neutral300: '#cbd5e1',
  neutral400: '#94a3b8',
  neutral500: '#64748b',
  neutral600: '#475569',
  neutral700: '#334155',
  neutral800: '#1e293b',
  neutral850: '#141c2e',
  neutral900: '#0f172a',
  neutral950: '#080e1a',

  // Surface layers
  surface0: '#07090f',   // app background
  surface1: '#0d1117',   // base card
  surface2: '#131920',   // elevated card
  surface3: '#1a2232',   // hover / active

  // Semantic
  green400:  '#4ade80',
  green500:  '#22c55e',
  green600:  '#16a34a',
  amber400:  '#fbbf24',
  amber500:  '#f59e0b',
  red400:    '#f87171',
  red500:    '#ef4444',
  blue400:   '#60a5fa',
  blue500:   '#3b82f6',
} as const

// ─── Semantic Tokens ─────────────────────────────────────────────────────────

export const color = {
  // Brand
  primary:        palette.violet500,
  primaryHover:   palette.violet400,
  primaryActive:  palette.violet600,
  primarySubtle:  'rgba(139,92,246,0.12)',
  primaryBorder:  'rgba(139,92,246,0.30)',

  // Backgrounds
  bgApp:          palette.surface0,
  bgBase:         palette.surface1,
  bgElevated:     palette.surface2,
  bgHover:        palette.surface3,
  bgOverlay:      'rgba(7,9,15,0.85)',

  // Borders
  borderSubtle:   'rgba(255,255,255,0.06)',
  borderDefault:  'rgba(255,255,255,0.10)',
  borderStrong:   'rgba(255,255,255,0.18)',

  // Text
  textPrimary:    palette.neutral50,
  textSecondary:  palette.neutral400,
  textTertiary:   palette.neutral500,
  textDisabled:   palette.neutral600,
  textInverse:    palette.neutral900,

  // Status
  success:        palette.green500,
  successSubtle:  'rgba(34,197,94,0.10)',
  successBorder:  'rgba(34,197,94,0.25)',
  warning:        palette.amber500,
  warningSubtle:  'rgba(245,158,11,0.10)',
  warningBorder:  'rgba(245,158,11,0.25)',
  error:          palette.red500,
  errorSubtle:    'rgba(239,68,68,0.10)',
  errorBorder:    'rgba(239,68,68,0.25)',
  info:           palette.blue500,
  infoSubtle:     'rgba(59,130,246,0.10)',
  infoBorder:     'rgba(59,130,246,0.25)',
} as const

// ─── Typography ──────────────────────────────────────────────────────────────

export const font = {
  family: "-apple-system, BlinkMacSystemFont, 'Inter', 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif",
  familyMono: "'JetBrains Mono', 'Fira Code', 'Cascadia Code', Menlo, Monaco, 'Courier New', monospace",

  size: {
    xs:   11,
    sm:   12,
    base: 13,
    md:   14,
    lg:   15,
    xl:   16,
    '2xl': 18,
    '3xl': 20,
    '4xl': 24,
  },

  weight: {
    normal:   400,
    medium:   500,
    semibold: 600,
    bold:     700,
  },

  lineHeight: {
    tight:  1.3,
    normal: 1.5,
    loose:  1.75,
  },
} as const

// ─── Spacing ─────────────────────────────────────────────────────────────────

export const space = {
  0:   0,
  1:   4,
  2:   8,
  3:   12,
  4:   16,
  5:   20,
  6:   24,
  7:   28,
  8:   32,
  10:  40,
  12:  48,
  16:  64,
} as const

// ─── Border Radius ───────────────────────────────────────────────────────────

export const radius = {
  sm:   4,
  md:   6,
  lg:   8,
  xl:   10,
  '2xl': 12,
  '3xl': 16,
  full: 9999,
} as const

// ─── Shadows ─────────────────────────────────────────────────────────────────

export const shadow = {
  sm:  '0 1px 2px rgba(0,0,0,0.4)',
  md:  '0 4px 12px rgba(0,0,0,0.5)',
  lg:  '0 8px 24px rgba(0,0,0,0.6)',
  xl:  '0 16px 48px rgba(0,0,0,0.7)',
  glow: '0 0 20px rgba(139,92,246,0.25)',
} as const

// ─── Transitions ─────────────────────────────────────────────────────────────

export const transition = {
  fast:   'all 0.12s ease',
  base:   'all 0.18s ease',
  slow:   'all 0.28s ease',
  spring: 'all 0.22s cubic-bezier(0.34,1.56,0.64,1)',
} as const

// ─── Layout ──────────────────────────────────────────────────────────────────

export const layout = {
  headerHeight:   56,
  sidebarWidth:   220,
  contentMaxWidth: 1200,
  contentPadding: 32,
} as const

// ─── Ant Design Theme Tokens ─────────────────────────────────────────────────

export const antdTokens = {
  colorPrimary:       color.primary,
  colorLink:          palette.violet400,
  colorSuccess:       color.success,
  colorWarning:       color.warning,
  colorError:         color.error,
  colorInfo:          color.info,

  borderRadius:       radius.lg,
  borderRadiusSM:     radius.md,
  borderRadiusLG:     radius.xl,

  fontFamily:         font.family,
  fontSize:           font.size.md,
  fontSizeSM:         font.size.base,
  fontSizeLG:         font.size.lg,

  controlHeight:      36,
  controlHeightSM:    28,
  controlHeightLG:    44,

  colorBgContainer:   color.bgElevated,
  colorBgElevated:    color.bgElevated,
  colorBgLayout:      color.bgApp,
  colorBgSpotlight:   color.bgHover,

  colorBorder:        color.borderDefault,
  colorBorderSecondary: color.borderSubtle,

  colorText:          color.textPrimary,
  colorTextSecondary: color.textSecondary,
  colorTextTertiary:  color.textTertiary,
  colorTextDisabled:  color.textDisabled,

  colorFill:          color.bgHover,
  colorFillSecondary: color.bgElevated,
  colorFillTertiary:  color.bgBase,

  lineWidth:          1,
  lineType:           'solid',

  motionDurationFast: '0.12s',
  motionDurationMid:  '0.18s',
  motionDurationSlow: '0.28s',
} as const

export const antdComponentTokens = {
  Card: {
    paddingLG:        20,
    headerBg:         'transparent',
    colorBorderSecondary: color.borderSubtle,
  },
  Button: {
    controlHeight:    40,
    controlHeightSM:  30,
    controlHeightLG:  44,
    fontWeight:       font.weight.medium,
    primaryShadow:    'none',
    defaultShadow:    'none',
    dangerShadow:     'none',
  },
  Menu: {
    itemBg:           'transparent',
    itemSelectedBg:   color.primarySubtle,
    itemHoverBg:      color.bgHover,
    horizontalItemSelectedColor: palette.violet400,
    itemSelectedColor: palette.violet400,
    itemColor:        color.textSecondary,
    itemHoverColor:   color.textPrimary,
    activeBarBorderWidth: 0,
  },
  Input: {
    colorBgContainer: color.bgBase,
    activeBorderColor: color.primary,
    hoverBorderColor: color.borderStrong,
  },
  Select: {
    colorBgContainer: color.bgBase,
    optionSelectedBg: color.primarySubtle,
  },
  Slider: {
    trackBg:          color.primary,
    trackHoverBg:     color.primaryHover,
    handleColor:      color.primary,
    handleActiveColor: color.primaryHover,
    railBg:           color.bgHover,
    railHoverBg:      color.borderDefault,
  },
  Progress: {
    defaultColor:     color.primary,
  },
  Alert: {
    colorInfoBg:      color.infoSubtle,
    colorInfoBorder:  color.infoBorder,
    colorSuccessBg:   color.successSubtle,
    colorSuccessBorder: color.successBorder,
    colorWarningBg:   color.warningSubtle,
    colorWarningBorder: color.warningBorder,
    colorErrorBg:     color.errorSubtle,
    colorErrorBorder: color.errorBorder,
  },
  Collapse: {
    colorBgContainer: color.bgElevated,
    headerBg:         color.bgElevated,
    contentBg:        color.bgBase,
  },
  Descriptions: {
    colorSplit:       color.borderSubtle,
    labelBg:          color.bgBase,
  },
  Tag: {
    defaultBg:        color.bgHover,
    defaultColor:     color.textSecondary,
  },
  Badge: {
    colorBgContainer: color.bgElevated,
  },
  Steps: {
    colorPrimary:     color.primary,
    colorSplit:       color.borderSubtle,
  },
  Tabs: {
    inkBarColor:      color.primary,
    itemSelectedColor: palette.violet400,
    itemHoverColor:   color.textPrimary,
    itemColor:        color.textSecondary,
    cardBg:           color.bgElevated,
  },
  Modal: {
    contentBg:        color.bgElevated,
    headerBg:         color.bgElevated,
    footerBg:         color.bgElevated,
  },
  Drawer: {
    colorBgElevated:  color.bgElevated,
  },
  Table: {
    colorBgContainer: color.bgElevated,
    headerBg:         color.bgBase,
    rowHoverBg:       color.bgHover,
    borderColor:      color.borderSubtle,
  },
  Form: {
    labelColor:       color.textSecondary,
    labelFontSize:    font.size.base,
  },
} as const
