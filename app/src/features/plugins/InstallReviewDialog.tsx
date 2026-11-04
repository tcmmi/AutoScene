import type * as React from "react"
import { useId, useState } from "react"
import { toast } from "sonner"
import type { EventKind, PageSpec, PluginInfo, PluginReview, PluginSandbox, PluginUpdate, ReviewCatalog } from "@/bindings"
import { CircleNotch, ShieldCheck } from "@/components/icons"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogFooter } from "@/components/ui/dialog"
import { Field, Input } from "@/components/ui/input"
import { Progress } from "@/components/ui/progress"
import { formatBytes } from "@/lib/format"
import { commands, errorMessage, isAppError } from "@/lib/ipc"
import { qk, queryClient } from "@/lib/queries"
import { type Getting, usePluginReview } from "@/state/plugin-review"
import { catalogDay } from "./catalog-items"
import { PluginIcon } from "./PluginIcon"
import { allowUnreviewed } from "./plugin-actions"
import { MAX_RAIL_PAGES } from "./page-items"
import { checkedWhen, firstTime, joinWords, limitsLine, permissionLabel, resourceLine, reviewedPackage, trustLabel, trustLine } from "./plugin-items"

/**
 * A plugin package (or folder) checked for install, before the person says
 * yes: who stands behind it, what it says it does (or, in AutoScene's
 * sandbox, what it may do), how it runs and what it adds. One AutoScene hasn't reviewed needs the setting (which can be turned
 * on here), its name typed, and the person's yes in the dialog AutoScene
 * then shows outside the page. Closed without installing, the review goes,
 * and the files it unpacked. It shows what `usePluginReview` holds, from
 * wherever that was asked for, so the shell mounts it once. A package got
 * from the catalog shows here while it comes, with a Cancel.
 */
export function InstallReviewDialog() {
  const review = usePluginReview((s) => s.review)
  const getting = usePluginReview((s) => s.getting)
  const close = usePluginReview((s) => s.close)
  const cancelGet = usePluginReview((s) => s.cancelGet)
  // While it installs it stays open: closed, it would install all the same.
  const [busy, setBusy] = useState(false)
  return (
    <Dialog
      open={review != null || getting != null}
      onOpenChange={(open) => {
        if (open || busy) return
        if (review) close()
        else cancelGet()
      }}
    >
      {!review && getting && <GettingForm getting={getting} onCancel={cancelGet} />}
      {review && (
        <ReviewForm
          key={review.ticketId}
          review={review}
          busy={busy}
          setBusy={setBusy}
          onCancel={() => close()}
          onInstalled={() => close({ installed: true })}
        />
      )}
    </Dialog>
  )
}

/**
 * A package on its way from the catalog: how far it came, and Cancel. Once
 * it is in and asks for nothing new, it installs at once: then it can't be
 * cancelled.
 */
function GettingForm({ getting, onCancel }: { getting: Getting; onCancel: () => void }) {
  const { label, done, total, installing } = getting
  const part = done != null && total != null && total > 0 ? done / total : null
  return (
    <DialogContent title={installing ? `Updating ${label}…` : `Getting ${label}…`} hideClose>
      <div className="flex flex-col gap-2.5">
        <Progress value={installing ? null : part} />
        <p className="text-sm text-fg-2">
          {installing
            ? "It asks for nothing new, so it installs at once."
            : done != null && total != null && total > 0
              ? `${formatBytes(done)} of ${formatBytes(total)} from AutoScene's plugin catalog`
              : "From AutoScene's plugin catalog"}
        </p>
      </div>
      <DialogFooter>
        <Button variant="ghost" disabled={installing} onClick={onCancel}>
          Cancel
        </Button>
      </DialogFooter>
    </DialogContent>
  )
}

/**
 * A plugin's name as it is typed to confirm it: the spaces around it, and an
 * ellipsis at its end (a long name is cut to one), don't count.
 */
function typedName(s: string): string {
  return s.normalize("NFC").trim().replace(/(?:…|\.\.\.)$/u, "").trimEnd()
}

