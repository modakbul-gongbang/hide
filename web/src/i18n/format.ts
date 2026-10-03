import { requireInterfaceLanguage, type InterfaceLanguage } from "./locale";

type Formats = {
  number: Intl.NumberFormat;
  percent: Intl.NumberFormat;
  relative: Intl.RelativeTimeFormat;
  units: Readonly<Record<"second" | "minute" | "hour" | "day", Intl.NumberFormat>>;
};

/** At most four bundles, one for each supported language; no timer or I/O. */
const formats = new Map<InterfaceLanguage, Formats>();

function bundle(selected: InterfaceLanguage): Formats {
  const language = requireInterfaceLanguage(selected);
  const cached = formats.get(language);
  if (cached) return cached;
  const unit = (name: "second" | "minute" | "hour" | "day") => new Intl.NumberFormat(language, { style: "unit", unit: name, unitDisplay: "narrow" });
  const next: Formats = {
    number: new Intl.NumberFormat(language),
    percent: new Intl.NumberFormat(language, { style: "percent", maximumFractionDigits: 0 }),
    relative: new Intl.RelativeTimeFormat(language, { style: "short", numeric: "auto" }),
    units: { second: unit("second"), minute: unit("minute"), hour: unit("hour"), day: unit("day") },
  };
  formats.set(language, next);
  return next;
}

function finite(value: number): number {
  if (!Number.isFinite(value)) throw new Error("invalid_localized_number");
  return value;
}

export function formatNumber(language: InterfaceLanguage, value: number): string {
  return bundle(language).number.format(finite(value));
}

/** Provider usage is reported in percentage points, such as 70 for 70%. */
export function formatPercent(language: InterfaceLanguage, percent: number): string {
  return bundle(language).percent.format(finite(percent) / 100);
}

/** Absolute display dates use the selected UI language and the host timezone. */
export function formatDateTime(language: InterfaceLanguage, unixMs: number, options: Intl.DateTimeFormatOptions): string {
  requireInterfaceLanguage(language);
  finite(unixMs);
  const date = new Date(unixMs);
  if (!Number.isFinite(date.getTime())) throw new Error("invalid_localized_date");
  return new Intl.DateTimeFormat(language, options).format(date);
}

export function formatUnit(language: InterfaceLanguage, value: number, unit: "second" | "minute" | "hour" | "day"): string {
  return bundle(language).units[unit].format(finite(value));
}

/** Relative labels already carry their localized past/future wording. */
export function formatRelativeTime(language: InterfaceLanguage, value: number, unit: Intl.RelativeTimeFormatUnit): string {
  return bundle(language).relative.format(finite(value), unit);
}
