import { toast } from "sonner"
import type { LinkItem } from "@/bindings"
import { say, t } from "@/lib/i18n"
import { commands, errorMessage } from "@/lib/ipc"

/**
 * Queues the downloads of `links` into the project `projectId`, and says how
 * it went: how many download, those waiting already, and why any couldn't be
 * added. Never throws: the project is there whatever the downloads do.
 */
export async function queueLinks(projectId: string, links: LinkItem[]): Promise<void> {
  try {
    const done = await commands.sourceLinksQueue(
      projectId,
      links.map((l) => l.link),
    )
    if (done.queued > 0) {
      toast.success(t("library:links.queued", { count: done.queued }), { description: t("library:links.queuedNote") })
    } else if (done.waiting > 0) {
      toast(t("library:links.waiting"))
    }
    if (done.refused.length > 0) {
      const why = done.refused.map((r) => `${r.short || r.text}: ${r.refused ? say(r.refused) : ""}`).join("\n")
      toast.error(t("library:links.refused", { count: done.refused.length }), { description: why })
    }
  } catch (e) {
    toast.error(t("library:links.queueFailed"), { description: errorMessage(e) })
  }
}
