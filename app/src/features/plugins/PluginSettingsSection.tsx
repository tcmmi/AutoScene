import type * as React from "react"
import { useCallback, useEffect, useId, useRef, useState } from "react"
import { useNavigate } from "@tanstack/react-router"
import { toast } from "sonner"
import type { EventKind, PluginInfo, SettingKind, SettingSpec, SettingValue } from "@/bindings"
import { AppWindow, ArrowCounterClockwise, FolderOpen, GraphicsCard, Prohibit, PuzzlePiece, ShieldCheck, Warning } from "@/components/icons"
import { Banner } from "@/components/ui/banner"
import { Button } from "@/components/ui/button"
import { ConfirmDialog } from "@/components/ui/confirm-dialog"
import { Dialog, DialogContent, DialogFooter } from "@/components/ui/dialog"
import { EmptyState } from "@/components/ui/empty-state"
import { ErrorState } from "@/components/ui/error-state"
import { Field, Input } from "@/components/ui/input"
import { Segmented } from "@/components/ui/segmented"
import { Select } from "@/components/ui/select"
import { Slider } from "@/components/ui/slider"
import { Switch } from "@/components/ui/switch"
import { PathText, SettingRow, SettingsGroup } from "@/features/settings/primitives"
import { useAppActions } from "@/lib/actions"
import { commands, errorMessage } from "@/lib/ipc"
import { reveal } from "@/lib/os"
import { isMac } from "@/lib/platform"
import { qk, queryClient, usePluginSettings, usePlugins, useSettings } from "@/lib/queries"
import { useLoadAgain } from "./load-again"
import { pageLink } from "./page-items"
import { allowUnreviewed, keepPlugin } from "./plugin-actions"
import { checkedWhen, downloadSize, permissionLabel } from "./plugin-items"
import { WithdrawnNotice } from "./WithdrawnBanner"

type Values = Partial<Record<string, SettingValue>>
type NumberKind = Extract<SettingKind, { type: "number" }>

/** How long a slider rests before what it shows is saved. */
const SETTLE_MS = 400

/**
 * The longest secret the keychain keeps (`MAX_SECRET_CHARS`), in UTF-16 units
 * as `length` counts them. Not a `maxLength`: a pasted token would be cut
 * short without a word, in a field that hides it.
 */
const MAX_SECRET = 1024

/** When an event of each kind runs, as its row says it. */
const EVENT_WHEN: Record<EventKind, string> = {
  analysisFinished: "When a project's episodes are all analysed",
}

/** What a blocked plugin's screen says of it, beside "Allow them…". */
const NOT_REVIEWED = "AutoScene hasn't reviewed this plugin, so it doesn't run."

/**
 * A plugin's settings, under its hero (`PluginSettingsPage`): what needs
 * fixing, with the control that fixes it; what it lets the person set (a
 * secret only into the system's keychain), what runs by itself once turned
 * on (export actions after every export, events), its pages, what it may
 * use, and where its files and log are.
 */
export function PluginSettingsSection({ id }: { id: string }) {
  const { data: plugins, isPending, error, refetch, isFetching } = usePlugins()
  const { goPlugins } = useAppActions()
  const p = plugins?.find((x) => x.id === id)
  if (p) return <PluginSettings p={p} />
  if (isPending) return null
  if (error)
    return (
      <ErrorState
        error={error}
        icon={<PuzzlePiece />}
        title="Couldn't read the plugins"
        retrying={isFetching}
        onRetry={() => void refetch()}
        actions={
          <Button variant="ghost" onClick={goPlugins}>
            Go to Plugins
          </Button>
        }
        className="mt-24"
      />
    )
  return (
    <EmptyState
      icon={<PuzzlePiece />}
      tone="accent"
      title="This plugin isn't installed"
      description="It was uninstalled, or this link is to another one."
      actions={<Button onClick={goPlugins}>Go to Plugins</Button>}
      className="mt-24"
    />
  )
}

