import { useTranslation } from "react-i18next"
import { FolderOpen, Plus } from "@/components/icons"
import { ContextMenuItem } from "@/components/ui/context-menu"
import type { usePluginInstall } from "./install-plugin"

/** What the Plugins page and its rail tile offer: install a plugin from its file, and load a folder while that is allowed. */
export function InstallItems({ flow }: { flow: ReturnType<typeof usePluginInstall> }) {
  const { t } = useTranslation("plugins")
  return (
    <>
      <ContextMenuItem disabled={flow.checking != null} onSelect={flow.install}>
        <Plus /> {t("installFromFile")}
      </ContextMenuItem>
      {flow.allowed && (
        <ContextMenuItem disabled={flow.checking != null} onSelect={flow.loadFolder}>
          <FolderOpen /> {t("loadFolder")}
        </ContextMenuItem>
      )}
    </>
  )
}
