/**
 * The computer the mock analyses on: its graphics cards (Settings → General →
 * Graphics card) and how Fast Scan's default classes the one in use. One
 * switch for both, as in the app, where one adapter list answers both:
 *
 * `?mockMachine=rtx|gtx|integrated|processor|linux|apple-base|apple-pro|unknown|two-gpus|hybrid|no-dxgi`
 * (before the `#`). `rtx`, the default, keeps the full scan as the dev
 * machine does; the platform follows the machine.
 *
 * `?mockHardware=loading|busy|fell-back|missing` shows the row's other
 * states: still looking, an analysis running (a choice then applies from the
 * next), the last analysis on the processor in part, a chosen card that isn't
 * connected. `?mockFail=hardware_info` shows the error.
 */
import type { AppInfo, GraphicsClass, HardwareInfo, ThisComputer } from "@/bindings"

/** A card DirectML opens, and how Fast Scan's default classes it. */
type Card = { key: string; name: string; class: GraphicsClass }

type Machine = {
  platform: AppInfo["platform"]
  /** In DirectML's order: the first is Automatic's. */
  cards: Card[]
  /** Apple silicon: the chip. */
  chip?: string
  /** Discovery gave no adapter numbers: only Automatic can be honoured. */
  noDxgi?: boolean
  /** DXCore can't be read, so Fast Scan can't class the graphics. */
  unknown?: boolean
}

/** Like `as_infer::hardware::key`. */
const card = (vendor: string, device: string, name: string, cls: GraphicsClass, nth = 0): Card => ({
  key: `${vendor}:${device}:${name}${nth > 0 ? `#${nth + 1}` : ""}`,
  name,
  class: cls,
})

const RTX_LAPTOP = card("10de", "28e0", "NVIDIA GeForce RTX 4060 Laptop GPU", "forAi")
const IRIS = card("8086", "a7a0", "Intel Iris Xe Graphics", "integrated")

const MACHINES: Record<string, Machine> = {
  rtx: { platform: "windows", cards: [card("10de", "24c9", "NVIDIA GeForce RTX 3060 Ti", "forAi")] },
  gtx: { platform: "windows", cards: [card("10de", "1c03", "NVIDIA GeForce GTX 1060 6GB", "notForAi")] },
  integrated: { platform: "windows", cards: [card("8086", "9a49", "Intel Iris Xe Graphics", "integrated")] },
  processor: { platform: "windows", cards: [] },
  linux: { platform: "linux", cards: [] },
  "apple-base": { platform: "macos", cards: [], chip: "Apple M2" },
  "apple-pro": { platform: "macos", cards: [], chip: "Apple M3 Max" },
  unknown: { platform: "windows", cards: [card("1ed5", "0300", "Moore Threads MTT S80", "unknown")], unknown: true },
  "two-gpus": {
    platform: "windows",
    cards: [card("10de", "2204", "NVIDIA GeForce RTX 3090", "forAi"), card("10de", "2204", "NVIDIA GeForce RTX 3090", "forAi", 1)],
  },
  hybrid: { platform: "windows", cards: [RTX_LAPTOP, IRIS] },
  "no-dxgi": { platform: "windows", cards: [RTX_LAPTOP, IRIS], noDxgi: true },
}

const params = new URLSearchParams(window.location.search)
const machineName = params.get("mockMachine")
const machine = MACHINES[machineName ?? "rtx"] ?? MACHINES.rtx
const state = params.get("mockHardware")

/** The platform the machine runs, when one is named. */
export const mockPlatform: AppInfo["platform"] | null = machineName ? machine.platform : null

/** A choice to start with (`missing`: a card that isn't connected any more). */
export const initialGpu = state === "missing" ? "1002:744c:AMD Radeon RX 7900 XTX" : ""

/** Its ids and place among identical cards, like `Choice::from_setting`. */
function ids(key: string): [string, number] {
  const [vendor = "", device = "", ...rest] = key.toLowerCase().split(":")
  const n = Number(/#(\d+)$/.exec(rest.join(":"))?.[1] ?? 1)
  return [`${vendor}:${device}`, Math.max(n, 1) - 1]
}

/** The card a choice opens (an index into the cards), whether it is missing. */
function resolve(gpu: string): { index: number | "cpu" | null; missing: boolean } {
  const auto = machine.cards.length > 0 ? 0 : null
  if (gpu === "cpu") return { index: "cpu", missing: false }
  if (gpu === "" || machine.cards.length === 0) return { index: auto, missing: false }
  const [id, nth] = ids(gpu)
  const same = machine.cards.flatMap((c, i) => (ids(c.key)[0] === id ? [i] : []))
  const found = same[nth] ?? same[0]
  if (found == null) return { index: auto, missing: true }
  return { index: machine.noDxgi ? auto : found, missing: false }
}

/** What Fast Scan's default sees with the graphics `gpu` chooses (`AppSettings.gpu`). */
export function thisComputer(gpu: string): ThisComputer {
  const { index } = resolve(gpu)
  if (index === "cpu") return { class: "processor", name: "" }
  if (machine.chip) return { class: "integrated", name: machine.chip }
  if (machine.unknown) return { class: "unknown", name: "" }
  const c = index == null ? null : machine.cards[index]
  return c ? { class: c.class, name: c.name } : { class: "processor", name: "" }
}

/** Identical cards are numbered. */
function label(c: Card): string {
  const twins = machine.cards.filter((o) => ids(o.key)[0] === ids(c.key)[0])
  return twins.length > 1 ? `${c.name} (${ids(c.key)[1] + 1})` : c.name
}

/** Like `commands::hardware::info`: `hardware_info` with the graphics `gpu` chooses. */
export function hardwareInfo(gpu: string, pending: boolean): HardwareInfo | Promise<never> {
  if (state === "loading") return new Promise<never>(() => {})
  const { index, missing } = resolve(gpu)
  const openable = machine.noDxgi ? [] : machine.cards
  const choice = openable.length >= 2 ? "cards" : machine.cards.length === 0 && !machine.chip ? "fixed" : "autoOrProcessor"
  const c = typeof index === "number" ? machine.cards[index] : null
  const using: HardwareInfo["using"] =
    index === "cpu" || (!c && !machine.chip)
      ? { kind: "processor" }
      : machine.chip
        ? { kind: "chip", name: machine.chip }
        : { kind: "gpu", name: c ? label(c) : "" }
  const selected = index === "cpu" ? "cpu" : choice === "cards" && gpu !== "" && !missing && c ? c.key : "auto"
  return {
    gpus: openable.map((o) => ({ key: o.key, name: label(o) })),
    choice,
    selected,
    using,
    missing,
    pending,
    fellBack: state === "fell-back",
  }
}

/** `busy`: an analysis runs, so a choice applies from the next one. */
export const analysisRuns = state === "busy"
