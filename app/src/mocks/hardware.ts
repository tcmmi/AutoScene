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
 *
 * `?mockAccel=on|off|downloading|failed|no-room|removing|driver|stopped|update-licence|update-stopped|half`
 * starts NVIDIA acceleration in that state: on an offered card, or with
 * `on`, `off` and `removing` on any card as a pack left there. `half` is an
 * unfinished download found at a start. Without it
 * the row is Available wherever it is offered. Agree and download then
 * fills the bar; `downloading` waits at 45 MB.
 */
import { emit } from "@tauri-apps/api/event"
import type { AccelInfo, AccelRow, AppInfo, GraphicsClass, HardwareInfo, Msg, ThisComputer } from "@/bindings"

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

/** The bytes the pack's download fetches, and where `accelInfo` finds its pins. */
const ACCEL_BYTES = 110_866_443
const ACCEL_LICENCE = "https://docs.nvidia.com/deeplearning/tensorrt-rtx/latest/reference/sla.html"
const ACCEL_ENGINE = "1.7.1"

const accelState = params.get("mockAccel")

/** Like `as_infer::accel::serves`: an NVIDIA card of the Ampere generation or newer. */
function serves(c: Card | null | undefined): boolean {
  const [vendor, device] = (c?.key ?? "").split(":")
  return machine.platform === "windows" && vendor === "10de" && Number.parseInt(device ?? "0", 16) >= 0x2200
}

/** The switch to start with. */
export const initialAccelOn = accelState !== "off"

type Phase = { state: "idle" } | { state: "downloading"; done: number } | { state: "failed"; message: Msg } | { state: "removing" }

const NO_NET: Msg = { key: "accel.downloadFailed", text: "NVIDIA's files couldn't be downloaded. Check the internet connection and try again." }
const NO_ROOM: Msg = {
  key: "accel.noSpace",
  args: { mb: 124 },
  text: "There isn't enough free space for NVIDIA's files. Free up 124 MB on the drive of AutoScene's data folder and try again.",
}

/** What the mock's download and removal are doing, and whether the pack is here. */
let phase: Phase =
  accelState === "downloading"
    ? { state: "downloading", done: 45_000_000 }
    : accelState === "failed"
      ? { state: "failed", message: NO_NET }
      : accelState === "no-room"
        ? { state: "failed", message: NO_ROOM }
        : accelState === "removing"
          ? { state: "removing" }
          : { state: "idle" }
let installed = ["on", "off", "removing", "driver", "stopped", "update-licence", "update-stopped"].includes(accelState ?? "")
let stopped = accelState === "stopped"
/** The person stopped the update to the release this build pins. */
let updateStopped = accelState === "update-stopped"
/** An unfinished download is here: nothing to use, but something to remove. */
let half = accelState === "half"
let ticking: ReturnType<typeof setInterval> | undefined

/** Like `accel::row`: the first state that applies. */
function accelRow(offered: boolean): AccelRow {
  if (phase.state === "downloading") return { state: "downloading", done: phase.done, total: ACCEL_BYTES }
  if (phase.state === "failed") return { state: "failed", message: phase.message }
  if (phase.state === "removing") return { state: "removing" }
  if (offered && (!installed || accelState === "update-licence")) return { state: "available" }
  if (!installed) return { state: "hidden" }
  if (!offered) return { state: "notUsed" }
  if (stopped) return { state: "stopped" }
  if (accelState === "driver") return { state: "needsDriver" }
  return { state: "ready" }
}

function accelInfo(offered: boolean, on: boolean): AccelInfo {
  const row = accelRow(offered)
  return {
    row,
    installed,
    removable: installed || half,
    active: on && row.state === "ready",
    updateStopped: offered && updateStopped,
    downloadBytes: ACCEL_BYTES,
    licence: ACCEL_LICENCE,
    engine: ACCEL_ENGINE,
  }
}

/** The commands of the row: `switchedOn` is what a finished download does to the setting. */
export function accelCalls(switchedOn: () => void) {
  const changed = () => void emit("hardware:changed", null)
  const stop = () => {
    clearInterval(ticking)
    ticking = undefined
  }
  return {
    accel_install: () => {
      updateStopped = false
      phase = { state: "downloading", done: 0 }
      changed()
      ticking = setInterval(() => {
        if (phase.state !== "downloading") return stop()
        const done = Math.min(phase.done + 3_700_000, ACCEL_BYTES)
        phase = { state: "downloading", done }
        void emit("accel:progress", { done, total: ACCEL_BYTES })
        if (done === ACCEL_BYTES) {
          stop()
          phase = { state: "idle" }
          installed = true
          half = false
          switchedOn()
          changed()
          void emit("accel:finished", { error: null })
        }
      }, 120)
      return null
    },
    accel_cancel: () => {
      stop()
      phase = { state: "idle" }
      changed()
      return null
    },
    accel_remove: () => {
      phase = { state: "removing" }
      changed()
      setTimeout(() => {
        phase = { state: "idle" }
        installed = false
        half = false
        changed()
      }, 900)
      return null
    },
    accel_try_again: () =>
      new Promise<null>((done) =>
        // It checks every file and starts an engine.
        setTimeout(() => {
          stopped = false
          changed()
          done(null)
        }, 900),
      ),
    accel_notices: () => null,
  }
}

/** Like `commands::hardware::info`: `hardware_info` with the graphics `gpu` chooses. */
export function hardwareInfo(gpu: string, pending: boolean, accelOn: boolean): HardwareInfo | Promise<never> {
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
    gpus: openable.map((o) => ({ key: o.key, name: { key: "", text: label(o) } })),
    choice,
    selected,
    using,
    missing,
    pending,
    fellBack: state === "fell-back",
    runtime: machine.platform === "windows" ? "1.24.4" : "1.31.1",
    accel: accelInfo(serves(c), accelOn),
  }
}

/** `busy`: an analysis runs, so a choice applies from the next one. */
export const analysisRuns = state === "busy"
