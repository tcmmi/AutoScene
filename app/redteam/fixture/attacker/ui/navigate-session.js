// Check 7: the frame sends itself to the victim's session.
;(async () => {
  const info = await RT.info
  await RT.report({ target: info.victim.src })
  location.href = info.victim.src
})()