function ReviewForm({
  review,
  busy,
  setBusy,
  onCancel,
  onInstalled,
}: {
  review: PluginReview
  busy: boolean
  setBusy: (busy: boolean) => void
  onCancel: () => void
  onInstalled: () => void
}) {
  const p = review.plugin
  const [typed, setTyped] = useState("")
  // The setting turned on from here: the review stays, and may install.
  const [allowedHere, setAllowedHere] = useState(false)
  const allowed = review.allowed || allowedHere
  const nameId = useId()
  const confirmed = !review.needsUnreviewed || (typedName(typed) !== "" && typedName(typed) === typedName(p.name))
  // A developer's folder is loaded where it is, not installed.
  const verb = review.update ? "Update" : p.source === "folder" ? "Load" : "Install"
  const install = async () => {
    setBusy(true)
    try {
      const installed = await commands.pluginInstall(review.ticketId, review.needsUnreviewed)
      void queryClient.invalidateQueries({ queryKey: qk.plugins })
      // What the catalog offers is worked out against what is installed.
      void queryClient.invalidateQueries({ queryKey: qk.catalog })
      onInstalled()
      toast(`${verb === "Update" ? "Updated" : verb === "Load" ? "Loaded" : "Installed"} ${installed.name}`)
    } catch (e) {
      // The person said no in AutoScene's own dialog: the review stays as it is.
      if (isAppError(e) && e.code === "cancelled") return
      toast.error("Couldn't install the plugin", { description: errorMessage(e) })
      // Only a refusal while the plugin runs keeps the review, to try again.
      if (!(isAppError(e) && e.code === "not_ready")) onCancel()
    } finally {
      setBusy(false)
    }
  }
  const trust = trustLabel(p)
  return (
    <DialogContent title={`${verb} ${p.name}?`} className="max-h-[calc(100vh-48px)] overflow-y-auto">
      <div className="flex flex-col gap-5">
        <div className="flex items-start gap-4">
          <span className="flex size-12 shrink-0 items-center justify-center rounded-2xl bg-white/[0.06] text-fg-1 [&_svg]:size-6">
            <PluginIcon src={p.icon} className="size-7" />
          </span>
          <dl className="grid min-w-0 flex-1 grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
            <Fact label="Id">
              <span data-selectable className="font-mono">
                {p.id}
              </span>
            </Fact>
            <Fact label="Version">
              {p.version}
              {review.update && <span className="text-fg-3"> · {review.update.fromVersion} is installed</span>}
            </Fact>
            <Fact label={reviewedPackage(p) ? "Author" : "Says it is by"}>{p.author || "Not named"}</Fact>
            {p.authorKeyId && (
              <Fact label="Key">
                <span data-selectable className="font-mono">
                  {p.authorKeyId}
                </span>
              </Fact>
            )}
            {p.source === "folder" && (
              <Fact label="Folder">
                <span data-selectable className="font-mono break-all">
                  {p.folder}
                </span>
              </Fact>
            )}
          </dl>
        </div>

        <div className="flex flex-col gap-2 rounded-lg border border-hairline bg-inset px-3.5 py-3">
          <Badge tone={trust.tone} className="self-start">
            {trust.label}
          </Badge>
          <p className="text-sm text-fg-1">{trustLine(p, review.update)}</p>
          {review.catalog && <Built catalog={review.catalog} />}
          {review.update && <Before update={review.update} p={p} />}
          {review.update?.leavesSandbox && (
            <p className="text-sm text-danger">The installed version ran in a sandbox. This version runs with your user rights.</p>
          )}
        </div>

        {/* The plugin's words, not AutoScene's. */}
        {p.description && (
          <section>
            <h3 className="text-sm font-medium text-fg-2">In its own words</h3>
            <p className="mt-1.5 text-md text-fg-1">{p.description}</p>
          </section>
        )}

        <Permissions review={review} />
        <Runs review={review} />
        <Adds p={p} addedPages={review.update?.addedPages ?? []} />

        {p.warnings.length > 0 && (
          <section>
            <h3 className="text-sm font-medium text-fg-2">Skipped in its plugin.json</h3>
            <ul className="mt-1.5 list-disc space-y-1 pl-5 text-sm text-fg-2">
              {p.warnings.map((w, i) => (
                <li key={i}>{w}</li>
              ))}
            </ul>
          </section>
        )}

        {/* What the check of its component found: nothing was skipped, it runs as it is. */}
        {review.componentWarnings.length > 0 && (
          <section>
            <h3 className="text-sm font-medium text-fg-2">Noticed in its component</h3>
            <ul className="mt-1.5 list-disc space-y-1 pl-5 text-sm text-fg-2">
              {review.componentWarnings.map((w, i) => (
                <li key={i}>{w}</li>
              ))}
            </ul>
          </section>
        )}

        {review.needsUnreviewed &&
          (allowed ? (
            <Field
              label={
                <>
                  Type <span data-selectable>{p.name}</span> to {verb.toLowerCase()} it
                </>
              }
              hint="AutoScene can't check what it does."
              htmlFor={nameId}
            >
              <Input id={nameId} value={typed} onChange={(e) => setTyped(e.target.value)} />
            </Field>
          ) : (
            <div className="flex items-center justify-between gap-4 rounded-lg border border-danger/30 bg-danger-subtle px-3.5 py-3">
              <p className="text-sm text-fg-1">Plugins that AutoScene has not reviewed are turned off.</p>
              <Button size="sm" variant="ghost" onClick={() => void allowUnreviewed(true).then(setAllowedHere)}>
                Allow them…
              </Button>
            </div>
          ))}
      </div>
      <DialogFooter>
        <Button variant="ghost" disabled={busy} onClick={onCancel}>
          Cancel
        </Button>
        <Button
          variant={review.needsUnreviewed ? "danger" : "primary"}
          disabled={busy || !allowed || !confirmed}
          onClick={() => void install()}
        >
          {busy && <CircleNotch className="animate-spin" />}
          {verb}
        </Button>
      </DialogFooter>
    </DialogContent>
  )
}

