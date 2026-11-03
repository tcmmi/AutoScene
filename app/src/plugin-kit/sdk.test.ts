import { afterEach, describe, expect, it, vi } from "vitest"
import type { FrameToHost, HostToFrame, PageInit } from "./bridge-types"
import { connect } from "./sdk"

// jsdom has no ports and no `event.source` on window messages, so the frame
// is a stand-in: the test's window, whose parent is a fake and whose
// `message` listeners the test calls itself, with the ports of Node's
// MessageChannel.

const INIT: PageInit = {
  plugin: { id: "dev.example.page", name: "Example", version: "1.0.0" },
  page: { id: "main", title: "Example", scope: "global" },
  projectId: null,
  path: "",
  theme: { v: 1, scheme: "dark" },
  permissions: [],
  hosts: [],
  service: false,
  protocol: 2,
  app: { version: "1.1.0", platform: "windows" },
}

let cleanups: (() => void)[] = []

afterEach(() => {
  for (const c of cleanups) c()
  cleanups = []
  document.body.innerHTML = ""
})

/** A window framed by a parent that isn't there. */
function framed() {
  const parent = { postMessage: vi.fn() }
  const messages = new EventTarget()
  const added: [string, EventListenerOrEventListenerObject, boolean | AddEventListenerOptions | undefined][] = []
  const win = new Proxy(window, {
    get(target, key) {
      if (key === "parent") return parent
      if (key === "addEventListener") {
        return (type: string, cb: EventListenerOrEventListenerObject, o?: boolean | AddEventListenerOptions) => {
          if (type === "message") return messages.addEventListener(type, cb, o)
          added.push([type, cb, o])
          target.addEventListener(type, cb, o)
        }
      }
      if (key === "removeEventListener") {
        return (type: string, cb: EventListenerOrEventListenerObject, o?: boolean | EventListenerOptions) =>
          (type === "message" ? messages : target).removeEventListener(type, cb, o)
      }
      const value: unknown = Reflect.get(target, key)
      return typeof value === "function" ? value.bind(target) : value
    },
  })
  cleanups.push(() => added.forEach(([type, cb, o]) => window.removeEventListener(type, cb, o)))
  /** A window message, as the browser would hand it to the frame. */
  const post = (data: unknown, ports: MessagePort[], source: unknown = parent) => {
    messages.dispatchEvent(Object.assign(new Event("message"), { data, ports, source }))
  }
  return { win, parent, post }
}

/** A connected page, and the host's end of its port. */
async function connected() {
  const frame = framed()
  const page = connect({ win: frame.win, timeoutMs: 1000 })
  const { port1: host, port2 } = new MessageChannel()
  frame.post({ type: "autoscene:init", v: 1, init: INIT }, [port2])
  const heard: FrameToHost[] = []
  const answer = vi.fn<(m: Extract<FrameToHost, { t: "call" }>) => HostToFrame | null>(() => null)
  host.addEventListener("message", (e: MessageEvent<FrameToHost>) => {
    heard.push(e.data)
    if (e.data.t !== "call") return
    const reply = answer(e.data)
    if (reply) host.postMessage(reply)
  })
  host.start()
  cleanups.push(() => host.close())
  return { page: await page, host, heard, answer }
}

function press(target: EventTarget, init: KeyboardEventInit): KeyboardEvent {
  const e = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init })
  target.dispatchEvent(e)
  return e
}

function rightClick(target: EventTarget): MouseEvent {
  const e = new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 12, clientY: 34 })
  target.dispatchEvent(e)
  return e
}

