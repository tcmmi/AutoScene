// Check 13: the frame goes to its own SVG, which holds an inline script.
;(async () => {
  await RT.report({ target: "inline.svg" })
  location.href = "inline.svg"
})()
