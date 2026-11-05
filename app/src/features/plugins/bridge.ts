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
 * session the host holds, never one the page names, each method's params
 * only up to its cap, and its keys, its right-clicks, what it copies and the
 * dialogs it opens count only while its frame has the focus in a window in
 * front (and what it copies and the dialogs it opens only right after the
 * person clicked or typed, never just after they did elsewhere in
 * AutoScene). What Rust tells an open page (`plugin-page:event`)
 * reaches it as a message of its own (`pageMessage`); its service's last
 * state reaches a document that connects later too.
 */
import { toast } from "sonner"
import type { PageEffect, PageEvent, PageReply } from "@/bindings"
import { say } from "@/lib/i18n"
import { commands, englishMessage } from "@/lib/ipc"
import {
  BRIDGE_VERSION,
  LIMITS,
  type ForwardedKey,
  type FrameToHost,
  type HostToFrame,
  type Init,
  type PageInit,
  type PageLocale,
} from "@/plugin-kit/bridge-types"
import frame from "@/plugin-kit/frame.json"
import { type CallCode, type HostMethod, MAX_PARAMS } from "@/plugin-kit/protocol"

/** The frame's attributes: scripts and forms, never its own origin (Rust checks the file, `pages.rs`). */
export const FRAME = frame as { sandbox: "allow-scripts allow-forms"; allow: ""; referrerPolicy: "no-referrer" }

/** The host's end of a page's channel: a `MessagePort` (Node's in tests). */
export type PortLike = Pick<MessagePort, "postMessage" | "close" | "start" | "addEventListener">

/** What the host does for a page: its calls, and what the page hands over. */
export type HostDeps = {
  /** A call, to Rust, with its params as their JSON. */
  call(method: HostMethod, params: string): Promise<PageReply>
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
  /** Whether the frame has the focus now, in a window in front. */
  focused(): boolean
  /** Whether the person clicked or typed in AutoScene's own page in the last 5 s, outside the frame. */
  elsewhere(): boolean
  /** Whether the person just clicked or typed in AutoScene's window (in the frame too). */
  activated(): boolean
}

/** What a host holds to: the page's own limits, and how often it is pinged and how long it may stay quiet. */
export type Limits = { [K in keyof typeof LIMITS]: number } & { pingMs: number; quietMs: number }

const HOST_LIMITS: Limits = { ...LIMITS, pingMs: 2000, quietMs: 10_000 }

/** A page's port, as the host holds it. */
export type Host = { send(m: HostToFrame): void; close(): void }

/** A call as the host holds it: with its params as their JSON. */
type Call = { id: number; method: HostMethod; params: string }

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
 * The calls a page makes only while the person is in it, right after they
 * clicked or typed, and what the page does with them: what it copies, they
 * meant to; a dialog of the system's opens over AutoScene, where they may
 * be working. A page can take the focus itself; it can't click for them.
 */
// The words are part of the English `denied` messages the page's code gets (see `call`), not the person's.
const FOCUSED: Partial<Record<HostMethod, string>> = {
  "ui.copyText": "copies",
  "ui.openExternal": "opens a dialog",
  "files.save": "opens a dialog",
  "files.open": "opens a dialog",
}

/** Whether `method` is one AutoScene has (never a name of an object's own, like `constructor`). */
const isMethod = (method: string): method is HostMethod => Object.hasOwn(MAX_PARAMS, method)

/** The JSON of `params`, none for a value JSON can't hold. */
function json(params: unknown): string | null {
  try {
    return JSON.stringify(params ?? null) ?? null
  } catch {
    return null
  }
}

