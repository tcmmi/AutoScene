// Check 15: with theme.css the page is dark, so its frame doesn't paint a
// white canvas of its own over AutoScene's.
;(async () => {
  await RT.page
  await RT.report({
    colorScheme: getComputedStyle(document.documentElement).colorScheme,
    background: getComputedStyle(document.body).backgroundColor,
  })
})()
