/**
 * The browser's own actions inside the app's window — the find bar,
 * printing, saving the page, reloading, caret browsing, a link's download or
 * new window, the middle button's autoscroll — stopped before they show (the
 * kit's guards, `plugin-kit/keys`, which plugin pages get through the SDK
 * too). The app always goes first: only a key none of its handlers took is
 * cancelled, and editing keys stay with text fields.
 *
 * Zoom (Ctrl + wheel, Ctrl ±, pinch) and the browser's menu are already off
 * (Tauri's window settings; `AppMenu`).
 */
import { installGuards } from "@/plugin-kit/keys"
import { isMac } from "./platform"

/** Install the guards on `win`; returns what removes them. */
export function installBrowserGuards({
  dev,
  mac = isMac,
  win = window,
}: {
  dev: boolean
  mac?: boolean
  win?: Window
}): () => void {
  return installGuards({ dev, mac, win })
}
