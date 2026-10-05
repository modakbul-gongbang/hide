import { AlertTriangleIcon, HardDriveIcon } from "lucide-react";
import { useInterfaceTranslation } from "./i18n/client";
import { cn } from "./lib/utils";
import { FACT } from "./MainScreen";
import { LOW_FREE_BYTES, entranceBytes, layerLines, lowFree, reclaimable, type Filter } from "./diskCleanup";
import { LAYER_DOT } from "./DiskCleanupSheet";
import { Tooltip, TooltipContent, TooltipTrigger, useHintOpen } from "./components/ui/tooltip";
import { formatBytes } from "./i18n/format";
import { requireInterfaceLanguage } from "./i18n/locale";
import type { Workspace } from "./snapshot";
import { useUiStore } from "./ui";

// The Overview facts line's way into the disk cleanup sheet (PRD disk-layers
// B1, B2): the disk number, whose hover lists the layers, and a warning cell
// that stands only while the volume is short of space. Both open the same
// sheet; the warning cell opens it on what is finished.

export function openDiskCleanup(workspaceId: string, filter: Filter) {
  useUiStore.getState().setWorkspaceDialog({ kind: "disk_cleanup", workspaceId, filter });
}

/**
 * A local Git project's size, drawn as a button once its layers are known; a
 * plain number before. While a checkout could not be measured the number is
 * the subtotal of the others, marked `≥`, and the tooltip says so: the entrance
 * never states a total the system cannot produce (B6).
 */
export function DiskFact({ workspace, bytes }: { workspace: Workspace; bytes: number }) {
  const { t, i18n } = useInterfaceTranslation();
  const language = requireInterfaceLanguage(i18n.language);
  const { open, onOpenChange, triggerProps } = useHintOpen();
  const lines = layerLines(workspace, t);
  const local = !workspace.remote_target_id;
  const partial = entranceBytes(workspace)?.partial ?? false;
  const size = `${partial ? "≥ " : ""}${formatBytes(language, bytes)}`;
  const content = (
    <span className={cn(FACT, "rounded-xs")} data-stat="disk" data-disk-partial={partial || undefined}>
      <HardDriveIcon aria-hidden="true" className="size-(--size-icon)" />
      {size}
    </span>
  );
  if (!local || !lines) {
    return content;
  }
  return (
    <Tooltip open={open} onOpenChange={onOpenChange}>
      <TooltipTrigger asChild {...triggerProps}>
        <button
          type="button"
          aria-label={t("cleanup.entrance", { size })}
          className={cn(FACT, "rounded-xs outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring")}
          data-stat="disk"
          data-disk-entrance="true"
          data-disk-partial={partial || undefined}
          onClick={() => openDiskCleanup(workspace.id, "all")}
        >
          <HardDriveIcon aria-hidden="true" className="size-(--size-icon)" />
          {size}
        </button>
      </TooltipTrigger>
      <TooltipContent>
        <div className="flex flex-col gap-xxs" data-disk-tooltip="true">
          <span>{t("cleanup.allocatedHint", { size })}</span>
          {partial ? (
            <span className="text-warning" data-disk-tooltip-partial="true">
              {t("cleanup.partial")}
            </span>
          ) : null}
          {lines.map((line) => (
            <span key={line.key} className="flex items-center justify-between gap-md" data-disk-tooltip-line={line.key}>
              <span className="inline-flex items-center gap-xs">
                <span aria-hidden="true" className={cn("size-(--size-status-mark) rounded-full", LAYER_DOT[line.key])} />
                {line.label}
              </span>
              <span className="font-mono">{formatBytes(language, line.bytes)}</span>
            </span>
          ))}
        </div>
      </TooltipContent>
    </Tooltip>
  );
}

/** `Free 1.6 GB · 23 GB can be freed`, only once measured and under the limit (B2, D-20). */
export function LowFreeFact({ workspace }: { workspace: Workspace }) {
  const { t, i18n } = useInterfaceTranslation();
  const language = requireInterfaceLanguage(i18n.language);
  const free = lowFree(workspace);
  if (free === null || workspace.remote_target_id) return null;
  const freeable = reclaimable(workspace);
  const text = freeable > 0 ? t("cleanup.freeAndReclaimable", { free: formatBytes(language, free), reclaimable: formatBytes(language, freeable) }) : t("cleanup.free", { size: formatBytes(language, free) });
  return (
    <button
      type="button"
      className={cn(FACT, "rounded-xs text-warning outline-none hover:underline focus-visible:ring-1 focus-visible:ring-ring")}
      data-stat="low-free"
      data-disk-low-free={LOW_FREE_BYTES}
      onClick={() => openDiskCleanup(workspace.id, "done")}
    >
      <AlertTriangleIcon aria-hidden="true" className="size-(--size-icon)" />
      {text}
    </button>
  );
}
