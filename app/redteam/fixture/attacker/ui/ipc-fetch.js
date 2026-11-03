// Check 2: the IPC's own scheme, fetched with the invoke key the frame can
// see (on Windows, the real one).
;(async () => {
  const key = await RT.stealKey()
  const out = { key: key ? "seen" : "not seen" }
  const headers = {
    "Content-Type": "application/json",
    "Tauri-Callback": "1",
    "Tauri-Error": "2",
    "Tauri-Invoke-Key": key || "guess",
  }
  for (const url of ["http://ipc.localhost/redteam_pwned", "ipc://localhost/redteam_pwned"]) {
    out[url] = await RT.attempt(async () => {
      const r = await fetch(url, { method: "POST", body: JSON.stringify({ from: `ipc-fetch ${url}` }), headers })
      return `${r.status} ${(await r.text()).slice(0, 80)}`
    })
  }
  await RT.report(out)
})()
