/*
 * The red team's harness: AutoScene's side of a plugin page, played as the
 * app plays it (B5.1), and the judge of each check (B8.2). It mounts one
 * attack page after another in a frame with frame.json's attributes, binds
 * each load of the frame with one init, answers the page's two red-team
 * calls (`redteam.info`, `redteam.report`) and refuses every other, and
 * asks the Rust side what reached it while the check ran: a call into
 * Tauri, or a request the wall listener heard, fails the check whatever
 * the page said. A connection to the wall that sent nothing is noted.
 *
 * Loaded in a frame instead of as the main page, it says so (check 6's
 * canary) and does nothing else. Loaded with ?recovered, the window came
 * back after it was sent home while a page was busy (check 14).
 */
"use strict"

const tauri = window.__TAURI_INTERNALS__
const invoke = (cmd, args = {}) => tauri.invoke(cmd, args)
const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

/** What the Rust side starts the harness with (`Setup`). */
let setup = null
/** The frames mounted now, each with AutoScene's side of its bridge. */
const hosts = new Set()
/** The types of window messages from a window that no mounted frame is. */
const strays = []

const CHECKS = [
  { n: 1, name: "ipc-invoke", kind: "held", run: ipcInvoke },
  { n: 2, name: "ipc-fetch", kind: "held", run: ipcFetch },
  { n: 3, name: "ipc-post", kind: "held", run: ipcPost },
  { n: 4, name: "parent", kind: "held", run: parentWindow },
  { n: 5, name: "storage", kind: "held", run: storage },
  { n: 6, name: "navigate", kind: "held", run: navigate },
  { n: 7, name: "navigate-session", kind: "measured", run: navigateSession },
  { n: 8, name: "exfil", kind: "held", run: exfil },
  { n: 9, name: "side-channels", kind: "measured", run: sideChannels },
  { n: 10, name: "script", kind: "held", run: script },
  { n: 11, name: "frames", kind: "held", run: frames },
  { n: 12, name: "handshake", kind: "held", run: handshake },
  { n: 13, name: "svg-doc", kind: "held", run: svgDoc },
  { n: 14, name: "busy", kind: "measured", run: busy },
  { n: 15, name: "scheme", kind: "held", run: scheme },
  { n: 16, name: "headers", kind: "held", run: () => invoke("redteam_headers") },
  { n: 17, name: "guards", kind: "held", run: guards },
]

if (window.top !== window) {
  window.parent.postMessage({ type: "redteam:canary", href: location.href }, "*")
} else if (new URLSearchParams(location.search).has("recovered")) {
  invoke("redteam_recovered")
} else {
  window.addEventListener("message", onWindowMessage)
  run().catch((e) => invoke("redteam_done", { report: { userAgent: navigator.userAgent, strays, error: String(e) } }))
}

async function run() {
  setup = await invoke("redteam_setup")
  document.documentElement.style.setProperty("--bg-canvas", setup.canvas)
  document.getElementById("engine").textContent = navigator.userAgent
  await control()
  const list = document.getElementById("checks")
  for (const check of CHECKS) {
    const item = list.appendChild(document.createElement("li"))
    item.textContent = check.name
    item.dataset.state = "running"
    await invoke("redteam_begin", { check: check.name })
    let found
    try {
      found = await check.run()
    } catch (e) {
      found = { held: false, notes: [`The check failed: ${e}`] }
    }
    const seen = await invoke("redteam_seen", { check: check.name })
    const notes = [...(found.notes || [])]
    if (seen.pwned.length) notes.push(`A page reached Tauri: ${JSON.stringify(seen.pwned)}`)
    if (seen.wall.length) notes.push(`The wall listener heard: ${seen.wall.join(", ")}`)
    if (seen.opened.length) notes.push(`${count(seen.opened.length, "connection")} to the wall sent no request`)
    const held = found.held === true && !seen.pwned.length && !seen.wall.length
    await invoke("redteam_record", {
      check: { n: check.n, name: check.name, kind: check.kind, held, notes, measured: found.measured || {}, seen },
    })
    item.dataset.state = held ? check.kind : "fell"
  }
  // Check 14's second half: the window is sent home while a page is busy,
  // and the run ends when it is back.
  await invoke("redteam_begin", { check: "busy" })
  const host = mountFrame(await invoke("redteam_mount", { plugin: "attacker" }), "ui/busy.html#8000")
  await report(host)
  await invoke("redteam_done", { report: { userAgent: navigator.userAgent, strays, recover: true } })
}

