import { Link } from "@tanstack/react-router"
import { useTranslation } from "react-i18next"
import type { CatalogPlugin } from "@/bindings"
import { ArrowsClockwise, Copy, DownloadSimple, SlidersHorizontal } from "@/components/icons"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { CardMenuButton } from "@/components/ui/card-menu-button"
import { ContextArea } from "@/components/ui/context-area"
import { DropdownMenu, DropdownMenuContent } from "@/components/ui/dropdown-menu"
import { type MenuKit, contextKit, dropdownKit } from "@/components/ui/menu-actions"
import { TruncatedText } from "@/components/ui/tooltip"
import { useAppActions } from "@/lib/actions"
import { copyText } from "@/lib/os"
import { usePlugins } from "@/lib/queries"
import { getsPlugin, usePluginReview } from "@/state/plugin-review"
import { getFromCatalog, updatePlugin } from "./catalog-actions"
import { catalogAsks, catalogDownloads, catalogNewHosts } from "./catalog-items"
import { PluginArt } from "./PluginArt"

/** What a catalog entry offers here: to install, to update, nothing more, or why not. */
function offerOf(p: CatalogPlugin): "install" | "update" | "installed" | "elsewhere" | "newer" {
  if (p.needsNewerApp) return "newer"
  if (!p.runsHere) return "elsewhere"
  if (p.installed != null) return p.update ? "update" : "installed"
  return "install"
}

/**
 * A plugin the catalog offers, in the language of an installed plugin's
 * card: its icon as the art (the catalog carries no banners), else its
 * name. Under it AutoScene's caption: "Reviewed by AutoScene" (and
 * "Sandboxed" when it runs in the sandbox), who it is by and its version,
 * its summary, and what it may do in one line; then
 * Install…, Update…, or that it is installed. The art opens what the button
 * does, or the plugin's settings once it is installed. An installed plugin
 * AutoScene withdrew says "Withdrawn" in place of "Reviewed by AutoScene"
 * (the index may still list it, every version withdrawn), and nothing about
 * where it runs: its banner on Plugins says why. A plugin that names large
 * files says what its first run downloads, when it is to be installed. An
 * update says only the new host its files come from, when it brings one: the
 * review states the size, and the files may be here already.
 */
