import { ExternalLinkIcon } from "lucide-react";
import { Badge } from "../components/ui/badge";
import { Popover, PopoverContent, PopoverTrigger } from "../components/ui/popover";
import { useInterfaceTranslation } from "../i18n/client";
import type { KitAgent } from "../snapshot";
import { docsUrl } from "./agentRows";

/**
 * The Partial chip of an agent Hide does only some things for, on or off (PRD
 * settings-cleanup D-10, B18): a button that opens the list of what works,
 * one line per feature with a mark and a word, never a colour alone. The list
 * is the kit's feature table, so it is what this build does with the agent.
 * Radix owns the keyboard: Enter or Space opens it, Escape closes it and
 * focus returns to the chip.
 */
export function PartialChip({ agent }: { agent: KitAgent }) {
  const { t } = useInterfaceTranslation();
  const features = agent.features ?? [];
  const docs = docsUrl(agent.id);
  const screenOnly = features.some((feature) => feature.id === "herdr_integration" && !feature.supported);
  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          type="button"
          aria-label={t("agents.partialAria", { agent: agent.label })}
          className="cursor-pointer rounded-xs outline-none focus-visible:ring-1 focus-visible:ring-ring"
          data-agent-partial={agent.id}
        >
          <Badge variant="outline">{t("agents.partial")}</Badge>
        </button>
      </PopoverTrigger>
      <PopoverContent align="end" data-agent-partial-popover={agent.id}>
        <p className="font-semibold text-foreground">{t("agents.partialTitle", { agent: agent.label })}</p>
        <ul className="mt-sm space-y-xs">
          {features.map((feature) => (
            <li key={feature.id} className="flex items-baseline gap-xs" data-agent-feature={`${feature.id}:${feature.supported ? "yes" : "no"}`}>
              <span aria-hidden="true" className={`font-mono ${feature.supported ? "text-success" : "text-muted-foreground"}`}>
                {feature.supported ? "✓" : "–"}
              </span>
              <span className="sr-only">{t(feature.supported ? "agents.feature.supported" : "agents.feature.unsupported")}: </span>
              <span className={feature.supported ? "text-foreground" : "text-muted-foreground"}>{t(`agents.feature.${feature.id}`)}</span>
            </li>
          ))}
        </ul>
        {/* B15: an agent Herdr has no integration for says how its status is judged. */}
        {screenOnly ? <p className="mt-sm text-caption text-muted-foreground">{t("agents.screenOnly", { agent: agent.label })}</p> : null}
        {docs ? (
          <a
            href={docs}
            target="_blank"
            rel="noopener noreferrer"
            className="mt-sm inline-flex items-center gap-xs text-caption text-primary outline-none focus-visible:ring-1 focus-visible:ring-ring"
            aria-label={t("agents.docsAria", { agent: agent.label })}
          >
            {t("agents.docs")}
            <ExternalLinkIcon aria-hidden="true" className="size-(--size-icon-sm)" />
          </a>
        ) : null}
      </PopoverContent>
    </Popover>
  );
}
