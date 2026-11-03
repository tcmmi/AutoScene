// @ts-check
/// <reference path="../.autoscene/types/global.d.ts" />
// {{^service}}
// The plugin's page: it keeps a note in what AutoScene keeps for the plugin
// (state.get, state.set), and says so in a toast (ui.toast), which AutoScene
// shows under the plugin's name.
// {{/service}}
// {{#service}}
// The plugin's page: it sends its text to the plugin's service
// (service.request "count", which main.py answers) and shows the answer.
// AutoScene starts the service as the page opens; a request waits for that.
// {{/service}}
// AutoScene serves /_as/sdk.js, which defines `AutoScene`. Your editor checks
// this file against the SDK's types in .autoscene/types.

/** @param {string} id */
const $ = (id) => /** @type {HTMLElement} */ (document.getElementById(id))
const text = /** @type {HTMLTextAreaElement} */ ($("text"))
const go = /** @type {HTMLButtonElement} */ ($("go"))

AutoScene.connect()
  .then(async (page) => {
    $("state").textContent = `${page.plugin.name} ${page.plugin.version}, open in AutoScene.`
    // {{^service}}
    const note = await page.state.get("note")
    text.value = typeof note === "string" ? note : ""
    go.addEventListener("click", async () => {
      await page.state.set("note", text.value)
      await page.ui.toast("Kept your note.")
    })
    // {{/service}}
    // {{#service}}
    page.service.onState((s) => {
      if (s.state === "starting") $("said").textContent = "Its service is starting…"
      else if (s.state === "ready") $("said").textContent = ""
      else $("said").textContent = s.message ?? "Its service stopped."
    })
    go.addEventListener("click", async () => {
      $("said").textContent = "Counting…"
      try {
        const answer = /** @type {{ words: number }} */ (await page.service.request("count", { text: text.value }))
        $("said").textContent = answer.words === 1 ? "1 word." : `${answer.words} words.`
      } catch (e) {
        $("said").textContent = e instanceof Error ? e.message : String(e)
      }
    })
    // {{/service}}
    text.disabled = false
    go.disabled = false
    page.onClose((reason) => {
      $("state").textContent = `AutoScene closed this page. ${reason}`
    })
  })
  .catch((e) => {
    $("state").textContent = e instanceof Error ? e.message : String(e)
  })
