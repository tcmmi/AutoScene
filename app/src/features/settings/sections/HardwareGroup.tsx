import type * as React from "react"
import { toast } from "sonner"
import type { AppSettings, HardwareInfo } from "@/bindings"
import { ArrowCounterClockwise, CircleNotch } from "@/components/icons"
import { Button } from "@/components/ui/button"
import { Select, type SelectOption } from "@/components/ui/select"
import { commands, errorMessage } from "@/lib/ipc"
import { qk, queryClient, useAppInfo, useHardware, useSettings } from "@/lib/queries"
import { SettingRow, SettingsGroup } from "../primitives"

const AUTO: SelectOption = { value: "auto", label: "Automatic" }
const PROCESSOR: SelectOption = { value: "cpu", label: "Processor only" }

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
  return `Analysis runs on ${device}.`
}

/** Automatic, each card when there are two or more, and the processor only. */
function options(hw: HardwareInfo): SelectOption[] {
  const cards = hw.choice === "cards" ? hw.gpus.map((g) => ({ value: g.key, label: g.name })) : []
  return [AUTO, ...cards, PROCESSOR]
}

/** Saves the choice; it applies from the next analysis, and Fast Scan's default follows the card. */
async function choose(settings: AppSettings, selected: string) {
  try {
    const s = await commands.settingsUpdate({ ...settings, gpu: selected === "auto" ? "" : selected })
    queryClient.setQueryData(qk.settings, s)
    void queryClient.invalidateQueries({ queryKey: qk.hardware })
    void queryClient.invalidateQueries({ queryKey: qk.scanDefaults })
  } catch (e) {
    toast.error("Couldn't save the setting", { description: errorMessage(e) })
  }
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
          value={data.selected}
          disabled={!settings}
          onChange={(v) => settings && v !== data.selected && void choose(settings, v)}
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
    </SettingsGroup>
  )
}