describe("the SDK", () => {
  it("takes init only from its parent and with one port", async () => {
    const frame = framed()
    let settled = false
    const pending = connect({ win: frame.win, timeoutMs: 1000 }).finally(() => (settled = true))
    expect(frame.parent.postMessage).toHaveBeenCalledWith({ type: "autoscene:ready", v: 1 }, "*")

    const init = { type: "autoscene:init", v: 1, init: INIT }
    const planted = new MessageChannel()
    const [a, b] = [new MessageChannel(), new MessageChannel()]
    frame.post(init, [planted.port2], { postMessage() {} }) // a sibling frame
    frame.post(init, [a.port2, b.port2]) // two ports
    frame.post(init, []) // none
    frame.post({ ...init, type: "autoscene:ready" }, [a.port2]) // not init
    await new Promise((r) => setTimeout(r, 0))
    expect(settled).toBe(false)

    const real = new MessageChannel()
    frame.post(init, [real.port2])
    const page = await pending
    expect(page.plugin.id).toBe("dev.example.page")
    expect(document.documentElement.dataset.scheme).toBe("dark")

    // Its calls go to the port init brought, and to no other.
    const calls = (port: MessagePort) => {
      const seen: unknown[] = []
      port.addEventListener("message", (e: MessageEvent) => seen.push(e.data))
      port.start()
      return seen
    }
    const [toPlanted, toReal] = [calls(planted.port1), calls(real.port1)]
    void page.state.keys()
    await vi.waitFor(() => expect(toReal).toEqual([{ t: "call", id: 1, method: "state.keys", params: {} }]))
    expect(toPlanted).toEqual([])
    for (const c of [planted, a, b, real]) c.port1.close()
  })

  it("a call resolves with its reply and rejects with its error", async () => {
    const { page, answer } = await connected()
    answer.mockImplementation((m) =>
      m.method === "state.get"
        ? { t: "reply", id: m.id, ok: true, result: { value: 42 } }
        : { t: "reply", id: m.id, ok: false, error: { code: "denied", message: "project.get needs the project:read permission." } },
    )
    await expect(page.state.get("answer")).resolves.toBe(42)
    await expect(page.project.get()).rejects.toMatchObject({
      name: "CallFailure",
      code: "denied",
      message: "project.get needs the project:read permission.",
    })
    expect(answer.mock.calls.map(([m]) => [m.method, m.params])).toEqual([
      ["state.get", { key: "answer" }],
      ["project.get", {}],
    ])
  })

  it("a call past 64 waiting is refused at once", async () => {
    const { page, host, heard } = await connected()
    const first = page.state.keys()
    for (let i = 1; i < 64; i++) void page.state.keys()
    await expect(page.state.keys()).rejects.toMatchObject({ code: "busy" })
    await vi.waitFor(() => expect(heard).toHaveLength(64))

    // One answered makes room for one more.
    host.postMessage({ t: "reply", id: 1, ok: true, result: [] } satisfies HostToFrame)
    await first
    void page.state.keys()
    await vi.waitFor(() => expect(heard).toHaveLength(65))
  })

  it("the guards cancel F5 and Ctrl+F but leave Ctrl+C in a field", async () => {
    await connected()
    expect(press(document.body, { key: "F5" }).defaultPrevented).toBe(true)
    expect(press(document.body, { key: "f", ctrlKey: true }).defaultPrevented).toBe(true)
    const input = document.createElement("input")
    document.body.append(input)
    expect(press(input, { key: "c", ctrlKey: true }).defaultPrevented).toBe(false)
    // The browser's menu: not outside a field, still in one.
    expect(rightClick(document.body).defaultPrevented).toBe(true)
    expect(rightClick(input).defaultPrevented).toBe(false)
  })

  it("keys the page took are not forwarded", async () => {
    const { page, heard, answer } = await connected()
    answer.mockImplementation((m) => ({ t: "reply", id: m.id, ok: true, result: [] }))
    const own = (e: KeyboardEvent) => {
      if (e.key === "s" && e.ctrlKey) e.preventDefault()
    }
    window.addEventListener("keydown", own)
    cleanups.push(() => window.removeEventListener("keydown", own))
    const input = document.createElement("input")
    document.body.append(input)

    press(document.body, { key: "s", ctrlKey: true }) // the page's own
    press(document.body, { key: "k", ctrlKey: true, code: "KeyK" }) // AutoScene's palette
    press(document.body, { key: "a" }) // typing
    press(input, { key: "@", ctrlKey: true, altKey: true }) // AltGr types in a field
    press(document.body, { key: "Escape", code: "Escape" })
    rightClick(document.body)
    rightClick(input)
    // The port keeps its order: once this call is there, so is all before it.
    await page.state.keys()

    expect(heard.filter((m) => m.t !== "call")).toEqual([
      { t: "key", key: "k", code: "KeyK", ctrl: true, meta: false, shift: false, alt: false, repeat: false },
      { t: "key", key: "Escape", code: "Escape", ctrl: false, meta: false, shift: false, alt: false, repeat: false },
      { t: "menu", x: 12, y: 34 },
    ])
  })

  it("a page that listens for its service late hears its state as it is", async () => {
    const { page, host } = await connected()
    const said: unknown[] = []
    const events: unknown[] = []
    page.service.on("counted", (data) => events.push(data))
    host.postMessage({ t: "service", state: "starting", message: "Getting Python" } satisfies HostToFrame)
    host.postMessage({ t: "service", state: "ready" } satisfies HostToFrame)
    host.postMessage({ t: "event", name: "counted", data: { words: 3 } } satisfies HostToFrame)
    await vi.waitFor(() => expect(events).toEqual([{ words: 3 }]))
    // The last state at once, then each change.
    const stop = page.service.onState((s) => said.push(s))
    expect(said).toEqual([{ state: "ready", message: undefined }])
    host.postMessage({ t: "service", state: "stopped", message: "Example stopped." } satisfies HostToFrame)
    await vi.waitFor(() => expect(said).toHaveLength(2))
    expect(said[1]).toEqual({ state: "stopped", message: "Example stopped." })
    stop()
  })

  it("the mock answers like the host when there is no parent", async () => {
    await expect(connect()).rejects.toThrow("Open this page in AutoScene, or pass { mock } to connect().")
    const info = vi.spyOn(console, "info").mockImplementation(() => {})
    cleanups.push(() => info.mockRestore())

    const page = await connect({ mock: { settings: { channel: "clips" }, secrets: { token: null } } })
    expect(page.plugin.name).toBe("Mock")
    await expect(page.settings.get()).resolves.toEqual({ channel: "clips" })
    await expect(page.project.get()).rejects.toMatchObject({
      code: "denied",
      message: "project.get needs the project:read permission.",
    })
    await expect(page.secrets.get("token")).resolves.toBeNull()
    await expect(page.secrets.get("webhook")).rejects.toMatchObject({ code: "denied" })
    await expect(page.state.get("no way")).rejects.toMatchObject({ code: "invalid" })
    await page.state.set("seen", [1, 2])
    await expect(page.state.get("seen")).resolves.toEqual([1, 2])
    await expect(page.state.keys()).resolves.toEqual(["seen"])
    await page.state.delete("seen")
    await expect(page.state.get("seen")).resolves.toBeNull()
    await expect(page.net.fetch({ url: "https://discord.com/" })).rejects.toMatchObject({ code: "denied" })
    await page.ui.toast("Saved")
    expect(info).toHaveBeenCalledWith("Plugin “Mock”: Saved")
  })
})
