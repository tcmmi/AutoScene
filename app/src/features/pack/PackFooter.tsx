import type { ReactNode } from "react"
import { Trans, useTranslation } from "react-i18next"
import { ArrowCounterClockwise, DotsThree, Package } from "@/components/icons"
import type { JobInfo, PackTotals } from "@/bindings"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { ProgressRing } from "@/components/ui/progress"
import { Tooltip } from "@/components/ui/tooltip"
import { PluginIcon } from "@/features/plugins/PluginIcon"
import type { PackActionItem } from "@/features/plugins/plugin-items"
import { formatBytes, formatDuration, formatPercent } from "@/lib/format"
import { useShortcutKeys } from "@/lib/shortcuts"
import { jobBar } from "@/state/activity"
import { checkingText, type PackCheck } from "./spec"

/**
 * The pack at a glance, floating over the shots: how many, how long, about
 * how big in the format an export starts with, the picks by hand (to hand
 * back to the filters) and Export. While the pack's export waits or runs,
 * Export shows how far it is and opens Activity instead: a second click
 * doesn't queue the same pack again. While a plugin's filter checks shots
 * the footer says how far it is and Export waits; a check that failed says
 * so and offers to try again, and Export goes on: the shots it didn't
 * judge keep the word it gave them with its settings before (out where it
 * said no), else they are in, and the export takes them as the grid shows
 * them. Beside Export, a ⋯ lists what can be done to every shot at once and
 * what the plugins that may run now save the pack as (a timeline file, before
 * anything is rendered): saving goes with Export's wait for a check, and the
 * menu says so; taking shots in and out goes on.
 */
export function PackFooter({
  totals,
  bytes,
  format,
  picks,
  canExport,
  exporting,
  check,
  items,
  actions,
  canSave = false,
  saveHeading,
  onSave,
  onClearPicks,
  onTryAgain,
  onExport,
  onShowExport,
}: {
  totals: PackTotals
  /** In `format`, the one an export starts with. */
  bytes: number | null
  /** The format's short name. */
  format: string | null
  picks: number
  canExport: boolean
  /** The job of the pack's export while it waits or runs. */
  exporting?: Pick<JobInfo, "state" | "progress">
  /** What the plugin filters turned on are doing, if anything. */
  check?: PackCheck | null
  /** What can be done to every shot at once: the ⋯ menu's first entries. */
  items?: ReactNode
  /** What the plugins that may run now save the pack as. */
  actions?: PackActionItem[]
  /** Whether the pack can be saved now: as Export, but also while it exports. */
  canSave?: boolean
  /** The save group's heading: what it waits for, while it does. */
  saveHeading?: string
  /** Save the pack with `action`. */
  onSave?: (action: PackActionItem) => void
  onClearPicks: () => void
  /** Ask the filters whose check failed to start it again. */
  onTryAgain?: () => void
  onExport: () => void
  /** Show the export's progress (in Activity). */
  onShowExport: () => void
}) {
  const { t } = useTranslation(["pack", "common"])
  const keys = useShortcutKeys()
  const waiting = check?.kind === "checking"
  const saves = actions ?? []
  return (
    <div className="glass-strong absolute bottom-6 left-1/2 z-30 flex -translate-x-1/2 animate-pop-in items-center gap-1 rounded-full py-1.5 pr-1.5 pl-5 whitespace-nowrap">
      <span className="text-md font-semibold text-fg-1">{t("shots", { count: totals.clips })}</span>
      <span className="pr-2 pl-1.5 text-sm text-fg-3">
        {[
          formatDuration(totals.seconds ?? 0),
          bytes != null && bytes > 0 && (format ? t("footer.aboutIn", { size: formatBytes(bytes), format }) : t("footer.about", { size: formatBytes(bytes) })),
        ]
          .filter(Boolean)
          .join(" · ")}
      </span>
      {check?.kind === "checking" && (
        <span role="status" className="flex items-center gap-2 pr-2 text-sm text-fg-2">
          <ProgressRing aria-hidden className="size-3.5" value={check.checked > 0 ? check.checked / check.total : null} />
          {checkingText(check.checked, check.total)}
        </span>
      )}
      {check?.kind === "failed" && (
        <span role="status" className="flex items-center gap-1 pr-1 text-sm text-fg-2">
          <Tooltip label={t("footer.failedHint")}>
            <span tabIndex={0} className="rounded-full outline-none focus-visible:ring-2 focus-visible:ring-accent-line">
              {t("check.failed", { count: check.unchecked })}
            </span>
          </Tooltip>
          {onTryAgain && (
            <Button size="sm" variant="ghost" onClick={onTryAgain}>
              {t("common:action.tryAgain")}
            </Button>
          )}
        </span>
      )}
      {picks > 0 && (
        <Tooltip label={t("footer.handBackAll")}>
          <Button size="sm" variant="ghost" onClick={onClearPicks}>
            <ArrowCounterClockwise /> {t("footer.picksByHand", { count: picks })}
          </Button>
        </Tooltip>
      )}
      {(items || saves.length > 0) && (
        <DropdownMenu>
          <Tooltip label={t("footer.options")}>
            <DropdownMenuTrigger asChild>
              <Button size="icon" variant="ghost" aria-label={t("footer.options")}>
                <DotsThree weight="bold" />
              </Button>
            </DropdownMenuTrigger>
          </Tooltip>
          <DropdownMenuContent side="top" align="end" className="min-w-[280px]">
            {items}
            {saves.length > 0 && (
              <>
                {items && <DropdownMenuSeparator />}
                <DropdownMenuLabel>{saveHeading ?? t("save.heading")}</DropdownMenuLabel>
                {saves.map((a) => (
                  <DropdownMenuItem key={a.key} disabled={!canSave} onSelect={() => onSave?.(a)}>
                    <PluginIcon src={a.icon} /> {a.title}
                    <span className="ml-auto pl-4 text-xs text-fg-3">{a.pluginName}</span>
                  </DropdownMenuItem>
                ))}
              </>
            )}
          </DropdownMenuContent>
        </DropdownMenu>
      )}
      {exporting ? (
        <Tooltip label={t("footer.showExport")}>
          {/* Gold while it runs; waiting its turn it is a quiet button with an empty ring. */}
          <Button size="md" variant={exporting.state === "queued" ? "secondary" : "accent"} onClick={onShowExport}>
            <ProgressRing aria-hidden value={jobBar(exporting)} waiting={exporting.state === "queued"} />
            {exporting.state === "queued" ? (
              t("footer.exportQueued")
            ) : (
              <span>
                <Trans
                  t={t}
                  i18nKey="footer.exporting"
                  values={{ percent: formatPercent(exporting.progress) }}
                  components={{ x: <span className="timecode" /> }}
                />
              </span>
            )}
          </Button>
        </Tooltip>
      ) : waiting ? (
        // Busy, not disabled: it keeps the keyboard and the pointer, so the tooltip can be on the button itself.
        <Tooltip label={t("footer.waitsForCheck")}>
          <Button size="md" variant="primary" busy aria-label={t("footer.exportWaitsAria")}>
            <Package weight="bold" /> {t("footer.export")}
          </Button>
        </Tooltip>
      ) : (
        <Tooltip label={t("footer.exportTooltip")} shortcut={keys("pack.export")}>
          <Button size="md" variant="primary" disabled={!canExport} onClick={onExport}>
            <Package weight="bold" /> {t("footer.export")}
          </Button>
        </Tooltip>
      )}
    </div>
  )
}
