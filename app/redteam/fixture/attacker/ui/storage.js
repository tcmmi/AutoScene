// Check 5: storage of its own. An opaque origin has none.
;(async () => {
  const out = {}
  out.localStorage = await RT.attempt(() => {
    localStorage.setItem("redteam", "1")
    return localStorage.getItem("redteam")
  })
  out.sessionStorage = await RT.attempt(() => {
    sessionStorage.setItem("redteam", "1")
    return sessionStorage.getItem("redteam")
  })
  out.indexedDB = await RT.attempt(
    () =>
      new Promise((resolve, reject) => {
        const open = indexedDB.open("redteam")
        open.onsuccess = () => resolve("opened")
        open.onerror = () => reject(open.error)
      }),
  )
  out.cookie = await RT.attempt(() => {
    document.cookie = "redteam=1"
    return document.cookie
  })
  out.caches = await RT.attempt(() => caches.open("redteam").then(() => "opened"))
  await RT.report(out)
})()
