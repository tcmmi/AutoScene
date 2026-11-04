import { FolderOpen, Plus } from "@/components/icons"
import { ContextMenuItem } from "@/components/ui/context-menu"
import type { usePluginInstall } from "./install-plugin"

/** What the Plugins page and its rail tile offer: install a plugin, and load a folder while that is allowed. */
export function InstallItems({ flow }: { flow: ReturnType<typeof usePluginInstall> }) {
  return (
    <>
      <ContextMenuItem disabled={flow.checking != null} onSelect={flow.install}>
        <Plus /> Install a plugin…
      </ContextMenuItem>
      {flow.allowed && (
        <ContextMenuItem disabled={flow.checking != null} onSelect={flow.loadFolder}>
          <FolderOpen /> Load a folder…
        </ContextMenuItem>
      )}
    </>
  )
}
