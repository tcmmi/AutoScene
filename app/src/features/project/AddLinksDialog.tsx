import { useMemo } from "react"
import { useTranslation } from "react-i18next"
import { Dialog, DialogContent } from "@/components/ui/dialog"
import { LinkSourcesPanel } from "@/features/library/LinkSourcesPanel"
import { queueLinks } from "@/features/library/queue-links"
import { sourceItems } from "@/features/plugins/plugin-items"
import { usePlugins } from "@/lib/queries"

/** A project's Add links…: links a plugin's source downloads into it, each video joining it when done. */
export function AddLinksDialog({
  projectId,
  open,
  onOpenChange,
}: {
  projectId: string
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const { t } = useTranslation("library")
  const { data: plugins } = usePlugins()
  const sources = useMemo(() => sourceItems(plugins), [plugins])
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      {open && (
        <DialogContent
          title={t("links.dialogTitle")}
          description={t("links.dialogDescription")}
          className="w-[min(560px,calc(100vw-32px))]"
        >
          <LinkSourcesPanel
            className="p-0"
            sources={sources}
            onCancel={() => onOpenChange(false)}
            onAdd={(items) => {
              onOpenChange(false)
              void queueLinks(projectId, items)
            }}
          />
        </DialogContent>
      )}
    </Dialog>
  )
}
