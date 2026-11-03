/**
 * The SDK of plugin pages: `sdk.js` (built from `entry.ts`), which AutoScene
 * serves at `/_as/sdk.js` beside `theme.css`. A page loads it as a classic
 * script and asks `AutoScene.connect()` for its `Page`: who it is, where it
 * is open, and the calls into AutoScene its plugin may make.
 *
 * In AutoScene the page sits in a sandboxed frame. `connect()` says `ready`
 * to its parent, takes the one port the host's `init` brings, and from then
 * on speaks only over it (`bridge-types.ts`). It also guards the frame as
 * the app guards its window (`keys.ts`), and hands the keys and right-clicks
 * the page didn't take to AutoScene, so its shortcuts and menus work over a
 * page too.
 *
 * Opened outside AutoScene (a plain browser, while it is being made), a page
 * gets a `Page` over an in-memory host instead, when it passes one
 * (`mock.ts`).
 *
 * Kept apart from the app: nothing here imports React, Radix, Tauri or the
 * app's modules (`.oxlintrc.json`), since the build is a page's script.
 */
import {
  BRIDGE_VERSION,
  LIMITS,
  callFailure,
  type CallFailure,
  type ForwardedKey,
  type FrameToHost,
  type HostToFrame,
  type Init,
  type PageInit,
  type Ready,
  type ServiceState,
} from "./bridge-types"
import { installGuards, isEditable } from "./keys"
import { createMock, type MockHost } from "./mock"
import type {
  ActionsRun,
  CopyText,
  ExportRef,
  FileRef,
  HostMethod,
  Navigate,
  NetRequest,
  NetResponse,
  OpenExternal,
  OpenFile,
  OpenedFile,
  PageExport,
  PageExportSummary,
  PageProject,
  Permission,
  ProjectRef,
  ResolvedFile,
  SaveFile,
  SecretsGet,
  ServiceRequestParams,
  SettingValue,
  StateKey,
  StateSet,
  Toast,
} from "./protocol"

export type * from "./protocol"
export type { MockHost } from "./mock"
export type { CallFailure, PageInit, ServiceState, ServiceStatus } from "./bridge-types"

/** The params of a method that takes none. */
type Empty = Record<string, never>

/** Fails to compile unless `T` types exactly the methods AutoScene has. */
type Exactly<T extends Record<HostMethod, { params: unknown; result: unknown }> & Record<Exclude<keyof T, HostMethod>, never>> = T

/**
 * What each host method takes and gives. `HostMethod` comes from Rust
 * (`protocol.ts`): a method added there and missing here, or one here that
 * Rust no longer has, fails `npm run typecheck`.
 */
export type MethodTypes = Exactly<{
  "settings.get": { params: Empty; result: Record<string, SettingValue> }
  "secrets.get": { params: SecretsGet; result: { value: string | null } }
  "project.get": { params: ProjectRef; result: PageProject }
  "exports.list": { params: ProjectRef; result: PageExportSummary[] }
  "exports.get": { params: ExportRef; result: PageExport }
  "files.resolve": { params: FileRef; result: ResolvedFile }
  "net.fetch": { params: NetRequest; result: NetResponse }
  "state.get": { params: StateKey; result: { value: unknown } }
  "state.set": { params: StateSet; result: null }
  "state.delete": { params: StateKey; result: null }
  "state.keys": { params: Empty; result: string[] }
  "service.request": { params: ServiceRequestParams; result: unknown }
  "actions.run": { params: ActionsRun; result: { jobId: string | null } }
  "ui.toast": { params: Toast; result: { shown: boolean } }
  "ui.navigate": { params: Navigate; result: null }
  "ui.copyText": { params: CopyText; result: null }
  "ui.openExternal": { params: OpenExternal; result: { opened: boolean } }
  "files.save": { params: SaveFile; result: { saved: boolean } }
  "files.open": { params: OpenFile; result: OpenedFile | null }
}>

export type Params<M extends HostMethod> = MethodTypes[M]["params"]
export type Result<M extends HostMethod> = MethodTypes[M]["result"]

/** The global `AutoScene` that `sdk.js` defines. */
export interface AutoSceneSdk {
  readonly version: 1
  connect(options?: ConnectOptions): Promise<Page>
}

