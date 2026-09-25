// The web shell's shared controls for Settings and the management dialogs:
// the same buttons, fields, grouped rows and modal every one of those screens
// draws, on the generated tokens. They mirror the native shared control family
// (HideTextButtonStyle, HideSettingsGroup/Row, HideSettingsField) so a web
// sheet reads like the native one it replaces.

import { useEffect, useRef, type ButtonHTMLAttributes, type InputHTMLAttributes, type KeyboardEvent, type ReactNode, type SelectHTMLAttributes } from "react";
import { restoreFocus } from "../../terminals";
import { useUiStore } from "../../ui";

type Appearance = "standard" | "quiet" | "prominent" | "danger";

const APPEARANCE: Record<Appearance, string> = {
  standard: "border border-border bg-secondary text-foreground hover:bg-border",
  quiet: "text-subtle-foreground hover:bg-accent hover:text-foreground",
  prominent: "bg-primary text-primary-foreground hover:opacity-[var(--opacity-emphasis-fill)]",
  danger: "bg-destructive text-destructive-foreground hover:opacity-[var(--opacity-emphasis-fill)]",
};

export function Button({
  appearance = "standard",
  className = "",
  type = "button",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { appearance?: Appearance }) {
  return (
    <button
      type={type}
      className={`inline-flex h-[var(--size-control-compact)] shrink-0 items-center justify-center gap-xs whitespace-nowrap rounded-sm px-sm text-body font-semibold outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-default disabled:opacity-[var(--opacity-disabled)] ${APPEARANCE[appearance]} ${className}`}
      {...props}
    />
  );
}

/** A text field. `mono` is for machine text (paths, aliases, branches); prose such as a purpose or a label uses the interface face, which keeps Korean readable. */
export function Field({ className = "", mono = true, ...props }: InputHTMLAttributes<HTMLInputElement> & { mono?: boolean }) {
  return (
    <input
      className={`h-[var(--size-settings-field)] min-w-0 rounded-sm border border-border bg-secondary px-xs text-body text-foreground outline-none placeholder:text-muted-foreground focus:border-primary disabled:opacity-[var(--opacity-disabled)] ${mono ? "font-mono" : ""} ${className}`}
      {...props}
    />
  );
}

export function Select({ className = "", children, ...props }: SelectHTMLAttributes<HTMLSelectElement> & { children: ReactNode }) {
  return (
    <select
      className={`h-[var(--size-settings-field)] w-[var(--size-settings-control-w)] max-w-full rounded-sm border border-border bg-secondary px-xs text-body text-foreground outline-none focus:border-primary disabled:opacity-[var(--opacity-disabled)] ${className}`}
      {...props}
    >
      {children}
    </select>
  );
}

/** A titled card whose rows are separated by hairlines, like `HideSettingsGroup`. */
export function Group({ title, note, children, ...data }: { title: string; note?: ReactNode; children: ReactNode } & Record<`data-${string}`, string>) {
  return (
    <section className="mb-lg" {...data}>
      <h3 className="mb-sm text-body font-semibold text-subtle-foreground">{title}</h3>
      <div className="divide-y divide-border rounded-md border border-border bg-card">{children}</div>
      {note ? <p className="mt-sm text-body text-muted-foreground">{note}</p> : null}
    </section>
  );
}

/** One row: its label on the left, whatever the row is about on the right; wraps on a narrow sheet. */
export function Row({ label, children, detail }: { label: ReactNode; children?: ReactNode; detail?: ReactNode }) {
  return (
    <div className="px-md py-sm">
      <div className="flex flex-wrap items-center gap-x-md gap-y-xs">
        <div className="min-w-[min(100%,var(--size-settings-control-w))] flex-1 text-subhead text-foreground">{label}</div>
        {children ? <div className="flex min-w-0 max-w-full flex-wrap items-center justify-end gap-sm">{children}</div> : null}
      </div>
      {detail ? <div className="mt-xs">{detail}</div> : null}
    </div>
  );
}

/** A value a row reports; long paths break anywhere rather than overflow the sheet. */
export function Value({ children, mono = true }: { children: ReactNode; mono?: boolean }) {
  return <span className={`min-w-0 break-all text-right text-body text-subtle-foreground ${mono ? "font-mono" : ""}`}>{children}</span>;
}

export type Tone = "ok" | "warn" | "error" | "pending" | "local" | "muted";

const TONE_TEXT: Record<Tone, string> = {
  ok: "text-success",
  warn: "text-warning",
  error: "text-destructive",
  pending: "text-subtle-foreground",
  local: "text-primary",
  muted: "text-muted-foreground",
};

const TONE_SYMBOL: Record<Tone, string> = { ok: "✓", warn: "!", error: "✕", pending: "…", local: "●", muted: "·" };

/** A state as symbol plus words, never color alone (design 7). */
export function Status({ tone, children, ...data }: { tone: Tone; children: ReactNode } & Record<`data-${string}`, string>) {
  return (
    <span className={`inline-flex min-w-0 items-baseline gap-xs text-body ${TONE_TEXT[tone]}`} {...data}>
      <span aria-hidden="true" className="font-mono">
        {TONE_SYMBOL[tone]}
      </span>
      <span className="min-w-0 break-words">{children}</span>
    </span>
  );
}

/** One line a row carries about itself: a failure, a warning, or what happens next. */
export function Note({ tone = "muted", children, ...data }: { tone?: Tone; children: ReactNode } & Record<`data-${string}`, string>) {
  return (
    <p role={tone === "error" ? "alert" : undefined} className={`break-words text-body ${TONE_TEXT[tone]}`} {...data}>
      {children}
    </p>
  );
}

/**
 * A modal layer. Escape closes the innermost open layer first (the window
 * listener consults `escapeLayers`), focus moves into the layer on open and
 * back to whatever held it on close, and Tab stays inside while it is open.
 */
export function Dialog({
  label,
  role = "dialog",
  width = "w-[var(--size-worktree-dialog)]",
  onClose,
  initialFocus = "first",
  children,
  ...data
}: {
  label: string;
  role?: "dialog" | "alertdialog";
  width?: string;
  onClose: () => void;
  /** `container` focuses the dialog itself, so an irreversible choice has no default (design 6). */
  initialFocus?: "first" | "container";
  children: ReactNode;
} & Record<`data-${string}`, string>) {
  const ref = useRef<HTMLDivElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  useEffect(() => {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const remove = useUiStore.getState().pushEscape(() => onCloseRef.current());
    const root = ref.current;
    if (root) {
      const first = initialFocus === "first" ? root.querySelector<HTMLElement>("input, select, textarea, button:not([disabled])") : null;
      (first ?? root).focus();
    }
    return () => {
      remove();
      restoreFocus(previous);
    };
    // Mount-only: the layer's identity is its lifetime.
  }, []);
  const trapTab = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "Tab" || !ref.current) return;
    const focusable = [...ref.current.querySelectorAll<HTMLElement>("a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex='0']")];
    const first = focusable[0];
    const last = focusable.at(-1);
    if (!first || !last) return;
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };
  return (
    <div className="absolute inset-0 z-40 flex items-center justify-center p-lg" role="presentation">
      <div className="absolute inset-0 bg-background opacity-[var(--opacity-secondary)]" onClick={() => onCloseRef.current()} />
      <div
        ref={ref}
        role={role}
        aria-modal="true"
        aria-label={label}
        tabIndex={-1}
        onKeyDown={trapTab}
        className={`relative flex max-h-full max-w-full flex-col overflow-hidden rounded-lg border border-border bg-popover text-body text-foreground shadow-lg outline-none ${width}`}
        {...data}
      >
        {children}
      </div>
    </div>
  );
}
