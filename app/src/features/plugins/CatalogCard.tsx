import { Link } from "@tanstack/react-router"
import type { CatalogPlugin } from "@/bindings"
import { ArrowsClockwise, CircleNotch, Copy, DotsThree, DownloadSimple, SlidersHorizontal } from "@/components/icons"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { ContextArea } from "@/components/ui/context-area"
import { DropdownMenu, DropdownMenuContent, DropdownMenuTrigger } from "@/components/ui/dropdown-menu"
import { type MenuKit, contextKit, dropdownKit } from "@/components/ui/menu-actions"
import { TruncatedText } from "@/components/ui/tooltip"
import { useAppActions } from "@/lib/actions"
import { copyText } from "@/lib/os"
import { cn } from "@/lib/utils"
import { getsPlugin, usePluginReview } from "@/state/plugin-review"
import { getFromCatalog, updatePlugin } from "./catalog-actions"
import { catalogAsks } from "./catalog-items"
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
 * name. Under it AutoScene's caption: "Reviewed by AutoScene", who it is by
 * and its version, its summary, and what it may do in one line; then
 * Install…, Update…, or that it is installed. The art opens what the button
 * does, or the plugin's settings once it is installed.
 */
export function CatalogCard({ p }: { p: CatalogPlugin }) {
  const offer = offerOf(p)
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
            <Link to="/plugins/$id" params={{ id: p.id }} aria-label={`${p.name} settings`} className="block rounded-2xl">
              {art}
            </Link>
          ) : (
            <button
              type="button"
              aria-label={offer === "install" ? `Install ${p.name}…` : p.name}
              disabled={act == null || getting}
              onClick={act}
              className="block w-full rounded-2xl text-left disabled:cursor-default"
            >
              {art}
            </button>
          )}
          <DropdownMenu>
            <DropdownMenuTrigger
              aria-label={`${p.name} options`}
              className={cn(
                "absolute top-3 right-3 flex size-8 items-center justify-center rounded-full border border-white/15 bg-black/40 text-white opacity-0 backdrop-blur-md transition-opacity",
                "group-hover/art:opacity-100 hover:bg-black/60 focus-visible:opacity-100 data-[state=open]:opacity-100",
              )}
            >
              <DotsThree className="size-5" weight="bold" />
            </DropdownMenuTrigger>
            <DropdownMenuContent>
              <CatalogItems kit={dropdownKit} p={p} act={act} />
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
        <div className="mt-3 flex min-h-8 items-center justify-between gap-3 pl-0.5">
          <Badge className="min-w-0 truncate">Reviewed by AutoScene</Badge>
          <Offer offer={offer} getting={getting} act={act} />
        </div>
        <TruncatedText className="mt-1.5 pl-0.5 text-sm text-fg-2">{`By ${p.author} · version ${p.version}`}</TruncatedText>
        {/* The plugin's words, as its review says them: "In its own words". */}
        {p.summary && <p className="mt-1.5 line-clamp-2 pl-0.5 text-sm text-fg-1">{p.summary}</p>}
        <TruncatedText className="mt-1 pl-0.5 text-sm text-fg-3">{catalogAsks(p)}</TruncatedText>
      </div>
    </ContextArea>
  )
}

/** The caption's right side: the button that gets it, or where it stands. */
function Offer({ offer, getting, act }: { offer: ReturnType<typeof offerOf>; getting: boolean; act?: () => void }) {
  switch (offer) {
    case "install":
    case "update":
      return (
        <Button size="sm" disabled={getting} onClick={act}>
          {getting ? (
            <CircleNotch className="animate-spin" />
          ) : offer === "install" ? (
            <DownloadSimple />
          ) : (
            <ArrowsClockwise />
          )}
          {offer === "install" ? "Install…" : "Update…"}
        </Button>
      )
    case "installed":
      return <Badge tone="outline">Installed</Badge>
    case "elsewhere":
      return <span className="shrink-0 text-sm text-fg-3">Not for this computer</span>
    case "newer":
      return <span className="shrink-0 text-sm text-fg-3">Needs a newer AutoScene</span>
  }
}

/** What a catalog card's ⋯ and its right-click offer, one list for both. */
function CatalogItems({ kit, p, act }: { kit: MenuKit; p: CatalogPlugin; act?: () => void }) {
  const { goPlugin } = useAppActions()
  const { Item } = kit
  return (
    <>
      {act && (
        <Item onSelect={act}>
          {p.installed != null ? <ArrowsClockwise /> : <DownloadSimple />} {p.installed != null ? "Update…" : "Install…"}
        </Item>
      )}
      {p.installed != null && (
        <Item onSelect={() => goPlugin(p.id)}>
          <SlidersHorizontal /> Its settings
        </Item>
      )}
      {/* No "Show its source" yet: plugin_open_source opens only an installed
          plugin's own link, and a catalog card's must be the catalog's
          attested one (design-pr97 §4.5), which the backend doesn't build yet. */}
      <Item onSelect={() => void copyText(p.id, "plugin id")}>
        <Copy /> Copy its id
      </Item>
    </>
  )
}