function PluginSettings({ p }: { p: PluginInfo }) {
  const { data: values, error: unread } = usePluginSettings(p.id)
  const { goPlugins } = useAppActions()
  const save = async (key: string, value: SettingValue) => {
    // Shown at once; read again when it couldn't be saved. Not before the
    // others were read: they would show their defaults meanwhile.
    queryClient.setQueryData<Values>(qk.pluginSettings(p.id), (old) => old && { ...old, [key]: value })
    try {
      queryClient.setQueryData(qk.pluginSettings(p.id), await commands.pluginSettingsSet(p.id, { [key]: value }))
    } catch (e) {
      toast.error("Couldn't save the setting", { description: errorMessage(e) })
      void queryClient.invalidateQueries({ queryKey: qk.pluginSettings(p.id) })
    }
  }
  const setAfterExport = async (actionId: string, on: boolean) => {
    try {
      keepPlugin(await commands.pluginSetAfterExport(p.id, actionId, on))
    } catch (e) {
      toast.error("Couldn't save the setting", { description: errorMessage(e) })
    }
  }
  const setEvent = async (eventId: string, on: boolean) => {
    try {
      keepPlugin(await commands.pluginSetEvent(p.id, eventId, on))
    } catch (e) {
      toast.error("Couldn't save the setting", { description: errorMessage(e) })
    }
  }
  return (
    <>
      {/* Gone, the plugin leaves this screen for Plugins. */}
      <WithdrawnNotice p={p} onUninstalled={goPlugins} className="mb-8" />
      <Fix p={p} />

      {p.settings.length > 0 && (
        <SettingsGroup
          title="Settings"
          description={
            p.settings.some((s) => s.kind.type === "secret")
              ? "Each run gets them as they are when it starts, and a secret each time it asks for it."
              : "Each run gets them as they are when it starts."
          }
        >
          {unread && values === undefined && (
            <p className="px-5 py-3.5 text-sm text-danger">Couldn't read its settings: {errorMessage(unread)}</p>
          )}
          {p.settings.map((s) => (
            <SettingRow key={`${s.key}:${s.kind.type}`} label={s.title} description={about(s)}>
              {s.kind.type === "secret" ? (
                <SecretSetting p={p} spec={s} placeholder={s.kind.placeholder} />
              ) : (
                <SettingControl
                  spec={s}
                  value={valueOf(s, values)}
                  ready={values !== undefined && p.withdrawn == null}
                  onSave={(v) => void save(s.key, v)} />
              )}
            </SettingRow>
          ))}
        </SettingsGroup>
      )}

      {/* A withdrawn plugin never runs again: nothing here would do anything. */}
      {(p.exportActions.length > 0 || p.events.length > 0) && p.withdrawn == null && (
        <SettingsGroup title="Runs by itself" description="Each runs without being asked once you turn it on, as a job in Activity.">
          {p.exportActions.map((a) => (
            <SettingRow
              key={`action-${a.id}`}
              label={a.title}
              description={runsWhen(a.description, a.afterFailed ? "After every export, also one that failed" : "After every export")}
            >
              <Switch
                aria-label={a.afterFailed ? `Run ${a.title} after every export, also one that failed` : `Run ${a.title} after every export`}
                checked={a.afterExport}
                onCheckedChange={(on) => void setAfterExport(a.id, on)}
              />
            </SettingRow>
          ))}
          {p.events.map((e) => (
            <SettingRow key={`event-${e.id}`} label={e.title} description={runsWhen(e.description, EVENT_WHEN[e.on])}>
              <Switch
                aria-label={`Run ${e.title}, ${lowerFirst(EVENT_WHEN[e.on])}`}
                checked={e.turnedOn}
                onCheckedChange={(on) => void setEvent(e.id, on)}
              />
            </SettingRow>
          ))}
        </SettingsGroup>
      )}

      {p.pages.length > 0 && <Pages p={p} />}
      {(p.pages.length > 0 || p.permissions.length > 0 || p.gpu) && <MayUse p={p} />}

      <SettingsGroup title="Files">
        <SettingRow label="Its folder" description={<PathText path={p.folder} />}>
          <Button size="sm" variant="ghost" onClick={() => reveal(p.folder)}>
            <FolderOpen /> Reveal
          </Button>
        </SettingRow>
        <SettingRow label="Its log" description={<PathText path={p.logPath} />}>
          <Button size="sm" variant="ghost" onClick={() => reveal(p.logPath)}>
            <FolderOpen /> Reveal
          </Button>
        </SettingRow>
        {p.serviceLogPath && (
          <SettingRow label="Its service log" description={<PathText path={p.serviceLogPath} />}>
            <Button size="sm" variant="ghost" onClick={() => reveal(p.serviceLogPath!)}>
              <FolderOpen /> Reveal
            </Button>
          </SettingRow>
        )}
        {(p.resourceBytes ?? 0) > 0 && (
          <SettingRow
            label="Downloaded files"
            description={`${downloadSize(p.resourceBytes ?? 0)}. AutoScene keeps them in its cache and removes them with the plugin.`}
          />
        )}
      </SettingsGroup>

      {p.warnings.length > 0 && (
        <SettingsGroup title="Skipped in its plugin.json" description="What this AutoScene left out of what the plugin asks for.">
          {p.warnings.map((w, i) => (
            <p key={i} className="px-5 py-3.5 text-sm text-fg-2">
              {w}
            </p>
          ))}
        </SettingsGroup>
      )}
    </>
  )
}

