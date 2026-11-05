import { FilePlus, FilmStrip, FolderOpen, FolderPlus, Package, PencilSimple, Play, Plus, Trash, Users } from "@/components/icons"
import { useTranslation } from "react-i18next"
import type { ProjectSummary } from "@/bindings"
import type { MenuKit } from "@/components/ui/menu-actions"
import { useAppActions } from "@/lib/actions"
import { pickFolder, pickVideoFiles } from "@/lib/media"
import { reveal } from "@/lib/os"
import { useShortcutKeys } from "@/lib/shortcuts"
import { useNewProject } from "@/state/new-project"

/**
 * What a project offers wherever it is shown — its card, its rail tile, its
 * ⋯ menu: open it or one of its places, its folder, rename, remove.
 */
export function ProjectItems({
  kit,
  project: p,
  onRename,
  onDelete,
}: {
  kit: MenuKit
  project: ProjectSummary
  onRename: () => void
  onDelete: () => void
}) {
  const { t } = useTranslation("library")
  const actions = useAppActions()
  const { Item, Separator } = kit
  return (
    <>
      <Item onSelect={() => actions.goProject(p.id)}>
        <Play /> {t("project.open")}
      </Item>
      <Item onSelect={() => actions.goProject(p.id, "episodes")}>
        <FilmStrip /> {t(p.kind === "movie" ? "project.files" : "project.episodes")}
      </Item>
      <Item onSelect={() => actions.goProject(p.id, "cast")}>
        <Users /> {t("project.cast")}
      </Item>
      <Item onSelect={() => actions.goProject(p.id, "exports")}>
        <Package /> {t("project.exports")}
      </Item>
      <Separator />
      <Item onSelect={() => reveal(p.dir)}>
        <FolderOpen /> {t("project.showFolder")}
      </Item>
      <Item onSelect={onRename}>
        <PencilSimple /> {t("project.editDetails")}
      </Item>
      <Separator />
      <Item destructive onSelect={onDelete}>
        <Trash /> {t("project.remove")}
      </Item>
    </>
  )
}

/**
 * A new project: from nothing, from a season folder, from video files.
 * `page`: on the library's empty space, where "New project…" is already in
 * the app's own entries below.
 */
export function NewProjectItems({ kit, page = false }: { kit: MenuKit; page?: boolean }) {
  const { t } = useTranslation("library")
  const keys = useShortcutKeys()
  const { Item } = kit
  const show = (paths: string[]) => useNewProject.getState().show(paths)
  return (
    <>
      {!page && (
        <Item onSelect={() => show([])} shortcut={keys("global.newProject")}>
          <Plus /> {t("menu.newProject")}
        </Item>
      )}
      <Item onSelect={() => void pickFolder().then((p) => p && show([p]))}>
        <FolderPlus /> {t(page ? "menu.newFromFolderPage" : "menu.fromFolder")}
      </Item>
      <Item onSelect={() => void pickVideoFiles().then((files) => files.length > 0 && show(files))}>
        <FilePlus /> {t(page ? "menu.newFromFilesPage" : "menu.fromFiles")}
      </Item>
    </>
  )
}
