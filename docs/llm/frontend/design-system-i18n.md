# Web -- i18n, Timezone & Date Formatting

> SSOT | **Last Updated**: 2026-03-04 | Split from design-system.md

Related files:
- [design-system.md](design-system.md) -- brand, tokens, theme, nav, DataTable, state management
- [design-system-components.md](design-system-components.md) -- auth guard, login, API client, status colors, flow viz

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new i18n key | `web/messages/en.json` -> `ko.json` -> `ja.json` | Always add to all 3 locales |
| Add new locale | `web/i18n/config.ts` `locales[]` + new `web/messages/{locale}.json` | Copy en.json, translate; add locale->timezone default in `timezone-provider.tsx` |
| Display a new date/time field | `web/lib/date.ts` + component | Import formatter; call `useTimezone()` in component |
| Change language detection order | `web/i18n/index.ts` | `localStorage('hg-lang')` -> `navigator.language` -> `'en'` |
| Change timezone cookie | `web/components/timezone-provider.tsx` | Cookie name: `veronex-tz`, 1-year expiry |

## Key Files

| File | Purpose |
|------|---------|
| `web/i18n/config.ts` | `locales[]`, `localeLabels{}`, `defaultLocale` |
| `web/i18n/index.ts` | i18next init |
| `web/messages/en.json` | Source of truth for all i18n keys |
| `web/messages/ko.json` | Korean translations |
| `web/messages/ja.json` | Japanese translations |
| `web/components/i18n-provider.tsx` | react-i18next wrapper |
| `web/components/timezone-provider.tsx` | `TimezoneProvider` + `useTimezone()` hook; cookie `veronex-tz` |
| `web/lib/date.ts` | Centralized date formatters |

---

## i18n Setup

- 3 locales: `en` (default), `ko`, `ja`
- Labels: `en: 'English'`, `ko: '한국어'`, `ja: '日本語'`
- Detection order: `localStorage('hg-lang')` -> `navigator.language` -> `'en'`

### Adding i18n Keys

1. Add key to `web/messages/en.json` (source of truth)
2. Add to `web/messages/ko.json` (Korean)
3. Add to `web/messages/ja.json` (Japanese)
4. Use: `const { t } = useTranslation()` -> `t('section.key')`

### i18n Key Namespaces

| Namespace | Scope |
|-----------|-------|
| `nav.*` | Sidebar navigation labels |
| `common.*` | Settings dialog, timezone presets, shared labels |
| `auth.*` | Login page labels |
| `overview.*` | Dashboard page, network flow, provider taxonomy |
| `jobs.*` | Jobs page, job table, session grouping |
| `keys.*` | API keys page, KeyUsageModal |
| `usage.*` | Usage page, breakdown sections |
| `performance.*` | Performance page |
| `providers.servers.*` | Servers page |
| `providers.ollama.*` | Ollama tab |
| `providers.gemini.*` | Gemini tab |
| `providers.capacity.*` | Concurrency control section |

---

## Timezone

Timezone is stored in cookie `veronex-tz` (1-year expiry, `SameSite=Lax`).

### Supported Timezones (IANA)

Preset timezones appear in the Settings dialog (gear icon in nav footer). Users can also enter any IANA identifier via "Custom..." (shown inline within the same dialog).

| Value | i18n key | Label (en) | Offset |
|-------|----------|------------|--------|
| `UTC` | `common.utc` | UTC | UTC+0 |
| `America/New_York` | `common.eastern` | Eastern (ET) | UTC-5/-4 |
| `America/Chicago` | `common.central` | Central (CT) | UTC-6/-5 |
| `America/Denver` | `common.mountain` | Mountain (MT) | UTC-7/-6 |
| `America/Los_Angeles` | `common.pacific` | Pacific (PT) | UTC-8/-7 |
| `Europe/London` | `common.london` | London (GMT) | UTC+0/+1 |
| `Africa/Johannesburg` | `common.johannesburg` | South Africa (SAST) | UTC+2 |
| `Asia/Seoul` | `common.kst` | Korea (KST) | UTC+9 |
| `Asia/Tokyo` | `common.jst` | Japan (JST) | UTC+9 |
| `Australia/Sydney` | `common.sydney` | Sydney (AEST) | UTC+10/+11 |
| `Pacific/Auckland` | `common.auckland` | Auckland (NZST) | UTC+12/+13 |
| _(any IANA)_ | `common.custom` | Custom... | -- |

**Custom timezone**: Selecting "Custom..." reveals an inline IANA input within the Settings dialog (no nested modal). Validated via `isValidTimezone()` (Intl.DateTimeFormat). The `Timezone` type is `PresetTimezone | (string & {})`. The cookie stores the raw IANA string; on reload, `readCookie()` validates before accepting.

### Locale -> Default Timezone

When `veronex-tz` cookie is absent, `TimezoneProvider` picks a default from locale:

| Locale | Default |
|--------|---------|
| `ko` | `Asia/Seoul` |
| `ja` | `Asia/Tokyo` |
| `en` (or any) | `America/New_York` |

Changing language in the nav calls `resetToLocaleDefault(locale)` -- only takes effect if no explicit cookie. User-selected timezone is sticky.

---

## Date Formatter SSOT (`web/lib/date.ts`)

All date display goes through these functions -- never call `toLocaleString()` or `toLocaleDateString()` directly.

```ts
import { useTimezone } from '@/components/timezone-provider'
import { fmtDatetime, fmtDatetimeShort, fmtDateOnly, fmtHourLabel } from '@/lib/date'

const { tz } = useTimezone()

fmtDatetime(iso, tz)       // "Mar 1, 12:34:56" -- job detail, audit
fmtDatetimeShort(iso, tz)  // "Mar 1, 12:34"    -- dashboard recent jobs
fmtDateOnly(iso, tz)       // "Mar 1, 2026"     -- API keys, registered_at
fmtHourLabel(iso, tz)      // "3/1 14h"         -- hourly chart x-axis
```

Backend always returns ISO 8601 UTC strings. All timezone conversion is client-side only.

### Backend UTC Guarantee

All PostgreSQL columns use `TIMESTAMPTZ` -- stored and returned as UTC. sqlx deserializes to `DateTime<Utc>` -- serialized to ISO 8601 with `Z` suffix. No timezone info on the server.

---

## i18n Keys Reference (common.*)

**Timezone**: `timezone`, `utc`, `eastern`, `central`, `mountain`, `pacific`, `london`, `johannesburg`, `kst`, `jst`, `sydney`, `auckland`, `custom`, `customTimezone`, `customTimezonePlaceholder`, `customTimezoneHint`, `customTimezoneInvalid`

**Settings / language**: `settings`, `language`

## i18n Keys Reference (nav.*)

`overview`, `monitor`, `dashboard`, `flow`, `jobs`, `keys`, `usage`, `performance`, `servers`, `providers`, `ollama`, `gemini`, `accounts`, `audit`, `apiDocs`

## Translation Workflow

1. English (`en.json`) is the source of truth -- add keys here first
2. Copy key structure to `ko.json` and `ja.json`, translate values
3. Use `t('namespace.key')` in components -- never hardcode user-visible strings
4. For interpolation: `t('overview.queueWaiting', { count: n })` -- `{{count}} waiting`
5. Test all 3 locales after adding keys (Settings dialog language selector)