/**
 * What keeps a plugin from running, and the control that changes it: one
 * AutoScene hasn't reviewed while they are off, and a broken one. A loaded
 * folder is loaded again through its review, the one way to agree to a
 * changed program; a broken package's reason says what to do. One that is
 * turned off says so by its switch and its dimmed picture.
 *
 * Blocked is a choice of the person's, not a fault: its words are this
 * screen's own (the backend's `blockedReason` sends people to Plugins, where
 * this screen already is), on a calm banner. Broken is a fault, in red.
 */
function Fix({ p }: { p: PluginInfo }) {
  const { checking, loadAgain } = useLoadAgain()
  // The backend refuses a folder's review while unreviewed plugins are off.
  const allowed = useSettings().data?.allowUnreviewedPlugins ?? false
  const blocked = p.broken == null && p.enabled && p.blockedReason != null
  // The withdrawn banner above says why it doesn't run, and nothing here would change that.
  if (p.withdrawn != null || (p.broken == null && !blocked)) return null
  return (
    <Banner
      tone={blocked ? "neutral" : "danger"}
      icon={blocked ? <Prohibit weight="fill" /> : <Warning weight="fill" />}
      title={blocked ? NOT_REVIEWED : "This plugin can't run"}
      description={blocked ? undefined : p.broken}
      actions={
        blocked ? (
          // The backend asks in a dialog of the system's own before it allows any.
          <Button size="sm" variant="ghost" onClick={() => void allowUnreviewed(true)}>
            Allow them…
          </Button>
        ) : (
          p.source === "folder" &&
          allowed && (
            // The icon is the menu entry's: busy, the spinner takes its place and the words stay.
            <Button size="sm" variant="ghost" busy={checking} onClick={() => void loadAgain(p)}>
              <ArrowCounterClockwise /> Load its folder again…
            </Button>
          )
        )
      }
      className="mb-8"
    />
  )
}

/** A plugin's pages: one of its own opens from here, a project's from that project. */
function Pages({ p }: { p: PluginInfo }) {
  const navigate = useNavigate()
  return (
    <SettingsGroup title="Pages" description="Each opens full screen. What is on it is the plugin's.">
      {p.pages.map((pg) => (
        <SettingRow
          key={pg.id}
          label={pg.title}
          description={pg.scope === "project" ? "Open it from a project's More menu." : (pg.description ?? undefined)}
        >
          {pg.scope === "global" && (
            <Button
              size="sm"
              variant="ghost"
              aria-label={`Open ${pg.title}`}
              disabled={!p.active}
              onClick={() => void navigate({ to: pageLink({ pluginId: p.id, pageId: pg.id }) })}
            >
              <AppWindow /> Open
            </Button>
          )}
        </SettingRow>
      ))}
    </SettingsGroup>
  )
}

/**
 * What a plugin may use: what it asked for and the person agreed to, the
 * hosts it reaches, and the secrets set for it. What checks that depends on
 * how it runs: its pages always ask AutoScene, a program needn't, and the
 * sandbox holds a WebAssembly plugin to them, in the review's words.
 */
