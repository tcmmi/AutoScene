/**
 * The bridge between a plugin page and AutoScene: the window messages of the
 * handshake, then the messages on the port it hands over. The SDK speaks the
 * page's side (`sdk.ts`), the app the host's (`features/plugins/bridge.ts`).
 *
 * The handshake: the page posts `ready` to its parent; the host checks that
 * it came from its frame and answers `init` with one port. From then on both
 * sides speak only over that port.
 */
import type { CallCode, CallError, HostMethod, PageScope, Permission } from "./protocol"

/** The bridge's version, in both handshake messages. */
export const BRIDGE_VERSION = 1

/** The page's first message, to its parent. */
export type Ready = { type: "autoscene:ready"; v: 1 }

/** The host's answer, with the port as the message's one transfer. */
export type Init = { type: "autoscene:init"; v: 1; init: PageInit }

/** How a page should look. Only AutoScene's dark theme for now. */
export type PageTheme = { v: 1; scheme: "dark" }

/** The language AutoScene speaks, as a page is told it: the languages it has, whatever the person chose or the system said. */
export type PageLocale = "en" | "ro"

/** What a page is told when it connects. */
export type PageInit = {
  plugin: { id: string; name: string; version: string }
  page: { id: string; title: string; scope: PageScope }
  /** The project it is open in; null for a page of its own. */
  projectId: string | null
  /** The path inside the page, after its entry (`#/x/<plugin>/<page>/<path>`). */
  path: string
  theme: PageTheme
  /** What its calls may use: the permissions the person agreed to and its manifest still asks for. */
  permissions: Permission[]
  /** Where `net.fetch` may connect. */
  hosts: string[]
  /** Its plugin runs a service, which `service.request` reaches. */
  service: boolean
  /** The protocol its plugin speaks (`engines.protocol`). */
  protocol: number
  app: { version: string; platform: "windows" | "macos" | "linux"; locale: PageLocale }
}

/** A key the page didn't take, for AutoScene's own shortcuts. */
export type ForwardedKey = {
  key: string
  code: string
  ctrl: boolean
  meta: boolean
  shift: boolean
  alt: boolean
  repeat: boolean
}

/** What a page's service is doing. */
export type ServiceStatus = "starting" | "ready" | "stopped"

export type ServiceState = { state: ServiceStatus; message?: string }

/** Page to host, on the port. */
export type FrameToHost =
  | { t: "call"; id: number; method: HostMethod; params: unknown }
  | ({ t: "key" } & ForwardedKey)
  /** A right-click outside a field, in the frame's pixels. */
  | { t: "menu"; x: number; y: number }
  | { t: "pong"; n: number }

/** Host to page, on the port. */
export type HostToFrame =
  | { t: "reply"; id: number; ok: true; result: unknown }
  | { t: "reply"; id: number; ok: false; error: CallError }
  /** An event of the plugin's service. */
  | { t: "event"; name: string; data: unknown }
  | ({ t: "service" } & ServiceState)
  /** Back or Forward changed the path inside the page. */
  | { t: "route"; path: string }
  /** The person changed the plugin's settings. */
  | { t: "settings" }
  /** The page was closed; nothing it calls is answered any more. */
  | { t: "closed"; reason: string }
  | { t: "ping"; n: number }
  | { t: "theme"; theme: PageTheme }
  /** The person changed AutoScene's language. */
  | { t: "locale"; locale: PageLocale }

/** What either side holds to. The host checks again, and Rust after it. */
export const LIMITS = {
  /** Calls a page may have waiting; past that the reply is `busy`. */
  waiting: 64,
  /** Calls the host hands to Rust at once. */
  inflight: 16,
  /** The JSON of the calls a page has waiting and in flight, at most (each method's own cap is `MAX_PARAMS`). */
  heldBytes: 64 * 1024 * 1024,
  /** Messages the host can't read before it closes the port. */
  drops: 1000,
} as const

/** A call AutoScene refused or couldn't answer, as the page gets it: its kind, and a sentence that says why. */
export interface CallFailure extends Error {
  readonly code: CallCode
}

export function callFailure({ code, message }: CallError): CallFailure {
  return Object.assign(new Error(message), { name: "CallFailure", code })
}
