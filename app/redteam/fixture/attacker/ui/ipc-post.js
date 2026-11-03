// Check 3: the raw message posts each webview's IPC listens to, with the
// invoke key the frame can see (on Windows, the real one).
;(async () => {
  const key = await RT.stealKey()
  const message = (via) =>
    JSON.stringify({
      cmd: "redteam_pwned",
      callback: 1,
      error: 2,
      payload: { from: `ipc-post ${via}` },
      options: {},
      __TAURI_INVOKE_KEY__: key || "guess",
    })
  const out = { key: key ? "seen" : "not seen" }
  out.ipc = RT.post(() => window.ipc.postMessage(message("window.ipc")))
  out.chromeWebview = RT.post(() => window.chrome.webview.postMessage(message("chrome.webview")))
  out.webkit = RT.post(() => window.webkit.messageHandlers.ipc.postMessage(message("webkit")))
  // A post that got through has called redteam_pwned by then.
  await RT.wait(1500)
  await RT.report(out)
})()