/** Where AutoScene's catalog built a plugin it reviewed from: the repository, the commit and the folder. */
function Built({ catalog }: { catalog: ReviewCatalog }) {
  const repo = catalog.repository.replace(/^https:\/\//, "")
  const folder = catalog.path && catalog.path !== "." ? `, in ${catalog.path}` : ""
  return (
    <p className="flex items-start gap-2 text-sm text-fg-2">
      <ShieldCheck className="mt-0.5 size-4 shrink-0 text-fg-3" />
      <span>
        Built from{" "}
        <span data-selectable className="text-fg-1">
          {repo}
        </span>{" "}
        at{" "}
        <span data-selectable className="font-mono text-fg-1">
          {catalog.commit.slice(0, 7)}
        </span>
        {folder}.{catalog.published != null && ` Catalog of ${catalogDay(catalog.published)}.`}
      </span>
    </p>
  )
}

/** What the installed version was, when the update changes who stands behind it or where its files are. */
function Before({ update, p }: { update: PluginUpdate; p: PluginInfo }) {
  if (update.trustBefore === p.trust && update.sourceBefore === p.source) return null
  // A signed update of an unsigned package pins whatever key it brings.
  if (update.sourceBefore === "package" && update.trustBefore === "unreviewed" && p.source === "package" && p.trust === "pinned") {
    return (
      <p className="text-sm text-fg-1">
        The installed version wasn't signed. This one pins its key: from now on, only updates signed with it install.
      </p>
    )
  }
  const before = trustLabel({ trust: update.trustBefore, source: update.sourceBefore })
  return (
    <p className="text-sm text-fg-2">
      The installed version: {before.label}.
      {update.sourceBefore === "folder" && p.source === "package" && " This package takes its place; that folder stays as it is."}
    </p>
  )
}

function Fact({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <>
      <dt className="text-fg-3">{label}</dt>
      <dd className="min-w-0 text-fg-1">{children}</dd>
    </>
  )
}

/**
 * What the plugin says it does, with what an update adds marked: a
 * program's claim, which AutoScene checks only when its pages or its
 * program ask it for something; or what AutoScene checks for a plugin that
 * runs none, which is its pages. A plugin in AutoScene's sandbox says what
 * it may do instead ([`Sandboxed`]).
 */
function Permissions({ review }: { review: PluginReview }) {
  const p = review.plugin
  if (p.sandbox) return <Sandboxed review={review} sandbox={p.sandbox} />
  const added = review.update?.addedPermissions ?? []
  const newHosts = review.update?.addedHosts ?? []
  const hosts = p.networkHosts.filter((h) => !newHosts.includes(h))
  const program = p.runtime !== "none"
  const checked = checkedWhen(p)
  const heading = !program ? "What its pages may use" : checked ? "What it says it does" : "What it says it does (not enforced)"
  return (
    <section>
      {program && <p className="mb-3 text-md text-fg-1">It runs with your user rights: it can read, change and send anything you can.</p>}
      <h3 className="text-sm font-medium text-fg-2">{heading}</h3>
      <ul className="mt-1.5 flex flex-col gap-1.5 text-md text-fg-1">
        {p.permissions.map((perm) => (
          <li key={perm} className="flex flex-wrap items-center gap-x-2 gap-y-1">
            {permissionLabel(perm)}
            {added.includes(perm) && <Badge tone="accent">New</Badge>}
            {perm === "network" && hosts.length > 0 && <span className="text-sm text-fg-2">{hosts.join(", ")}</span>}
            {perm === "network" && newHosts.length > 0 && (
              <span className="text-sm text-accent">New: {newHosts.join(", ")}</span>
            )}
          </li>
        ))}
        {p.permissions.length === 0 && <li className="text-fg-2">It names no permissions.</li>}
        {p.gpu && <li>Uses the graphics card: its runs wait while an analysis runs</li>}
      </ul>
      {checked && <p className="mt-1.5 text-sm text-fg-2">{checked}</p>}
    </section>
  )
}

/**
 * What a WebAssembly plugin may do in AutoScene's sandbox, with what an
 * update adds marked: walls AutoScene keeps, not the plugin's word. Then
 * what it can't do, its limits, and what the sandbox doesn't limit. It never
 * says "safe".
 */
function Sandboxed({ review, sandbox }: { review: PluginReview; sandbox: PluginSandbox }) {
  const p = review.plugin
  const added = review.update?.addedPermissions ?? []
  const newHosts = review.update?.addedHosts ?? []
  const hosts = p.networkHosts.filter((h) => !newHosts.includes(h))
  // What it sends is unlimited only where it may send it.
  const network = p.permissions.includes("network") && p.networkHosts.length > 0
  return (
    <section>
      <p className="mb-3 text-md text-fg-1">It runs in a sandbox. It sees only its own folder and what is listed here.</p>
      <h3 className="text-sm font-medium text-fg-2">What it may do (enforced)</h3>
      <ul className="mt-1.5 flex flex-col gap-1.5 text-md text-fg-1">
        {p.permissions.map((perm) => (
          <li key={perm} className="flex flex-wrap items-center gap-x-2 gap-y-1">
            {permissionLabel(perm, { hosts: p.networkHosts })}
            {added.includes(perm) && <Badge tone="accent">New</Badge>}
            {perm === "network" && hosts.length > 0 && <span className="text-sm text-fg-2">{hosts.join(", ")}</span>}
            {perm === "network" && newHosts.length > 0 && (
              <span className="text-sm text-accent">New: {newHosts.join(", ")}</span>
            )}
          </li>
        ))}
        {p.permissions.length === 0 && <li className="text-fg-2">It asks for nothing more.</li>}
      </ul>
      <div className="mt-1.5 flex flex-col gap-1 text-sm text-fg-2">
        <p>It can't start programs or see your other files.</p>
        <p>{limitsLine(sandbox)}</p>
        <p>{network ? "Not limited: what it sends to those sites, and how much it writes." : "Not limited: how much it writes."}</p>
      </div>
    </section>
  )
}

/**
 * What AutoScene starts to run it, and what that fetches on its own: marked
 * new when the installed version runs no program. A service keeps its
 * program running while its page is open, marked new when the installed
 * version didn't. A uv script that comes before AutoScene's own uv says that
 * its first run (a service's first start) gets it, and how much that is. The
 * large files it names say how much its first run (an update's next run)
 * downloads and from where, a host the installed version didn't name marked
 * new on the line under.
 */
function Runs({ review }: { review: PluginReview }) {
  const p = review.plugin
  const startsProgram = review.update?.startsProgram ?? false
  const becomesService = review.update?.becomesService ?? false
  const first = firstTime(p.runtime)
  const files = resourceLine(review, p.runtime, review.update != null)
  const newFileHosts = review.update?.addedResourceHosts ?? []
  // As AutoScene's other downloads say their size.
  const getsUv = review.uvMissing
    ? ` AutoScene gets uv (${formatBytes(review.uvBytes ?? 0, 0)}, from github.com/astral-sh/uv), and uv downloads`
    : ", uv downloads"
  return (
    <section>
      <h3 className="text-sm font-medium text-fg-2">How it runs</h3>
      <p className="mt-1.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-md text-fg-1">
        {p.runtime === "none"
          ? "Runs no program. Its pages run in a sandbox."
          : p.sandbox
            ? "A WebAssembly plugin. AutoScene checked and compiled it."
            : p.uvScript
            ? `A Python script that uv runs. On its ${first}${getsUv} Python (if this computer has none) and the packages the script names.`
            : "A program in its folder."}
        {startsProgram && <Badge tone="accent">New</Badge>}
      </p>
      {startsProgram && <p className="mt-1.5 text-sm text-fg-2">The installed version runs no program.</p>}
      {p.runtime === "service" && (
        <p className="mt-1.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-md text-fg-1">
          {becomesService ? "Now keeps its program running while its page is open." : "Keeps its program running while its page is open."}
          {becomesService && <Badge tone="accent">New</Badge>}
        </p>
      )}
      {files && (
        <>
          <p className="mt-1.5 text-md text-fg-1">{files}</p>
          {newFileHosts.length > 0 && (
            <p className="mt-1.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-sm text-fg-2">
              <Badge tone="accent">New</Badge>
              The installed version names no files from {joinWords(newFileHosts)}.
            </p>
          )}
        </>
      )}
    </section>
  )
}

/**
 * Where AutoScene offers a page: in the rail while it has room for it, in
 * each project's More menu, else in the palette.
 */
function pagePlace(page: PageSpec): string {
  if (page.scope === "project") return ", in each project's More menu"
  return page.rail
    ? `, in the rail (it holds ${MAX_RAIL_PAGES} plugin pages; others are in the command palette)`
    : ", in the command palette"
}

/** What the review says an event of each kind adds. */
const EVENT_ADDS: Record<EventKind, string> = {
  analysisFinished: "Can run when an analysis finishes",
}

/**
 * The pages (and where they show), menu entries, palette commands, what may
 * run by itself (once turned on) and settings it adds, with the pages an
 * update adds marked, and the secrets it keeps.
 */
function Adds({ p, addedPages }: { p: PluginInfo; addedPages: string[] }) {
  const adds = [
    ...p.pages.map((page) => ({
      key: `page-${page.id}`,
      where: "A page",
      what: page.title,
      after: pagePlace(page),
      added: addedPages.includes(page.id),
    })),
    ...p.exportActions.map((a) => ({ key: `action-${a.id}`, where: "Each export's menu", what: a.title, after: "", added: false })),
    ...p.commands.map((c) => ({ key: `command-${c.id}`, where: "Command palette", what: c.title, after: "", added: false })),
    ...p.exportActions
      .filter((a) => a.afterFailed)
      .map((a) => ({
        key: `failed-${a.id}`,
        where: "Can run after every export, also one that failed",
        what: a.title,
        after: " (you turn it on)",
        added: false,
      })),
    ...p.events.map((e) => ({
      key: `event-${e.id}`,
      where: EVENT_ADDS[e.on],
      what: e.title,
      after: " (you turn it on)",
      added: false,
    })),
  ]
  const secrets = p.settings.filter((s) => s.kind.type === "secret")
  const settings = p.settings.length - secrets.length
  return (
    <section>
      <h3 className="text-sm font-medium text-fg-2">What it adds</h3>
      <ul className="mt-1.5 flex flex-col gap-1 text-md text-fg-1">
        {adds.map((a) => (
          <li key={a.key} className="flex flex-wrap items-center gap-x-2 gap-y-1">
            <span>
              <span className="text-fg-2">{a.where}:</span> {a.what}
              {a.after && <span className="text-fg-2">{a.after}</span>}
            </span>
            {a.added && <Badge tone="accent">New</Badge>}
          </li>
        ))}
        {settings > 0 && (
          <li>
            <span className="text-fg-2">Settings:</span> {settings === 1 ? "1 setting" : `${settings} settings`}
          </li>
        )}
        {secrets.map((s) => (
          <li key={`secret-${s.key}`}>Keeps {s.title} in your system's keychain</li>
        ))}
        {adds.length === 0 && p.settings.length === 0 && <li className="text-fg-2">No menu entries, commands or settings.</li>}
      </ul>
    </section>
  )
}
