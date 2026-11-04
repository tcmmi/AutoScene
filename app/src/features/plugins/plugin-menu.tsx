import type { PluginInfo } from "@/bindings"
import { ArrowSquareOut, FileText, FolderOpen, SlidersHorizontal, Trash } from "@/components/icons"
import type { MenuKit } from "@/components/ui/menu-actions"
import { useAppActions } from "@/lib/actions"
import { reveal } from "@/lib/os"
import { openSource } from "./plugin-actions"
import { sourceLabel } from "./plugin-items"

/**
 * What a plugin's ⋯ and its right-click list offer, one list for both
 * (`kit`). Its settings only where they aren't open already (`settings`):
 * each plugin has them, its files and what it may use at least. Uninstall
 * only where it can be asked (`onUninstall`).
 */
export function PluginItems({
  kit,
  p,
  settings = true,
  onUninstall,
}: {
  kit: MenuKit
  p: PluginInfo
  settings?: boolean
  onUninstall?: () => void
}) {
  const { goPlugin } = useAppActions()
  const { Item, Separator } = kit
  const source = sourceLabel(p)
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
