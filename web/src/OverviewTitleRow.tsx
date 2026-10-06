import { PlusIcon, SquareTerminalIcon } from "lucide-react";
import { useLayoutEffect, useRef, useState, type ComponentProps, type ReactNode } from "react";
import { Badge } from "./components/ui/badge";
import { Button } from "./components/ui/button";
import { Kbd } from "./components/ui/kbd";
import { Hint } from "./components/ui/tooltip";
import { useInterfaceTranslation } from "./i18n/client";
import { cn } from "./lib/utils";

// The Project Overview's title row: the path back with the project's name on
// the left, New agent and New issue on the right. The name has the row first
// (issue 618): the actions keep their word and keycap only while the whole
// name fits beside them, and otherwise give way to their icon, which is what
// every width, label length, language, device badge and sidebar width reduces
// to. A browser cannot ask CSS whether content fits, so the row measures it:
// the name's natural width, the labelled actions' natural width (an invisible
// twin of them) and the room. Neither depends on which state is showing, so
// the answer cannot flip back and forth.

const gapOf = (node: Element) => Number.parseFloat(getComputedStyle(node).columnGap) || 0;
// A truncated name reports the width of the whole text as its scrollWidth.
const naturalWidth = (node: HTMLElement) => Math.max(node.offsetWidth, node.scrollWidth);

/** Whether the nav at its natural width and the labelled actions fit in the row. */
function labelledFit(row: HTMLElement, nav: HTMLElement, twin: HTMLElement): boolean {
  const children = [...nav.children] as HTMLElement[];
  const navWidth = children.reduce((sum, child) => sum + naturalWidth(child), 0) + gapOf(nav) * Math.max(children.length - 1, 0);
  return row.clientWidth >= navWidth + gapOf(row) + twin.offsetWidth;
}

type ActionProps = {
  label: string;
  icon: ReactNode;
  keycap?: string;
  variant?: ComponentProps<typeof Button>["variant"];
  iconOnly: boolean;
  /** The invisible copy that measures the labelled width: no handlers, hooks or hint. */
  twin?: boolean;
  button?: ComponentProps<"button"> & Record<`data-${string}`, string>;
};

/** An icon button that also carries its word and keycap while there is room; a hint says them while there is not. */
function Action({ label, icon, keycap, variant, iconOnly, twin = false, button }: ActionProps) {
  const control = (
    <Button variant={variant} className={cn(iconOnly && "size-(--size-control) px-0")} tabIndex={twin ? -1 : undefined} {...(twin ? {} : button)}>
      {icon}
      <span className={cn(iconOnly && "sr-only")}>{label}</span>
      {keycap ? <Kbd className={cn(iconOnly && "hidden")}>{keycap}</Kbd> : null}
    </Button>
  );
  if (twin) return control;
  // The span takes the pointer a disabled button does not, so a dimmed icon still says what it is.
  return (
    <Hint reveals active={iconOnly} label={label} shortcut={keycap ? <Kbd>{keycap}</Kbd> : undefined}>
      <span className="inline-flex">{control}</span>
    </Hint>
  );
}

export function OverviewTitleRow({
  name,
  path,
  remoteDevice,
  onAllProjects,
  newAgent,
  newIssue,
}: {
  name: string;
  path: string;
  /** The device's label when the project is on another machine. */
  remoteDevice: string | null;
  onAllProjects: () => void;
  newAgent: { run: () => void; disabled: boolean };
  /** Null when the project cannot take issues. */
  newIssue: (() => void) | null;
}) {
  const { t } = useInterfaceTranslation();
  const row = useRef<HTMLDivElement>(null);
  const nav = useRef<HTMLElement>(null);
  const twin = useRef<HTMLDivElement>(null);
  const [iconOnly, setIconOnly] = useState(false);
  const issues = newIssue !== null;
  const agentLabel = t("requests.newAgent");
  const issueLabel = t("issue.newTitle");
  useLayoutEffect(() => {
    const [rowNode, navNode, twinNode] = [row.current, nav.current, twin.current];
    if (!rowNode || !navNode || !twinNode) return;
    const measure = () => setIconOnly(!labelledFit(rowNode, navNode, twinNode));
    measure();
    const observer = new ResizeObserver(measure);
    for (const node of [rowNode, twinNode, ...navNode.children]) observer.observe(node);
    return () => observer.disconnect();
  }, [name, remoteDevice, issues, agentLabel, issueLabel]);
  const agentIcon = <SquareTerminalIcon aria-hidden="true" />;
  const issueIcon = <PlusIcon aria-hidden="true" />;
  return (
    <div ref={row} className="relative flex min-w-0 items-center gap-lg">
      <nav ref={nav} aria-label={t("overview.location")} className="flex min-w-0 flex-1 items-center gap-xs">
        <button type="button" className="shrink-0 rounded-xs px-xs text-caption text-subtle-foreground hover:bg-accent hover:text-foreground focus-visible:bg-accent" data-go-main="true" onClick={onAllProjects}>
          {t("overview.allProjects")}
        </button>
        <span aria-hidden="true" className="text-caption text-muted-foreground">/</span>
        <Hint label={path} reveals>
          <h1 className="min-w-0 truncate text-headline font-semibold text-foreground" aria-current="page">
            {name}
          </h1>
        </Hint>
        {remoteDevice ? <Badge variant="secondary" className="min-w-0 shrink">{remoteDevice}</Badge> : null}
      </nav>
      <Action label={agentLabel} icon={agentIcon} variant="ghost" iconOnly={iconOnly} button={{ onClick: newAgent.run, disabled: newAgent.disabled, "data-overview-new-agent": "true" }} />
      {newIssue ? <Action label={issueLabel} icon={issueIcon} keycap="C" iconOnly={iconOnly} button={{ onClick: newIssue, "data-overview-new-issue": "true" }} /> : null}
      <div ref={twin} aria-hidden="true" className="invisible pointer-events-none absolute flex w-max items-center gap-lg">
        <Action label={agentLabel} icon={agentIcon} variant="ghost" iconOnly={false} twin />
        {newIssue ? <Action label={issueLabel} icon={issueIcon} keycap="C" iconOnly={false} twin /> : null}
      </div>
    </div>
  );
}
