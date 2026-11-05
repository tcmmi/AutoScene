import { cleanup, renderHook } from "@testing-library/react"
import { toast } from "sonner"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { PageReply } from "@/bindings"
import { GLOBAL_SHORTCUTS, useShortcuts } from "@/lib/shortcuts"
import { LIMITS, type PageInit } from "@/plugin-kit/bridge-types"
import { inLanguage } from "@/test/i18n"
import {
  type HostDeps,
  type Limits,
  type PortLike,
  createConnector,
  createHost,
  inputElsewhere,
  pageCaller,
  pageMessage,
  replayKey,
  showToast,
} from "./bridge"

vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }))

// jsdom has no ports and no `event.source`: the host runs over Node's
// MessageChannel (the global one here), and the frame's window is a stand-in.
const channels: MessageChannel[] = []
function channel(): { port1: PortLike; port2: Transferable } {
  const ch = new MessageChannel()
  channels.push(ch)
  return ch
}

afterEach(() => {
  vi.useRealTimers()
  for (const ch of channels.splice(0)) {
    ch.port1.close()
    ch.port2.close()
  }
  cleanup()
  vi.clearAllMocks()
})

/** No pings while a test runs. */
const QUIET = { pingMs: 1e9 }

const INIT: PageInit = {
  plugin: { id: "dev.a.page", name: "Page", version: "1.0.0" },
  page: { id: "main", title: "Main", scope: "global" },
  projectId: null,
  path: "",
  theme: { v: 1, scheme: "dark" },
  permissions: [],
  hosts: [],
  service: false,
  protocol: 2,
  app: { version: "1.0.3-BETA", platform: "windows" },
}
const READY = { type: "autoscene:ready", v: 1 }

function deps(o: Partial<HostDeps> = {}): HostDeps {
  return {
    call: vi.fn(async (): Promise<PageReply> => ({ kind: "ok", result: null })),
    effect: vi.fn(),
    key: vi.fn(),
    menu: vi.fn(),
    unresponsive: vi.fn(),
    overflow: vi.fn(),
    focused: () => true,
    elsewhere: () => false,
    activated: () => true,
    ...o,
  }
}

/** The next message the page's end hears. */
function heard(port: MessagePort): Promise<unknown> {
  return new Promise((resolve) => {
    const once = (e: MessageEvent) => {
      port.removeEventListener("message", once)
      resolve(e.data)
    }
    port.addEventListener("message", once)
    port.start()
  })
}

/** A host over a new channel, and the page's end of it. */
function hosted(d: HostDeps, limits: Partial<Limits> = {}) {
  const ch = channel()
  createHost(ch.port1, d, { ...QUIET, ...limits })
  return channels.at(-1)!.port2
}

/** Every message the page's end hears, as it hears them. */
function hearing(port: MessagePort): unknown[] {
  const heard: unknown[] = []
  port.addEventListener("message", (e: MessageEvent) => void heard.push(e.data))
  port.start()
  return heard
}

/** Whether `ms` went by: Node's messages arrive in the meantime. */
const after = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

/** A frame's window that keeps what the host posts it. */
function frameWindow() {
  return { postMessage: vi.fn() } as unknown as Window & { postMessage: ReturnType<typeof vi.fn> }
}

