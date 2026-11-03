// Check 17: the SDK's guards stop the browser's own keys the page didn't
// take, and leave a field's editing keys alone.
;(async () => {
  await RT.page
  const press = (target, init) => {
    const e = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init })
    target.dispatchEvent(e)
    return e.defaultPrevented ? "prevented" : "not prevented"
  }
  await RT.report({
    f5: press(document.body, { key: "F5", code: "F5" }),
    ctrlP: press(document.body, { key: "p", code: "KeyP", ctrlKey: true }),
    ctrlF: press(document.body, { key: "f", code: "KeyF", ctrlKey: true }),
    ctrlC: press(document.getElementById("field"), { key: "c", code: "KeyC", ctrlKey: true }),
    mac: /Mac OS X|Macintosh/.test(navigator.userAgent),
  })
})()
