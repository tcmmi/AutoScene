import type * as React from "react"
import type { TFunction } from "i18next"
import { useTranslation } from "react-i18next"
import { ArrowSquareOut, CheckCircle } from "@/components/icons"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Switch } from "@/components/ui/switch"
import { openExternalUrl } from "@/lib/os"
import { useAppInfo } from "@/lib/queries"
import { releaseNotesUrl, useUpdates } from "@/state/updates"
import { SettingRow, SettingsGroup } from "../primitives"

/** When the last check ended, as one phrase ("checked 5 min ago"). */
function checkedAgo(ms: number, t: TFunction<"settings">): string {
  const s = Math.round((Date.now() - ms) / 1000)
  if (s < 60) return t("updates.checked.justNow")
  const m = Math.round(s / 60)
  if (m < 60) return t("updates.checked.minutes", { count: m })
  return t("updates.checked.hours", { count: Math.round(m / 60) })
}

export function UpdatesSection() {
  const { t } = useTranslation("settings")
  const { data: info } = useAppInfo()
  const u = useUpdates()
  const checking = u.phase === "checking"

  let statusLine: React.ReactNode = u.auto ? t("updates.autoLine") : t("updates.manualLine")
  if (checking) statusLine = t("updates.checkingLine")
  else if (u.phase === "current")
    statusLine = (
      <span className="inline-flex items-center gap-1.5">
        <CheckCircle className="size-3.5 text-fg-2" /> {t("updates.upToDate")}
        {u.lastChecked && <span className="text-fg-3"> · {checkedAgo(u.lastChecked, t)}</span>}
      </span>
    )
  else if (u.phase === "error") statusLine = <span className="text-danger">{u.error}</span>
  else if (u.status?.state === "available") statusLine = u.status.version ? t("updates.ready", { version: u.status.version }) : t("updates.readyNoVersion")

  return (
    <SettingsGroup title={t("updates.title")} description={t("updates.description")}>
      <SettingRow
        label={
          <span className="flex items-center gap-2">
            AutoScene {info?.version}
            {u.status?.simulated && <Badge tone="outline">{t("updates.simulated")}</Badge>}
          </span>
        }
        description={statusLine}
      >
        {u.status?.state === "available" ? (
          <Button variant="primary" size="sm" onClick={() => u.setDialogOpen(true)}>
            {t("updates.view")}
          </Button>
        ) : (
          <Button size="sm" busy={checking} onClick={() => void u.check(true)}>
            {t("updates.check")}
          </Button>
        )}
      </SettingRow>
      <SettingRow
        label={t("updates.auto.label")}
        description={t("updates.auto.description", { check: t("updates.check") })}
      >
        <Switch aria-label={t("updates.auto.label")} checked={u.auto} onCheckedChange={u.setAuto} />
      </SettingRow>
      {info?.version && (
        <SettingRow label={t("updates.whatsNew.label")} description={t("updates.whatsNew.description", { version: info.version })}>
          <Button size="sm" variant="ghost" onClick={() => void openExternalUrl(releaseNotesUrl(info.version))}>
            <ArrowSquareOut /> {t("updates.whatsNew.button")}
          </Button>
        </SettingRow>
      )}
    </SettingsGroup>
  )
}
