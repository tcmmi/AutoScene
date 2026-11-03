import fs from "node:fs"
import http from "node:http"
import path from "node:path"
import type { Plugin } from "vite"

// The browser mock's asp scheme: a second server, on an origin of its own,
// that serves plugin pages as the app's scheme does (crates/as-plugin/src/
// pages.rs). Each session's page and the files under its plugin's `ui/`,
// the files a page was given (`~f/`, in parts of 1 MiB), and the kit at
// `/_as/`, with the headers the app sends: the page CSP for this origin
// (page-csp.golden.txt, which a Rust test checks against `page_csp`),
// framed only by Vite's page. The browser mock tells it what it serves
// (mocks/plugins.ts, through Vite's origin at /__mock-asp). The frame's
// sandbox makes its origin opaque here too, so storage, popups and the
// handshake behave as in the app.

/** Where the mock's pages load from: Vite is fixed at 5173, so this port is free. */
export const MOCK_ASP_PORT = 5174
/** The page CSP holds this session in its golden file. */
export const GOLDEN_SESSION = "0123456789abcdef0123456789abcdef"
/** The most of a given file one answer holds, as the app's scheme answers a media element. */
const RANGE_PART = 1 << 20

/** What the mock's page server serves: each session's plugin folder, and each file a page was given. */
export type MockAsp = {
  /** Each example plugin's folder, by its id. */
  plugins: Map<string, string>
  sessions: Map<string, string>
  files: Map<string, string>
  /** The CSP of a page of the golden session. */
  csp: string
  /** Where the kit's built files are (`dist/_as`). */
  kit: string
  /** Where the fixtures a page's files play are (`fixtures/out`). */
  fixtures: string
}

/** The mock's page server's state: the example plugins in `examples` and the CSP in `golden`. */
export function mockAspState({ examples, golden, kit, fixtures }: { examples: string; golden: string; kit: string; fixtures: string }): MockAsp {
  const plugins = new Map<string, string>()
  for (const dir of fs.existsSync(examples) ? fs.readdirSync(examples) : []) {
    try {
      const manifest = JSON.parse(fs.readFileSync(path.join(examples, dir, "plugin.json"), "utf8")) as { id?: unknown }
      if (typeof manifest.id === "string") plugins.set(manifest.id, path.join(examples, dir))
    } catch {
      // Not a plugin's folder.
    }
  }
  return { plugins, sessions: new Map(), files: new Map(), csp: fs.readFileSync(golden, "utf8").trim(), kit, fixtures }
}

/** The type of a page's file by its name, as `content_type` in pages.rs has it. */
const TYPES: Record<string, string> = {
  html: "text/html; charset=utf-8",
  js: "text/javascript; charset=utf-8",
  mjs: "text/javascript; charset=utf-8",
  css: "text/css; charset=utf-8",
  json: "application/json",
  map: "application/json",
  svg: "image/svg+xml",
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
  avif: "image/avif",
  ico: "image/x-icon",
  woff2: "font/woff2",
  woff: "font/woff",
  ttf: "font/ttf",
  otf: "font/otf",
  txt: "text/plain; charset=utf-8",
  md: "text/markdown; charset=utf-8",
  mp4: "video/mp4",
  m4v: "video/mp4",
  webm: "video/webm",
  mov: "video/quicktime",
  mp3: "audio/mpeg",
  m4a: "audio/mp4",
  wav: "audio/wav",
  ogg: "audio/ogg",
  opus: "audio/opus",
  flac: "audio/flac",
}

const typeOf = (name: string) => TYPES[name.split(".").pop()?.toLowerCase() ?? ""]
const isSession = (s: string | undefined): s is string => s != null && /^[0-9a-f]{32}$/.test(s)
/** A path in a package: parts of letters, digits and `._@+-`, none of them `.` or `..`. */
const plainPath = (p: string) => p.split("/").every((part) => /^[A-Za-z0-9._@+-]+$/.test(part) && part !== "." && part !== "..")

/** The headers every answer gets: the page CSP of `session` (none: one that allows nothing), and the rest pages.rs sends. */
function headers(state: MockAsp, session: string | null): Record<string, string> {
  const ancestors = "http://localhost:5173"
  const csp = session
    ? state.csp.replaceAll(GOLDEN_SESSION, session)
    : `default-src 'none'; form-action 'none'; base-uri 'none'; frame-ancestors ${ancestors}`
  return {
    "Content-Security-Policy": csp,
    "X-Content-Type-Options": "nosniff",
    "Access-Control-Allow-Origin": "*",
    "Cross-Origin-Resource-Policy": "cross-origin",
    "Cache-Control": "no-store",
    "Referrer-Policy": "no-referrer",
    "Permissions-Policy":
      "camera=(), microphone=(), geolocation=(), display-capture=(), clipboard-read=(), clipboard-write=(), usb=(), serial=(), hid=(), bluetooth=(), payment=(), picture-in-picture=()",
  }
}

