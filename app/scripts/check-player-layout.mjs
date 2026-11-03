// Checks the player's transport row in a real browser: at every tier edge
// and the common window sizes, with the details panel open and closed, it
// shows the row idle, shuttling at −4×, with both marks set (also while
// shuttling) and with Go to open, for a 586-frame episode and a 2-hour film
// (172,800 frames), in the elapsed and remaining timecode modes. It fails
// when two of the row's controls or numbers overlap, when something leaves
// the row, when Play isn't under the middle (tiers 1 to 3), when the row's
// tier isn't the one its width calls for (`tierFor`: the overflow guard
// stepped in), or when the timecode moves as the speed shows.
//
//   npm run check:player-layout
//
// It starts the browser mock (Vite, `--mode mock`) on a free port with its
// own cache, and headless Edge or Chrome (`AUTOSCENE_BROWSER` names
// another), and stops both at the end. The plugin-page mock stays off: it
// would claim port 5174, which a running `dev:mock` holds. Like `dev:mock` it plays the test
// fixtures: `cargo xtask fixtures` makes them in fixtures/out, and
// `AUTOSCENE_FIXTURES` points elsewhere. Not in CI: CI has neither the
// fixtures nor a browser. Run it again for every language the app gets.

import { spawn } from "node:child_process"
import { existsSync, mkdtempSync, rmSync } from "node:fs"
import { createServer as listen } from "node:net"
import { tmpdir } from "node:os"
import { delimiter, join, resolve } from "node:path"
import { createServer, loadConfigFromFile } from "vite"

const app = resolve(import.meta.dirname, "..")
const fixtures = resolve(process.env.AUTOSCENE_FIXTURES ?? join(app, "..", "fixtures", "out"))
if (!existsSync(join(fixtures, "cfr24_h264.mp4"))) {
  console.error(`No test fixtures in ${fixtures}: run \`cargo xtask fixtures\` or set AUTOSCENE_FIXTURES.`)
  process.exit(2)
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

/** A port nothing listens on right now. */
function freePort() {
  return new Promise((done, fail) => {
    const s = listen()
    s.on("error", fail)
    s.listen(0, "127.0.0.1", () => {
      const { port } = s.address()
      s.close(() => done(port))
    })
  })
}

function findBrowser() {
  if (process.env.AUTOSCENE_BROWSER) return process.env.AUTOSCENE_BROWSER
  const local = process.env.LOCALAPPDATA ?? ""
  const candidates = {
    win32: [
      "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
      "C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe",
      "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
      join(local, "Google\\Chrome\\Application\\chrome.exe"),
    ],
    darwin: [
      "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
      "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
      "/Applications/Chromium.app/Contents/MacOS/Chromium",
    ],
  }[process.platform] ?? []
  const onPath = ["microsoft-edge", "google-chrome", "chromium", "chromium-browser"].flatMap((name) =>
    (process.env.PATH ?? "").split(delimiter).map((dir) => join(dir, name)),
  )
  const found = [...candidates, ...onPath].find((p) => existsSync(p))
  if (!found) {
    console.error("No Edge or Chrome found: set AUTOSCENE_BROWSER to one.")
    process.exit(2)
  }
  return found
}

/** A Chrome DevTools Protocol session with the browser's first page. */
async function devtools(port) {
  let pages = []
  for (let i = 0; i < 100 && pages.length === 0; i++) {
    try {
      pages = (await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()).filter((t) => t.type === "page")
    } catch {
      // The browser is still starting.
    }
    if (pages.length === 0) await sleep(200)
  }
  if (pages.length === 0) throw new Error("The browser didn't open a page.")
  const ws = new WebSocket(pages[0].webSocketDebuggerUrl)
  await new Promise((r, f) => {
    ws.addEventListener("open", r, { once: true })
    ws.addEventListener("error", f, { once: true })
  })
  let id = 0
  const pending = new Map()
  ws.addEventListener("message", (ev) => {
    const msg = JSON.parse(ev.data)
    const p = pending.get(msg.id)
    if (!p) return
    pending.delete(msg.id)
    if (msg.error) p.reject(new Error(JSON.stringify(msg.error)))
    else p.resolve(msg.result)
  })
  const send = (method, params = {}) =>
    new Promise((res, rej) => {
      const i = ++id
      pending.set(i, { resolve: res, reject: rej })
      ws.send(JSON.stringify({ id: i, method, params }))
    })
  const evaluate = async (expression) => {
    const r = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true })
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? JSON.stringify(r.exceptionDetails))
    return r.result.value
  }
  return { send, evaluate, close: () => ws.close() }
}

