import { useState } from "react"
import type * as React from "react"
import type { TFunction } from "i18next"
import { toast } from "sonner"
import { Trans, useTranslation } from "react-i18next"
import type { AppSettings, HardwareInfo } from "@/bindings"
import { ArrowCounterClockwise, ArrowSquareOut, CircleNotch, DownloadSimple } from "@/components/icons"
import { Button } from "@/components/ui/button"
import { ConfirmDialog } from "@/components/ui/confirm-dialog"
import { Progress } from "@/components/ui/progress"
import { Select, type SelectOption } from "@/components/ui/select"
import { Switch } from "@/components/ui/switch"
import { say, t } from "@/lib/i18n"
import { commands, errorMessage } from "@/lib/ipc"
import { openExternalUrl } from "@/lib/os"
import { qk, queryClient, useAppInfo, useHardware, useSettings } from "@/lib/queries"
import { SettingRow, SettingsGroup } from "../primitives"

type T = TFunction<["settings", "common"]>

const auto = (t: T): SelectOption => ({ value: "auto", label: t("hardware.automatic") })
const processor = (t: T): SelectOption => ({ value: "cpu", label: t("hardware.processorOnly") })

/** Where "Get the driver" leads: NVIDIA's own driver page. */
const DRIVERS = "https://www.nvidia.com/drivers/"

/**
 * What the analysis runs on, in one sentence. The device is named here and
 * nowhere else in Settings: Fast Scan's line says what Automatic means
 * without naming it, and follows the card chosen here. A sentence per
 * device, whole: another language may bend the words around it.
 */
function hardwareLine(hw: HardwareInfo, t: T): string {
  if (hw.pending) return t("hardware.line.pending")
  // The processor, a card by its name, or this computer's card when it has no name.
  const name = hw.using.kind === "processor" ? "" : hw.using.name
  const device = hw.using.kind === "processor" ? "processor" : name ? "card" : "generic"
  if (hw.missing) {
    if (device === "processor") return t("hardware.line.missing.processor")
    return device === "card" ? t("hardware.line.missing.card", { name }) : t("hardware.line.missing.generic")
  }
  if (hw.fellBack) return t("hardware.line.fellBack")
  if (hw.using.kind === "processor" && hw.selected === "cpu") return t("hardware.line.processorChosen")
  if (device === "processor") return hw.accel.active ? t("hardware.line.runs.processorAccel") : t("hardware.line.runs.processor")
  if (device === "card") return hw.accel.active ? t("hardware.line.runs.cardAccel", { name }) : t("hardware.line.runs.card", { name })
  return hw.accel.active ? t("hardware.line.runs.genericAccel") : t("hardware.line.runs.generic")
}

/** Automatic, each card when there are two or more, and the processor only. */
function options(hw: HardwareInfo, t: T): SelectOption[] {
  const cards = hw.choice === "cards" ? hw.gpus.map((g) => ({ value: g.key, label: say(g.name) })) : []
  return [auto(t), ...cards, processor(t)]
}

/** Saves what the analysis runs on: it applies from the next analysis. */
async function save(settings: AppSettings) {
  try {
    const s = await commands.settingsUpdate(settings)
    queryClient.setQueryData(qk.settings, s)
    void queryClient.invalidateQueries({ queryKey: qk.hardware })
    return true
  } catch (e) {
    toast.error(t("settings:saveFailed"), { description: errorMessage(e) })
    return false
  }
}

/** Saves the choice; Fast Scan's default follows the card. */
async function choose(settings: AppSettings, selected: string) {
  if (await save({ ...settings, gpu: selected === "auto" ? "" : selected })) {
    void queryClient.invalidateQueries({ queryKey: qk.scanDefaults })
  }
}

/** Bytes as the row counts them. */
const mb = (bytes: number) => Math.round(bytes / 1_000_000)

