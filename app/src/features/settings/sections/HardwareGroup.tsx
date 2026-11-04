import { useState } from "react"
import type * as React from "react"
import { toast } from "sonner"
import type { AppSettings, HardwareInfo } from "@/bindings"
import { ArrowCounterClockwise, ArrowSquareOut, CircleNotch, DownloadSimple } from "@/components/icons"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogFooter } from "@/components/ui/dialog"
import { Progress } from "@/components/ui/progress"
import { Select, type SelectOption } from "@/components/ui/select"
import { Switch } from "@/components/ui/switch"
import { commands, errorMessage } from "@/lib/ipc"
import { openExternalUrl } from "@/lib/os"
import { qk, queryClient, useAppInfo, useHardware, useSettings } from "@/lib/queries"
import { SettingRow, SettingsGroup } from "../primitives"

const AUTOMATIC = "Automatic"
const AUTO: SelectOption = { value: "auto", label: AUTOMATIC }
const PROCESSOR: SelectOption = { value: "cpu", label: "Processor only" }

/** Where "Get the driver" leads: NVIDIA's own driver page. */
const DRIVERS = "https://www.nvidia.com/drivers/"

/**
 * What the analysis runs on, in one sentence. The device is named here and
 * nowhere else in Settings: Fast Scan's line says what Automatic means
 * without naming it, and follows the card chosen here.
 */
function hardwareLine(hw: HardwareInfo): string {
  if (hw.pending) return "Applies from the next analysis."
  const device = hw.using.kind === "processor" ? "the processor" : hw.using.name ? `the ${hw.using.name}` : "this computer's graphics card"
  if (hw.missing) return `The graphics card you chose isn't connected, so analysis uses ${device}.`
  if (hw.fellBack) return "In the last analysis some steps ran on the processor, which is slower."
  if (hw.using.kind === "processor" && hw.selected === "cpu") return "Analysis runs on the processor. That is much slower."
  return `Analysis runs on ${device}${hw.accel.active ? ", with NVIDIA acceleration" : ""}.`
}

/** Automatic, each card when there are two or more, and the processor only. */
function options(hw: HardwareInfo): SelectOption[] {
  const cards = hw.choice === "cards" ? hw.gpus.map((g) => ({ value: g.key, label: g.name })) : []
  return [AUTO, ...cards, PROCESSOR]
}