/** `n` bytes as Rust says them: 2 KB, 48 MB. */
const sizeWords = (n: number) => (n >= 1 << 20 ? `${n / (1 << 20)} MB` : `${n / (1 << 10)} KB`)

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
  // The JSON of the calls it has waiting and in flight.
  let held = 0
  const queue: Call[] = []
  const send = (m: HostToFrame) => {
    if (!closed) port.postMessage(m)
  }
  const fail = (id: number, code: CallCode, message: string) => send({ t: "reply", id, ok: false, error: { code, message } })

  const run = (c: Call) => {
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
        (e: unknown) => fail(c.id, "failed", englishMessage(e)),
      )
      .finally(() => {
        inflight--
        waiting--
        held -= c.params.length
        next()
      })
  }
  const next = () => {
    while (!closed && inflight < l.inflight && queue.length > 0) run(queue.shift()!)
  }
  // What `fail` says goes to the page's code, in English, as the protocol's own text for plugin authors (PLUGINS.md):
  // the person never reads it, so none of it is in the catalog.
  const call = (c: Extract<FrameToHost, { t: "call" }>) => {
    if (!isMethod(c.method)) return fail(c.id, "invalid", "AutoScene has no such method.")
    if (waiting >= l.waiting) return fail(c.id, "busy", `This page has ${l.waiting} calls waiting already.`)
    const params = json(c.params)
    if (params == null) return fail(c.id, "invalid", `The params of ${c.method} aren't JSON.`)
    const most = MAX_PARAMS[c.method]
    if (params.length > most) return fail(c.id, "invalid", `The params of ${c.method} are larger than ${sizeWords(most)}.`)
    const does = FOCUSED[c.method]
    if (does && !deps.focused()) return fail(c.id, "denied", `A page ${does} only while it has the focus.`)
    if (does && deps.elsewhere())
      return fail(c.id, "denied", `A page ${does} only 5 seconds after the person clicks or types elsewhere in AutoScene.`)
    if (does && !deps.activated()) return fail(c.id, "denied", `A page ${does} only right after the person clicks or types in it.`)
    if (held + params.length > l.heldBytes) return fail(c.id, "busy", "This page has too much waiting already.")
    waiting++
    held += params.length
    queue.push({ id: c.id, method: c.method, params })
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
  // Its service's last state, which a document that connects later hears first.
  let service: Extract<HostToFrame, { t: "service" }> | null = null
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
      if (service) host.send(service)
      return true
    },
    send(m) {
      if (m.t === "service") service = m
      host?.send(m)
    },
    close() {
      host?.close()
      host = null
      readys = 0
    },
  }
}

/**
 * The language a page is told for the UI's: Romanian, else English. The
 * development pseudo-locale (`en-XA`) is English text, so it tells English.
 */
export function pageLocale(language: string | undefined): PageLocale {
  return language === "ro" || language?.startsWith("ro-") ? "ro" : "en"
}

/** What Rust told an open page, as the page hears it on its port. */
export function pageMessage(e: PageEvent): HostToFrame {
  switch (e.type) {
    case "service":
      return { t: "event", name: e.name, data: e.data }
    case "serviceState":
      return e.message == null ? { t: "service", state: e.state } : { t: "service", state: e.state, message: say(e.message) }
    case "settings":
      return { t: "settings" }
    case "closed":
      return { t: "closed", reason: e.reason.text }
  }
}

/** The calls of the page `session`: Rust knows the plugin by the session alone, whatever the page's message says. */
export function pageCaller(session: string, send: (session: string, method: string, params: string) => Promise<PageReply> = commands.pluginPageCall) {
  return (method: HostMethod, params: string) => send(session, method, params)
}

/** A toast a page's call asked for, with Rust's title: it starts with its fixed word, then the plugin's name. No action button. */
export function showToast(e: Extract<PageEffect, { type: "toast" }>) {
  const options = { description: e.description ?? undefined }
  if (e.tone === "error") toast.error(say(e.title), options)
  else toast(say(e.title), options)
}

/**
 * How long Chromium holds a click or a key as the person's activation: 5 s.
 * A little longer here, so its clock and this one never meet.
 */
const ACTIVE_MS = 5_250

/** The person's clicks and keys, as Chromium counts them. */
const INPUT = ["keydown", "mousedown", "pointerdown", "pointerup", "touchend"] as const

/**
 * Whether the person clicked or typed in `w`'s own page in the last 5 s, as
 * it hears them from now on: never in a frame, whose clicks and keys don't
 * reach it, nor the keys the host presses again for a page (`replayKey`).
 * The page's activation (`navigator.userActivation`) comes from both, so
 * while one of these still counts there, it is no sign of a click in the frame.
 */
export function inputElsewhere(w: Pick<Window, "addEventListener" | "performance">): () => boolean {
  let at = Number.NEGATIVE_INFINITY
  const heard = (e: Event) => {
    if (e.isTrusted) at = w.performance.now()
  }
  for (const type of INPUT) w.addEventListener(type, heard, { capture: true, passive: true })
  return () => w.performance.now() - at < ACTIVE_MS
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
