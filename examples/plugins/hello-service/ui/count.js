// The page of a service plugin: it sends its text to the service
// (service.request "count"), shows the answer, and lists what the service
// tells every open page of its plugin (its "counted" events). AutoScene
// starts the service as the page opens; a request waits for that start.
const $ = (id) => document.getElementById(id)

/** `n` words, as a sentence says it. */
const words = (n) => (n === 1 ? "1 word" : `${n} words`)

/** What the page says of its service: starting, ready, or stopped and why. */
function showState(s) {
  if (s.state === "starting") $("state").textContent = s.message ? `Its service is starting: ${s.message}` : "Its service is starting…"
  else if (s.state === "ready") $("state").textContent = "Its service is ready."
  else $("state").textContent = s.message ?? "Its service stopped."
}

AutoScene.connect()
  .then((page) => {
    $("state").textContent = page.plugin.name
    page.service.onState(showState)
    page.service.on("counted", (data) => {
      const item = document.createElement("li")
      item.textContent = `Counted ${words(data.words)}, ${words(data.total)} since it started.`
      $("said").prepend(item)
    })
    for (const id of ["text", "count"]) $(id).disabled = false

    $("count").addEventListener("click", async () => {
      $("words").textContent = "Counting…"
      try {
        const answer = await page.service.request("count", { text: $("text").value })
        $("words").textContent = `${words(answer.words)}.`
      } catch (e) {
        $("words").textContent = e.message
      }
    })
    page.onClose((reason) => {
      $("state").textContent = `AutoScene closed this page. ${reason}`
    })
  })
  .catch((e) => {
    $("state").textContent = e.message
  })
