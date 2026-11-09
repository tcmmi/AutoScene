import type * as React from "react"
import { useId, useState } from "react"
import { Trans, useTranslation } from "react-i18next"
import { toast } from "sonner"
import type { EventKind, PageSpec, PluginInfo, PluginReview, PluginSandbox, PluginUpdate, ReviewCatalog } from "@/bindings"
import { ShieldCheck } from "@/components/icons"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogFooter } from "@/components/ui/dialog"
import { Field, Input } from "@/components/ui/input"
import { Progress } from "@/components/ui/progress"
import { formatBytes } from "@/lib/format"
import { fmt, say } from "@/lib/i18n"
import { commands, errorMessage, isAppError } from "@/lib/ipc"
import { qk, queryClient } from "@/lib/queries"
import { type Getting, usePluginReview } from "@/state/plugin-review"
import { catalogDay } from "./catalog-items"
import { PluginIcon } from "./PluginIcon"
import { allowUnreviewed } from "./plugin-actions"
import { MAX_RAIL_PAGES } from "./page-items"
import { checkedWhen, joinWords, limitsLine, permissionLabel, resourceLine, reviewedPackage, trustLabel, trustLine } from "./plugin-items"

/**
 * A plugin package (or folder) checked for install, before the person says
 * yes: who stands behind it, what it says it does (or, in AutoScene's
 * sandbox, what it may do), how it runs and what it adds. One AutoScene hasn't reviewed needs the setting (which can be turned
 * on here), its name typed, and the person's yes in the dialog AutoScene
 * then shows outside the page. Closed without installing, the review goes,
 * and the files it unpacked. It shows what `usePluginReview` holds, from
 * wherever that was asked for, so the shell mounts it once. A package got
 * from the catalog shows here while it comes, with a Cancel. Past the yes, a
 * WebAssembly plugin shows here while it compiles, with a Cancel too.
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
 * A plugin that compiles, past the person's yes: nothing says how far that is,
 * it can take minutes, and it can be stopped. Stopped, it leaves things as they
 * were before the yes.
 */
function CompilingForm({ name }: { name: string }) {
  const { t } = useTranslation(["plugins", "common"])
  const stopCompile = usePluginReview((s) => s.stopCompile)
  const [stopping, setStopping] = useState(false)
  return (
    <DialogContent title={t("review.gettingReady", { name })} hideClose>
      <div className="flex flex-col gap-2.5">
        <Progress value={null} />
        <p className="text-sm text-fg-2">{t("review.compiling")}</p>
      </div>
      <DialogFooter>
        <Button
          variant="ghost"
          disabled={stopping}
          onClick={() => {
            setStopping(true)
            stopCompile()
          }}
        >
          {t("common:action.cancel")}
        </Button>
      </DialogFooter>
    </DialogContent>
  )
}

/**
 * A package on its way from the catalog: how far it came, and Cancel. Once
 * it is in and asks for nothing new, it installs without a review: then it
 * can't be cancelled, until it compiles.
 */