export interface ConnectOptions {
  /** Stop the browser's own keys, link clicks and menu in the page (default on). */
  guards?: boolean
  /** Hand the keys and right-clicks the page didn't take to AutoScene (default on). */
  forwardKeys?: boolean
  /** How long to wait for AutoScene, in ms (default 10 000). */
  timeoutMs?: number
  /** The host to answer from when the page isn't open in AutoScene. */
  mock?: MockHost
  /** @internal The window the page runs in; tests pass their own. */
  win?: Window
}

/** A plugin page, connected. */
export interface Page {
  readonly plugin: { id: string; name: string; version: string }
  readonly page: { id: string; title: string; scope: "global" | "project" }
  /** The project it is open in; null for a page of its own. */
  readonly projectId: string | null
  /** The path inside the page; `onRoute` says when it changes. */
  readonly path: string
  /** What its calls may use. */
  readonly permissions: Permission[]
  /** Any host method, as it is: the helpers below are made of it. */
  call<M extends HostMethod>(method: M, params: Params<M>): Promise<Result<M>>
  project: { get(): Promise<PageProject> }
  exports: { list(): Promise<PageExportSummary[]>; get(exportId: string): Promise<PageExport> }
  files: {
    resolve(ref: FileRef): Promise<ResolvedFile>
    save(f: SaveFile): Promise<boolean>
    open(f?: OpenFile): Promise<OpenedFile | null>
  }
  net: { fetch(r: NetRequest): Promise<NetResponse> }
  secrets: { get(key: string): Promise<string | null> }
  settings: { get(): Promise<Record<string, SettingValue>>; onChange(cb: () => void): () => void }
  state: {
    get(key: string): Promise<unknown>
    set(key: string, value: unknown): Promise<void>
    delete(key: string): Promise<void>
    keys(): Promise<string[]>
  }
  service: {
    request(method: string, params?: unknown, o?: { timeoutMs?: number }): Promise<unknown>
    on(event: string, cb: (data: unknown) => void): () => void
    /** Its service's state as the page knows it now, if it knows one, then each change. */
    onState(cb: (s: ServiceState) => void): () => void
  }
  actions: { run(action: string, o?: { exportId?: string; input?: unknown }): Promise<string | null> }
  ui: {
    toast(text: string, o?: { description?: string; tone?: "neutral" | "error" }): Promise<void>
    navigate(path: string, o?: { replace?: boolean }): Promise<void>
    copyText(text: string): Promise<void>
    openExternal(url: string): Promise<boolean>
  }
  onRoute(cb: (path: string) => void): () => void
  onClose(cb: (reason: string) => void): () => void
}

/**
 * The host as a page sees it, over the bridge or from the mock: what the
 * page was told, its calls, and what the host says unasked.
 */
export interface HostLink {
  readonly init: PageInit
  call(method: HostMethod, params: unknown): Promise<unknown>
  send(m: FrameToHost): void
  listen(cb: (m: HostToFrame) => void): void
}

export const version = 1

/**
 * One page per window: a second `connect()` gets the first one's page (in
 * AutoScene the host answers one `ready` per load of the frame).
 */
const connected = new WeakMap<Window, Promise<Page>>()

/** Connect the page to AutoScene (or to `options.mock` outside it). */
export function connect(options: ConnectOptions = {}): Promise<Page> {
  const { win = window, mock } = options
  const framed = win.parent !== win
  if (!framed && !mock) return Promise.reject(new Error("Open this page in AutoScene, or pass { mock } to connect()."))
  let page = connected.get(win)
  if (!page) {
    page = framed || !mock ? connectFramed(win, options) : Promise.resolve(pageOver(createMock(mock, win), win))
    connected.set(win, page)
  }
  return page
}

function connectFramed(win: Window, o: ConnectOptions): Promise<Page> {
  let link: HostLink | null = null
  const guards = o.guards !== false
  // Before the guards: they cancel what the page didn't take, and what it
  // didn't take is what goes to AutoScene.
  watchInput(win, { guards, forward: o.forwardKeys !== false }, (m) => link?.send(m))
  if (guards) installGuards({ dev: false, mac: /Mac OS X|Macintosh/.test(win.navigator.userAgent), win })
  return new Promise((resolve, reject) => {
    const onMessage = (e: MessageEvent) => {
      if (!isInit(e, win.parent)) return
      stop()
      try {
        link = portLink(e.ports[0], e.data.init)
        resolve(pageOver(link, win))
      } catch (err) {
        reject(err)
      }
    }
    const timer = win.setTimeout(() => {
      stop()
      reject(new Error("AutoScene didn't answer this page."))
    }, o.timeoutMs ?? 10_000)
    const stop = () => {
      win.clearTimeout(timer)
      win.removeEventListener("message", onMessage)
    }
    win.addEventListener("message", onMessage)
    const ready: Ready = { type: "autoscene:ready", v: BRIDGE_VERSION }
    // Once the document has loaded: the host takes one `ready` per load of
    // its frame.
    if (win.document.readyState === "complete") win.parent.postMessage(ready, "*")
    else win.addEventListener("load", () => win.parent.postMessage(ready, "*"), { once: true })
  })
}

