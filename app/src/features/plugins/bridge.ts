/**
 * The host's side of the bridge to a plugin page (the page's is the SDK,
 * `plugin-kit/sdk.ts`; the messages are in `plugin-kit/bridge-types.ts`).
 * Plain functions over a port, so tests run them over Node's
 * `MessageChannel`: jsdom has no ports and no `event.source`.
 *
 * The frame says `ready` once its document has loaded. Only a `ready` from
 * the frame's own window counts, and one per load of the frame
 * (`createConnector`): the host answers it with `init` and one port of a
 * new channel, and drops the port of the load before. After that the page
 * speaks only over its port (`createHost`): its calls go to Rust with the
 * session the host holds, never one the page names, and its keys, its
 * right-clicks, what it copies and the dialogs it opens count only while its
 * frame has the focus.
 */
import { toast } from "sonner"
import type { PageEffect, PageReply } from "@/bindings"
import { commands, errorMessage } from "@/lib/ipc"
import {
  BRIDGE_VERSION,
  LIMITS,
  type ForwardedKey,
  type FrameToHost,
  type HostToFrame,
  type Init,
  type PageInit,
} from "@/plugin-kit/bridge-types"
import frame from "@/plugin-kit/frame.json"
import type { CallCode, HostMethod } from "@/plugin-kit/protocol"

/** The frame's attributes: scripts and forms, never its own origin (Rust checks the file, `pages.rs`). */
export const FRAME = frame as { sandbox: "allow-scripts allow-forms"; allow: ""; referrerPolicy: "no-referrer" }

/** The host's end of a page's channel: a `MessagePort` (Node's in tests). */
export type PortLike = Pick<MessagePort, "postMessage" | "close" | "start" | "addEventListener">

/** What the host does for a page: its calls, and what the page hands over. */
export type HostDeps = {
  /** A call, to Rust. */
  call(method: HostMethod, params: unknown): Promise<PageReply>
  /** What a call's answer asks the app's page to do first. */
  effect(e: PageEffect): void
  /** A key the page didn't take, for AutoScene's shortcuts. */
  key(k: ForwardedKey): void
  /** A right-click outside a field, in the frame's pixels. */
  menu(x: number, y: number): void
  /** The page stopped answering pings, or answers again. */
  unresponsive(on: boolean): void
  /** The page sent too much the host couldn't read: its port is closed. */
  overflow(): void
  /** Whether the frame has the focus now. */
  focused(): boolean
}

/** What a host holds to: the page's own limits, and how often it is pinged and how long it may stay quiet. */
export type Limits = { [K in keyof typeof LIMITS]: number } & { pingMs: number; quietMs: number }

const HOST_LIMITS: Limits = { ...LIMITS, pingMs: 2000, quietMs: 10_000 }

/** A page's port, as the host holds it. */
export type Host = { send(m: HostToFrame): void; close(): void }

/** Whether `e` is the `ready` of the frame whose window is `frame`, and of no other window. */
export function isReady(e: { source: unknown; data: unknown }, frame: Window | null | undefined): boolean {
  const d = e.data as { type?: unknown; v?: unknown } | null
  return frame != null && e.source === frame && typeof d === "object" && d !== null && d.type === "autoscene:ready" && d.v === BRIDGE_VERSION
}

const isBool = (v: unknown): v is boolean => typeof v === "boolean"
const isName = (v: unknown): v is string => typeof v === "string" && v.length <= 64

/** A page's message as the host reads it: one of the shapes it knows, else none. */
export function parseFrameMessage(data: unknown): FrameToHost | null {
  if (typeof data !== "object" || data === null) return null
  const m = data as Record<string, unknown>
  switch (m.t) {
    case "call":
      return Number.isSafeInteger(m.id) && isName(m.method)
        ? { t: "call", id: m.id as number, method: m.method as HostMethod, params: m.params }
        : null
    case "key":
      return isName(m.key) && isName(m.code) && [m.ctrl, m.meta, m.shift, m.alt, m.repeat].every(isBool)
        ? {
            t: "key",
            key: m.key,
            code: m.code,
            ctrl: m.ctrl as boolean,
            meta: m.meta as boolean,
            shift: m.shift as boolean,
            alt: m.alt as boolean,
            repeat: m.repeat as boolean,
          }
        : null
    case "menu":
      return Number.isFinite(m.x) && Number.isFinite(m.y) ? { t: "menu", x: m.x as number, y: m.y as number } : null
    case "pong":
      return Number.isSafeInteger(m.n) ? { t: "pong", n: m.n as number } : null
    default:
      return null
  }
}