/** A link inside a row's text. */
function TextLink({ onClick, children }: { onClick: () => void; children?: React.ReactNode }) {
  return (
    <button
      type="button"
      className="cursor-pointer rounded-xs text-fg-1 underline decoration-white/30 underline-offset-2 transition-colors hover:decoration-white/70"
      onClick={onClick}
    >
      {children}
    </button>
  )
}

/**
 * Asks before NVIDIA's files go. `slower`: analysis uses them now, so it
 * says what that costs; anywhere else nothing changes but the disk.
 */
function RemoveDialog({
  open,
  onOpenChange,
  slower,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  slower: boolean
}) {
  const { t } = useTranslation(["settings", "common"])
  return (
    <ConfirmDialog
      open={open}
      onOpenChange={onOpenChange}
      title={t("hardware.accel.remove.title")}
      description={slower ? t("hardware.accel.remove.slower") : t("hardware.accel.remove.plain")}
      answer={t("hardware.accel.remove.answer")}
      failure={t("hardware.accel.remove.failed")}
      onConfirm={() => commands.accelRemove()}
    />
  )
}

/** Says a command failed; the row shows what came of the rest. */
function run(what: string, command: () => Promise<unknown>) {
  return command().catch((e: unknown) => toast.error(what, { description: errorMessage(e) }))
}

/** An action in the row's text, quieter than its control. */
const ROW_ACTION = "mt-1.5 block cursor-pointer rounded-xs text-fg-3 transition-colors hover:text-fg-1"

/**
 * NVIDIA acceleration: offered where analysis runs on a card it serves, and
 * shown wherever its files are. The row is the download's only home.
 */
function AccelSetting({ hw, settings }: { hw: HardwareInfo; settings: AppSettings | undefined }) {
  const { t } = useTranslation(["settings", "common"])
  const [removing, setRemoving] = useState(false)
  // Trying again checks every file and starts an engine: that takes a while.
  const [retrying, setRetrying] = useState(false)
  const { row, downloadBytes, licence, removable, updateStopped } = hw.accel
  if (row.state === "hidden") return null

  let text: React.ReactNode
  /** What changes too often to read out, under the text. */
  let detail: React.ReactNode = null
  let control: React.ReactNode = null
  const switchOn = settings?.nvidiaAcceleration ?? true
  const accelSwitch = (disabled: boolean) => (
    <Switch
      aria-label={t("hardware.accel.label")}
      checked={switchOn}
      disabled={disabled || !settings}
      onCheckedChange={(on) => settings && void save({ ...settings, nvidiaAcceleration: on })}
    />
  )
  const tryAgain = (onClick: () => void, busy = false) => (
    <Button size="sm" busy={busy} onClick={onClick}>
      <ArrowCounterClockwise /> {t("common:action.tryAgain")}
    </Button>
  )
  const retry = () => {
    setRetrying(true)
    void run(t("hardware.accel.retryFailed"), commands.accelTryAgain).finally(() => setRetrying(false))
  }
  const install = () => run(t("hardware.accel.installFailed"), commands.accelInstall)

  switch (row.state) {
    case "available":
      text = (
        <Trans
          t={t}
          i18nKey="hardware.accel.available"
          values={{ mb: mb(downloadBytes) }}
          components={{ go: <TextLink onClick={() => void openExternalUrl(licence)} /> }}
        />
      )
      control = (
        <Button variant="primary" size="sm" onClick={install}>
          <DownloadSimple /> {t("hardware.accel.agree")}
        </Button>
      )
      break
    case "downloading":
      text = t("hardware.accel.downloading")
      detail = (
        <>
          <Progress value={row.total > 0 ? row.done / row.total : null} className="mt-2.5 w-80 max-w-full" />
          <span className="timecode mt-1.5 block text-fg-3">
            {t("hardware.accel.progress", { done: mb(row.done), total: mb(row.total) })}
          </span>
        </>
      )
      control = (
        <Button size="sm" onClick={() => run(t("hardware.accel.cancelFailed"), commands.accelCancel)}>
          {t("common:action.cancel")}
        </Button>
      )
      break
    case "failed":
      text = say(row.message)
      control = tryAgain(install)
      break
    case "removing":
      text = t("hardware.accel.removing")
      break
    case "notUsed":
      text = hw.using.kind === "processor" ? t("hardware.accel.notUsedProcessor") : t("hardware.accel.notUsedOtherCard")
      control = accelSwitch(true)
      break
    case "stopped":
      text = t("hardware.accel.stopped")
      control = tryAgain(retry, retrying)
      break
    case "needsDriver":
      text = t("hardware.accel.needsDriver")
      control = (
        <Button size="sm" onClick={() => void openExternalUrl(DRIVERS)}>
          <ArrowSquareOut /> {t("hardware.accel.getDriver")}
        </Button>
      )
      break
    case "ready":
      text = t("hardware.accel.ready")
      control = accelSwitch(false)
      break
  }

  return (
    <>
      <SettingRow
        label={t("hardware.accel.label")}
        description={
          <>
            <span aria-live="polite" className="block text-pretty">
              {text}
            </span>
            {detail}
            {updateStopped && (
              <button type="button" className={ROW_ACTION} onClick={() => void install()}>
                {t("hardware.accel.update", { mb: mb(downloadBytes) })}
              </button>
            )}
            {removable && row.state !== "downloading" && row.state !== "removing" && (
              <button type="button" className={ROW_ACTION} onClick={() => setRemoving(true)}>
                {t("hardware.accel.removeFiles")}
              </button>
            )}
          </>
        }
      >
        {control}
      </SettingRow>
      <RemoveDialog open={removing} onOpenChange={setRemoving} slower={row.state === "ready" && switchOn} />
    </>
  )
}

