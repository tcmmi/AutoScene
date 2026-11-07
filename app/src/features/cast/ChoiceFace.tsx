import { useTranslation } from "react-i18next"
import type { Character, RescanFace } from "@/bindings"
import { Avatar } from "@/components/ui/avatar"
import { characterLabel } from "./cast"
import { RescanFaceTile } from "./RescanFaceTile"

/** One of the two people a face could be: a click says it is them. */
function Side({ who, onPick }: { who: Character; onPick: () => void }) {
  const label = characterLabel(who)
  return (
    <button
      onClick={onPick}
      className="group/side flex min-w-0 flex-1 flex-col items-center gap-2 rounded-2xl p-3 outline-none transition-[background-color,transform] duration-[var(--dur-fast)] hover:bg-white/[0.05] focus-visible:ring-2 focus-visible:ring-accent-line active:scale-[0.98]"
    >
      <Avatar
        path={who.coverSmall ?? who.cover}
        className="size-16 ring-1 ring-white/15 transition-shadow group-hover/side:ring-2 group-hover/side:ring-accent"
      />
      <span className="display w-full truncate text-center text-xl text-fg-1">{label}</span>
    </button>
  )
}

/** A face that could be either of two people, between them: one click picks. */
export function ChoiceFace({
  pid,
  face,
  a,
  b,
  onPick,
  onNeither,
}: {
  pid: string
  face: RescanFace
  a: Character
  b: Character
  onPick: (who: Character) => void
  onNeither: () => void
}) {
  const { t } = useTranslation("cast")
  return (
    <div className="rounded-3xl border border-hairline bg-white/[0.02] p-4">
      <div className="flex items-center gap-2">
        <Side who={a} onPick={() => onPick(a)} />
        <RescanFaceTile pid={pid} face={face.face} className="w-28 shrink-0 rounded-2xl" />
        <Side who={b} onPick={() => onPick(b)} />
      </div>
      <button onClick={onNeither} className="mt-2 w-full text-center text-sm font-medium text-fg-3 hover:text-fg-1">
        {t("rescan.choice.neither")}
      </button>
    </div>
  )
}