/**
 * The calls a page makes only while the person is in it, and why one from a
 * page they aren't in is denied: what it copies, they meant to; a dialog of
 * the system's opens over AutoScene, where they may be working.
 */
const FOCUSED: Partial<Record<HostMethod, string>> = {
  "ui.copyText": "A page copies only while it has the focus.",
  "ui.openExternal": "A page opens a dialog only while it has the focus.",
  "files.save": "A page opens a dialog only while it has the focus.",
  "files.open": "A page opens a dialog only while it has the focus.",
}

/** Whether `params` fit in a message of `most` characters of JSON (a value JSON can't hold doesn't). */
function fits(params: unknown, most: number): boolean {
  try {
    return (JSON.stringify(params ?? null)?.length ?? 0) <= most
  } catch {
    return false
  }
}

/**
 * Speak with a page over `port`: answer its calls through `deps.call`, at
 * most `inflight` at once and `waiting` in all, ping it, and hand on what
 * it hands over. Messages it can't read are dropped; past `drops` of them
 * the port closes.
 */
export function createHost(port: PortLike, deps: HostDeps, limits: Partial<Limits> = {}): Host {
  const l = { ...HOST_LIMITS, ...limits }
  let closed = false
  let drops = 0
  let waiting = 0
  let inflight = 0
  const queue: Extract<FrameToHost, { t: "call" }>[] = []
  const send = (m: HostToFrame) => {
    if (!closed) port.postMessage(m)
  }
  const fail = (id: number, code: CallCode, message: string) => send({ t: "reply", id, ok: false, error: { code, message } })

  const run = (c: Extract<FrameToHost, { t: "call" }>) => {
    inflight++
    deps
      .call(c.method, c.params)
      .then(
        (r) => {
          if (r.kind === "err") return fail(c.id, r.error.code, r.error.message)
          if (r.kind === "effect" && !closed) deps.effect(r.effect)
          send({ t: "reply", id: c.id, ok: true, result: r.result })
        },
        // The command itself failed (plugins couldn't start): the page hears it as AutoScene's.
        (e: unknown) => fail(c.id, "failed", errorMessage(e)),
      )
      .finally(() => {
        inflight--
        waiting--
        next()
      })
  }
  const next = () => {
    while (!closed && inflight < l.inflight && queue.length > 0) run(queue.shift()!)
  }
  const call = (c: Extract<FrameToHost, { t: "call" }>) => {
    if (waiting >= l.waiting) return fail(c.id, "busy", `This page has ${l.waiting} calls waiting already.`)
    if (!fits(c.params, l.messageBytes)) return fail(c.id, "invalid", `The params of ${c.method} are larger than AutoScene takes.`)
    const unfocused = FOCUSED[c.method]
    if (unfocused && !deps.focused()) return fail(c.id, "denied", unfocused)
    waiting++
    queue.push(c)
    next()
  }

  // A page that stops answering pings shows that it isn't responding.
  let pinged = 0
  let heard = Date.now()
  let quiet = false
  const pings = setInterval(() => {
    if (!quiet && Date.now() - heard >= l.quietMs) {
      quiet = true
      deps.unresponsive(true)
    }
    send({ t: "ping", n: ++pinged })
  }, l.pingMs)

  const close = () => {
    if (closed) return
    closed = true
    clearInterval(pings)
    queue.length = 0
    port.close()
  }
  port.addEventListener("message", (e: MessageEvent) => {
    if (closed) return
    const m = parseFrameMessage(e.data)
    if (!m) {
      if (++drops >= l.drops) {
        close()
        deps.overflow()
      }
      return
    }
    switch (m.t) {
      case "call":
        return call(m)
      case "key":
        // AutoScene answers a page's keys only while the person is in it.
        if (deps.focused()) deps.key({ key: m.key, code: m.code, ctrl: m.ctrl, meta: m.meta, shift: m.shift, alt: m.alt, repeat: m.repeat })
        return
      case "menu":
        // AutoScene's menu opens over a page the person is in, never over what they do elsewhere.
        if (deps.focused()) deps.menu(m.x, m.y)
        return
      case "pong":
        heard = Date.now()
        if (quiet) {
          quiet = false
          deps.unresponsive(false)
        }
        return
    }
  })
  port.start()
  return { send, close }
}