/**
 * The control of check 3: this page's own raw IPC post, made as the attack
 * page makes it, with the key Tauri's script hands to this page's fetch.
 * That it reaches Tauri shows the post is a way in from the main frame.
 */
async function control() {
  const fetch = window.fetch
  let key = null
  window.fetch = (url, init) => {
    key = key || new Headers(init && init.headers).get("Tauri-Invoke-Key")
    return fetch(url, init)
  }
  try {
    await invoke("redteam_begin", { check: "start" })
  } finally {
    window.fetch = fetch
  }
  if (!key || !window.ipc) return
  const callback = tauri.transformCallback(() => {}, true)
  const error = tauri.transformCallback(() => {}, true)
  window.ipc.postMessage(
    JSON.stringify({ cmd: "redteam_control", callback, error, payload: {}, options: {}, __TAURI_INVOKE_KEY__: key }),
  )
  await wait(300)
}

/** The frame a window message came from: a ready binds it, anything else is noted. */
function onWindowMessage(e) {
  const type = e.data && typeof e.data.type === "string" ? e.data.type : typeof e.data
  for (const host of hosts) {
    if (e.source !== host.frame.contentWindow) continue
    if (type === "autoscene:ready" && e.data.v === 1) {
      host.readys++
      ready(host)
    } else {
      host.others.push(type)
    }
    return
  }
  strays.push(type)
}

/**
 * A frame for `mount`'s session at its `path`, with frame.json's
 * attributes exactly, and AutoScene's side of its bridge. With `holdInit`,
 * its init goes out that many ms after its ready (check 12).
 */
function mountFrame(mount, path, { holdInit = 0 } = {}) {
  const frame = document.createElement("iframe")
  frame.setAttribute("sandbox", setup.frame.sandbox)
  frame.setAttribute("allow", setup.frame.allow)
  frame.setAttribute("referrerpolicy", setup.frame.referrerPolicy)
  frame.title = mount.page.title
  const host = {
    mount,
    frame,
    holdInit,
    loads: 0,
    readys: 0,
    bound: 0,
    inits: 0,
    early: false,
    port: null,
    reports: [],
    read: 0,
    others: [],
    keys: [],
  }
  frame.addEventListener("load", () => {
    host.loads++
    if (host.early && host.bound < host.loads) {
      host.early = false
      bind(host)
    }
  })
  hosts.add(host)
  frame.src = mount.root + path
  document.getElementById("stage").append(frame)
  return host
}

/**
 * A ready from the frame of `host`: bound when this load of the frame has
 * no init yet (one per load, B5.1). A load's ready may come before the
 * frame's load event or after it, so one that finds this load bound
 * already waits for the next load.
 */
function ready(host) {
  if (host.bound < Math.max(host.loads, 1)) bind(host)
  else host.early = true
}

/** Binds the frame of `host`: a new port, the last one closed, and an init with it. */
function bind(host) {
  host.bound++
  if (host.port) host.port.close()
  const { port1, port2 } = new MessageChannel()
  host.port = port1
  port1.onmessage = (m) => onPort(host, port1, m.data)
  const init = {
    type: "autoscene:init",
    v: 1,
    init: {
      plugin: host.mount.plugin,
      page: host.mount.page,
      projectId: null,
      path: "",
      theme: { v: 1, scheme: "dark" },
      permissions: [],
      hosts: [],
      service: false,
      protocol: 2,
      app: { version: setup.version, platform: setup.platform },
    },
  }
  const send = () => {
    host.inits++
    if (host.frame.contentWindow) host.frame.contentWindow.postMessage(init, "*", [port2])
  }
  if (host.holdInit) setTimeout(send, host.holdInit)
  else send()
}

/** A message on the port of `host`: the red team's two calls are answered, every other call is refused. */
function onPort(host, port, m) {
  if (typeof m !== "object" || m === null) return
  if (m.t === "key") {
    const mods = ["ctrl", "meta", "alt", "shift"].filter((k) => m[k]).map((k) => `${k}+`)
    host.keys.push(mods.join("") + m.key)
    return
  }
  if (m.t !== "call") return
  const reply = (body) => port.postMessage({ t: "reply", id: m.id, ...body })
  if (m.method === "redteam.report") {
    host.reports.push(m.params)
    return reply({ ok: true, result: null })
  }
  if (m.method === "redteam.info") {
    const { wall, side, udp, victim, targets } = setup
    return reply({ ok: true, result: { wall, side, udp, victim, targets } })
  }
  reply({ ok: false, error: { code: "denied", message: "The red team answers only its own calls." } })
}

