import { toast } from "sonner"
import { ContextMenuItem } from "@/components/ui/context-menu"
import { commands, errorMessage } from "@/lib/ipc"
import { getExtensionIcon } from "./icons"
import { useRailContributions } from "./registry"
import { RailButton } from "@/components/shell/Rail"

export function ExtensionRailItems() {
  const railContributions = useRailContributions()

  if (railContributions.length === 0) return null

  const handleAction = async (extId: string, label: string) => {
    const toastId = toast.loading(`Running ${label}…`)
    try {
      const res = await commands.extensionTest(extId)
      toast.success(res, { id: toastId })
    } catch (e) {
      toast.error(`Action failed: ${errorMessage(e)}`, { id: toastId })
    }
  }

  return (
    <>
      {railContributions.map(({ extension, button }) => {
        const IconComponent = getExtensionIcon(button.icon)
        return (
          <RailButton
            key={`${extension.manifest.id}-${button.id}`}
            icon={<IconComponent />}
            label={button.label}
            onClick={() => void handleAction(extension.manifest.id, button.label)}
            menu={
              <ContextMenuItem onSelect={() => void handleAction(extension.manifest.id, button.label)}>
                <IconComponent /> {button.label}
              </ContextMenuItem>
            }
          />
        )
      })}
    </>
  )
}