// Key codes for the keys the check presses.
const KEYS = {
  Home: ["Home", 36],
  End: ["End", 35],
  Escape: ["Escape", 27],
  i: ["KeyI", 73],
  o: ["KeyO", 79],
  j: ["KeyJ", 74],
  k: ["KeyK", 75],
  x: ["KeyX", 88],
  g: ["KeyG", 71],
}

async function press(c, key, { alt = false, ctrl = false } = {}) {
  const [code, keyCode] = KEYS[key]
  const modifiers = (alt ? 1 : 0) | (ctrl ? 2 : 0)
  const base = { key, code, windowsVirtualKeyCode: keyCode, nativeVirtualKeyCode: keyCode, modifiers }
  await c.send("Input.dispatchKeyEvent", { type: "rawKeyDown", ...base })
  await c.send("Input.dispatchKeyEvent", { type: "keyUp", ...base })
}

/** Two frames: the row has laid itself out (and its guard has run). */
const settle = (c) => c.evaluate("new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(r, 30))))")

/** What the row looks like now, measured in the page. */
const MEASURE = `(async () => {
  const { tierFor } = await import("/src/features/player/transport-tier.ts")
  const row = document.querySelector("[data-transport]")
  const box = row.getBoundingClientRect()
  const shown = (el) => {
    const r = el.getBoundingClientRect()
    return r.width > 0 && r.height > 0 && getComputedStyle(el).visibility !== "hidden"
  }
  const name = (el) => el.getAttribute("aria-label") || el.textContent.trim() || el.tagName
  const items = [...row.querySelectorAll("button, input, [role=slider], .timecode")].filter(shown)
  const rects = items.map((el) => el.getBoundingClientRect())
  const overlaps = []
  for (let a = 0; a < items.length; a++)
    for (let b = a + 1; b < items.length; b++) {
      if (items[a].contains(items[b]) || items[b].contains(items[a])) continue
      const w = Math.min(rects[a].right, rects[b].right) - Math.max(rects[a].left, rects[b].left)
      const h = Math.min(rects[a].bottom, rects[b].bottom) - Math.max(rects[a].top, rects[b].top)
      if (w > 0.5 && h > 0.5) overlaps.push(name(items[a]) + " / " + name(items[b]))
    }
  const outside = items
    .filter((_, i) => rects[i].left < box.left - 0.5 || rects[i].right > box.right + 0.5 || rects[i].top < box.top - 0.5 || rects[i].bottom > box.bottom + 0.5)
    .map(name)
  const play = row.querySelector("button[aria-label=Play], button[aria-label=Pause]").getBoundingClientRect()
  const readout = row.querySelector("[data-readout]")?.getBoundingClientRect() ?? null
  const speed = row.querySelector("[data-speed]")
  return {
    width: row.clientWidth,
    tier: Number(row.dataset.tier),
    expected: tierFor(row.clientWidth),
    overlaps,
    outside,
    playOff: (play.left + play.right) / 2 - (box.left + box.right) / 2,
    readout: readout && { left: readout.left, right: readout.right },
    speed: speed ? getComputedStyle(speed).visibility !== "hidden" : null,
    goto: !!row.querySelector("input"),
    marks: !!row.querySelector("button[aria-label^='Go to the in point']"),
  }
})()`

