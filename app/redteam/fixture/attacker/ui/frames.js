// Check 11: frames and plugins inside the page. A child that loads says so
// to the page; none may. about:blank is measured: it keeps the page's
// sandbox and CSP.
;(async () => {
  // After the load: the page's body is there, and the SDK's ready is out.
  await RT.page
  const ran = []
  window.addEventListener("message", (e) => {
    if (e.data && e.data.type === "redteam:child") ran.push(e.data.how)
  })
  const add = (tag, props) => document.body.appendChild(Object.assign(document.createElement(tag), props))
  const out = {}
  const blank = add("iframe", { src: "about:blank" })
  await RT.wait(300)
  out.blank = await RT.attempt(() => {
    const d = blank.contentDocument
    if (!d) return "no document"
    const s = d.createElement("script")
    s.textContent = "parent.postMessage({ type: 'redteam:child', how: 'blank' }, '*')"
    d.body.append(s)
    let storage = "storage open"
    try {
      void blank.contentWindow.localStorage.length
    } catch (e) {
      storage = `storage ${e.name}`
    }
    return `reachable, ${storage}`
  })
  add("iframe", { srcdoc: "<script>parent.postMessage({ type: 'redteam:child', how: 'srcdoc' }, '*')<\/script>" })
  add("iframe", { src: "child.html#iframe" })
  add("object", { data: "child.html#object", type: "text/html" })
  add("embed", { src: "child.html#embed", type: "text/html" })
  await RT.wait(2000)
  out.ran = ran
  await RT.report(out)
})()