export function CatalogCard({ p }: { p: CatalogPlugin }) {
  const { t } = useTranslation("plugins")
  const offer = offerOf(p)
  const { data: plugins } = usePlugins()
  const withdrawn = p.installed != null && plugins?.find((x) => x.id === p.id)?.withdrawn != null
  // An install says how much its first run downloads. An update says only the host that is new: the review knows what is still to get.
  const downloads = withdrawn ? null : offer === "install" ? catalogDownloads(p) : offer === "update" ? catalogNewHosts(p) : null
  // Its package is being got, or waits its turn: the button spins, and
  // waits (a second click would only ask again).
  const getting = usePluginReview((s) => getsPlugin(s, p.id))
  const act = offer === "install" ? () => void getFromCatalog(p) : offer === "update" ? () => void updatePlugin(p) : undefined
  const art = (
    <PluginArt
      name={p.name}
      icon={p.icon}
      className="aspect-[2/1] rounded-2xl picture-edge transition-[box-shadow] duration-[var(--dur-med)] group-hover/art:shadow-lift"
    />
  )
  return (
    <ContextArea menu={<CatalogItems kit={contextKit} p={p} act={act} />}>
      <div role="group" aria-label={p.name}>
        <div className="group/art relative transition-transform duration-[var(--dur-med)] ease-out hover:-translate-y-1 has-[:focus-visible]:-translate-y-1">
          {p.installed != null ? (
            <Link to="/plugins/$id" params={{ id: p.id }} aria-label={t("catalog.settingsNamed", { name: p.name })} className="block rounded-2xl">
              {art}
            </Link>
          ) : (
            <button
              type="button"
              aria-label={offer === "install" ? t("catalog.installNamed", { name: p.name }) : p.name}
              disabled={act == null || getting}
              onClick={act}
              className="block w-full rounded-2xl text-left"
            >
              {art}
            </button>
          )}
          <DropdownMenu>
            <CardMenuButton over="picture" aria-label={t("options", { name: p.name })} />
            <DropdownMenuContent>
              <CatalogItems kit={dropdownKit} p={p} act={act} />
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
        <div className="mt-3 flex min-h-8 items-start justify-between gap-3 pl-0.5">
          <div className="flex min-h-8 min-w-0 flex-wrap items-center gap-x-1.5 gap-y-1">
            {withdrawn ? (
              <Badge tone="danger" className="min-w-0 truncate">
                {t("trust.withdrawn")}
              </Badge>
            ) : (
              <Badge className="min-w-0 truncate">{t("trust.reviewed")}</Badge>
            )}
            {/* Only what the index says runs in the sandbox; an entry without a runtime is a program. */}
            {p.runtime === "wasm" && (
              <Badge tone="outline" className="shrink-0">
                {t("sandboxed")}
              </Badge>
            )}
          </div>
          <Offer offer={offer} withdrawn={withdrawn} getting={getting} act={act} />
        </div>
        <TruncatedText className="mt-1.5 pl-0.5 text-sm text-fg-2">{`${t("by", { author: p.author })} · ${t("version", { version: p.version })}`}</TruncatedText>
        {/* The plugin's words, as its review says them: "In its own words". */}
        {p.summary && <p className="mt-1.5 line-clamp-2 pl-0.5 text-sm text-fg-1">{p.summary}</p>}
        <TruncatedText className="mt-1 pl-0.5 text-sm text-fg-3">{catalogAsks(p)}</TruncatedText>
        {downloads && <TruncatedText className="mt-1 pl-0.5 text-sm text-fg-3">{downloads}</TruncatedText>}
      </div>
    </ContextArea>
  )
}

/** The caption's right side: the button that gets it, or where it stands. */
function Offer({
  offer,
  withdrawn,
  getting,
  act,
}: {
  offer: ReturnType<typeof offerOf>
  withdrawn: boolean
  getting: boolean
  act?: () => void
}) {
  const { t } = useTranslation("plugins")
  // Withdrawn, only a version to update to is worth a word here.
  if (withdrawn && offer !== "update") return null
  switch (offer) {
    case "install":
    case "update":
      return (
        <Button size="sm" busy={getting} onClick={act}>
          {offer === "install" ? <DownloadSimple /> : <ArrowsClockwise />}
          {offer === "install" ? t("catalog.install") : t("catalog.update")}
        </Button>
      )
    case "installed":
      return <Badge tone="outline">{t("catalog.installed")}</Badge>
    case "elsewhere":
      return <span className="shrink-0 text-sm text-fg-3">{t("catalog.elsewhere")}</span>
    case "newer":
      return <span className="shrink-0 text-sm text-fg-3">{t("catalog.needsNewer")}</span>
  }
}

/** What a catalog card's ⋯ and its right-click offer, one list for both. */
function CatalogItems({ kit, p, act }: { kit: MenuKit; p: CatalogPlugin; act?: () => void }) {
  const { t } = useTranslation("plugins")
  const { goPlugin } = useAppActions()
  const { Item } = kit
  return (
    <>
      {act && (
        <Item onSelect={act}>
          {p.installed != null ? <ArrowsClockwise /> : <DownloadSimple />} {p.installed != null ? t("catalog.update") : t("catalog.install")}
        </Item>
      )}
      {p.installed != null && (
        <Item onSelect={() => goPlugin(p.id)}>
          <SlidersHorizontal /> {t("menu.settings")}
        </Item>
      )}
      {/* No "Show its source" yet: plugin_open_source opens only an installed
          plugin's own link, and a catalog card's must be the catalog's
          attested one (design-pr97 §4.5), which the backend doesn't build yet. */}
      <Item onSelect={() => void copyText(p.id, "plugin id")}>
        <Copy /> {t("menu.copyId")}
      </Item>
    </>
  )
}