const PLAYER = `(async () => {
  const inv = window.__TAURI_INTERNALS__.invoke
  for (const p of await inv("projects_list")) {
    const m = (await inv("media_list", { projectId: p.id })).find((x) => x.status === "ready")
    if (m) {
      location.hash = "#/p/" + p.id + "/timeline/" + m.id
      return true
    }
  }
  return false
})()`

// The row widths to check: every tier edge, and the common windows.
const EDGES = [807, 808, 959, 960, 1291, 1292]
const WINDOWS = [
  [1920, 1080],
  [1440, 900],
  [1366, 768],
  [1280, 800],
  [1024, 700],
  [1024, 640],
]

const vitePort = await freePort()
const cdpPort = await freePort()
const scratch = mkdtempSync(join(tmpdir(), "autoscene-layout-"))
// The app's config without the plugin-page mock (vite.mock-asp.ts): this check
// opens no plugin page, and that mock's second server would take port 5174
// from a running `dev:mock` or another check.
const PAGE_MOCK = "autoscene-mock-asp"
const env = { command: "serve", mode: "mock", isSsrBuild: false, isPreview: false }
const loaded = await loadConfigFromFile(env, join(app, "vite.config.ts"), app)
const plugins = (loaded?.config.plugins ?? []).flat(Infinity).filter(Boolean)
if (!plugins.some((p) => p.name === PAGE_MOCK)) {
  console.error(`vite.config.ts has no plugin named ${PAGE_MOCK} any more: update PAGE_MOCK here.`)
  process.exit(2)
}
const server = await createServer({
  ...loaded.config,
  configFile: false,
  plugins: plugins.filter((p) => p.name !== PAGE_MOCK),
  root: app,
  mode: "mock",
  cacheDir: join(scratch, "vite"),
  logLevel: "error",
  clearScreen: false,
  server: { port: vitePort, strictPort: true, fs: { allow: [app, fixtures] } },
  define: { "import.meta.env.VITE_FIXTURES_URL": JSON.stringify(`/@fs/${fixtures.split("\\").join("/")}/`) },
})
await server.listen()
const base = `http://localhost:${vitePort}/`

const browser = spawn(
  findBrowser(),
  [
    "--headless=new",
    "--mute-audio",
    `--remote-debugging-port=${cdpPort}`,
    `--user-data-dir=${join(scratch, "browser")}`,
    "--no-first-run",
    "--no-default-browser-check",
    "--disable-extensions",
    "about:blank",
  ],
  { stdio: "ignore" },
)

