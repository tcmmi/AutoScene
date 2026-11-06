import { useTranslation } from "react-i18next"
import { characterLabel } from "@/features/cast/cast"
import { mediaTitle } from "@/lib/media"
import { useCast, useMediaList, useProject } from "@/lib/queries"

/** Names of the views for the window title; they follow renames live. */
export function ProjectCrumb({ pid }: { pid: string }) {
  const { t } = useTranslation("project")
  const { data } = useProject(pid)
  return <>{data?.title ?? t("crumbs.project")}</>
}

export function MediaCrumb({ pid, mid }: { pid: string; mid: string }) {
  const { t } = useTranslation("project")
  const { data } = useMediaList(pid)
  const m = data?.find((x) => x.id === mid)
  return <>{m ? mediaTitle(m) : t("crumbs.episode")}</>
}

export function PackCrumb({ pid, cid }: { pid: string; cid: string }) {
  const { t } = useTranslation("project")
  const { data } = useCast(pid)
  const c = data?.characters.find((x) => x.id === cid)
  return <>{t("crumbs.pack", { name: c ? characterLabel(c) : t("crumbs.character") })}</>
}

export function CharacterCrumb({ pid, cid }: { pid: string; cid: string }) {
  const { t } = useTranslation("project")
  const { data } = useCast(pid)
  if (cid === "unassigned") return <>{t("crumbs.everyoneElse")}</>
  const c = data?.characters.find((x) => x.id === cid)
  return <>{t("crumbs.cast", { name: c ? characterLabel(c) : t("crumbs.character") })}</>
}