function MayUse({ p }: { p: PluginInfo }) {
  const secrets = p.settings.filter((s) => s.kind.type === "secret" && p.secretsSet.includes(s.key))
  const when = checkedWhen(p)
  // By how the person agreed it runs: a broken one has no limits to show, but stays a WebAssembly plugin.
  const sandbox = p.runtime === "wasm" ? { hosts: p.networkHosts } : undefined
  const checked =
    p.runtime === "none" || sandbox
      ? (when ?? undefined)
      : `Its program runs with your user rights. ${when ?? "AutoScene doesn't check these: they are what it says it does."}`
  return (
    <SettingsGroup title="What it may use" description={checked}>
      {p.permissions.map((perm) => (
        <SettingRow
          key={perm}
          leading={<ShieldCheck className="size-5 shrink-0 text-fg-3" />}
          label={permissionLabel(perm, sandbox)}
          description={perm === "network" && p.networkHosts.length > 0 ? p.networkHosts.join(", ") : undefined}
        />
      ))}
      {p.gpu && (
        <SettingRow
          leading={<GraphicsCard className="size-5 shrink-0 text-fg-3" />}
          label="Uses the graphics card"
          description="Its runs wait while an analysis runs."
        />
      )}
      {p.permissions.length === 0 && !p.gpu && (
        <p className="px-5 py-3.5 text-sm text-fg-2">Nothing beyond its own settings.</p>
      )}
      {secrets.map((s) => (
        <SettingRow
          key={s.key}
          leading={<ShieldCheck className="size-5 shrink-0 text-fg-3" />}
          label={s.title}
          description="Saved in your system's keychain"
        />
      ))}
    </SettingsGroup>
  )
}

/**
 * What the row of something that runs by itself says under its title: what
 * it does, as its plugin says, and `when` it runs.
 */
function runsWhen(description: string | null, when: string): React.ReactNode {
  if (!description) return when
  return (
    <>
      <span className="block">{description}</span>
      <span className="block">{when}</span>
    </>
  )
}

/** `text` with its first letter small, to follow a comma. */
function lowerFirst(text: string): string {
  return text.charAt(0).toLowerCase() + text.slice(1)
}

/**
 * What the row of `s` says under its title. On macOS a secret also says that
 * the Keychain may ask again: each update of AutoScene is signed anew.
 */
function about(s: SettingSpec): React.ReactNode {
  const again = s.kind.type === "secret" && isMac ? "macOS may ask you to allow it again after AutoScene updates." : null
  if (!again) return s.description ?? undefined
  if (!s.description) return again
  return (
    <>
      <span className="block">{s.description}</span>
      <span className="block">{again}</span>
    </>
  )
}

/**
 * The value of `s` in `values`, else its default; one of another type (its
 * plugin.json changed) counts as none. Only the object's own keys count: a
 * setting may be called `constructor`.
 */
function valueOf(s: SettingSpec, values: Values | undefined): SettingValue {
  const v = values && Object.hasOwn(values, s.key) ? values[s.key] : undefined
  const k = s.kind
  switch (k.type) {
    case "boolean":
      return typeof v === "boolean" ? v : k.default
    case "number":
      return typeof v === "number" ? v : k.default
    case "string":
      return typeof v === "string" ? v : (k.default ?? "")
    case "select":
      return typeof v === "string" && k.options.some((o) => o.value === v) ? v : k.default
    case "secret":
      return null
  }
}

/** `n` within the bounds of `k`, and whole when it must be. */
function bounded(n: number, k: Pick<NumberKind, "min" | "max" | "integer">): number {
  let v = k.integer ? Math.round(n) : n
  if (k.min != null) v = Math.max(k.min, v)
  if (k.max != null) v = Math.min(k.max, v)
  return v
}

function SettingControl({
  spec,
  value,
  ready,
  onSave,
}: {
  spec: SettingSpec
  value: SettingValue
  ready: boolean
  onSave: (v: SettingValue) => void
}) {
  const k = spec.kind
  switch (k.type) {
    case "boolean":
      return <Switch aria-label={spec.title} checked={value === true} disabled={!ready} onCheckedChange={onSave} />
    case "number": {
      const step = k.min != null && k.max != null ? sliderStep(k, k.min, k.max) : null
      return k.min != null && k.max != null && step != null ? (
        <SliderSetting spec={spec} kind={k} min={k.min} max={k.max} step={step} value={value} ready={ready} onSave={onSave} />
      ) : (
        <NumberSetting spec={spec} kind={k} value={value} ready={ready} onSave={onSave} />
      )
    }
    case "select": {
      const current = typeof value === "string" ? value : undefined
      const totalChars = k.options.reduce((sum, o) => sum + o.label.length, 0)
      const fitsInSegmented = k.options.length <= 3 && totalChars <= 32
      return fitsInSegmented ? (
        <Segmented aria-label={spec.title} value={current ?? ""} disabled={!ready} onChange={onSave} options={k.options} />
      ) : (
        <Select
          aria-label={spec.title}
          className="w-[260px]"
          value={current}
          placeholder="Choose…"
          disabled={!ready}
          onChange={onSave}
          options={k.options}
        />
      )
    }
    case "string":
      return <TextSetting spec={spec} kind={k} value={value} ready={ready} onSave={onSave} />
    case "secret":
      // Never a value here: see SecretSetting.
      return null
  }
}