/**
 * The host's `init`, and only that: from the parent, with exactly one port.
 * Anything else that writes to the window (a sibling frame, the page
 * itself) can't plant a port.
 */
function isInit(e: MessageEvent, parent: Window): e is MessageEvent<Init> {
  const d: unknown = e.data
  return (
    e.source === parent &&
    typeof d === "object" &&
    d !== null &&
    (d as Init).type === "autoscene:init" &&
    (d as Init).v === BRIDGE_VERSION &&
    e.ports?.length === 1
  )
}

/** Calls and messages over the port `init` brought. */
function portLink(port: MessagePort, init: PageInit): HostLink {
  const waiting = new Map<number, { resolve(v: unknown): void; reject(e: CallFailure): void }>()
  const listeners = new Set<(m: HostToFrame) => void>()
  let next = 1
  let closed: string | null = null
  const send = (m: FrameToHost) => {
    if (closed === null) port.postMessage(m)
  }
  port.addEventListener("message", (e: MessageEvent) => {
    const m = e.data as HostToFrame | null
    if (typeof m !== "object" || m === null || typeof m.t !== "string") return
    if (m.t === "reply") {
      const call = waiting.get(m.id)
      waiting.delete(m.id)
      if (m.ok) call?.resolve(m.result)
      else call?.reject(callFailure(m.error))
      return
    }
    if (m.t === "ping") return send({ t: "pong", n: m.n })
    if (m.t === "closed") {
      closed = m.reason
      for (const call of waiting.values()) call.reject(callFailure({ code: "closed", message: m.reason }))
      waiting.clear()
      port.close()
    }
    for (const cb of listeners) cb(m)
  })
  port.start()
  return {
    init,
    send,
    listen: (cb) => void listeners.add(cb),
    call(method, params) {
      if (closed !== null) return Promise.reject(callFailure({ code: "closed", message: closed }))
      if (waiting.size >= LIMITS.waiting) {
        return Promise.reject(callFailure({ code: "busy", message: `This page has ${LIMITS.waiting} calls waiting already.` }))
      }
      const id = next++
      try {
        port.postMessage({ t: "call", id, method, params } satisfies FrameToHost)
      } catch {
        return Promise.reject(callFailure({ code: "invalid", message: `The params of ${method} can't be sent.` }))
      }
      return new Promise((resolve, reject) => waiting.set(id, { resolve, reject }))
    },
  }
}

/**
 * Keys and right-clicks the page didn't take: with `forward`, for
 * AutoScene, so its shortcuts and its menu work over a page too; with
 * `guards`, the browser's own menu doesn't show outside a field. Each is
 * looked at after every other listener of the window, so the page always
 * goes first.
 */
function watchInput(win: Window, { guards, forward }: { guards: boolean; forward: boolean }, send: (m: FrameToHost) => void) {
  const last = <E extends Event>(e: E, then: () => void) => {
    win.addEventListener(e.type, (ev) => ev === e && then(), { once: true })
  }
  if (forward) {
    const onKey = (e: KeyboardEvent) =>
      last(e, () => {
        if (!e.defaultPrevented && forwards(e)) send({ t: "key", ...keyOf(e) })
      })
    win.addEventListener("keydown", onKey, { capture: true })
  }
  const onMenu = (e: MouseEvent) =>
    last(e, () => {
      // A field keeps the browser's edit menu where there is one (AutoScene
      // turns WebView2's off on Windows: there a field has its keys only);
      // a page may draw its own.
      if (e.defaultPrevented || isEditable(e.target)) return
      if (guards) e.preventDefault()
      if (forward) send({ t: "menu", x: e.clientX, y: e.clientY })
    })
  if (guards || forward) win.addEventListener("contextmenu", onMenu, { capture: true })
}

