import { useRef, useState } from "react"
import { useTranslation } from "react-i18next"
import type { Character } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogFooter } from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { castEdit, characterLabel } from "./cast"

export function RenameCharacterDialog({
  pid,
  character,
  open,
  onOpenChange,
}: {
  pid: string
  character: Character | null
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  if (!open || !character) return null

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <RenameForm pid={pid} character={character} onDone={() => onOpenChange(false)} />
    </Dialog>
  )
}

function RenameForm({
  pid,
  character,
  onDone,
}: {
  pid: string
  character: Character
  onDone: () => void
}) {
  const { t } = useTranslation(["cast", "common"])
  const [name, setName] = useState(character.name ?? "")
  const [busy, setBusy] = useState(false)
  const inputRef = useRef<HTMLInputElement>(null)

  const save = async () => {
    const trimmed = name.trim()
    if (trimmed === (character.name ?? "")) {
      onDone()
      return
    }
    setBusy(true)
    try {
      const res = await castEdit(pid, {
        op: "rename",
        characterId: character.id,
        name: trimmed || null,
      })
      if (res !== undefined) {
        onDone()
      }
    } finally {
      setBusy(false)
    }
  }

  return (
    <DialogContent
      title={t("rename.title", { name: characterLabel(character) })}
      description={t("rename.description")}
    >
      <form
        onSubmit={(e) => {
          e.preventDefault()
          void save()
        }}
        className="space-y-4"
      >
        <Input
          ref={inputRef}
          autoFocus
          value={name}
          placeholder={characterLabel(character)}
          onChange={(e) => setName(e.target.value)}
          aria-label={t("rename.nameLabel")}
          disabled={busy}
        />
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={onDone} disabled={busy}>
            {t("common:action.cancel")}
          </Button>
          <Button type="submit" variant="primary" disabled={busy}>
            {t("rename.save")}
          </Button>
        </DialogFooter>
      </form>
    </DialogContent>
  )
}
