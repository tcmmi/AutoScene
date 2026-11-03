// Check 1: Tauri's invoke, from inside the frame. WebView2 puts Tauri's
// scripts in every frame, WebKit only in the main one.
;(async () => {
  const internals = window.__TAURI_INTERNALS__
  const out = { internals: typeof internals }
  out.invoke =
    internals && typeof internals.invoke === "function"
      ? await RT.attempt(() => internals.invoke("redteam_pwned", { from: "ipc-invoke" }), 3000)
      : "no invoke here"
  await RT.report(out)
})()