/** Settings → General → Hardware acceleration: the graphics the analysis runs on. */
export function HardwareGroup() {
  const { t } = useTranslation(["settings", "common"])
  const { data: info } = useAppInfo()
  const { data: settings } = useSettings()
  const hw = useHardware()
  const mac = info?.platform === "macos"
  const label = mac ? t("hardware.chip") : t("hardware.graphicsCard")

  let description: React.ReactNode
  let control: React.ReactNode = null
  if (hw.isError && !hw.isFetching) {
    description = mac ? t("hardware.checkFailed.chip") : t("hardware.checkFailed.cards")
    control = (
      <Button size="sm" onClick={() => void hw.refetch()}>
        <ArrowCounterClockwise /> {t("common:action.tryAgain")}
      </Button>
    )
  } else if (!hw.data) {
    description = (
      <span className="inline-flex items-center gap-1.5">
        <CircleNotch aria-hidden className="size-3.5 animate-spin" />
        {mac ? t("hardware.checking.chip") : t("hardware.checking.cards")}
      </span>
    )
    // Linux analyses on the processor: nothing to choose, not even while looking.
    if (info && info.platform !== "linux") {
      control = (
        <Select aria-label={label} className="w-[260px]" value="auto" disabled onChange={() => {}} options={[auto(t), processor(t)]} />
      )
    }
  } else {
    const data = hw.data
    description = (
      <span aria-live="polite" className="block text-pretty">
        {hardwareLine(data, t)}
      </span>
    )
    if (data.choice !== "fixed") {
      control = (
        <Select
          aria-label={label}
          className="w-[260px]"
          // A stored card the list no longer shows (gone, or one of fewer than two left) shows Automatic as a hint, not as chosen:
          // picking Automatic is then a change, and clears the card.
          value={data.selected === "auto" && settings?.gpu && settings.gpu !== "cpu" ? "" : data.selected}
          placeholder={t("hardware.automatic")}
          disabled={!settings}
          onChange={(v) => settings && void choose(settings, v)}
          options={options(data, t)}
        />
      )
    }
  }

  return (
    <SettingsGroup title={t("hardware.title")} description={t("hardware.description")}>
      <SettingRow label={label} description={description}>
        {control}
      </SettingRow>
      {hw.data && <AccelSetting hw={hw.data} settings={settings} />}
    </SettingsGroup>
  )
}