/**
 * Keys AutoScene may answer: Ctrl / ⌘ or Alt combinations, F-keys and
 * Escape. In a field only Ctrl / ⌘ ones: Alt and AltGr type there.
 */
function forwards(e: KeyboardEvent): boolean {
  if (e.isComposing) return false
  if (e.key === "Escape" || /^F\d{1,2}$/.test(e.key)) return true
  if (["Control", "Meta", "Alt", "AltGraph", "Shift"].includes(e.key)) return false
  if (isEditable(e.target)) return (e.ctrlKey || e.metaKey) && !e.altKey
  return e.ctrlKey || e.metaKey || e.altKey
}

function keyOf(e: KeyboardEvent): ForwardedKey {
  return {
    key: e.key,
    code: e.code,
    ctrl: e.ctrlKey,
    meta: e.metaKey,
    shift: e.shiftKey,
    alt: e.altKey,
    repeat: e.repeat,
  }
}

/** The page's view of a host: the helpers over its calls, and what it says unasked. */
function pageOver(link: HostLink, win: Window): Page {
  const { init } = link
  let path = init.path
  const routes = new Set<(path: string) => void>()
  const closes = new Set<(reason: string) => void>()
  const settings = new Set<() => void>()
  const states = new Set<(s: ServiceState) => void>()
  // The last state the host told: a page that listens late hears it too.
  let service: ServiceState | null = null
  const events = new Map<string, Set<(data: unknown) => void>>()
  const theme = (scheme: string) => {
    win.document.documentElement.dataset.scheme = scheme
  }
  theme(init.theme.scheme)
  link.listen((m) => {
    switch (m.t) {
      case "route":
        path = m.path
        return routes.forEach((cb) => cb(m.path))
      case "settings":
        return settings.forEach((cb) => cb())
      case "event":
        return events.get(m.name)?.forEach((cb) => cb(m.data))
      case "service": {
        const now: ServiceState = { state: m.state, message: m.message }
        service = now
        return states.forEach((cb) => cb(now))
      }
      case "closed":
        return closes.forEach((cb) => cb(m.reason))
      case "theme":
        return theme(m.theme.scheme)
    }
  })
  const call = <M extends HostMethod>(method: M, params: Params<M>) => link.call(method, params) as Promise<Result<M>>
  return {
    plugin: init.plugin,
    page: init.page,
    projectId: init.projectId,
    get path() {
      return path
    },
    permissions: init.permissions,
    call,
    project: { get: () => call("project.get", {}) },
    exports: {
      list: () => call("exports.list", {}),
      get: (exportId) => call("exports.get", { exportId }),
    },
    files: {
      resolve: (ref) => call("files.resolve", ref),
      save: async (f) => (await call("files.save", f)).saved,
      open: (f) => call("files.open", f ?? {}),
    },
    net: { fetch: (r) => call("net.fetch", r) },
    secrets: { get: async (key) => (await call("secrets.get", { key })).value },
    settings: {
      get: () => call("settings.get", {}),
      onChange: (cb) => listen(settings, cb),
    },
    state: {
      get: async (key) => (await call("state.get", { key })).value,
      set: async (key, value) => void (await call("state.set", { key, value })),
      delete: async (key) => void (await call("state.delete", { key })),
      keys: () => call("state.keys", {}),
    },
    service: {
      request: (method, params, o) => call("service.request", { method, params, timeoutMs: o?.timeoutMs }),
      on: (event, cb) => {
        const set = events.get(event) ?? new Set()
        events.set(event, set)
        return listen(set, cb)
      },
      onState: (cb) => {
        if (service) cb(service)
        return listen(states, cb)
      },
    },
    actions: {
      run: async (action, o) =>
        (await call("actions.run", { action, exportId: o?.exportId, input: o?.input })).jobId,
    },
    ui: {
      toast: async (text, o) => void (await call("ui.toast", { text, ...o })),
      // AutoScene doesn't echo the page's own moves back as a route.
      navigate: async (to, o) => {
        await call("ui.navigate", { path: to, replace: o?.replace })
        path = to
      },
      copyText: async (text) => void (await call("ui.copyText", { text })),
      openExternal: async (url) => (await call("ui.openExternal", { url })).opened,
    },
    onRoute: (cb) => listen(routes, cb),
    onClose: (cb) => listen(closes, cb),
  }
}

function listen<T>(set: Set<T>, cb: T): () => void {
  set.add(cb)
  return () => void set.delete(cb)
}