/**
 * A secret setting: whether the keychain holds one, set in a dialog of its
 * own and never shown again, and removed once the person says so.
 */
function SecretSetting({ p, spec, placeholder }: { p: PluginInfo; spec: SettingSpec; placeholder: string | null }) {
  const [setting, setSetting] = useState(false)
  const [removing, setRemoving] = useState(false)
  const stored = p.secretsSet.includes(spec.key)
  return (
    <div className="flex items-center gap-3">
      <span className="text-sm text-fg-2">{stored ? "Saved in your system's keychain" : "Not set"}</span>
      <Button size="sm" variant="ghost" aria-label={`Set ${spec.title}`} disabled={p.withdrawn != null} onClick={() => setSetting(true)}>
        Set…
      </Button>
      {stored && (
        <Button size="sm" variant="ghost" aria-label={`Remove ${spec.title}`} onClick={() => setRemoving(true)}>
          Remove…
        </Button>
      )}
      {setting && <SecretDialog p={p} spec={spec} placeholder={placeholder} onDone={() => setSetting(false)} />}
      {removing && <RemoveSecretDialog p={p} spec={spec} onDone={() => setRemoving(false)} />}
    </div>
  )
}

/**
 * Asks before a secret leaves the keychain: many services show a token only
 * once. While it goes the dialog stays, and Remove waits.
 */
function RemoveSecretDialog({ p, spec, onDone }: { p: PluginInfo; spec: SettingSpec; onDone: () => void }) {
  return (
    <ConfirmDialog
      open
      onOpenChange={(open) => !open && onDone()}
      title={`Remove ${spec.title}?`}
      description="It goes from your system's keychain, and the plugin can't use it until you enter it again."
      answer="Remove"
      failure="Couldn't remove the secret"
      onConfirm={async () => keepPlugin(await commands.pluginSecretClear(p.id, spec.key))}
    />
  )
}

