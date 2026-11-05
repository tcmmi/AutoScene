// Check 12: the handshake. The page posts a fake init to itself before
// AutoScene's comes, plants fake inits with ports of its own in every
// frame beside it (the harness holds the victim's real one back for this),
// and says ready five times. It counts the inits it got and what came in
// on its own ports.
;(() => {
  const fake = (name) => ({
    type: "autoscene:init",
    v: 1,
    init: {
      plugin: { id: "dev.redteam.fake", name, version: "6.6.6" },
      page: { id: "main", title: "Fake", scope: "global" },
      projectId: null,
      path: "",
      theme: { v: 1, scheme: "dark" },
      permissions: ["project:read"],
      hosts: [],
      service: false,
      protocol: 2,
      app: { version: "0.0.0", platform: "windows", locale: "en" },
    },
  })
  const stolen = []
  const own = new MessageChannel()
  own.port1.onmessage = (m) => stolen.push(m.data)
  // Before AutoScene's init, which comes only after this document has loaded.
  window.postMessage(fake("Self"), "*", [own.port2])
  let inits = 0
  window.addEventListener("message", (e) => {
    if (e.source === parent && e.data && e.data.type === "autoscene:init") inits++
  })
  const plant = () => {
    for (let i = 0; i < parent.frames.length; i++) {
      const channel = new MessageChannel()
      channel.port1.onmessage = (m) => stolen.push(m.data)
      try {
        parent.frames[i].postMessage(fake("Sibling"), "*", [channel.port2])
      } catch {
        // A frame that went away.
      }
    }
  }
  const planting = setInterval(plant, 100)
  window.addEventListener("load", () => {
    // The SDK says ready once; four more, after it.
    for (let i = 1; i <= 4; i++) setTimeout(() => parent.postMessage({ type: "autoscene:ready", v: 1 }, "*"), 100 * i)
  })
  ;(async () => {
    const page = await RT.page
    await RT.wait(2500)
    clearInterval(planting)
    await RT.wait(500)
    await RT.report({ plugin: page.plugin.id, inits, stolen: stolen.length })
  })()
})()