/** Takes the frame of `host` away, and closes its session. */
function unmount(host) {
  hosts.delete(host)
  if (host.port) host.port.close()
  host.frame.remove()
  return invoke("redteam_unmount", { session: host.mount.session })
}

/** The next report of the page of `host`, or null after `ms`. */
async function report(host, ms = 8000) {
  const until = Date.now() + ms
  while (host.reports.length <= host.read) {
    if (Date.now() > until) return null
    await wait(50)
  }
  return host.reports[host.read++]
}

/**
 * Mounts the attacker's `path`, takes its first `count` reports (as
 * `got`, each within `ms`), waits `settle` ms more and takes the frame
 * away; gives its host.
 */
async function attack(path, { count = 1, settle = 500, ms = 10000 } = {}) {
  const host = mountFrame(await invoke("redteam_mount", { plugin: "attacker" }), path)
  host.got = []
  for (let i = 0; i < count; i++) {
    const r = await report(host, ms)
    if (!r) break
    host.got.push(r)
  }
  await wait(settle)
  await unmount(host)
  return host
}

/** `n` of `what`, as words: "1 connection", "2 connections". */
const count = (n, what) => `${n} ${what}${n === 1 ? "" : "s"}`

/** A check whose page never reported. */
const silent = () => ({ held: false, notes: ["The page never reported."] })

/** Whether an attempt's outcome means it worked. */
const worked = (outcome) => String(outcome).startsWith("ok")

async function ipcInvoke() {
  const [r] = (await attack("ui/ipc-invoke.html")).got
  if (!r) return silent()
  return { held: true, notes: [`__TAURI_INTERNALS__ ${r.internals}`, `invoke ${r.invoke}`] }
}

async function ipcFetch() {
  const [r] = (await attack("ui/ipc-fetch.html")).got
  if (!r) return silent()
  const urls = ["http://ipc.localhost/redteam_pwned", "ipc://localhost/redteam_pwned"]
  const answered = urls.filter((u) => worked(r[u]))
  return {
    held: answered.length === 0,
    notes: [
      `the key ${r.key}`,
      ...urls.map((u) => `${u} ${r[u]}`),
      ...answered.map((u) => `The CSP let ${u} through`),
      `${r.violations.length} CSP reports`,
    ],
  }
}

async function ipcPost() {
  const [r] = (await attack("ui/ipc-post.html")).got
  if (!r) return silent()
  const { controls } = await invoke("redteam_seen", { check: "ipc-post" })
  return {
    held: true,
    notes: [
      `the key ${r.key}`,
      `window.ipc ${r.ipc}`,
      `chrome.webview ${r.chromeWebview}`,
      `webkit.messageHandlers ${r.webkit}`,
      `the main page's own raw post reached Tauri: ${controls ? "yes" : "no"}`,
    ],
    measured: { controls },
  }
}

async function parentWindow() {
  const [r] = (await attack("ui/parent.html")).got
  if (!r) return silent()
  const reads = ["parentDocument", "topHref", "parentInternals"]
  return {
    held: !reads.some((k) => worked(r[k])),
    notes: [...reads.map((k) => `${k} ${r[k]}`), `top.location = … ${r.topLocation}`],
  }
}

async function storage() {
  const [r] = (await attack("ui/storage.html")).got
  if (!r) return silent()
  const kinds = ["localStorage", "sessionStorage", "indexedDB", "cookie", "caches"]
  // A cookie that didn't stick is no storage either.
  const kept = kinds.filter((k) => worked(r[k]) && (k !== "cookie" || String(r[k]).includes("redteam=1")))
  return { held: kept.length === 0, notes: kinds.map((k) => `${k} ${r[k]}`) }
}

async function navigate() {
  let held = true
  const notes = []
  for (let i = 0; i < setup.targets.length; i++) {
    const target = setup.targets[i]
    const before = (await invoke("redteam_seen", { check: "navigate" })).opened.length
    const host = await attack(`ui/navigate.html#${i}`, { settle: 2500 })
    const opened = (await invoke("redteam_seen", { check: "navigate" })).opened.length - before
    const readys = host.readys - 1
    const canaries = host.others.filter((t) => t === "redteam:canary").length
    if (!host.got.length) {
      held = false
      notes.push(`${target}: the page never reported`)
    } else if (readys > 0 || canaries > 0) {
      held = false
      notes.push(`${target}: ${readys} more pages and ${canaries} of the app's loaded in the frame`)
    } else {
      const socket = opened ? `, a connection opened to it and sent nothing` : ""
      notes.push(`${target}: nothing ran (${host.loads} loads${socket})`)
    }
  }
  return { held, notes }
}

