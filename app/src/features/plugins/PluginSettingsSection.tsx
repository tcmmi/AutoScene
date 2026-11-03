import type * as React from "react"
import { useCallback, useEffect, useRef, useState } from "react"
import { useNavigate } from "@tanstack/react-router"
import { toast } from "sonner"
import type { PluginInfo, SettingKind, SettingSpec, SettingValue } from "@/bindings"
import { AppWindow, CircleNotch, FolderOpen, PuzzlePiece, ShieldCheck } from "@/components/icons"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogFooter } from "@/components/ui/dialog"
import { EmptyState } from "@/components/ui/empty-state"
import { Input } from "@/components/ui/input"
import { Segmented } from "@/components/ui/segmented"
import { Select } from "@/components/ui/select"
import { Slider } from "@/components/ui/slider"
import { Switch } from "@/components/ui/switch"
import { PathText, SettingRow, SettingsGroup } from "@/features/settings/primitives"
import { useAppActions } from "@/lib/actions"
import { commands, errorMessage } from "@/lib/ipc"
import { reveal } from "@/lib/os"
import { isMac } from "@/lib/platform"
import { qk, queryClient, usePluginSettings, usePlugins } from "@/lib/queries"
import { pageLink } from "./page-items"
import { keepPlugin } from "./plugin-actions"
import { permissionLabel } from "./plugin-items"

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

/** The window title's name of a plugin's settings. */
export function PluginCrumb({ id }: { id: string }) {
  return <>Settings · <PluginName id={id} /></>
}

/** A plugin's name, as its settings are headed. */
export function PluginName({ id }: { id: string }) {
  const { data } = usePlugins()
  return <>{data?.find((p) => p.id === id)?.name ?? "Plugin"}</>
}

/**
 * A plugin's settings: what it lets the person set (a secret only into the
 * system's keychain), the export actions to run after every export, its
 * pages, what it may use, and where its files and log are.
 */
export function PluginSettingsSection({ id }: { id: string }) {
  const { data: plugins, isPending, error } = usePlugins()
  const { goSettings } = useAppActions()
  const p = plugins?.find((x) => x.id === id)
  if (p) return <PluginSettings p={p} />
  if (isPending) return null
  return (
    <EmptyState
      icon={<PuzzlePiece />}
      tone={error ? "danger" : "accent"}
      title={error ? "Couldn't read the plugins" : "This plugin isn't installed"}
      description={error ? errorMessage(error) : "It was uninstalled, or this link is to another one."}
      actions={<Button onClick={() => goSettings("plugins")}>Go to Plugins</Button>}
    />
  )
}

function PluginSettings({ p }: { p: PluginInfo }) {
  const { data: values, error: unread } = usePluginSettings(p.id)
  const { goSettings } = useAppActions()
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
  const why = p.broken ?? (p.enabled ? p.blockedReason : `${p.name} is turned off.`)
  return (
    <>
      {p.description && <p className="-mt-4 mb-8 text-md text-fg-2">{p.description}</p>}
      {why && (
        <div className="mb-8 flex items-center justify-between gap-4 rounded-2xl border border-white/[0.07] bg-white/[0.03] px-5 py-4">
          <p className="text-md text-fg-1">{why}</p>
          <Button size="sm" variant="ghost" onClick={() => goSettings("plugins")}>
            Go to Plugins
          </Button>
        </div>
      )}

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
                <SettingControl spec={s} value={valueOf(s, values)} ready={values !== undefined} onSave={(v) => void save(s.key, v)} />
              )}
            </SettingRow>
          ))}
        </SettingsGroup>
      )}

      {p.exportActions.length > 0 && (
        <SettingsGroup title="After every export" description="Runs by itself each time an export finishes with every clip made.">
          {p.exportActions.map((a) => (
            <SettingRow key={a.id} label={a.title} description={a.description ?? undefined}>
              <Switch
                aria-label={`Run ${a.title} after every export`}
                checked={a.afterExport}
                onCheckedChange={(on) => void setAfterExport(a.id, on)}
              />
            </SettingRow>
          ))}
        </SettingsGroup>
      )}

      {p.pages.length > 0 && <Pages p={p} />}
      {(p.pages.length > 0 || p.permissions.length > 0) && <MayUse p={p} />}

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
 * how it runs: its pages always ask AutoScene, a program needn't.
 */
function MayUse({ p }: { p: PluginInfo }) {
  const secrets = p.settings.filter((s) => s.kind.type === "secret" && p.secretsSet.includes(s.key))
  const checked =
    p.runtime === "none"
      ? "AutoScene checks these each time its pages ask."
      : "Its program runs with your user rights. AutoScene checks these only when its pages ask."
  return (
    <SettingsGroup title="What it may use" description={checked}>
      {p.permissions.map((perm) => (
        <SettingRow
          key={perm}
          leading={<ShieldCheck className="size-5 shrink-0 text-fg-3" />}
          label={permissionLabel(perm)}
          description={perm === "network" && p.networkHosts.length > 0 ? p.networkHosts.join(", ") : undefined}
        />
      ))}
      {p.permissions.length === 0 && <p className="px-5 py-3.5 text-sm text-fg-2">Nothing beyond its own settings.</p>}
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
      return k.options.length <= 3 ? (
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
      <Button size="sm" variant="ghost" aria-label={`Set ${spec.title}`} onClick={() => setSetting(true)}>
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
  const [busy, setBusy] = useState(false)
  const remove = async () => {
    setBusy(true)
    try {
      keepPlugin(await commands.pluginSecretClear(p.id, spec.key))
      onDone()
    } catch (e) {
      toast.error("Couldn't remove the secret", { description: errorMessage(e) })
      setBusy(false)
    }
  }
  return (
    <Dialog open onOpenChange={(open) => !open && !busy && onDone()}>
      <DialogContent
        title={`Remove ${spec.title}?`}
        description="It goes from your system's keychain, and the plugin can't use it until you enter it again."
      >
        <DialogFooter>
          <Button variant="ghost" disabled={busy} onClick={onDone}>
            Cancel
          </Button>
          <Button variant="danger" disabled={busy} onClick={() => void remove()}>
            {busy && <CircleNotch className="animate-spin" />}
            Remove
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
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
    <Dialog open onOpenChange={(open) => !open && onDone()}>
      <DialogContent title={spec.title} description="Kept in your system's keychain. AutoScene never shows it again.">
        <form
          onSubmit={(e) => {
            e.preventDefault()
            void save()
          }}
        >
          <Input
            type="password"
            autoFocus
            autoComplete="off"
            spellCheck={false}
            aria-label={spec.title}
            aria-invalid={tooLong}
            value={value}
            placeholder={placeholder ?? undefined}
            disabled={busy}
            onChange={(e) => setValue(e.target.value)}
          />
          {tooLong && <p className="mt-2 text-sm text-danger">A secret can be at most {MAX_SECRET} characters.</p>}
          <DialogFooter>
            <Button variant="ghost" onClick={onDone} disabled={busy}>
              Cancel
            </Button>
            <Button type="submit" variant="primary" disabled={busy || value === "" || tooLong}>
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
