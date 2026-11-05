import { useTranslation } from "react-i18next"
import { toast } from "sonner"
import type { PluginInfo } from "@/bindings"
import { ArrowCounterClockwise, ArrowSquareOut, ArrowsClockwise, Copy, FileText, FolderOpen, Scan, SlidersHorizontal, Trash } from "@/components/icons"
import type { MenuKit } from "@/components/ui/menu-actions"
import { useAppActions } from "@/lib/actions"
import { t as tNow } from "@/lib/i18n"
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
 * Update… first while the catalog offers a newer version (`onUpdate`).
 */
export function PluginItems({
  kit,
  p,
  settings = true,
  onLoadAgain,
  onUninstall,
  onUpdate,
}: {
  kit: MenuKit
  p: PluginInfo
  settings?: boolean
  onLoadAgain?: () => void
  onUninstall?: () => void
  onUpdate?: () => void
}) {
  const { t } = useTranslation("plugins")
  const { goPlugin } = useAppActions()
  const allowed = useSettings().data?.allowUnreviewedPlugins ?? false
  const { Item, Separator } = kit
  const source = sourceLabel(p)
  const folder = p.source === "folder"
  return (
    <>
      {onUpdate && (
        <Item onSelect={onUpdate}>
          <ArrowsClockwise /> {t("menu.update")}
        </Item>
      )}
      {settings && (
        <Item onSelect={() => goPlugin(p.id)}>
          <SlidersHorizontal /> {t("menu.settings")}
        </Item>
      )}
      {source && (
        <Item onSelect={() => void openSource(p)}>
          <ArrowSquareOut /> {source}
        </Item>
      )}
      <Item onSelect={() => reveal(p.folder)}>
        <FolderOpen /> {t("menu.showFolder")}
      </Item>
      <Item onSelect={() => reveal(p.logPath)}>
        <FileText /> {t("menu.showLog")}
      </Item>
      <Item onSelect={() => void copyText(p.id, "plugin id")}>
        <Copy /> {t("menu.copyId")}
      </Item>
      {folder && p.broken == null && (
        <Item onSelect={() => void checkFolder(p)}>
          <Scan /> {t("menu.checkFolder")}
        </Item>
      )}
      {folder && p.broken != null && allowed && onLoadAgain && (
        <Item onSelect={onLoadAgain}>
          <ArrowCounterClockwise /> {t("menu.loadFolderAgain")}
        </Item>
      )}
      {onUninstall && (
        <>
          <Separator />
          <Item destructive onSelect={onUninstall}>
            <Trash /> {t("menu.uninstall")}
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
    toast.error(tNow("plugins:menu.checkFailed"), { description: errorMessage(e) })
    return
  }
  // Its settings may have new defaults.
  void queryClient.invalidateQueries({ queryKey: qk.pluginSettings(p.id) })
  const now = queryClient.getQueryData<PluginInfo[]>(qk.plugins)?.find((x) => x.id === p.id)
  toast.success(now != null && JSON.stringify(now) === JSON.stringify(p) ? tNow("plugins:menu.folderUnchanged") : tNow("plugins:menu.folderChanged"))
}