/** Asks for a secret, hidden as it is typed. It goes to the keychain and leaves with the dialog. */
function SecretDialog({
  p,
  spec,
  placeholder,
  onDone,
}: {
  p: PluginInfo
  spec: SettingSpec
  placeholder: string | null
  onDone: () => void
}) {
  const [value, setValue] = useState("")
  const [busy, setBusy] = useState(false)
  const id = useId()
  const inputId = `${id}-secret`
  const errorId = `${id}-problem`
  const tooLong = value.length > MAX_SECRET
  const save = async () => {
    if (value === "" || tooLong) return
    setBusy(true)
    try {
      keepPlugin(await commands.pluginSecretSet(p.id, spec.key, value))
      onDone()
    } catch (e) {
      toast.error("Couldn't save the secret", { description: errorMessage(e) })
      setBusy(false)
    }
  }
  return (
    <Dialog open onOpenChange={(open) => !open && !busy && onDone()}>
      <DialogContent title={spec.title} description="Kept in your system's keychain. AutoScene never shows it again." hideClose={busy}>
        <form
          onSubmit={(e) => {
            e.preventDefault()
            void save()
          }}
        >
          {/* The dialog's title is its label, so the field's is for a screen reader only. */}
          <Field
            label={spec.title}
            hideLabel
            htmlFor={inputId}
            error={tooLong ? `A secret can be at most ${MAX_SECRET} characters.` : undefined}
            errorId={errorId}
          >
            <Input
              id={inputId}
              type="password"
              autoFocus
              aria-invalid={tooLong}
              aria-describedby={tooLong ? errorId : undefined}
              value={value}
              placeholder={placeholder ?? undefined}
              disabled={busy}
              onChange={(e) => setValue(e.target.value)}
            />
          </Field>
          <DialogFooter>
            <Button variant="ghost" onClick={onDone} disabled={busy}>
              Cancel
            </Button>
            <Button type="submit" variant="primary" busy={busy} disabled={value === "" || tooLong}>
              Save
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

/** A text field that saves once it is left (Enter leaves it). */
function TextSetting({
  spec,
  kind,
  value,
  ready,
  onSave,
}: {
  spec: SettingSpec
  kind: Extract<SettingKind, { type: "string" }>
  value: SettingValue
  ready: boolean
  onSave: (v: string) => void
}) {
  const [draft, setDraft] = useState<string | null>(null)
  const stored = typeof value === "string" ? value : ""
  return (
    <Input
      aria-label={spec.title}
      className="w-[260px]"
      value={draft ?? stored}
      maxLength={kind.maxLength}
      placeholder={kind.placeholder ?? undefined}
      disabled={!ready}
      onChange={(e) => setDraft(e.target.value)}
      onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
      onBlur={() => {
        if (draft !== null && draft !== stored) onSave(draft)
        setDraft(null)
      }}
    />
  )
}

/** A number without both bounds: typed, and saved within its bounds once the field is left. */
function NumberSetting({
  spec,
  kind,
  value,
  ready,
  onSave,
}: {
  spec: SettingSpec
  kind: NumberKind
  value: SettingValue
  ready: boolean
  onSave: (v: number) => void
}) {
  const [draft, setDraft] = useState<string | null>(null)
  const stored = typeof value === "number" ? value : null
  const leave = () => {
    if (draft === null) return
    setDraft(null)
    const text = draft.trim().replace(",", ".")
    if (text === "" || text === String(stored ?? "")) return
    const n = Number(text)
    if (!Number.isFinite(n)) {
      toast.error("Couldn't save the setting", { description: `${spec.title} takes a number.` })
      return
    }
    const next = bounded(n, kind)
    if (next !== stored) onSave(next)
  }
  return (
    <Input
      aria-label={spec.title}
      inputMode="decimal"
      className="tabular w-[140px] text-right"
      value={draft ?? (stored == null ? "" : String(stored))}
      disabled={!ready}
      onChange={(e) => setDraft(e.target.value)}
      onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
      onBlur={leave}
    />
  )
}

/**
 * The step of a slider from `min` to `max`, or null when they make none: no
 * room between them, or more than a number holds, so that a slider would
 * only give NaN.
 */
function sliderStep(k: NumberKind, min: number, max: number): number | null {
  const step = k.step ?? (k.integer ? 1 : (max - min) / 100)
  return max > min && Number.isFinite(max - min) && Number.isFinite(step) && step > 0 ? step : null
}

/** A number between two bounds: a slider, saved once it rests (and when the page goes meanwhile). */
function SliderSetting({
  spec,
  kind,
  min,
  max,
  step,
  value,
  ready,
  onSave,
}: {
  spec: SettingSpec
  kind: NumberKind
  min: number
  max: number
  step: number
  value: SettingValue
  ready: boolean
  onSave: (v: number) => void
}) {
  const [draft, setDraft] = useState<number | null>(null)
  const settle = useSettled((v: number) => {
    onSave(v)
    setDraft(null)
  })
  const decimals = kind.integer ? 0 : Math.min(4, (String(step).split(".")[1] ?? "").length)
  const format = (v: number) => v.toFixed(decimals)
  const shown = draft ?? (typeof value === "number" ? value : min)
  return (
    <div className="flex w-[260px] items-center gap-3">
      <Slider
        label={spec.title}
        className="flex-1"
        min={min}
        max={max}
        step={step}
        value={shown}
        format={format}
        disabled={!ready}
        onChange={(v) => {
          if (!Number.isFinite(v)) return
          const next = bounded(v, kind)
          setDraft(next)
          settle(next)
        }}
      />
      <span className="tabular w-12 text-right text-sm text-fg-2">{format(shown)}</span>
    </div>
  )
}

/**
 * `save`, called with a value once no other came for `SETTLE_MS`, and at
 * once for one still waiting when the control goes.
 */
function useSettled<T>(save: (v: T) => void): (v: T) => void {
  const latest = useRef(save)
  const waiting = useRef<{ value: T; timer: number } | null>(null)
  useEffect(() => {
    latest.current = save
  })
  const flush = useCallback(() => {
    const w = waiting.current
    if (!w) return
    window.clearTimeout(w.timer)
    waiting.current = null
    latest.current(w.value)
  }, [])
  useEffect(() => flush, [flush])
  return useCallback(
    (value: T) => {
      if (waiting.current) window.clearTimeout(waiting.current.timer)
      waiting.current = { value, timer: window.setTimeout(flush, SETTLE_MS) }
    },
    [flush],
  )
}
