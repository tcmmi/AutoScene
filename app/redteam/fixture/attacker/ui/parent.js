// Check 4: the window around the frame: AutoScene's document, its address
// and Tauri's internals, and sending it somewhere else.
;(async () => {
  const info = await RT.info
  const out = {}
  out.parentDocument = await RT.attempt(() => typeof parent.document.body)
  out.topHref = await RT.attempt(() => top.location.href)
  out.parentInternals = await RT.attempt(() => typeof parent.__TAURI_INTERNALS__)
  out.topLocation = await RT.attempt(() => {
    top.location = `${info.wall}/top`
    return "set"
  })
  await RT.wait(1000)
  await RT.report(out)
})()
