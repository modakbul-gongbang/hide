import { FileSearchIcon, GitCompareArrowsIcon } from "lucide-react";
import { Button } from "./ui/button";
import { Kbd } from "./ui/kbd";
import { useInterfaceTranslation } from "../i18n/client";

/** Only things a display can show belong here; tools and panes have their own controls. */
export function NewTabBody({ hasChanges, fileChord, onFile, onDiff }: { hasChanges: boolean; fileChord: string; onFile: () => void; onDiff: () => void }) {
  const { t } = useInterfaceTranslation();
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-sm overflow-auto p-xl" data-new-tab-page="true">
      <h2 className="text-body font-medium text-muted-foreground">{t("common.open")}</h2>
      <Button variant="ghost" size="lg" className="w-full justify-start bg-secondary text-foreground" onClick={onFile}>
        <FileSearchIcon /><span className="flex-1 text-left">{t("panes.newTab.file")}</span>{fileChord ? <Kbd>{fileChord}</Kbd> : null}
      </Button>
      {hasChanges ? (
        <Button variant="ghost" size="lg" className="w-full justify-start bg-secondary text-foreground" onClick={onDiff}>
          <GitCompareArrowsIcon /><span className="flex-1 text-left">{t("panes.newTab.diff")}</span>
        </Button>
      ) : null}
    </div>
  );
}