/** Saves what the analysis runs on: it applies from the next analysis. */
async function save(settings: AppSettings) {
  try {
    const s = await commands.settingsUpdate(settings)
    queryClient.setQueryData(qk.settings, s)
    void queryClient.invalidateQueries({ queryKey: qk.hardware })
    return true
  } catch (e) {
    toast.error("Couldn't save the setting", { description: errorMessage(e) })
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
function TextLink({ onClick, children }: { onClick: () => void; children: React.ReactNode }) {
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
  const [busy, setBusy] = useState(false)
  const remove = async () => {
    setBusy(true)
    try {
      await commands.accelRemove()
      onOpenChange(false)
    } catch (e) {
      toast.error("Couldn't remove NVIDIA's files", { description: errorMessage(e) })
    } finally {
      setBusy(false)
    }
  }
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        title="Remove NVIDIA acceleration?"
        description={
          slower
            ? "Analysis keeps running on this graphics card, more slowly. You can download it again any time."
            : "You can download it again any time."
        }
      >
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button variant="danger" disabled={busy} onClick={() => void remove()}>
            {busy && <CircleNotch className="animate-spin" />}
            Remove
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

/** Says a command failed; the row shows what came of the rest. */
function run(what: string, command: () => Promise<unknown>) {
  return command().catch((e: unknown) => toast.error(what, { description: errorMessage(e) }))
}

/**
 * NVIDIA acceleration: offered where analysis runs on a card it serves, and
 * shown wherever its files are. The row is the download's only home.
 */
function AccelSetting({ hw, settings }: { hw: HardwareInfo; settings: AppSettings | undefined }) {
  const [removing, setRemoving] = useState(false)
  // Trying again checks every file and starts an engine: that takes a while.
  const [retrying, setRetrying] = useState(false)
  const { row, downloadBytes, licence, removable } = hw.accel
  if (row.state === "hidden") return null

  let text: React.ReactNode
  /** What changes too often to read out, under the text. */
  let detail: React.ReactNode = null
  let control: React.ReactNode = null
  const switchOn = settings?.nvidiaAcceleration ?? true
  const accelSwitch = (disabled: boolean) => (
    <Switch
      aria-label="NVIDIA acceleration"
      checked={switchOn}
      disabled={disabled || !settings}
      onCheckedChange={(on) => settings && void save({ ...settings, nvidiaAcceleration: on })}
    />
  )
  const tryAgain = (onClick: () => void, busy = false) => (
    <Button size="sm" disabled={busy} onClick={onClick}>
      {busy ? <CircleNotch className="animate-spin motion-reduce:animate-none" /> : <ArrowCounterClockwise />} Try again
    </Button>
  )
  const retry = () => {
    setRetrying(true)
    void run("Couldn't try again", commands.accelTryAgain).finally(() => setRetrying(false))
  }
  const install = () => run("Couldn't download NVIDIA's files", commands.accelInstall)

  switch (row.state) {
    case "available":
      text = (
        <>
          NVIDIA's engine for GeForce RTX cards, so anime analysis runs faster. AutoScene downloads NVIDIA's files (about{" "}
          {mb(downloadBytes)} MB) from the Python Package Index, which sees your IP address. By downloading you agree to{" "}
          <TextLink onClick={() => void openExternalUrl(licence)}>NVIDIA's licence</TextLink>.
        </>
      )
      control = (
        <Button variant="primary" size="sm" onClick={install}>
          <DownloadSimple /> Agree and download
        </Button>
      )
      break
    case "downloading":
      text = "Downloading NVIDIA's files."
      detail = (
        <>
          <Progress value={row.total > 0 ? row.done / row.total : null} className="mt-2.5 w-80 max-w-full" />
          <span className="timecode mt-1.5 block text-fg-3">
            {mb(row.done)} of {mb(row.total)} MB
          </span>
        </>
      )
      control = (
        <Button size="sm" onClick={() => run("Couldn't cancel the download", commands.accelCancel)}>
          Cancel
        </Button>
      )
      break
    case "failed":
      text = row.message
      control = tryAgain(install)
      break
    case "removing":
      text = "Removing after the current analysis."
      break
    case "notUsed":
      text =
        hw.using.kind === "processor" ? "Not used: analysis runs on the processor." : "Not used: another graphics card is chosen."
      control = accelSwitch(true)
      break
    case "stopped":
      text = "NVIDIA acceleration stopped working, so analysis runs without it."
      control = tryAgain(retry, retrying)
      break
    case "needsDriver":
      text = "NVIDIA acceleration couldn't use this card. A newer NVIDIA driver may help."
      control = (
        <Button size="sm" onClick={() => void openExternalUrl(DRIVERS)}>
          <ArrowSquareOut /> Get the driver
        </Button>
      )
      break
    case "ready":
      text = "NVIDIA's engine for GeForce RTX cards, so anime analysis runs faster."
      control = accelSwitch(false)
      break
  }

  return (
    <>
      <SettingRow
        label="NVIDIA acceleration"
        description={
          <>
            <span aria-live="polite" className="block text-pretty">
              {text}
            </span>
            {detail}
            {removable && row.state !== "downloading" && row.state !== "removing" && (
              <button
                type="button"
                className="mt-1.5 block cursor-pointer rounded-xs text-fg-3 transition-colors hover:text-fg-1"
                onClick={() => setRemoving(true)}
              >
                Remove NVIDIA's files
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
  const { data: info } = useAppInfo()
  const { data: settings } = useSettings()
  const hw = useHardware()
  const mac = info?.platform === "macos"
  const label = mac ? "Chip" : "Graphics card"

  let description: React.ReactNode
  let control: React.ReactNode = null
  if (hw.isError && !hw.isFetching) {
    description = mac ? "Couldn't check the chip." : "Couldn't check the graphics cards."
    control = (
      <Button size="sm" onClick={() => void hw.refetch()}>
        <ArrowCounterClockwise /> Try again
      </Button>
    )
  } else if (!hw.data) {
    description = (
      <span className="inline-flex items-center gap-1.5">
        <CircleNotch aria-hidden className="size-3.5 animate-spin motion-reduce:animate-none" />
        {mac ? "Checking the chip…" : "Looking for graphics cards…"}
      </span>
    )
    // Linux analyses on the processor: nothing to choose, not even while looking.
    if (info && info.platform !== "linux") {
      control = <Select aria-label={label} className="w-[260px]" value="auto" disabled onChange={() => {}} options={[AUTO, PROCESSOR]} />
    }
  } else {
    const data = hw.data
    description = (
      <span aria-live="polite" className="block text-pretty">
        {hardwareLine(data)}
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
          placeholder={AUTOMATIC}
          disabled={!settings}
          onChange={(v) => settings && void choose(settings, v)}
          options={options(data)}
        />
      )
    }
  }

  return (
    <SettingsGroup title="Hardware acceleration" description="Where analysis runs on this computer.">
      <SettingRow label={label} description={description}>
        {control}
      </SettingRow>
      {hw.data && <AccelSetting hw={hw.data} settings={settings} />}
    </SettingsGroup>
  )
}
