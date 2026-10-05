import { requireInterfaceLanguage, type InterfaceLanguage } from "./locale";

type Formats = {
  number: Intl.NumberFormat;
  integer: Intl.NumberFormat;
  decimal: Intl.NumberFormat;
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
    integer: new Intl.NumberFormat(language, { maximumFractionDigits: 0, useGrouping: false }),
    decimal: new Intl.NumberFormat(language, { minimumFractionDigits: 1, maximumFractionDigits: 1, useGrouping: false }),
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

/** Preserve the product's binary scaling and familiar unit abbreviations; a size never carries a group separator, so `1008 KB` reads the same in every language. */
export function formatBytes(language: InterfaceLanguage, bytes: number): string {
  const selected = bundle(language);
  finite(bytes);
  if (bytes < 0) throw new Error("invalid_localized_size");
  const units = ["B", "KB", "MB", "GB", "TB"] as const;
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const number = unit === 0 ? selected.integer.format(Math.trunc(value)) : (value < 10 ? selected.decimal : selected.integer).format(value);
  return `${number} ${units[unit]}`;
}

/** The cleanup result always shows gigabytes to one decimal place. */
export function formatGigabytes(language: InterfaceLanguage, bytes: number): string {
  finite(bytes);
  if (bytes < 0) throw new Error("invalid_localized_size");
  return `${bundle(language).decimal.format(bytes / 1024 ** 3)} GB`;
}
