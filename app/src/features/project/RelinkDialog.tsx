import { useState } from "react"
import { useTranslation } from "react-i18next"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogFooter } from "@/components/ui/dialog"
import { say } from "@/lib/i18n"
import { baseName, mediaTitle } from "@/lib/media"
import { useRelink } from "@/state/relink"
import { relinkAnyway } from "./relink"

/** Asks before a file that doesn't look like the missing episode takes its place. */
export function RelinkDialog() {
  const { t } = useTranslation(["project", "common"])
  const question = useRelink((s) => s.question)
  const close = useRelink((s) => s.close)
  const [busy, setBusy] = useState(false)
  return (
    <Dialog open={question != null} onOpenChange={(o) => !o && !busy && close()}>
      {question && (
        <DialogContent
          title={t("relink.questionTitle", { file: baseName(question.path), episode: mediaTitle(question.media) })}
          description={t("relink.questionDescription", { reason: say(question.reason) })}
        >
          <DialogFooter>
            <Button variant="ghost" disabled={busy} onClick={close}>
              {t("common:action.cancel")}
            </Button>
            <Button
              variant="primary"
              busy={busy}
              onClick={() => {
                setBusy(true)
                void relinkAnyway(question.pid, question.media, question.path).finally(() => {
                  setBusy(false)
                  // Only this question: another may have come meanwhile.
                  if (useRelink.getState().question === question) close()
                })
              }}
            >
              {t("relink.useAnyway")}
            </Button>
          </DialogFooter>
        </DialogContent>
      )}
    </Dialog>
  )
}
