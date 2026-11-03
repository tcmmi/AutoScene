// Check 14: a page that keeps its thread busy (for the address's hash, in
// ms). AutoScene's own timers should keep running meanwhile.
;(async () => {
  const ms = Number(location.hash.slice(1)) || 4000
  await RT.report({ phase: "start", ms })
  await RT.wait(300)
  const start = Date.now()
  while (Date.now() - start < ms) {
    // Busy.
  }
  await RT.report({ phase: "done", ms: Date.now() - start })
})()
