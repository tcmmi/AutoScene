// Check 6: the frame sends itself to the app's own page, outside and to
// the kit (the target's index is the address's hash).
;(async () => {
  const info = await RT.info
  const target = info.targets[Number(location.hash.slice(1))]
  await RT.report({ target })
  location.href = target
})()