/**
 * The handshake with the frame whose window `frame` gives: a `ready` of
 * that window, one per load of the frame, gets `init` and the port of a new
 * channel (`createHost`); the port of the load before goes. A document's
 * `ready` and its frame's load come from another process on WebView2, in
 * either order: so one `ready` may come ahead of its load, the first
 * document's or a later one's. A document that says `ready` twice uses
 * that one up: it gets a new port, and its next document's `ready` counts
 * only after that document's load.
 */
export function createConnector(o: {
  frame(): Window | null | undefined
  init(): PageInit
  deps: HostDeps
  limits?: Partial<Limits>
  /** A new channel (tests pass Node's). */
  channel?: () => { port1: PortLike; port2: Transferable }
}): { load(): void; message(e: { source: unknown; data: unknown }): boolean; send(m: HostToFrame): void; close(): void } {
  let host: Host | null = null
  // The readys it takes: one per load, and one ahead of its load. Never more
  // than two waiting, however often a frame without the SDK loads.
  let readys = 1
  return {
    load() {
      readys = Math.min(readys + 1, 2)
    },
    message(e) {
      const frame = o.frame()
      if (readys === 0 || !isReady(e, frame)) return false
      readys--
      host?.close()
      const { port1, port2 } = o.channel?.() ?? new MessageChannel()
      host = createHost(port1, o.deps, o.limits)
      // An opaque origin can't be named: the window itself is what counts.
      const init: Init = { type: "autoscene:init", v: BRIDGE_VERSION, init: o.init() }
      frame!.postMessage(init, "*", [port2])
      return true
    },
    send(m) {
      host?.send(m)
    },
    close() {
      host?.close()
      host = null
      readys = 0
    },
  }
}

/** The calls of the page `session`: Rust knows the plugin by the session alone, whatever the page's message says. */
export function pageCaller(session: string, send: (session: string, method: string, params: unknown) => Promise<PageReply> = commands.pluginPageCall) {
  return (method: HostMethod, params: unknown) => send(session, method, params)
}

/** A toast a page's call asked for, with Rust's title as it is: it starts with `Plugin “<name>”:`. No action button. */
export function showToast(e: Extract<PageEffect, { type: "toast" }>) {
  const options = { description: e.description ?? undefined }
  if (e.tone === "error") toast.error(e.title, options)
  else toast(e.title, options)
}

/** A key a page handed over, pressed again on `target` for AutoScene's shortcuts. */
export function replayKey(k: ForwardedKey, target: EventTarget) {
  target.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: k.key,
      code: k.code,
      ctrlKey: k.ctrl,
      metaKey: k.meta,
      shiftKey: k.shift,
      altKey: k.alt,
      repeat: k.repeat,
      bubbles: true,
      cancelable: true,
    }),
  )
}

/** A right-click a page handed over, at its place over `frame`: the window's menu opens there. */
export function replayMenu(frame: HTMLIFrameElement, x: number, y: number) {
  const box = frame.getBoundingClientRect()
  frame.dispatchEvent(
    new MouseEvent("contextmenu", { bubbles: true, cancelable: true, button: 2, clientX: box.left + x, clientY: box.top + y }),
  )
}
