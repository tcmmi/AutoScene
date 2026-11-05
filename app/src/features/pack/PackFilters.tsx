import type * as React from "react"
import { ArrowCounterClockwise, CaretDown, FunnelSimple, Warning } from "@/components/icons"
import type { MediaItem, PackFilter, PackOrder, PackSpec, PluginInfo, ShotSize } from "@/bindings"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { ProgressRing } from "@/components/ui/progress"
import { Segmented } from "@/components/ui/segmented"
import { Tooltip, TruncatedText } from "@/components/ui/tooltip"
import { plural } from "@/features/cast/cast"
import { PluginIcon } from "@/features/plugins/PluginIcon"
import { mediaTitle } from "@/lib/media"
import { cn } from "@/lib/utils"
import { DEFAULT_SPEC, HANDLES, LENGTHS, LIMITS, SIZES, episodesOn, filtersOn, isDefault, lengthOf, pluginFiltersOf, withPluginFilter } from "./spec"

/**
 * The pack's order, its filters in one menu and the handles, in a bar that
 * stays under the top bar while the shots scroll. The menu stays open while
 * filters are chosen, so several take one visit; a plugin's filters stay
 * chips of their own, because each can have something to say (a ring, a
 * warning mark).
 */
export function PackFilters({
  spec,
  media,
  faceless,
  filters = NO_FILTERS,
  plugins = NO_PLUGINS,
  onChange,
}: {
  spec: PackSpec
  media: MediaItem[]
  /** The pack has shots where the character's face doesn't show. */
  faceless: boolean
  /** The plugins' shot filters the pack can show: every one of a plugin that is on, and those `spec` turns on. */
  filters?: PackFilter[]
  /** The installed plugins: their marks. */
  plugins?: PluginInfo[]
  onChange: (spec: PackSpec) => void
}) {
  const set = (patch: Partial<PackSpec>) => onChange({ ...spec, ...patch })
  const toggleSize = (s: ShotSize) =>
    set({ sizes: spec.sizes.includes(s) ? spec.sizes.filter((x) => x !== s) : SIZES.map((x) => x.value).filter((x) => x === s || spec.sizes.includes(x)) })
  const length = lengthOf(spec)
  const narrowed = episodesOn(spec, media)
  const episodes = narrowed ? spec.media.filter((id) => media.some((m) => m.id === id)).length : media.length
  const count = filtersOn(spec, media)

  return (
    <div
      role="group"
      aria-label="Filters"
      className="sticky top-[var(--topbar-h)] z-10 flex flex-wrap items-center gap-2 border-b border-hairline bg-canvas/80 px-10 py-3 backdrop-blur-2xl backdrop-saturate-150"
    >
      <Segmented<PackOrder>
        aria-label="Order"
        value={spec.order}
        onChange={(order) => set({ order })}
        options={[
          { value: "episodes", label: "Episode order" },
          { value: "best", label: "Best first" },
        ]}
      />
      <Divider />
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Chip pressed={count > 0} aria-haspopup="menu" aria-pressed={undefined} aria-label={count > 0 ? `Filters, ${count} on` : "Filters"}>
            <FunnelSimple className="size-4" /> Filters
            {count > 0 && (
              <span className="flex h-4 min-w-4 items-center justify-center rounded-full bg-accent px-1 text-2xs font-semibold tabular-nums text-on-accent">
                {count}
              </span>
            )}
            <CaretDown className="size-3.5 opacity-70" />
          </Chip>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="w-[300px]">
          {/* No header of its own: the names say it, and the menu fits the window's smallest height (1024x640) at rest. */}
          <div role="group" aria-label="Shot sizes">
            {SIZES.map((s) => (
              <DropdownMenuCheckboxItem key={s.value} checked={spec.sizes.includes(s.value)} onCheckedChange={() => toggleSize(s.value)}>
                {s.label}
              </DropdownMenuCheckboxItem>
            ))}
          </div>
          <DropdownMenuSeparator />
          <Choice label="Length" value={length.label} active={spec.minSec != null || spec.maxSec != null}>
            <DropdownMenuRadioGroup
              value={length.id ?? ""}
              onValueChange={(id) => {
                const l = LENGTHS.find((x) => x.id === id)
                if (l) set({ minSec: l.min, maxSec: l.max })
              }}
            >
              {LENGTHS.map((l) => (
                <DropdownMenuRadioItem key={l.id} value={l.id} onSelect={stayOpen}>
                  {l.label}
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
          </Choice>
          {media.length > 1 && (
            <Choice label="Episodes" value={narrowed ? `${episodes} of ${media.length}` : "All"} active={narrowed}>
              <DropdownMenuCheckboxItem checked={!narrowed} disabled={!narrowed} onCheckedChange={() => set({ media: [] })}>
                All episodes
              </DropdownMenuCheckboxItem>
              <DropdownMenuSeparator />
              {/* A long series scrolls here, so "All episodes" stays in reach above. */}
              <div className="max-h-[min(420px,60vh)] overflow-y-auto">
                {media.map((m) => {
                  const picked = !narrowed || spec.media.includes(m.id)
                  const only = picked && episodes === 1
                  return (
                    <DropdownMenuCheckboxItem
                      key={m.id}
                      checked={picked}
                      // One episode at least: none would mean all again.
                      disabled={only}
                      onCheckedChange={(checked) => {
                        const ids = narrowed ? spec.media : media.map((x) => x.id)
                        const next = checked ? [...ids, m.id] : ids.filter((id) => id !== m.id)
                        const all = media.every((x) => next.includes(x.id))
                        set({ media: all ? [] : media.map((x) => x.id).filter((id) => next.includes(id)) })
                      }}
                    >
                      {/* Cut at the list's width; the rest shows in a tooltip, only when it is cut. */}
                      <TruncatedText>{mediaTitle(m)}</TruncatedText>
                    </DropdownMenuCheckboxItem>
                  )
                })}
              </div>
            </Choice>
          )}
          <Choice label="Limit" value={spec.limit == null ? "No limit" : `Best ${spec.limit}`} active={spec.limit != null}>
            <DropdownMenuRadioGroup
              value={String(spec.limit)}
              onValueChange={(v) => set({ limit: v === "null" ? null : Number(v) })}
            >
              {LIMITS.map((n) => (
                <DropdownMenuRadioItem key={String(n)} value={String(n)} onSelect={stayOpen}>
                  {n == null ? "No limit" : `The best ${n}`}
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
          </Choice>
          <DropdownMenuSeparator />
          <DropdownMenuCheckboxItem checked={spec.solo} onCheckedChange={(solo) => set({ solo })}>
            Only them in the shot
          </DropdownMenuCheckboxItem>
          <DropdownMenuCheckboxItem checked={spec.skipTiny} onCheckedChange={(skipTiny) => set({ skipTiny })}>
            Skip flash cuts
          </DropdownMenuCheckboxItem>
          {(faceless || spec.skipFaceless) && (
            // Its hint is a line of its own, not a tooltip: a tooltip would take the first Esc from the menu.
            <DropdownMenuCheckboxItem
              hint="Leaves out the shots found by their body"
              checked={spec.skipFaceless ?? false}
              onCheckedChange={(skipFaceless) => set({ skipFaceless })}
            >
              Only with their face
            </DropdownMenuCheckboxItem>
          )}
        </DropdownMenuContent>
      </DropdownMenu>
      <Menu label={spec.handles === 0 ? "No handles" : `Handles ${spec.handles} fr`} active={spec.handles > 0}>
        <DropdownMenuRadioGroup value={String(spec.handles)} onValueChange={(v) => set({ handles: Number(v) })}>
          {HANDLES.map((n) => (
            <DropdownMenuRadioItem key={n} value={String(n)}>
              {n === 0 ? "No handles" : `${n} frames before and after`}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </Menu>
      {filters.length > 0 && <Divider />}
      {filters.map((f) => (
        <PluginFilterChip
          key={f.key}
          filter={f}
          on={pluginFiltersOf(spec).includes(f.key)}
          icon={plugins.find((p) => p.id === pluginOf(f.key))?.icon}
          onChange={(on) => onChange(withPluginFilter(spec, f.key, on))}
        />
      ))}
      {!isDefault(spec) && (
        <Button size="sm" variant="ghost" className="ml-auto" onClick={() => onChange(DEFAULT_SPEC)}>
          <ArrowCounterClockwise /> Reset filters
        </Button>
      )}
    </div>
  )
}

/** A radio entry's choice shows at once and the menu stays: the next filter is one step away. */
const stayOpen = (e: Event) => e.preventDefault()

/** A filter with a list of its own: its name, what is chosen, and the list one step further. */
function Choice({ label, value, active, children }: { label: string; value: string; active: boolean; children: React.ReactNode }) {
  return (
    <DropdownMenuSub>
      <DropdownMenuSubTrigger className="pl-9">
        {label}
        <span className={cn("ml-auto text-sm", active ? "text-accent" : "text-fg-3")}>{value}</span>
      </DropdownMenuSubTrigger>
      <DropdownMenuSubContent className="min-w-[220px] max-w-[360px]">{children}</DropdownMenuSubContent>
    </DropdownMenuSub>
  )
}

const NO_FILTERS: PackFilter[] = []
const NO_PLUGINS: PluginInfo[] = []

/** The plugin a filter key (`<pluginId>/<filterId>`) belongs to. */
const pluginOf = (key: string) => key.slice(0, Math.max(0, key.indexOf("/")))

/**
 * A plugin's shot filter as a chip with its plugin's mark. Turned on, it
 * leaves out the shots its plugin says no to; while it checks them, a
 * ring fills; once its check failed, a warning mark says so (the footer
 * offers Try again). Its plugin is off: the chip is dimmed and says so, the
 * pack ignores it, and it can be turned off. No plugin has it now: the same,
 * and turning it off is all there is to do.
 */
function PluginFilterChip({
  filter: f,
  on,
  icon,
  onChange,
}: {
  filter: PackFilter
  on: boolean
  icon: string | null | undefined
  onChange: (on: boolean) => void
}) {
  const gone = f.state === "gone"
  const off = f.state === "off"
  const checking = on && f.state === "checking"
  const failed = on && f.state === "failed" && f.checked < f.total
  const label = gone
    ? "This filter's plugin is gone, or no longer has it. Turn this filter off."
    : off
      ? `Plugin “${f.plugin}” is off. Turn it on in Plugins, or turn this filter off.`
      : `Plugin “${f.plugin}”: ${f.description ?? f.title}`
  const hint = checking
    ? `Checking ${f.checked} of ${plural(f.total, "shot")}`
    : failed
      ? `Couldn't check ${plural(f.total - f.checked, "shot")}`
      : undefined
  return (
    <Tooltip label={label} hint={hint}>
      <Chip pressed={on} onClick={() => onChange(!on)} className={cn((off || gone) && "opacity-60")}>
        <PluginIcon src={icon} className="size-4" />
        {f.title}
        {checking && (
          <ProgressRing
            aria-label={`Checking ${f.title}`}
            className="size-3.5"
            value={f.checked > 0 && f.total > 0 ? f.checked / f.total : null}
          />
        )}
        {failed && <Warning role="img" aria-label={`Couldn't check ${f.title}`} className="size-3.5 text-danger" />}
        {off && <span className="text-xs font-normal text-fg-3">Plugin off</span>}
      </Chip>
    </Tooltip>
  )
}

function Divider() {
  return <span aria-hidden className="mx-1 h-5 w-px bg-white/10" />
}

function Chip({
  pressed,
  className,
  ...props
}: React.ComponentProps<"button"> & { pressed: boolean }) {
  return (
    <button
      type="button"
      aria-pressed={pressed}
      className={cn(
        "inline-flex h-8 items-center gap-1.5 rounded-full border px-3 text-sm font-medium transition-[color,background-color,border-color] duration-[var(--dur-fast)]",
        pressed
          ? "border-accent-line bg-accent-subtle text-fg-1"
          : "border-white/10 bg-white/[0.04] text-fg-2 hover:border-white/20 hover:text-fg-1",
        className,
      )}
      {...props}
    />
  )
}

function Menu({ label, active, children }: { label: string; active: boolean; children: React.ReactNode }) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Chip pressed={active} aria-haspopup="menu" aria-pressed={undefined}>
          {label} <CaretDown className="size-3.5 opacity-70" />
        </Chip>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="min-w-[220px]">
        {children}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