async function navigateSession() {
  const host = await attack("ui/navigate-session.html", { count: 2 })
  if (!host.got.length) return silent()
  const victim = host.got.find((r) => r.who === "victim")
  const attacker = host.mount.plugin.id
  if (!victim) return { held: true, notes: ["The victim's page didn't load in the frame."], measured: { loaded: false } }
  return {
    held: victim.plugin === attacker,
    notes: [
      victim.plugin === attacker
        ? "The victim's page loaded in the attacker's frame, bound to the attacker's session: nothing gained."
        : `The victim's page loaded in the attacker's frame and was told it is ${victim.plugin}.`,
    ],
    measured: { loaded: true, boundTo: victim.plugin },
  }
}

async function exfil() {
  const host = await attack("ui/exfil.html", { count: 2, settle: 1500, ms: 15000 })
  const [r, then] = host.got
  if (!r) return silent()
  const tried = [r, then || {}].flatMap((o) => Object.entries(o)).filter(([k]) => k !== "who" && k !== "violations")
  const after = then
    ? `the page stayed (${then.violations.length} CSP reports in all)`
    : `the page was gone after the link and the form (${host.loads} loads)`
  return { held: true, notes: [...tried.map(([k, v]) => `${k} ${v}`), `${r.violations.length} CSP reports`, after] }
}

async function sideChannels() {
  const [r] = (await attack("ui/side-channels.html", { settle: 1500 })).got
  if (!r) return silent()
  const seen = await invoke("redteam_seen", { check: "side-channels" })
  return {
    held: true,
    notes: [`link hints: ${seen.side.join(", ") || "nothing"}`, `WebRTC ${r.webrtc}, ${seen.udp.length} datagrams`],
    measured: { side: seen.side, udp: seen.udp.length, webrtc: r.webrtc },
  }
}

async function script() {
  const [r] = (await attack("ui/script.html")).got
  if (!r) return silent()
  const ran = []
  if (r.inline !== "blocked") ran.push("an inline script")
  if (worked(r.eval)) ran.push("eval")
  if (worked(r.function)) ran.push("new Function")
  if (r.victimScript === "ran") ran.push("the victim's script")
  return {
    held: ran.length === 0,
    notes: [
      `inline ${r.inline}`,
      `eval ${r.eval}`,
      `Function ${r.function}`,
      `the victim's script ${r.victimScript}`,
      `import() of its own ${r.import}`,
      `its own module tag ${r.moduleTag}`,
      `import() in that module ${r.moduleImport}`,
      `${r.violations.length} CSP reports`,
      ...ran.map((x) => `${x} ran`),
    ],
    measured: { import: r.import, moduleTag: r.moduleTag, moduleImport: r.moduleImport },
  }
}

async function frames() {
  const [r] = (await attack("ui/frames.html")).got
  if (!r) return silent()
  const loaded = r.ran.filter((how) => how !== "blank")
  return {
    held: loaded.length === 0,
    notes: [`about:blank ${r.blank}`, `ran in a child: ${r.ran.join(", ") || "nothing"}`],
    measured: { blank: r.blank, blankRan: r.ran.includes("blank") },
  }
}

async function handshake() {
  const mine = await invoke("redteam_mount", { plugin: "attacker" })
  const theirs = await invoke("redteam_mount", { plugin: "victim" })
  // The victim's init comes late, so the attacker's fake ones come first.
  const victim = mountFrame(theirs, theirs.src.slice(theirs.root.length), { holdInit: 1500 })
  const attacker = mountFrame(mine, "ui/handshake.html")
  const [a, v] = await Promise.all([report(attacker, 10000), report(victim, 10000)])
  await wait(500)
  await Promise.all([unmount(attacker), unmount(victim)])
  const wrong = []
  if (!a) wrong.push("The attacker never reported.")
  else {
    if (a.plugin !== mine.plugin.id) wrong.push(`The attacker's SDK took the init of ${a.plugin}.`)
    if (a.inits !== 1) wrong.push(`The attacker got ${a.inits} inits.`)
    if (a.stolen > 0) wrong.push(`${a.stolen} messages came in on the attacker's own ports.`)
  }
  if (attacker.inits !== 1) wrong.push(`AutoScene sent ${attacker.inits} inits for ${attacker.readys} readys.`)
  if (!v) wrong.push("The victim never reported to AutoScene.")
  else if (v.plugin !== theirs.plugin.id) wrong.push(`The victim's SDK took the init of ${v.plugin}.`)
  return {
    held: wrong.length === 0,
    notes: wrong.length
      ? wrong
      : [`${attacker.readys} readys, one init; the init to itself and those planted in the victim were ignored`],
  }
}