function GettingForm({ getting, onCancel }: { getting: Getting; onCancel: () => void }) {
  const { t } = useTranslation(["plugins", "common"])
  const { label, done, total, installing, ticketId } = getting
  const compiling = usePluginReview((s) => s.compiling != null && s.compiling === ticketId)
  const part = done != null && total != null && total > 0 ? done / total : null
  if (compiling) return <CompilingForm name={label} />
  return (
    <DialogContent title={installing ? t("review.updating", { name: label }) : t("review.getting", { name: label })} hideClose>
      <div className="flex flex-col gap-2.5">
        <Progress value={installing ? null : part} />
        <p className="text-sm text-fg-2">
          {installing
            ? t("review.nothingNew")
            : done != null && total != null && total > 0
              ? t("review.progress", { done: formatBytes(done), total: formatBytes(total) })
              : t("review.fromCatalog")}
        </p>
      </div>
      <DialogFooter>
        <Button variant="ghost" disabled={installing} onClick={onCancel}>
          {t("common:action.cancel")}
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
  const { t } = useTranslation(["plugins", "common"])
  const p = review.plugin
  const compiling = usePluginReview((s) => s.compiling === review.ticketId)
  const compileEnded = usePluginReview((s) => s.compileEnded)
  const [typed, setTyped] = useState("")
  // The setting turned on from here: the review stays, and may install.
  const [allowedHere, setAllowedHere] = useState(false)
  const allowed = review.allowed || allowedHere
  const nameId = useId()
  const confirmed = !review.needsUnreviewed || (typedName(typed) !== "" && typedName(typed) === typedName(p.name))
  // A developer's folder is loaded where it is, not installed.
  const verb = review.update ? "update" : p.source === "folder" ? "load" : "install"
  const install = async () => {
    setBusy(true)
    try {
      const installed = await commands.pluginInstall(review.ticketId, review.needsUnreviewed)
      void queryClient.invalidateQueries({ queryKey: qk.plugins })
      // What the catalog offers is worked out against what is installed.
      void queryClient.invalidateQueries({ queryKey: qk.catalog })
      onInstalled()
      const done = verb === "update" ? "review.updated" : verb === "load" ? "review.loaded" : "review.installed"
      toast(t(done, { name: installed.name }))
    } catch (e) {
      // The person said no in AutoScene's own dialog: the review stays as it is.
      if (isAppError(e) && e.code === "cancelled") return
      toast.error(t("review.failed"), { description: errorMessage(e) })
      // Only a refusal while the plugin runs keeps the review, to try again.
      if (!(isAppError(e) && e.code === "not_ready")) onCancel()
    } finally {
      compileEnded()
      setBusy(false)
    }
  }
  const trust = trustLabel(p)
  // What was typed here stays: Cancel comes back to it.
  if (compiling) return <CompilingForm name={p.name} />
  return (
    <DialogContent
      title={verb === "update" ? t("review.update", { name: p.name }) : verb === "load" ? t("review.load", { name: p.name }) : t("review.install", { name: p.name })}
      hideClose={busy}
      className="max-h-[calc(100vh-48px)] overflow-y-auto"
    >
      <div className="flex flex-col gap-5">
        <div className="flex items-start gap-4">
          <span className="flex size-12 shrink-0 items-center justify-center rounded-2xl bg-white/[0.06] text-fg-1 [&_svg]:size-6">
            <PluginIcon src={p.icon} className="size-7" />
          </span>
          <dl className="grid min-w-0 flex-1 grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
            <Fact label={t("review.fact.id")}>
              <span data-selectable className="font-mono">
                {p.id}
              </span>
            </Fact>
            <Fact label={t("review.fact.version")}>
              {p.version}
              {review.update && <span className="text-fg-3"> · {t("review.fact.isInstalled", { version: review.update.fromVersion })}</span>}
            </Fact>
            <Fact label={reviewedPackage(p) ? t("review.fact.author") : t("review.fact.saysBy")}>{p.author || t("review.fact.notNamed")}</Fact>
            {p.authorKeyId && (
              <Fact label={t("review.fact.key")}>
                <span data-selectable className="font-mono">
                  {p.authorKeyId}
                </span>
              </Fact>
            )}
            {p.source === "folder" && (
              <Fact label={t("review.fact.folder")}>
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
            <p className="text-sm text-danger">{t("review.before.leftSandbox")}</p>
          )}
        </div>

        {/* The plugin's words, not AutoScene's. */}
        {p.description && (
          <section>
            <h3 className="text-sm font-medium text-fg-2">{t("review.ownWords")}</h3>
            <p className="mt-1.5 text-md text-fg-1">{p.description}</p>
          </section>
        )}

        <Permissions review={review} />
        <Runs review={review} />
        <Adds p={p} update={review.update} />

        {p.warnings.length > 0 && (
          <section>
            <h3 className="text-sm font-medium text-fg-2">{t("review.warnings")}</h3>
            <ul className="mt-1.5 list-disc space-y-1 pl-5 text-sm text-fg-2">
              {p.warnings.map((w, i) => (
                <li key={i}>{say(w)}</li>
              ))}
            </ul>
          </section>
        )}

        {/* What the check of its component found: nothing was skipped, it runs as it is. */}
        {review.componentWarnings.length > 0 && (
          <section>
            <h3 className="text-sm font-medium text-fg-2">{t("review.componentWarnings")}</h3>
            <ul className="mt-1.5 list-disc space-y-1 pl-5 text-sm text-fg-2">
              {review.componentWarnings.map((w, i) => (
                <li key={i}>{say(w)}</li>
              ))}
            </ul>
          </section>
        )}

        {review.needsUnreviewed &&
          (allowed ? (
            <Field
              label={
                <Trans
                  t={t}
                  i18nKey={verb === "update" ? "review.typeToUpdate" : verb === "load" ? "review.typeToLoad" : "review.typeToInstall"}
                  components={{ x: <span data-selectable>{p.name}</span> }}
                />
              }
              hint={t("review.cantCheck")}
              htmlFor={nameId}
            >
              <Input id={nameId} value={typed} onChange={(e) => setTyped(e.target.value)} />
            </Field>
          ) : (
            <div className="flex items-center justify-between gap-4 rounded-lg border border-danger/30 bg-danger-subtle px-3.5 py-3">
              <p className="text-sm text-fg-1">{t("review.turnedOff")}</p>
              <Button size="sm" variant="ghost" onClick={() => void allowUnreviewed(true).then(setAllowedHere)}>
                {t("review.allowThem")}
              </Button>
            </div>
          ))}
      </div>
      <DialogFooter>
        <Button variant="ghost" disabled={busy} onClick={onCancel}>
          {t("common:action.cancel")}
        </Button>
        <Button
          variant={review.needsUnreviewed ? "danger" : "primary"}
          busy={busy}
          disabled={!allowed || !confirmed}
          onClick={() => void install()}
        >
          {verb === "update" ? t("review.updateButton") : verb === "load" ? t("review.loadButton") : t("review.installButton")}
        </Button>
      </DialogFooter>
    </DialogContent>
  )
}

/** Where AutoScene's catalog built a plugin it reviewed from: the repository, the commit and the folder. */
function Built({ catalog }: { catalog: ReviewCatalog }) {
  const { t } = useTranslation("plugins")
  const repo = catalog.repository.replace(/^https:\/\//, "")
  const path = catalog.path && catalog.path !== "." ? catalog.path : null
  return (
    <p className="flex items-start gap-2 text-sm text-fg-2">
      <ShieldCheck className="mt-0.5 size-4 shrink-0 text-fg-3" />
      <span>
        <Trans
          t={t}
          i18nKey={path != null ? "review.builtIn" : "review.built"}
          components={{
            repo: <span data-selectable className="text-fg-1">{repo}</span>,
            commit: <span data-selectable className="font-mono text-fg-1">{catalog.commit.slice(0, 7)}</span>,
            path: <span data-selectable>{path}</span>,
          }}
        />
        {catalog.published != null && ` ${t("catalog.of", { date: catalogDay(catalog.published) })}`}
      </span>
    </p>
  )
}

/** What the installed version was, when the update changes who stands behind it or where its files are. */
function Before({ update, p }: { update: PluginUpdate; p: PluginInfo }) {
  const { t } = useTranslation("plugins")
  if (update.trustBefore === p.trust && update.sourceBefore === p.source) return null
  // A signed update of an unsigned package pins whatever key it brings.
  if (update.sourceBefore === "package" && update.trustBefore === "unreviewed" && p.source === "package" && p.trust === "pinned") {
    return (
      <p className="text-sm text-fg-1">{t("review.before.unsigned")}</p>
    )
  }
  const before = trustLabel({ trust: update.trustBefore, source: update.sourceBefore })
  return (
    <p className="text-sm text-fg-2">
      {t("review.before.was", { label: before.label })}
      {update.sourceBefore === "folder" && p.source === "package" && ` ${t("review.before.folderReplaced")}`}
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
  const { t } = useTranslation("plugins")
  const p = review.plugin
  if (p.sandbox) return <Sandboxed review={review} sandbox={p.sandbox} />
  const added = review.update?.addedPermissions ?? []
  const newHosts = review.update?.addedHosts ?? []
  const hosts = p.networkHosts.filter((h) => !newHosts.includes(h))
  const program = p.runtime !== "none"
  const checked = checkedWhen(p)
  const heading = !program ? t("review.permissions.pagesMayUse") : checked ? t("review.permissions.saysItDoes") : t("review.permissions.notEnforced")
  return (
    <section>
      {program && <p className="mb-3 text-md text-fg-1">{t("review.permissions.userRights")}</p>}
      <h3 className="text-sm font-medium text-fg-2">{heading}</h3>
      <ul className="mt-1.5 flex flex-col gap-1.5 text-md text-fg-1">
        {p.permissions.map((perm) => (
          <li key={perm} className="flex flex-wrap items-center gap-x-2 gap-y-1">
            {permissionLabel(perm)}
            {added.includes(perm) && <Badge tone="accent">{t("new")}</Badge>}
            {perm === "network" && hosts.length > 0 && <span className="text-sm text-fg-2">{hosts.join(", ")}</span>}
            {perm === "network" && newHosts.length > 0 && (
              <span className="text-sm text-accent">{t("review.permissions.newHosts", { hosts: newHosts.join(", ") })}</span>
            )}
          </li>
        ))}
        {p.permissions.length === 0 && <li className="text-fg-2">{t("review.permissions.none")}</li>}
        {p.gpu && <li>{t("review.permissions.gpu")}</li>}
      </ul>
      {checked && <p className="mt-1.5 text-sm text-fg-2">{checked}</p>}
    </section>
  )
}

/**
 * What a WebAssembly plugin may do in AutoScene's sandbox, with what an
 * update adds marked: walls AutoScene keeps, not the plugin's word. Then
 * what it can't do, its limits, marked when an update raises them, and what
 * the sandbox doesn't limit. It never says "safe".
 */
function Sandboxed({ review, sandbox }: { review: PluginReview; sandbox: PluginSandbox }) {
  const { t } = useTranslation("plugins")
  const p = review.plugin
  const added = review.update?.addedPermissions ?? []
  const newHosts = review.update?.addedHosts ?? []
  const hosts = p.networkHosts.filter((h) => !newHosts.includes(h))
  // What it sends is unlimited only where it may send it.
  const network = p.permissions.includes("network") && p.networkHosts.length > 0
  return (
    <section>
      <p className="mb-3 text-md text-fg-1">{t("review.sandbox.intro")}</p>
      <h3 className="text-sm font-medium text-fg-2">{t("review.sandbox.mayDo")}</h3>
      <ul className="mt-1.5 flex flex-col gap-1.5 text-md text-fg-1">
        {p.permissions.map((perm) => (
          <li key={perm} className="flex flex-wrap items-center gap-x-2 gap-y-1">
            {permissionLabel(perm, { hosts: p.networkHosts })}
            {added.includes(perm) && <Badge tone="accent">{t("new")}</Badge>}
            {perm === "network" && hosts.length > 0 && <span className="text-sm text-fg-2">{hosts.join(", ")}</span>}
            {perm === "network" && newHosts.length > 0 && (
              <span className="text-sm text-accent">{t("review.permissions.newHosts", { hosts: newHosts.join(", ") })}</span>
            )}
          </li>
        ))}
        {p.permissions.length === 0 && <li className="text-fg-2">{t("review.sandbox.none")}</li>}
      </ul>
      <div className="mt-1.5 flex flex-col gap-1 text-sm text-fg-2">
        <p>{t("review.sandbox.cant")}</p>
        <p className="flex flex-wrap items-center gap-x-2 gap-y-1">
          {limitsLine(sandbox)}
          {review.update?.raisedLimits && <Badge tone="accent">{t("new")}</Badge>}
        </p>
        <p>{network ? t("review.sandbox.unlimitedNetwork") : t("review.sandbox.unlimited")}</p>
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
  const { t } = useTranslation("plugins")
  const p = review.plugin
  const startsProgram = review.update?.startsProgram ?? false
  const becomesService = review.update?.becomesService ?? false
  const service = p.runtime === "service"
  const files = resourceLine(review, p.runtime, review.update != null)
  const newFileHosts = review.update?.addedResourceHosts ?? []
  // As AutoScene's other downloads say their size.
  const size = formatBytes(review.uvBytes ?? 0, 0)
  const uv = review.uvMissing
    ? service
      ? t("review.runs.uvStartGets", { size })
      : t("review.runs.uvRunGets", { size })
    : service
      ? t("review.runs.uvStart")
      : t("review.runs.uvRun")
  return (
    <section>
      <h3 className="text-sm font-medium text-fg-2">{t("review.runs.title")}</h3>
      <p className="mt-1.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-md text-fg-1">
        {p.runtime === "none"
          ? t("review.runs.none")
          : p.sandbox
            ? t("review.runs.wasm")
            : p.uvScript
              ? uv
              : t("review.runs.program")}
        {startsProgram && <Badge tone="accent">{t("new")}</Badge>}
      </p>
      {startsProgram && <p className="mt-1.5 text-sm text-fg-2">{t("review.runs.noProgramBefore")}</p>}
      {p.runtime === "service" && (
        <p className="mt-1.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-md text-fg-1">
          {becomesService ? t("review.runs.becomesService") : t("review.runs.service")}
          {becomesService && <Badge tone="accent">{t("new")}</Badge>}
        </p>
      )}
      {files && (
        <>
          <p className="mt-1.5 text-md text-fg-1">{files}</p>
          {newFileHosts.length > 0 && (
            <p className="mt-1.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-sm text-fg-2">
              <Badge tone="accent">{t("new")}</Badge>
              {t("review.runs.newFileHosts", { hosts: joinWords(newFileHosts) })}
            </p>
          )}
        </>
      )}
    </section>
  )
}

/** The messages that list what a plugin adds, each a whole line with the plugin's own title in it. */
type AddLine =
  | "review.adds.pageRail"
  | "review.adds.pageProject"
  | "review.adds.pagePalette"
  | "review.adds.exportAction"
  | "review.adds.command"
  | "review.adds.shotFilter"
  | "review.adds.packAction"
  | "review.adds.source"
  | "review.adds.afterFailed"
  | "event.analysisFinished.adds"

/**
 * Where AutoScene offers a page: in the rail while it has room for it, in
 * each project's More menu, else in the palette.
 */
function pagePlace(page: PageSpec): AddLine {
  if (page.scope === "project") return "review.adds.pageProject"
  return page.rail ? "review.adds.pageRail" : "review.adds.pagePalette"
}

/** What the review says an event of each kind adds. */
function eventAdds(on: EventKind): AddLine {
  switch (on) {
    case "analysisFinished":
      return "event.analysisFinished.adds"
  }
}

/**
 * The pages (and where they show), menu entries, palette commands, scenepack
 * filters and ⋯ entries, what may run by itself (once turned on) and settings
 * it adds, with the pages, filters and ⋯ entries an update adds marked, and the
 * secrets it keeps.
 */
function Adds({ p, update }: { p: PluginInfo; update: PluginUpdate | null | undefined }) {
  const { t } = useTranslation("plugins")
  const adds: { key: string; line: AddLine; what: string; added: boolean; hosts?: string }[] = [
    ...p.pages.map((page) => ({
      key: `page-${page.id}`,
      line: pagePlace(page),
      what: page.title,
      added: update?.addedPages.includes(page.id) ?? false,
    })),
    ...p.exportActions.map((a) => ({ key: `action-${a.id}`, line: "review.adds.exportAction" as const, what: a.title, added: false })),
    ...p.commands.map((c) => ({ key: `command-${c.id}`, line: "review.adds.command" as const, what: c.title, added: false })),
    ...p.shotFilters.map((f) => ({
      key: `filter-${f.id}`,
      line: "review.adds.shotFilter" as const,
      what: f.title,
      added: update?.addedShotFilters.includes(f.id) ?? false,
    })),
    ...p.packActions.map((a) => ({
      key: `pack-${a.id}`,
      line: "review.adds.packAction" as const,
      what: a.title,
      added: update?.addedPackActions.includes(a.id) ?? false,
    })),
    ...p.sources.map((s) => ({
      key: `source-${s.id}`,
      line: "review.adds.source" as const,
      what: s.title,
      hosts: fmt.list(s.linkHosts),
      added: (update?.addedSources.includes(s.id) || s.linkHosts.some((h) => update?.addedLinkHosts.includes(h))) ?? false,
    })),
    ...p.exportActions
      .filter((a) => a.afterFailed)
      .map((a) => ({ key: `failed-${a.id}`, line: "review.adds.afterFailed" as const, what: a.title, added: false })),
    ...p.events.map((e) => ({ key: `event-${e.id}`, line: eventAdds(e.on), what: e.title, added: false })),
  ]
  const secrets = p.settings.filter((s) => s.kind.type === "secret")
  const settings = p.settings.length - secrets.length
  // The label before the colon, and a note after the title, are muted.
  const muted = { x: <span className="text-fg-2" /> }
  return (
    <section>
      <h3 className="text-sm font-medium text-fg-2">{t("review.adds.title")}</h3>
      <ul className="mt-1.5 flex flex-col gap-1 text-md text-fg-1">
        {adds.map((a) => (
          <li key={a.key} className="flex flex-wrap items-center gap-x-2 gap-y-1">
            <span>
              <Trans t={t} i18nKey={a.line} values={{ max: MAX_RAIL_PAGES, hosts: a.hosts ?? "" }} components={{ ...muted, what: <>{a.what}</> }} />
            </span>
            {a.added && <Badge tone="accent">{t("new")}</Badge>}
          </li>
        ))}
        {settings > 0 && (
          <li>
            <Trans t={t} i18nKey="review.adds.settings" count={settings} components={muted} />
          </li>
        )}
        {secrets.map((s) => (
          <li key={`secret-${s.key}`}>{t("review.adds.secret", { title: s.title })}</li>
        ))}
        {adds.length === 0 && p.settings.length === 0 && <li className="text-fg-2">{t("review.adds.nothing")}</li>}
      </ul>
    </section>
  )
}
