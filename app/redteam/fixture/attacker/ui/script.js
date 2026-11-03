// Check 10: scripts the page didn't ship as its own files (inline, eval,
// Function, the victim's file), and its own modules: import() from this
// classic script, a module tag, and import() from that module.
;(async () => {
  const info = await RT.info
  const out = { inline: window.inlineRan === true ? "ran" : "blocked" }
  out.eval = await RT.attempt(() => eval("1 + 1"))
  out.function = await RT.attempt(() => new Function("return 2")())
  out.victimScript = await new Promise((resolve) => {
    const s = document.createElement("script")
    s.src = `${info.victim.root}ui/mark.js`
    s.onload = () => resolve(window.victimMarkRan === true ? "ran" : "loaded, didn't run")
    s.onerror = () => resolve("blocked")
    document.head.append(s)
  })
  out.import = await RT.attempt(() => import("./imported.js").then((m) => m.ok))
  await RT.wait(300)
  out.moduleTag = window.moduleTagRan === true ? "ran" : "not run"
  out.moduleImport = window.moduleImport || "not run"
  await RT.report(out)
})()
