// The rail page: it keeps a note in what AutoScene keeps for the plugin
// (state.set, state.get), and says hello in a toast (ui.toast), which
// AutoScene shows as "Plugin “Hello page”: …".
const $ = (id) => document.getElementById(id)

AutoScene.connect()
  .then(async (page) => {
    $("about").textContent = `${page.plugin.name} ${page.plugin.version}, open in AutoScene.`
    const note = (await page.state.get("note")) ?? ""
    $("note").value = typeof note === "string" ? note : ""
    for (const id of ["note", "save", "hello"]) $(id).disabled = false

    $("save").addEventListener("click", async () => {
      await page.state.set("note", $("note").value)
      $("kept").textContent = "Kept."
    })
    $("hello").addEventListener("click", () => page.ui.toast("Hello from a plugin page."))
    page.onClose((reason) => {
      $("about").textContent = `AutoScene closed this page. ${reason}`
    })
  })
  .catch((e) => {
    $("about").textContent = e.message
  })
