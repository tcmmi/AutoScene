/**
 * An in-memory host for a page opened outside AutoScene: in a plain browser,
 * while it is being made. `connect({ mock })` answers the page from here as
 * AutoScene would — the same results, and the same refusals for what its
 * plugin may not use — from the data the page passes. What only AutoScene
 * has (files, the network, a service, runs) answers through `answer`, or
 * says the mock doesn't have it.
 */
import { callFailure, type PageInit } from "./bridge-types"
import type { CallCode, HostMethod, PageExport, PageProject, PageScope, Permission, SettingValue } from "./protocol"
import type { HostLink, Params, Result } from "./sdk"

export interface MockHost {
  plugin?: { id: string; name: string; version: string }
  page?: { id: string; title: string; scope: PageScope }
  /** The project the page is open in. */
  projectId?: string | null
  path?: string
  /** What its calls may use, as if the person agreed to it. */
  permissions?: Permission[]
  /** Where `net.fetch` may connect. */
  hosts?: string[]
  settings?: Record<string, SettingValue>
  /** Its secret settings by key; null for one the person hasn't set. */
  secrets?: Record<string, string | null>
  project?: PageProject
  exports?: PageExport[]
  /** Its plugin runs a service. */
  service?: boolean
  /** Answers for any method, in place of the mock's own. */
  answer?: { [M in HostMethod]?: (params: Params<M>) => Result<M> | Promise<Result<M>> }
}

type Answers = { [M in HostMethod]: (params: Params<M>) => Result<M> | Promise<Result<M>> }

