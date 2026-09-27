import { ArrowUpRightIcon, GitMergeIcon, GitPullRequestClosedIcon, GitPullRequestDraftIcon, GitPullRequestIcon } from "lucide-react";
import { useRef, useState, type MouseEvent, type ReactNode } from "react";
import { badgeParts } from "../agentRow";
import { cn } from "../lib/utils";
import { cardSingleValue, type CheckoutCard } from "../projects";
import type { PullRequest } from "../snapshot";
import { BadgeMarks } from "./status-badge";
import { Badge } from "./ui/badge";
import { useEscapeLayer } from "./ui/layer";
import { Hint, Tooltip, TooltipContent, TooltipTrigger } from "./ui/tooltip";

/** What a press on the card's link means: ⌘ asks for the default browser (PRD checkout-pr-glyph-card D-02). */
export function pullRequestOpenExternal(event: MouseEvent): boolean {
  return event.metaKey;
}

const LIFECYCLE_ICON: Record<PullRequest["badge"], typeof GitPullRequestIcon> = {
  open: GitPullRequestIcon,
  review: GitPullRequestIcon,
  merged: GitMergeIcon,
  closed: GitPullRequestClosedIcon,
};

/**
 * The card a checkout row opens on hover and keyboard focus (PRD
 * checkout-pr-glyph-card D-03, D-04): a pull request's badge, number and
 * Open PR link over its title, then the rows `checkoutCard` gave it. It is
 * the shell's tooltip with hoverable content, so it opens after the same
 * delay, stays while the pointer crosses onto it, and closes when the pointer
 * leaves the row and the card, on Escape, and on a press on the row. The
 * hidden tooltip text a screen reader gets is `description`, the detail
 * sentence the row's tooltip carried before. A card with one value and no
 * header is the plain `Hint` with that value instead (D-09).
 */
export function CheckoutCardHint({
  card,
  description,
  onOpenPullRequest,
  children,
}: {
  card: CheckoutCard;
  /** The screen reader's description of the row: the pull request, the agents, the branch and the path. */
  description: string;
  /** Opens the pull request the header names; `external` asks for the default browser. */
  onOpenPullRequest: (url: string, external: boolean) => void;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const pressed = useRef(false);
  useEscapeLayer(open, () => setOpen(false));
  const single = cardSingleValue(card);
  if (single !== null) return <Hint label={single}>{children}</Hint>;
  const press = () => {
    pressed.current = true;
    setOpen(false);
  };
  const header = card.header;
  return (
    <Tooltip open={open} onOpenChange={(next) => setOpen(next && !pressed.current)} disableHoverableContent={false}>
      <TooltipTrigger
        asChild
        onPointerDown={press}
        onContextMenu={press}
        onPointerEnter={() => {
          pressed.current = false;
        }}
        onBlur={() => {
          pressed.current = false;
        }}
      >
        {children}
      </TooltipTrigger>
      <TooltipContent
        side="right"
        align="start"
        aria-label={description}
        className="pointer-events-auto w-(--size-pr-popover) max-w-(--radix-tooltip-content-available-width) text-left text-wrap rounded-md p-md"
        data-checkout-card={header?.kind ?? "plain"}
      >
        <div className="flex flex-col gap-sm">
          {header?.kind === "pull_request" ? <PullRequestHeader header={header} onOpen={onOpenPullRequest} /> : null}
          {header?.kind === "missing" ? (
            <div className="text-subhead font-medium text-destructive" data-checkout-card-missing="true">
              {header.label}
            </div>
          ) : null}
          {header?.kind === "pull_request" ? <div className="border-t border-border" /> : null}
          <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-md gap-y-xs text-caption">
            {card.rows.map((row) => (
              <CardRow key={row.key} row={row} />
            ))}
          </dl>
        </div>
      </TooltipContent>
    </Tooltip>
  );
}

function PullRequestHeader({
  header,
  onOpen,
}: {
  header: Extract<NonNullable<CheckoutCard["header"]>, { kind: "pull_request" }>;
  onOpen: (url: string, external: boolean) => void;
}) {
  // The lifecycle shape the sidebar glyph draws (docs/UI_BEHAVIOR.md, PR chrome); a draft keeps its own.
  const Icon = header.isDraft && header.lifecycle === "open" ? GitPullRequestDraftIcon : LIFECYCLE_ICON[header.lifecycle];
  return (
    <>
      <div className="flex items-center gap-sm">
        <Badge variant="outline" className={cn("gap-xs", header.badge.color)} data-checkout-card-badge={header.badge.label}>
          <Icon aria-hidden="true" />
          {header.badge.label}
        </Badge>
        {header.badge.draft ? (
          <span className="text-caption text-pr-draft" data-checkout-card-draft="true">
            Draft
          </span>
        ) : null}
        <span className="font-mono text-caption text-muted-foreground">#{header.number}</span>
        <span className="flex-1" />
        <button
          type="button"
          data-checkout-card-open={header.number}
          className="inline-flex items-center gap-xxs rounded-xs text-caption font-medium text-pr-open outline-none hover:underline focus-visible:ring-1 focus-visible:ring-ring"
          onClick={(event) => {
            event.stopPropagation();
            onOpen(header.url, pullRequestOpenExternal(event));
          }}
        >
          Open PR
          <ArrowUpRightIcon aria-hidden="true" className="size-(--size-icon-sm)" />
        </button>
      </div>
      {header.title ? (
        <div className="line-clamp-2 text-subhead font-medium text-foreground" data-checkout-card-title="true">
          {header.title}
        </div>
      ) : null}
    </>
  );
}

function CardRow({ row }: { row: CheckoutCard["rows"][number] }) {
  return (
    <>
      <dt className="text-muted-foreground">{row.label}</dt>
      <dd className="min-w-0" data-checkout-card-row={row.key}>
        {row.key === "agents" ? (
          <span className="inline-flex items-center gap-sm font-mono">
            <BadgeMarks parts={badgeParts(row.marks)} />
          </span>
        ) : row.key === "path" ? (
          <span className="break-all font-mono text-muted-foreground">{row.value}</span>
        ) : row.key === "branch" ? (
          <span className="break-all font-mono text-foreground">{row.value}</span>
        ) : row.key === "review" || row.key === "checks" ? (
          <span className={row.tone}>{row.value}</span>
        ) : (
          <span className="text-foreground">{row.value}</span>
        )}
      </dd>
    </>
  );
}