/** The one part `range` asks of `len` bytes, at most `most` long; null when it can't be given. */
function part(range: string, len: number, most: number): [number, number] | null {
  const m = /^bytes=(\d*)-(\d*)$/.exec(range.trim())
  if (!m || (m[1] === "" && m[2] === "") || len === 0) return null
  const [start, end] = m[1] === "" ? [Math.max(0, len - Number(m[2])), len - 1] : [Number(m[1]), m[2] === "" ? len - 1 : Math.min(Number(m[2]), len - 1)]
  return start <= end && start < len ? [start, Math.min(end + 1, start + most)] : null
}

/** The answers of the mock's page server. */
export function mockAspHandler(state: MockAsp): http.RequestListener {
  return (req, res) => {
    const url = new URL(req.url ?? "/", "http://localhost")
    let parts: string[]
    try {
      parts = decodeURIComponent(url.pathname).split("/").slice(1)
    } catch {
      parts = []
    }
    const session = isSession(parts[0]) ? parts[0] : null
    const send = (status: number, body: Buffer | null, more: Record<string, string> = {}) => {
      res.writeHead(status, { ...headers(state, session), "Content-Length": String(body?.length ?? 0), ...more })
      res.end(req.method === "HEAD" ? undefined : (body ?? undefined))
    }
    if (req.method !== "GET" && req.method !== "HEAD") return send(405, null, { Allow: "GET, HEAD" })

    // The kit: sdk.js and theme.css.
    if (parts[0] === "_as" && parts.length === 2 && ["sdk.js", "theme.css"].includes(parts[1])) {
      const file = path.join(state.kit, parts[1])
      return fs.existsSync(file) ? send(200, fs.readFileSync(file), { "Content-Type": typeOf(parts[1]) }) : send(404, null)
    }
    if (!session) return send(404, null)

    // A file the page was given, in parts.
    if (parts[1] === "~f" && parts.length === 3) {
      const file = state.files.get(`${session}/${parts[2]}`)
      if (!file || !fs.existsSync(file)) return send(404, null)
      const len = fs.statSync(file).size
      const asked = req.headers.range
      const [start, end] = asked ? (part(asked, len, RANGE_PART) ?? [-1, -1]) : [0, Math.min(len, RANGE_PART)]
      if (start < 0) return send(416, null, { "Content-Range": `bytes */${len}` })
      const body = Buffer.alloc(end - start)
      const fd = fs.openSync(file, "r")
      fs.readSync(fd, body, 0, body.length, start)
      fs.closeSync(fd)
      const kind = { "Content-Type": typeOf(file) ?? "application/octet-stream", "Accept-Ranges": "bytes" }
      if (!asked && len <= RANGE_PART) return send(200, body, kind)
      return send(206, body, { ...kind, "Content-Range": `bytes ${start}-${end - 1}/${len}` })
    }

    // A page's own file, under its plugin's ui/.
    const dir = state.plugins.get(state.sessions.get(session) ?? "")
    const file = parts.slice(1).join("/")
    const kind = typeOf(file)
    if (!dir || !kind || !file.startsWith("ui/") || !plainPath(file)) return send(404, null)
    const at = path.join(dir, ...file.split("/"))
    if (!fs.existsSync(at) || !fs.lstatSync(at).isFile()) return send(404, null)
    send(200, fs.readFileSync(at), { "Content-Type": kind, "Accept-Ranges": "bytes" })
  }
}

/**
 * Starts the mock's page server with Vite's (`vite` and `npm run dev:mock`
 * only), and lets the browser mock tell it what it serves.
 */
export function mockAsp(o: { examples: string; golden: string; kit: string; fixtures: string }): Plugin {
  return {
    name: "autoscene-mock-asp",
    apply: "serve",
    configureServer(server) {
      const state = mockAspState(o)
      server.middlewares.use("/__mock-asp", (req, res) => {
        let body = ""
        req.on("data", (chunk: Buffer) => (body += chunk.toString("utf8")))
        req.on("end", () => {
          try {
            const said = JSON.parse(body) as { session?: unknown; pluginId?: unknown; token?: unknown; fixture?: unknown }
            if (isSession(said.session as string) && typeof said.pluginId === "string") state.sessions.set(said.session as string, said.pluginId)
            if (isSession(said.session as string) && typeof said.token === "string" && typeof said.fixture === "string") {
              state.files.set(`${said.session}/${said.token}`, path.join(state.fixtures, path.basename(said.fixture)))
            }
            res.statusCode = 204
          } catch {
            res.statusCode = 400
          }
          res.end()
        })
      })
      const pages = http.createServer(mockAspHandler(state))
      pages.on("error", (e) => server.config.logger.warn(`Plugin pages can't be served at port ${MOCK_ASP_PORT}: ${e.message}`))
      pages.listen(MOCK_ASP_PORT, "localhost")
      server.httpServer?.on("close", () => pages.close())
    },
  }
}