/** The mock's side of the bridge. */
export function createMock(host: MockHost, win: Window): HostLink {
  const plugin = host.plugin ?? { id: "dev.example.mock", name: "Mock", version: "0.0.0" }
  const page = host.page ?? { id: "main", title: "Mock page", scope: host.projectId ? "project" : "global" }
  const projectId = host.projectId ?? null
  const permissions = host.permissions ?? []
  const hosts = host.hosts ?? []
  const state = new Map<string, unknown>()
  let jobs = 0

  const may = (method: HostMethod, p: Permission) => {
    if (!permissions.includes(p)) throw refused("denied", `${method} needs the ${p} permission.`)
  }
  const project = (method: HostMethod, asked: string | null | undefined) => {
    may(method, method === "project.get" ? "project:read" : "exports:read")
    if (!projectId) throw refused("invalid", "This page isn't open in a project.")
    if (asked && asked !== projectId) throw refused("denied", "A page reads only the project it is open in.")
    return host.project ?? { id: projectId, title: "Mock project", episodes: [] }
  }
  const stateKey = (key: string) => {
    if (!/^[A-Za-z0-9._:-]{1,128}$/.test(key)) {
      throw refused("invalid", "A state key is 1 to 128 letters, digits, dots, colons, dashes or underscores.")
    }
    return key
  }

  const answers: Answers = {
    "settings.get": () => ({ ...host.settings }),
    "secrets.get": ({ key }) => {
      const secrets = host.secrets ?? {}
      if (Object.keys(secrets).length === 0) throw refused("denied", "secrets.get needs a secret setting, and this plugin has none.")
      if (!(key in secrets)) {
        throw refused("denied", `secrets.get reads only the plugin's secret settings, and ${key} isn't one.`)
      }
      return { value: secrets[key] ?? null }
    },
    "project.get": ({ projectId: asked }) => project("project.get", asked),
    "exports.list": ({ projectId: asked }) => {
      project("exports.list", asked)
      return (host.exports ?? []).map((e) => ({
        id: e.id,
        label: e.label,
        preset: e.preset,
        createdAt: e.createdAt,
        merged: e.merged,
        clips: e.clips.length,
        done: e.clips.filter((c) => c.status === "done").length,
        status: e.clips.every((c) => c.status === "done") ? "done" : "running",
      }))
    },
    "exports.get": ({ projectId: asked, exportId }) => {
      project("exports.get", asked)
      const found = host.exports?.find((e) => e.id === exportId)
      if (!found) throw refused("not_found", "That export doesn't exist.")
      return found
    },
    "files.resolve": (ref) => {
      may("files.resolve", ref.kind === "episode" ? "project:read" : "exports:read")
      throw refused("not_found", "The mock has no files.")
    },
    "net.fetch": ({ url }) => {
      may("net.fetch", "network")
      const name = URL.canParse(url) ? new URL(url).hostname : url
      if (!hosts.includes(name)) {
        throw refused("denied", `net.fetch reaches only the hosts the plugin names, and ${name} isn't one.`)
      }
      throw refused("failed", "The mock doesn't reach the network.")
    },
    "state.get": ({ key }) => ({ value: state.get(stateKey(key)) ?? null }),
    "state.set": ({ key, value }) => {
      // As JSON, as AutoScene keeps it.
      state.set(stateKey(key), JSON.parse(JSON.stringify(value ?? null)))
      return null
    },
    "state.delete": ({ key }) => {
      state.delete(stateKey(key))
      return null
    },
    "state.keys": () => [...state.keys()].sort(),
    "service.request": () => {
      if (!host.service) throw refused("denied", "service.request needs a plugin that runs a service.")
      throw refused("failed", "The mock has no service.")
    },
    "actions.run": ({ action }) => {
      console.info(`Mock: ${plugin.name} runs ${action}.`)
      return { jobId: `mock-${++jobs}` }
    },
    "ui.toast": ({ text, tone }) => {
      const title = `Plugin “${plugin.name}”: ${text}`
      if (tone === "error") console.error(title)
      else console.info(title)
      return { shown: true }
    },
    "ui.navigate": () => null,
    "ui.copyText": async ({ text }) => {
      await win.navigator.clipboard.writeText(text)
      return null
    },
    "ui.openExternal": ({ url }) => {
      if (!url.startsWith("https://")) throw refused("invalid", "A page opens only https:// links.")
      return { opened: win.open(url, "_blank", "noopener,noreferrer") !== null }
    },
    "files.save": ({ name, text, dataBase64 }) => {
      const body = text ?? Uint8Array.from(atob(dataBase64 ?? ""), (c) => c.charCodeAt(0))
      const a = win.document.createElement("a")
      a.href = URL.createObjectURL(new Blob([body]))
      a.download = name
      a.click()
      URL.revokeObjectURL(a.href)
      return { saved: true }
    },
    "files.open": ({ filters, maxBytes }) =>
      new Promise((resolve, reject) => {
        const input = win.document.createElement("input")
        input.type = "file"
        input.accept = (filters ?? []).flatMap((f) => f.extensions.map((x) => `.${x}`)).join(",")
        input.addEventListener("cancel", () => resolve(null))
        input.addEventListener("change", async () => {
          const file = input.files?.[0]
          if (!file) return resolve(null)
          const max = Math.min(maxBytes ?? 16 << 20, 16 << 20)
          if (file.size > max) return reject(refused("invalid", `That file is larger than ${Math.ceil(max / 1e6)} MB.`))
          const bytes = new Uint8Array(await file.arrayBuffer())
          let binary = ""
          for (const b of bytes) binary += String.fromCharCode(b)
          resolve({ name: file.name, size: file.size, dataBase64: btoa(binary) })
        })
        input.click()
      }),
  }

  const init: PageInit = {
    plugin,
    page,
    projectId,
    path: host.path ?? "",
    theme: { v: 1, scheme: "dark" },
    permissions,
    hosts,
    service: host.service ?? false,
    protocol: 2,
    app: { version: "mock", platform: "windows" },
  }
  return {
    init,
    async call(method, params) {
      const own = host.answer?.[method] as ((p: unknown) => unknown) | undefined
      const mine = answers[method] as (p: unknown) => unknown
      // The host answers in turn, never inside the page's own call.
      await Promise.resolve()
      return (own ?? mine)(params ?? {})
    },
    send: () => {},
    listen: () => {},
  }
}

function refused(code: CallCode, message: string) {
  return callFailure({ code, message })
}