let failures = 0
let cases = 0
let c
try {
  c = await devtools(cdpPort)
  await c.send("Page.enable")
  await c.send("Emulation.setFocusEmulationEnabled", { enabled: true })

  const viewport = (width, height) =>
    c.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false })

  for (const frames of [586, 172_800]) {
    for (const mode of ["elapsed", "remaining"]) {
      for (const panel of [true, false]) {
        await viewport(1440, 900)
        await c.send("Page.navigate", { url: base })
        for (let i = 0; i < 150 && !(await c.evaluate("!!window.__TAURI_INTERNALS__").catch(() => false)); i++) await sleep(200)
        await c.evaluate(`(() => {
          localStorage.setItem("autoscene.mock.frames", "${frames}")
          localStorage.setItem("autoscene.timecode-mode", JSON.stringify({ state: { mode: "${mode}" }, version: 0 }))
          localStorage.setItem("autoscene.ui", JSON.stringify({ state: { sidebarCollapsed: false, inspectorOpen: ${panel}, castView: "grid" }, version: 0 }))
          return true
        })()`)
        await c.send("Page.reload")
        for (let i = 0; i < 150 && !(await c.evaluate("!!window.__TAURI_INTERNALS__").catch(() => false)); i++) await sleep(200)
        await sleep(500)
        if (!(await c.evaluate(PLAYER))) throw new Error("The mock has no ready episode.")
        for (let i = 0; i < 100 && !(await c.evaluate("!!document.querySelector('[data-transport]')")); i++) await sleep(200)
        await c.evaluate(`document.getElementById("boot")?.remove(); true`)

        // How much wider the window is than the row, to aim at a row width.
        await settle(c)
        const extra = 1440 - (await c.evaluate("document.querySelector('[data-transport]').clientWidth"))
        const sizes = [...(panel ? EDGES.map((w) => [w + extra, 900]) : []), ...WINDOWS]

        for (const [w, h] of sizes) {
          await viewport(w, h)
          await settle(c)
          const look = {}
          await press(c, "x", { alt: true })
          await press(c, "Home")
          await settle(c)
          look.idle = await c.evaluate(MEASURE)
          await press(c, "End")
          for (let i = 0; i < 3; i++) await press(c, "j")
          await settle(c)
          look.shuttling = await c.evaluate(MEASURE)
          await press(c, "k")
          await press(c, "Home")
          await press(c, "i")
          await press(c, "End")
          await press(c, "o")
          await settle(c)
          look.marked = await c.evaluate(MEASURE)
          for (let i = 0; i < 3; i++) await press(c, "j")
          await settle(c)
          look["marked, shuttling"] = await c.evaluate(MEASURE)
          await press(c, "k")
          await press(c, "g", { ctrl: true })
          await settle(c)
          look["go to"] = await c.evaluate(MEASURE)
          await press(c, "Escape")
          await press(c, "x", { alt: true })

          const problems = []
          for (const [state, m] of Object.entries(look)) {
            for (const o of m.overlaps) problems.push(`${state}: ${o} overlap`)
            for (const o of m.outside) problems.push(`${state}: ${o} leaves the row`)
            if (m.tier !== m.expected) problems.push(`${state}: tier ${m.tier}, its width calls for ${m.expected}`)
            if (m.tier <= 3 && Math.abs(m.playOff) > 1) problems.push(`${state}: Play is ${m.playOff.toFixed(1)} px off the middle`)
          }
          if (!look.shuttling.speed || !look["marked, shuttling"].speed) problems.push("the shuttle's speed didn't show (the check couldn't shuttle)")
          if (!look.marked.marks) problems.push("the marks weren't set (the check couldn't mark)")
          if (!look["go to"].goto) problems.push("Go to didn't open")
          for (const [still, moving] of [
            ["idle", "shuttling"],
            ["marked", "marked, shuttling"],
          ]) {
            const a = look[still].readout
            const b = look[moving].readout
            if (a && b && (Math.abs(a.left - b.left) > 0.5 || Math.abs(a.right - b.right) > 0.5))
              problems.push(`the timecode moved ${(b.left - a.left).toFixed(1)} px as the speed showed (${still})`)
          }
          cases++
          const tag = `${w}×${h} ${panel ? "panel open  " : "panel closed"} row ${look.idle.width} px T${look.idle.tier} ${mode.padEnd(9)} ${frames.toLocaleString("en-GB").padStart(7)} frames`
          if (problems.length === 0) console.log(`ok    ${tag}`)
          else {
            failures++
            console.log(`FAIL  ${tag}\n      ${problems.join("\n      ")}`)
          }
        }
      }
    }
  }
} finally {
  c?.close()
  browser.kill()
  await server.close()
  // The browser lets go of its profile a moment after it ends.
  for (let i = 0; i < 20; i++) {
    try {
      rmSync(scratch, { recursive: true, force: true })
      break
    } catch {
      await sleep(250)
    }
  }
}

console.log(failures === 0 ? `\nAll ${cases} layouts pass.` : `\n${failures} of ${cases} layouts fail.`)
process.exit(failures === 0 ? 0 : 1)
