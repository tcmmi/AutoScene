import { useState } from "react"
import { toast } from "sonner"
import { useTranslation } from "react-i18next"
import type { CloudLimits, CloudStatus } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Switch } from "@/components/ui/switch"
import { fmt, say } from "@/lib/i18n"
import { commands, errorMessage } from "@/lib/ipc"
import { copyText } from "@/lib/os"
import { qk, queryClient, useCloudStatus } from "@/lib/queries"
import { SettingRow, SettingsGroup } from "../primitives"

/** A command's answer: the new status on screen, its failure in a toast. */
async function run(job: () => Promise<CloudStatus>, failed: string) {
  try {
    queryClient.setQueryData(qk.cloud, await job())
    return true
  } catch (e) {
    toast.error(failed, { description: errorMessage(e) })
    return false
  }
}

/**
 * Settings → Cloud. People see a balance, never a key: the account belongs to
 * this computer. Money comes in through Stripe's page or a code; the switch
 * sends new analyses to the cloud.
 */
export function CloudSection() {
  const { t } = useTranslation("settings")
  const { data: status, isLoading } = useCloudStatus()
  const [code, setCode] = useState("")
  const [redeeming, setRedeeming] = useState(false)
  const [paying, setPaying] = useState<number | null>(null)
  const account = status?.account ?? null

  // Before the launch (or without a gateway) there's nothing to show but that.
  if (!isLoading && status && (!status.available || status.stage === "hidden")) {
    return (
      <SettingsGroup title={t("cloud.title")} description={t("cloud.description")}>
        <SettingRow label={t("cloud.unavailable")} />
        {/* Off is always in reach: a switch left on would send analyses to a cloud that isn't there. */}
        {status.enabled && (
          <SettingRow label={t("cloud.use.label")} description={t("cloud.use.hiddenHint")}>
            <Switch
              aria-label={t("cloud.use.label")}
              checked
              onCheckedChange={(on) => void run(() => commands.cloudSetEnabled(on), t("saveFailed"))}
            />
          </SettingRow>
        )}
      </SettingsGroup>
    )
  }

  const redeem = async () => {
    setRedeeming(true)
    const ok = await run(() => commands.cloudRedeem(code), t("cloud.failed"))
    setRedeeming(false)
    if (ok) {
      setCode("")
      toast.success(t("cloud.redeem.done"))
    }
  }

  const addFunds = async (cents: number) => {
    setPaying(cents)
    try {
      await commands.cloudAddFunds(cents)
    } catch (e) {
      toast.error(t("cloud.failed"), { description: errorMessage(e) })
    } finally {
      setPaying(null)
    }
  }

  return (
    <SettingsGroup title={t("cloud.title")} description={t("cloud.description")}>
      <SettingRow label={t("cloud.use.label")} description={t("cloud.use.description")}>
        <Switch
          aria-label={t("cloud.use.label")}
          checked={status?.enabled ?? false}
          // Off is always in reach: an analysis can't be stuck with a cloud that fails.
          disabled={!status || (!status.enabled && (!account || account.blocked))}
          onCheckedChange={(on) => void run(() => commands.cloudSetEnabled(on), t("saveFailed"))}
        />
      </SettingRow>
      {status?.error && <SettingRow label={t("cloud.failed")} description={say(status.error)} />}
      {account && (
        <>
          <SettingRow
            label={t("cloud.balance.label")}
            description={
              account.blocked
                ? t("cloud.balance.blocked")
                : account.paused
                  ? t("cloud.balance.paused")
                  : account.betaUntil
                    ? t("cloud.balance.beta", { date: fmt.longDate(account.betaUntil * 1000) })
                    : t("cloud.balance.minutes", { count: account.minutes })
            }
          >
            <span className="text-2xl font-semibold tabular-nums text-fg-1">{account.balance}</span>
          </SettingRow>
          {account.limits && <LimitsRow limits={account.limits} />}
          {status?.stage === "beta" && (
            <SettingRow label={t("cloud.beta.label")} description={t("cloud.beta.description")} />
          )}
          {/* Money comes in only once the cloud is open: the beta runs on passes. */}
          {status?.stage === "open" && (
            <SettingRow label={t("cloud.addFunds.label")} description={t("cloud.addFunds.description")}>
              <div className="flex gap-2">
                {account.topUps.map((top) => (
                  <Button
                    key={top.cents}
                    variant="secondary"
                    busy={paying === top.cents}
                    disabled={account.blocked || paying !== null}
                    onClick={() => void addFunds(top.cents)}
                  >
                    {top.label}
                  </Button>
                ))}
              </div>
            </SettingRow>
          )}
          <SettingRow
            label={t("cloud.redeem.label")}
            description={t("cloud.redeem.description")}
            below={
              <form
                className="flex w-full gap-2"
                onSubmit={(e) => {
                  e.preventDefault()
                  if (code.trim()) void redeem()
                }}
              >
                <Input
                  aria-label={t("cloud.redeem.label")}
                  placeholder={t("cloud.redeem.placeholder")}
                  value={code}
                  maxLength={32}
                  onChange={(e) => setCode(e.target.value)}
                />
                <Button type="submit" busy={redeeming} disabled={!code.trim() || account.blocked}>
                  {t("cloud.redeem.action")}
                </Button>
              </form>
            }
          />
          <SettingRow
            label={t("cloud.prices.label")}
            description={t("cloud.prices.line", {
              standard: account.prices.standard,
              fast: account.prices.fast,
              best: account.prices.best,
            })}
          />
          <SettingRow label={t("cloud.supportId.label")} description={t("cloud.supportId.description")}>
            <div className="flex items-center gap-3">
              <span className="font-mono text-md text-fg-1">{account.supportId}</span>
              <Button variant="ghost" size="sm" onClick={() => void copyText(account.supportId)}>
                {t("cloud.supportId.copy")}
              </Button>
            </div>
          </SettingRow>
        </>
      )}
    </SettingsGroup>
  )
}

const KINDS = [
  "live",
  "live_fast",
  "animated",
  "animated_fast",
  "anime",
  "anime_fast",
  "anime_best",
  "people",
  "scenes",
] as const

/** What the account may scan: each limit on a line, how periods count below. */
function LimitsRow({ limits }: { limits: CloudLimits }) {
  const { t } = useTranslation("settings")
  const kinds = KINDS.filter((k) => limits.kinds?.includes(k)).map((k) => t(`cloud.limits.kind.${k}`))
  const lines: string[] = []
  if (limits.day) lines.push(t("cloud.limits.day", limits.day))
  if (limits.week) lines.push(t("cloud.limits.week", limits.week))
  if (limits.month) lines.push(t("cloud.limits.month", limits.month))
  if (limits.scansPerDay != null)
    lines.push(t("cloud.limits.scans", { used: limits.scansToday, count: limits.scansPerDay }))
  if (limits.maxMinutes != null) lines.push(t("cloud.limits.minutes", { count: limits.maxMinutes }))
  if (limits.kinds) lines.push(t("cloud.limits.kinds", { list: kinds.join(", ") }))
  return (
    <SettingRow
      label={t("cloud.limits.label")}
      description={
        <>
          {lines.map((line) => (
            <span key={line} className="block text-fg-1">
              {line}
            </span>
          ))}
          <span className="mt-1 block">{t("cloud.limits.note")}</span>
        </>
      }
    />
  )
}
