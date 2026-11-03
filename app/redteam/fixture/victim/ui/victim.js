// The victim's page: it connects, and says who AutoScene told it it is.
AutoScene.connect({ timeoutMs: 8000 }).then((page) =>
  page.call("redteam.report", { who: "victim", plugin: page.plugin.id, name: page.plugin.name }),
)
