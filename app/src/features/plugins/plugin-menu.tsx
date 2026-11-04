import { toast } from "sonner"
import type { PluginInfo } from "@/bindings"
import { ArrowCounterClockwise, ArrowSquareOut, Copy, FileText, FolderOpen, Scan, SlidersHorizontal, Trash } from "@/components/icons"
import type { MenuKit } from "@/components/ui/menu-actions"
import { useAppActions } from "@/lib/actions"
import { errorMessage } from "@/lib/ipc"
import { copyText, reveal } from "@/lib/os"
import { qk, queryClient, useSettings } from "@/lib/queries"
import { openSource } from "./plugin-actions"
import { sourceLabel } from "./plugin-items"

/**
 * What a plugin's ⋯ and its right-click list offer, one list for both
 * (`kit`). Its settings only where they aren't open already (`settings`):
 * each plugin has them, its files and what it may use at least. A loaded
 * folder is checked for changes while it works, and loaded again through
 * its review while it is broken (`onLoadAgain`, where that review can show)
 * and unreviewed plugins are allowed: the backend refuses that review while
 * they are not. Uninstall only where it can be asked (`onUninstall`).
 */
export function PluginItems({
  kit,
  p,
  settings = true,
  onLoadAgain,
  onUninstall,
}: {
  kit: MenuKit
  p: PluginInfo
  settings?: boolean
  onLoadAgain?: () => void
  onUninstall?: () => void
}) {
  const { goPlugin } = useAppActions()
  const allowed = useSettings().data?.allowUnreviewedPlugins ?? false
  const { Item, Separator } = kit
  const source = sourceLabel(p)
  const folder = p.source === "folder"
  return (
    <>
      {settings && (
        <Item onSelect={() => goPlugin(p.id)}>
          <SlidersHorizontal /> Its settings
        </Item>
      )}
      {source && (
        <Item onSelect={() => void openSource(p)}>
          <ArrowSquareOut /> {source}
        </Item>
      )}
      <Item onSelect={() => reveal(p.folder)}>
        <FolderOpen /> Show its folder
      </Item>
      <Item onSelect={() => reveal(p.logPath)}>
        <FileText /> Show its log
      </Item>
      <Item onSelect={() => void copyText(p.id, "plugin id")}>
        <Copy /> Copy its id
      </Item>
      {folder && p.broken == null && (
        <Item onSelect={() => void checkFolder(p)}>
          <Scan /> Check its folder for changes
        </Item>
      )}
      {folder && p.broken != null && allowed && onLoadAgain && (
        <Item onSelect={onLoadAgain}>
          <ArrowCounterClockwise /> Load its folder again…
        </Item>
      )}
      {onUninstall && (
        <>
          <Separator />
          <Item destructive onSelect={onUninstall}>
            <Trash /> Uninstall…
          </Item>
        </>
      )}
    </>
  )
}

/**
 * Reads the loaded folder of `p` anew (the list reads its plugin.json), and
 * says what came of it: a click that changes nothing still answers.
 */
async function checkFolder(p: PluginInfo) {
  try {
    await queryClient.refetchQueries({ queryKey: qk.plugins, exact: true }, { throwOnError: true })
  } catch (e) {
    toast.error("Couldn't check its folder", { description: errorMessage(e) })
    return
  }
  // Its settings may have new defaults.
  void queryClient.invalidateQueries({ queryKey: qk.pluginSettings(p.id) })
  const now = queryClient.getQueryData<PluginInfo[]>(qk.plugins)?.find((x) => x.id === p.id)
  toast.success(now != null && JSON.stringify(now) === JSON.stringify(p) ? "Its folder is as it was" : "Read the changes in its folder")
}
