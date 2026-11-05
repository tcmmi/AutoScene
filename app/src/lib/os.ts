/**
 * The few things the app asks of the system: copying text (natively — the
 * webview's clipboard API needs a user gesture or a permission prompt),
 * reading the clipboard's text for Paste, and showing a file or folder in
 * Explorer / Finder. Failures say so in a toast.
 */
import { toast } from "sonner"
import { t } from "./i18n"
import { commands, errorMessage } from "./ipc"

/** What a copy can be of: each kind has its own whole sentences in the catalog (`shell:copy`). */
const COPIED = {
  timecode: "timecode",
  timecodes: "timecodes",
  "file path": "filePath",
  "folder path": "folderPath",
  path: "path",
  error: "error",
  // i18n-ignore: the name of a catalog key, not words
  message: "message",
  "plugin id": "pluginId",
} as const
export type CopyKind = keyof typeof COPIED

/**
 * Copy `text`. With `what` ("timecode", "file path") a toast says what was
 * copied; without it the copy is quiet, like a text field's.
 */
export async function copyText(text: string, what?: CopyKind): Promise<boolean> {
  const kind = what ? COPIED[what] : undefined // a kind cast in from outside the set copies quietly
  try {
    await commands.clipboardWrite(text)
    if (kind) toast(t(`shell:copy.copied.${kind}`), { description: text.length > 96 ? `${text.slice(0, 93)}…` : text })
    return true
  } catch (e) {
    toast.error(kind ? t(`shell:copy.failed.${kind}`) : t("shell:copy.failed.other"), { description: errorMessage(e) })
    return false
  }
}

/** The clipboard's text, or null (a picture, files, nothing, no access). */
export async function readClipboard(): Promise<string | null> {
  try {
    return await commands.clipboardRead()
  } catch {
    return null
  }
}

/** Show a file or folder in Explorer / Finder; a toast when it isn't there. */
export function reveal(path: string): void {
  commands.revealPath(path).catch((e) => toast.error(t("shell:os.revealFailed"), { description: errorMessage(e) }))
}

/** Open a URL in the user's default web browser. */
export async function openExternalUrl(url: string): Promise<void> {
  try {
    const { openUrl } = await import("@tauri-apps/plugin-opener")
    await openUrl(url)
  } catch (e) {
    toast.error(t("shell:os.linkFailed"), { description: errorMessage(e) })
  }
}
