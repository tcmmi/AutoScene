// What every attack page shares, as the global RT: its page (the SDK's
// connect(), answered by the red team's harness as AutoScene would), what
// the red team tells it (`RT.info`: where its listeners are, the victim's
// session, where `navigate` goes), what the page's CSP reported, and the
// report each page sends when it is done. A classic script, as a plugin
// page's own are, loaded after /_as/sdk.js.
;(() => {
  const violations = []
  const seen = new WeakSet()
  const onViolation = (e) => {
    if (seen.has(e)) return
    seen.add(e)
    violations.push(`${e.effectiveDirective || e.violatedDirective} ${e.blockedURI || "inline"}`)
  }
  window.addEventListener("securitypolicyviolation", onViolation, true)
  document.addEventListener("securitypolicyviolation", onViolation, true)
  const page = AutoScene.connect({ timeoutMs: 8000 })
  const info = page.then((p) => p.call("redteam.info", {}))
  const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
  const late = {}

  /** What `fn` gives within `ms`: "ok: …", "threw: <name>" or "timed out". */
  async function attempt(fn, ms = 3000) {
    try {
      const value = await Promise.race([Promise.resolve().then(fn), wait(ms).then(() => late)])
      return value === late ? "timed out" : `ok: ${String(value).slice(0, 120)}`
    } catch (e) {
      return `threw: ${(e && e.name) || String(e)}`
    }
  }

  /** What a call that only sends gives: "sent", or "threw: <name>". */
  function post(fn) {
    try {
      fn()
      return "sent"
    } catch (e) {
      return `threw: ${(e && e.name) || String(e)}`
    }
  }

  /**
   * Tauri's invoke key, where Tauri's script is in this frame (WebView2
   * puts it in every frame): the script hands the key to the page's own
   * fetch, and after that fails to window.ipc, so wrapping both shows it.
   * Nothing is sent while they are wrapped.
   */
  async function stealKey() {
    const internals = window.__TAURI_INTERNALS__
    if (!internals || typeof internals.invoke !== "function") return null
    let key = null
    const fetch = window.fetch
    window.fetch = (_url, init) => {
      key = key || new Headers(init && init.headers).get("Tauri-Invoke-Key")
      return Promise.reject(new TypeError("kept"))
    }
    const ipc = window.ipc
    const ipcPost = ipc && ipc.postMessage
    if (ipc) {
      ipc.postMessage = (data) => {
        try {
          key = key || JSON.parse(data).__TAURI_INVOKE_KEY__
        } catch {
          // Not a message Tauri made.
        }
      }
    }
    internals.invoke("redteam_key", {}).catch(() => {})
    await wait(300)
    window.fetch = fetch
    if (ipc) ipc.postMessage = ipcPost
    return key
  }

  async function report(out) {
    const p = await page
    await p.call("redteam.report", { who: "attacker", ...out, violations: violations.slice() })
  }

  window.RT = { page, info, wait, attempt, post, stealKey, report, violations }
})()
