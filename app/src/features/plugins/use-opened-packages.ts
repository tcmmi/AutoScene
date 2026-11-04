import { useEffect } from "react"
import { commands, events } from "@/lib/ipc"
import { reviewPackages } from "./review-packages"

/**
 * Takes the plugin packages opened from outside AutoScene (a double-click on
 * an `.asp` file, a start with one) and opens the install review of each, as
 * a drop does: the window says it reads the first, or that they wait for the
 * review open now, and a package opened again while it waits or is reviewed
 * is not reviewed twice. A path only opens the review: the backend checks it
 * again as it reads the package.
 */
export async function takeOpened(): Promise<void> {
  const paths = await commands.pluginsTakeOpened()
  if (paths.length > 0) reviewPackages(paths)
}

/**
 * Mounted in the shell: takes what was opened before the page loaded, and
 * what is opened later (`plugins:opened`).
 */
export function useOpenedPackages() {
  useEffect(() => {
    const take = () => void takeOpened().catch((e: unknown) => console.warn("taking opened plugin packages", e))
    const unlisten = events.pluginsOpened.listen(take)
    // Once listening, so one opened in between is taken by one or the other.
    void unlisten.then(take, take)
    return () => void unlisten.then((un) => un()).catch(() => undefined)
  }, [])
}
