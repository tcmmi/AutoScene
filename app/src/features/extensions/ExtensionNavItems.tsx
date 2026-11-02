import { useRouterState } from "@tanstack/react-router"
import { toast } from "sonner"
import { Button } from "@/components/ui/button"
import { Tooltip } from "@/components/ui/tooltip"
import { commands, errorMessage } from "@/lib/ipc"
import { getExtensionIcon } from "./icons"
import { useNavContributions } from "./registry"

export function ExtensionNavItems() {
  const pathname = useRouterState({ select: (s) => s.location.pathname })
  const navContributions = useNavContributions(pathname)

  if (navContributions.length === 0) return null

  const handleAction = async (extId: string, label: string) => {
    const toastId = toast.loading(`Executing ${label}…`)
    try {
      const res = await commands.extensionTest(extId)
      toast.success(res, { id: toastId })
    } catch (e) {
      toast.error(`Action failed: ${errorMessage(e)}`, { id: toastId })
    }
  }

  return (
    <div className="flex items-center gap-1.5 border-r border-hairline pr-2 mr-1">
      {navContributions.map(({ extension, button }) => {
        const IconComponent = getExtensionIcon(button.icon)
        return (
          <Tooltip key={`${extension.manifest.id}-${button.id}`} label={button.tooltip ?? button.label}>
            <Button
              variant="glass"
              size="sm"
              className="h-8 gap-1.5 px-2.5 text-xs font-medium text-fg-2 hover:text-fg-1"
              onClick={() => void handleAction(extension.manifest.id, button.label)}
            >
              <IconComponent className="size-3.5" />
              <span>{button.label}</span>
            </Button>
          </Tooltip>
        )
      })}
    </div>
  )
}
