import type * as React from "react"
import { useState } from "react"
import { toast } from "sonner"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogFooter } from "@/components/ui/dialog"
import { errorMessage } from "@/lib/ipc"
import { useTranslation } from "react-i18next"

/**
 * Asks before something goes: Cancel and one answer, with whatever goes with
 * the answer (`children`) above them. While `onConfirm` runs the dialog holds:
 * Esc, a click outside, Cancel do nothing, the × is gone and `children` are inert, so
 * nothing is left running behind a closed dialog. Done, it closes. Failed, it
 * stays open for another try and a toast says why: `failure`, then the
 * error's own words.
 */
export function ConfirmDialog({
  open,
  onOpenChange,
  title,
  description,
  answer,
  icon,
  failure,
  onConfirm,
  children,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: React.ReactNode
  description?: React.ReactNode
  /** The answer's words. */
  answer: React.ReactNode
  /** The icon in front of them. */
  icon?: React.ReactNode
  /** The toast's title if `onConfirm` fails: "Couldn't remove the project". */
  failure: string
  onConfirm: () => Promise<unknown>
  children?: React.ReactNode
}) {
  const { t } = useTranslation("common")
  const [busy, setBusy] = useState(false)
  const confirm = async () => {
    setBusy(true)
    try {
      await onConfirm()
    } catch (e) {
      toast.error(failure, { description: errorMessage(e) })
      setBusy(false)
      return
    }
    setBusy(false)
    onOpenChange(false)
  }
  return (
    <Dialog open={open} onOpenChange={(next) => !busy && onOpenChange(next)}>
      <DialogContent title={title} description={description} hideClose={busy}>
        {children && (
          <div inert={busy} className="contents">
            {children}
          </div>
        )}
        <DialogFooter>
          <Button variant="ghost" disabled={busy} onClick={() => onOpenChange(false)}>
            {t("action.cancel")}
          </Button>
          <Button variant="danger" busy={busy} onClick={() => void confirm()}>
            {icon} {answer}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