async function svgDoc() {
  const host = await attack("ui/svg-doc.html", { settle: 2000 })
  if (!host.got.length) return silent()
  const ran = host.others.includes("redteam:svg")
  return {
    held: !ran,
    notes: [ran ? "The SVG's inline script ran." : `Its inline script didn't run (${host.loads} loads).`],
  }
}

async function busy() {
  let worst = 0
  let last = performance.now()
  const timer = setInterval(() => {
    const now = performance.now()
    worst = Math.max(worst, now - last)
    last = now
  }, 50)
  const host = mountFrame(await invoke("redteam_mount", { plugin: "attacker" }), "ui/busy.html#4000")
  const start = await report(host)
  const done = start && (await report(host, 15000))
  await wait(300)
  clearInterval(timer)
  await unmount(host)
  if (!done) return silent()
  const gap = Math.round(worst)
  return {
    held: true,
    notes: [`AutoScene's 50 ms timer, while the page was busy for ${done.ms} ms: worst gap ${gap} ms`],
    measured: { worstGapMs: gap, busyMs: done.ms },
  }
}

async function scheme() {
  const host = mountFrame(await invoke("redteam_mount", { plugin: "attacker" }), "ui/scheme.html")
  const r = await report(host)
  let pixels = null
  let unread = null
  if (r && setup.platform === "windows") {
    await wait(300)
    // Nine points in the lower half of the frame, where the page drew nothing,
    // as the window draws them.
    const box = host.frame.getBoundingClientRect()
    const points = []
    for (const fx of [0.25, 0.5, 0.75]) {
      for (const dy of [24, 64, 104]) {
        points.push([box.left + box.width * fx, box.bottom - dy].map((v) => Math.round(v * devicePixelRatio)))
      }
    }
    try {
      pixels = await invoke("redteam_pixels", { points })
    } catch (e) {
      unread = String(e)
    }
  }
  await unmount(host)
  if (!r) return silent()
  const notes = [`color-scheme ${r.colorScheme}`, `body ${r.background}`]
  let held = r.colorScheme === "dark"
  if (setup.platform === "windows") {
    const canvas = pixels ? pixels.filter((p) => near(p, setup.canvas)).length : 0
    held = held && canvas === 9
    const seen = [...new Set(pixels || [])].join(", ")
    notes.push(unread || `${canvas} of 9 points in the frame's empty area are AutoScene's canvas ${setup.canvas} (${seen})`)
  }
  return { held, notes, measured: { pixels } }
}

/** Whether the colours `a` and `b` (#rrggbb) are the same, give or take rounding. */
function near(a, b) {
  const rgb = (c) => [1, 3, 5].map((i) => parseInt(c.slice(i, i + 2), 16))
  const [x, y] = [rgb(a), rgb(b)]
  return x.every((v, i) => Math.abs(v - y[i]) <= 4)
}

async function guards() {
  const host = await attack("ui/guards.html")
  const [r] = host.got
  if (!r) return silent()
  // On macOS the SDK leaves keys alone by design: ⌘ keys belong to the
  // app's menu there, and WKWebView has no browser keys (keys.ts).
  const want = r.mac ? "not prevented" : "prevented"
  const wrong = ["f5", "ctrlP", "ctrlF"].filter((k) => r[k] !== want)
  if (r.ctrlC !== "not prevented") wrong.push("ctrlC")
  return {
    held: wrong.length === 0,
    notes: [
      `F5 ${r.f5}`,
      `Ctrl+P ${r.ctrlP}`,
      `Ctrl+F ${r.ctrlF}`,
      `Ctrl+C in a field ${r.ctrlC}`,
      `forwarded to AutoScene: ${host.keys.join(", ") || "none"}`,
    ],
  }
}
