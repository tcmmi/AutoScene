// The project page: it reads the project it is open in (project.get,
// exports.list, exports.get) and plays a file through files.resolve, which
// gives the page an address of its own, never the file's path.
const $ = (id) => document.getElementById(id)

/** A button in `list` that plays the file `ref` names. */
function add(page, list, label, ref) {
  const item = document.createElement("li")
  const button = document.createElement("button")
  button.className = "as-btn"
  button.dataset.variant = "ghost"
  button.dataset.size = "sm"
  button.textContent = label
  button.addEventListener("click", async () => {
    try {
      const file = await page.files.resolve(ref)
      $("player").src = file.url
      $("player").hidden = false
      $("playing").textContent = label
    } catch (e) {
      $("playing").textContent = e.message
    }
  })
  item.append(button)
  list.append(item)
}

AutoScene.connect()
  .then(async (page) => {
    const project = await page.project.get()
    $("title").textContent = project.title
    $("about").textContent = `${project.episodes.length} episodes, as AutoScene lists them.`
    for (const e of project.episodes) {
      if (!e.missing) add(page, $("episodes"), e.name, { kind: "episode", mediaId: e.id })
    }
    // Newest first.
    const exports = await page.exports.list()
    const last = exports[0]
    if (!last) return void ($("clips").textContent = "No export yet.")
    const export_ = await page.exports.get(last.id)
    for (const c of export_.clips) {
      if (c.status === "done") add(page, $("clips"), `${c.name} · ${c.episode}`, { kind: "clip", exportId: last.id, index: c.index })
    }
  })
  .catch((e) => {
    $("about").textContent = e.message
  })