describe("the bridge to a plugin page", () => {
  it("ready from another window is ignored", () => {
    const frame = frameWindow()
    const c = createConnector({ frame: () => frame, init: () => INIT, deps: deps(), limits: QUIET, channel })
    expect(c.message({ source: frameWindow(), data: READY })).toBe(false)
    expect(c.message({ source: window, data: READY })).toBe(false)
    expect(c.message({ source: frame, data: { type: "autoscene:ready", v: 2 } })).toBe(false)
    expect(c.message({ source: frame, data: "autoscene:ready" })).toBe(false)
    expect(frame.postMessage).not.toHaveBeenCalled()

    expect(c.message({ source: frame, data: READY })).toBe(true)
    expect(frame.postMessage).toHaveBeenCalledWith({ type: "autoscene:init", v: 1, init: INIT }, "*", [channels[0].port2])
    // One per load of the frame.
    expect(c.message({ source: frame, data: READY })).toBe(false)
    expect(frame.postMessage).toHaveBeenCalledTimes(1)
    c.close()
  })

  it("a new load drops the old port", async () => {
    const frame = frameWindow()
    const call = vi.fn(async (): Promise<PageReply> => ({ kind: "ok", result: "answered" }))
    const c = createConnector({ frame: () => frame, init: () => INIT, deps: deps({ call }), limits: QUIET, channel })
    c.load()
    c.message({ source: frame, data: READY })
    const old = channels[0].port2
    const gone = new Promise((resolve) => old.addEventListener("close", resolve, { once: true }))

    c.load()
    expect(c.message({ source: frame, data: READY })).toBe(true)
    await gone
    // The new document speaks over its own port.
    const page = channels[1].port2
    const reply = heard(page)
    page.postMessage({ t: "call", id: 1, method: "settings.get", params: {} })
    expect(await reply).toEqual({ t: "reply", id: 1, ok: true, result: "answered" })
    expect(call).toHaveBeenCalledTimes(1)
    c.close()
  })

  it("what Rust tells a page reaches it as a message of its own", () => {
    expect(pageMessage({ type: "service", name: "counted", data: { words: 3 } })).toEqual({
      t: "event",
      name: "counted",
      data: { words: 3 },
    })
    expect(pageMessage({ type: "serviceState", state: "ready", message: null })).toEqual({ t: "service", state: "ready" })
    expect(pageMessage({ type: "serviceState", state: "stopped", message: "Count stopped unexpectedly." })).toEqual({
      t: "service",
      state: "stopped",
      message: "Count stopped unexpectedly.",
    })
    expect(pageMessage({ type: "settings" })).toEqual({ t: "settings" })
    expect(pageMessage({ type: "closed", reason: "It was turned off." })).toEqual({ t: "closed", reason: "It was turned off." })
  })

  it("a document that connects after its service changed hears its last state first", async () => {
    const frame = frameWindow()
    const c = createConnector({ frame: () => frame, init: () => INIT, deps: deps(), limits: QUIET, channel })
    // Told before any document is there, and an event that only counts as it happens.
    c.send({ t: "service", state: "starting", message: "Getting Python" })
    c.send({ t: "service", state: "ready" })
    c.send({ t: "event", name: "counted", data: 1 })
    c.message({ source: frame, data: READY })
    const first = hearing(channels[0].port2)
    await vi.waitFor(() => expect(first).toEqual([{ t: "service", state: "ready" }]))
    // A document of a later load hears it too.
    c.load()
    c.message({ source: frame, data: READY })
    const later = hearing(channels[1].port2)
    await vi.waitFor(() => expect(later).toEqual([{ t: "service", state: "ready" }]))
    c.close()
  })

  it("a ready ahead of its load is taken, for the first document and a later one", () => {
    const frame = frameWindow()
    const connect = () => createConnector({ frame: () => frame, init: () => INIT, deps: deps(), limits: QUIET, channel })
    const ready = (c: ReturnType<typeof connect>) => c.message({ source: frame, data: READY })

    // Load before ready, then a document whose ready beats its load, then load before ready again.
    let c = connect()
    c.load()
    expect(ready(c)).toBe(true)
    expect(ready(c)).toBe(true)
    c.load()
    c.load()
    expect(ready(c)).toBe(true)
    c.close()

    // The first document's ready before its load, then load before ready.
    c = connect()
    expect(ready(c)).toBe(true)
    c.load()
    c.load()
    expect(ready(c)).toBe(true)
    // Its next document's ready ahead of its load.
    expect(ready(c)).toBe(true)
    c.load()
    c.close()

    // A frame that loads again and again without saying ready holds two at most.
    c = connect()
    for (let i = 0; i < 5; i++) c.load()
    expect([ready(c), ready(c), ready(c)]).toEqual([true, true, false])
    c.close()
  })

  it("a call goes out with the session, whatever the message says", async () => {
    const send = vi.fn(async (): Promise<PageReply> => ({ kind: "ok", result: { value: 1 } }))
    const page = hosted(deps({ call: pageCaller("session-mine", send) }))
    const reply = heard(page)
    page.postMessage({
      t: "call",
      id: 7,
      method: "state.get",
      params: { key: "a", session: "session-other" },
      session: "session-other",
      pluginId: "dev.evil.page",
    })
    expect(await reply).toEqual({ t: "reply", id: 7, ok: true, result: { value: 1 } })
    expect(send).toHaveBeenCalledWith("session-mine", "state.get", JSON.stringify({ key: "a", session: "session-other" }))

    // A refusal reaches the page as Rust said it.
    send.mockResolvedValueOnce({ kind: "err", error: { code: "denied", message: "project.get needs the project:read permission." } })
    const refused = heard(page)
    page.postMessage({ t: "call", id: 8, method: "project.get", params: {} })
    expect(await refused).toEqual({
      t: "reply",
      id: 8,
      ok: false,
      error: { code: "denied", message: "project.get needs the project:read permission." },
    })
  })

  it("keys are forwarded only while the frame has focus", async () => {
    let focused = false
    const key = vi.fn()
    const page = hosted(deps({ key, focused: () => focused }))
    const ctrlK = { t: "key", key: "k", code: "KeyK", ctrl: true, meta: false, shift: false, alt: false, repeat: false }
    const answered = () => {
      const reply = heard(page)
      page.postMessage({ t: "call", id: 1, method: "settings.get", params: {} })
      return reply
    }
    page.postMessage(ctrlK)
    // Messages arrive in order: once the call after it is answered, the key was seen.
    await answered()
    expect(key).not.toHaveBeenCalled()
    focused = true
    page.postMessage(ctrlK)
    await answered()
    expect(key).toHaveBeenCalledWith({ key: "k", code: "KeyK", ctrl: true, meta: false, shift: false, alt: false, repeat: false })

    // Pressed again in the app's window, it reaches AutoScene's shortcuts.
    const palette = vi.fn()
    renderHook(() => useShortcuts(GLOBAL_SHORTCUTS, { palette }))
    replayKey(key.mock.calls[0][0], window)
    expect(palette).toHaveBeenCalledTimes(1)
  })

  it("a call the command itself refuses reaches the page in English, whatever the UI speaks", async () => {
    await inLanguage("ro", { backend: { plugin: { hostFailed: "Pluginurile nu au pornit." } } })
    const message = "Plugins couldn't start. Details are in the log."
    const refused = { code: "internal", message, msg: { key: "plugin.hostFailed", args: {}, text: message } }
    const page = hosted(deps({ call: vi.fn(() => Promise.reject(refused)) }))
    const reply = heard(page)
    page.postMessage({ t: "call", id: 1, method: "project.get", params: {} })
    expect(await reply).toEqual({ t: "reply", id: 1, ok: false, error: { code: "failed", message } })
  })

  it("a toast effect shows Rust's title as it is", async () => {
    const title = "Plugin “AutoScene”: AutoScene: update now"
    const effect = vi.fn(showToast)
    const call = vi.fn(
      async (): Promise<PageReply> => ({
        kind: "effect",
        effect: { type: "toast", title, description: null, tone: "neutral" },
        result: { shown: true },
      }),
    )
    const page = hosted(deps({ call, effect }))
    const reply = heard(page)
    page.postMessage({ t: "call", id: 3, method: "ui.toast", params: { text: "AutoScene: update now" } })
    expect(await reply).toEqual({ t: "reply", id: 3, ok: true, result: { shown: true } })
    // Shown before the page hears the answer, with nothing added and no button.
    expect(toast).toHaveBeenCalledWith(title, { description: undefined })
    showToast({ type: "toast", title: "Plugin “Page”: Couldn't save", description: "Disk full", tone: "error" })
    expect(toast.error).toHaveBeenCalledWith("Plugin “Page”: Couldn't save", { description: "Disk full" })
  })

  it("a page's dialogs and right-clicks count only while it has the focus", async () => {
    let focused = false
    const call = vi.fn(async (): Promise<PageReply> => ({ kind: "ok", result: null }))
    const menu = vi.fn()
    const page = hosted(deps({ call, menu, focused: () => focused }))
    const asks = [
      ["ui.openExternal", { url: "https://example.org/" }],
      ["files.save", { name: "clips.txt", text: "S01E03" }],
      ["files.open", {}],
    ] as const
    for (const [i, [method, params]] of asks.entries()) {
      const reply = heard(page)
      page.postMessage({ t: "call", id: i, method, params })
      expect(await reply).toEqual({
        t: "reply",
        id: i,
        ok: false,
        error: { code: "denied", message: "A page opens a dialog only while it has the focus." },
      })
    }
    page.postMessage({ t: "menu", x: 12, y: 34 })
    // Messages arrive in order: once the call after it is answered, the right-click was seen.
    let reply = heard(page)
    page.postMessage({ t: "call", id: 9, method: "settings.get", params: {} })
    await reply
    expect(call).toHaveBeenCalledTimes(1)
    expect(menu).not.toHaveBeenCalled()

    focused = true
    for (const [i, [method, params]] of asks.entries()) {
      reply = heard(page)
      page.postMessage({ t: "call", id: i, method, params })
      expect(await reply).toEqual({ t: "reply", id: i, ok: true, result: null })
      expect(call).toHaveBeenLastCalledWith(method, JSON.stringify(params))
    }
    page.postMessage({ t: "menu", x: 12, y: 34 })
    reply = heard(page)
    page.postMessage({ t: "call", id: 9, method: "settings.get", params: {} })
    await reply
    expect(menu).toHaveBeenCalledWith(12, 34)
  })

  it("16 calls go to Rust at once, 64 wait, and the next is busy", async () => {
    expect(LIMITS).toEqual({ waiting: 64, inflight: 16, heldBytes: 64 * 1024 * 1024, drops: 1000 })
    const answers: (() => void)[] = []
    const call = vi.fn(() => new Promise<PageReply>((resolve) => answers.push(() => resolve({ kind: "ok", result: null }))))
    const page = hosted(deps({ call }))
    const inbox = hearing(page)
    for (let id = 0; id < 70; id++) page.postMessage({ t: "call", id, method: "settings.get", params: {} })
    await vi.waitFor(() => expect(inbox).toHaveLength(6))
    const busy = (id: number) => ({ t: "reply", id, ok: false, error: { code: "busy", message: "This page has 64 calls waiting already." } })
    expect(inbox).toEqual([64, 65, 66, 67, 68, 69].map(busy))
    expect(call).toHaveBeenCalledTimes(16)

    // One answered, the next that waited goes; and one more may wait.
    answers[0]()
    await vi.waitFor(() => expect(call).toHaveBeenCalledTimes(17))
    await vi.waitFor(() => expect(inbox).toHaveLength(7))
    expect(inbox[6]).toEqual({ t: "reply", id: 0, ok: true, result: null })
    page.postMessage({ t: "call", id: 70, method: "settings.get", params: {} })
    page.postMessage({ t: "call", id: 71, method: "settings.get", params: {} })
    await vi.waitFor(() => expect(inbox).toHaveLength(8))
    expect(inbox[7]).toEqual(busy(71))
    for (const answer of answers) answer()
  })

  it("params past their method's cap, and a method AutoScene doesn't have, never reach Rust", async () => {
    const call = vi.fn(async (): Promise<PageReply> => ({ kind: "ok", result: null }))
    const page = hosted(deps({ call, focused: () => false }))
    const inbox = hearing(page)
    const refused = (id: number, code: string, message: string) => ({ t: "reply", id, ok: false, error: { code, message } })
    // A toast's 2 KB, whatever the page's whole message may be.
    page.postMessage({ t: "call", id: 1, method: "ui.toast", params: { text: "x".repeat(3000) } })
    // Nor one JSON can't hold.
    page.postMessage({ t: "call", id: 2, method: "state.set", params: { key: "note", value: 10n } })
    // A name of an object's own, from a page that isn't focused either.
    page.postMessage({ t: "call", id: 3, method: "constructor", params: {} })
    page.postMessage({ t: "call", id: 4, method: "ui.nope", params: {} })
    await vi.waitFor(() => expect(inbox).toHaveLength(4))
    expect(inbox).toEqual([
      refused(1, "invalid", "The params of ui.toast are larger than 2 KB."),
      refused(2, "invalid", "The params of state.set aren't JSON."),
      refused(3, "invalid", "AutoScene has no such method."),
      refused(4, "invalid", "AutoScene has no such method."),
    ])
    expect(call).not.toHaveBeenCalled()
  })

  it("the params a page has waiting count together", async () => {
    const answers: (() => void)[] = []
    const call = vi.fn(() => new Promise<PageReply>((resolve) => answers.push(() => resolve({ kind: "ok", result: null }))))
    const page = hosted(deps({ call }), { heldBytes: 100 })
    const inbox = hearing(page)
    const note = { key: "note", value: "x".repeat(50) }
    page.postMessage({ t: "call", id: 1, method: "state.set", params: note })
    page.postMessage({ t: "call", id: 2, method: "state.set", params: note })
    await vi.waitFor(() => expect(inbox).toHaveLength(1))
    expect(inbox[0]).toEqual({ t: "reply", id: 2, ok: false, error: { code: "busy", message: "This page has too much waiting already." } })
    // Once the first is answered, there is room again.
    answers[0]()
    await vi.waitFor(() => expect(inbox).toHaveLength(2))
    page.postMessage({ t: "call", id: 3, method: "state.set", params: note })
    await vi.waitFor(() => expect(call).toHaveBeenCalledTimes(2))
    for (const answer of answers) answer()
  })

  it("a page that sends what the host can't read too often is cut off", async () => {
    const call = vi.fn(async (): Promise<PageReply> => ({ kind: "ok", result: null }))
    const overflow = vi.fn()
    const page = hosted(deps({ call, overflow }), { drops: 3 })
    const inbox = hearing(page)
    const gone = new Promise((resolve) => page.addEventListener("close", resolve, { once: true }))
    page.postMessage({ t: "nope" })
    page.postMessage("call")
    page.postMessage({ t: "call", id: "1", method: "settings.get" })
    await gone
    expect(overflow).toHaveBeenCalledTimes(1)
    // Its port is closed: nothing more is answered.
    page.postMessage({ t: "call", id: 4, method: "settings.get", params: {} })
    await after(20)
    expect(call).not.toHaveBeenCalled()
    expect(inbox).toEqual([])
  })

  it("a page quiet for 10 s shows that it isn't responding, until it answers", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval", "Date"] })
    const unresponsive = vi.fn()
    const ch = channel()
    createHost(ch.port1, deps({ unresponsive }))
    const page = channels.at(-1)!.port2
    const inbox = hearing(page)
    vi.advanceTimersByTime(9_999)
    expect(unresponsive).not.toHaveBeenCalled()
    vi.advanceTimersByTime(1)
    expect(unresponsive).toHaveBeenCalledExactlyOnceWith(true)
    // It was pinged every 2 s all along.
    await vi.waitFor(() => expect(inbox).toHaveLength(5))
    expect(inbox.at(-1)).toEqual({ t: "ping", n: 5 })
    vi.advanceTimersByTime(4_000)
    expect(unresponsive).toHaveBeenCalledTimes(1)

    page.postMessage({ t: "pong", n: 5 })
    await vi.waitFor(() => expect(unresponsive).toHaveBeenLastCalledWith(false))
    vi.advanceTimersByTime(9_000)
    expect(unresponsive).toHaveBeenCalledTimes(2)
  })

  it("copyText from an unfocused frame, or without the person's click in it, is denied", async () => {
    let focused = false
    let elsewhere = true
    let activated = false
    const call = vi.fn(async (): Promise<PageReply> => ({ kind: "ok", result: null }))
    const page = hosted(deps({ call, focused: () => focused, elsewhere: () => elsewhere, activated: () => activated }))
    const copy = { t: "call", id: 4, method: "ui.copyText", params: { text: "S01E03 00:01:02:03" } }
    let reply = heard(page)
    page.postMessage(copy)
    expect(await reply).toEqual({
      t: "reply",
      id: 4,
      ok: false,
      error: { code: "denied", message: "A page copies only while it has the focus." },
    })
    expect(call).not.toHaveBeenCalled()
    // The page took the focus itself as it opened, right after the person's click that opened it.
    focused = true
    activated = true
    reply = heard(page)
    page.postMessage(copy)
    expect(await reply).toEqual({
      t: "reply",
      id: 4,
      ok: false,
      error: { code: "denied", message: "A page copies only 5 seconds after the person clicks or types elsewhere in AutoScene." },
    })
    expect(call).not.toHaveBeenCalled()
    // Then nobody clicked.
    elsewhere = false
    activated = false
    reply = heard(page)
    page.postMessage(copy)
    expect(await reply).toEqual({
      t: "reply",
      id: 4,
      ok: false,
      error: { code: "denied", message: "A page copies only right after the person clicks or types in it." },
    })
    expect(call).not.toHaveBeenCalled()
    activated = true
    reply = heard(page)
    page.postMessage(copy)
    expect(await reply).toEqual({ t: "reply", id: 4, ok: true, result: null })
    expect(call).toHaveBeenCalledWith("ui.copyText", JSON.stringify({ text: "S01E03 00:01:02:03" }))
  })

  it("a click or a key in AutoScene counts as one elsewhere for 5 s, never one the host pressed again", () => {
    let now = 1_000
    const heard = new Map<string, (e: Pick<Event, "isTrusted">) => void>()
    const w = {
      addEventListener: (type: string, fn: (e: Pick<Event, "isTrusted">) => void) => void heard.set(type, fn),
      performance: { now: () => now },
    } as unknown as Window
    const elsewhere = inputElsewhere(w)
    expect(elsewhere()).toBe(false)
    expect([...heard.keys()]).toEqual(["keydown", "mousedown", "pointerdown", "pointerup", "touchend"])
    for (const type of heard.keys()) {
      heard.get(type)!({ isTrusted: true })
      now += 5_000
      expect(elsewhere(), type).toBe(true)
      now += 300
      expect(elsewhere(), type).toBe(false)
    }
    heard.get("keydown")!({ isTrusted: false })
    expect(elsewhere()).toBe(false)
    // What the window hears that nobody pressed, like a page's key pressed again.
    const inWindow = inputElsewhere(window)
    replayKey({ key: "k", code: "KeyK", ctrl: true, meta: false, shift: false, alt: false, repeat: false }, window)
    document.body.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }))
    expect(inWindow()).toBe(false)
  })
})
