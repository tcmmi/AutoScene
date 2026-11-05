import { type ReactNode, useEffect, useLayoutEffect, useRef, useState } from "react"
import { FolderOpen, Warning } from "@/components/icons"
import { useTranslation } from "react-i18next"
import { Toolbar } from "radix-ui"
import { toast } from "sonner"
import type { Character, ExportPlan } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Tooltip, TruncatedText } from "@/components/ui/tooltip"
import { Dialog, DialogContent, DialogFooter } from "@/components/ui/dialog"
import { Field, Input } from "@/components/ui/input"
import { Select } from "@/components/ui/select"
import { OptionRow } from "@/components/ui/option-row"
import { Switch } from "@/components/ui/switch"
import { rememberPreset } from "@/features/export/export"
import { formatBytes, shortFormatName } from "@/lib/format"
import { say } from "@/lib/i18n"
import { commands, errorMessage } from "@/lib/ipc"
import { pickFolder } from "@/lib/media"
import {
  ALL_PARTS,
  FILE_PARTS,
  insertion,
  type NamePart,
  parseTemplate,
  reportedPart,
  type Segment,
} from "@/lib/naming"
import { qk, queryClient, useExportDir, usePresets, useProject, useSettings } from "@/lib/queries"
import { insertText, restoreSelection, revealCaret } from "@/lib/text-edit"
import { unfinishedJobs } from "@/state/activity"
import { useUi } from "@/state/ui"
import { type Pack, packExportStarted } from "./use-pack"

/** The part of `template` the backend names when it refuses it, for the one file's name or not. */
const reported = (template: string, forFile: boolean): Segment | null =>
  reportedPart(parseTemplate(template, forFile ? FILE_PARTS : ALL_PARTS), forFile)

/** Is the backend's refusal about this template? An empty one has no part to name, but is refused too. */
const refuses = (template: string, forFile: boolean) => template.trim() === "" || reported(template, forFile) != null

/** How long a part being typed isn't called wrong yet, after the last key. */
const TYPING_MS = 1000

/**
 * The pack's plan as it last answered: the last one that worked, and the
 * problem of the last one that didn't until one works again. While the next
 * answer is on its way, both stay: a template that doesn't work keeps the
 * dialog as the last one that did, and nothing blinks while you type on.
 */
function useLastAnswer(plan: Pack["plan"]): { plan: ExportPlan | undefined; problem: string | null } {
  const [last, setLast] = useState(() => ({ plan: plan.data, problem: plan.isError ? errorMessage(plan.error) : null }))
  const next = plan.isError
    ? { plan: last.plan, problem: errorMessage(plan.error) }
    : plan.data && !plan.isPlaceholderData
      ? { plan: plan.data, problem: null }
      : last
  if (next.plan !== last.plan || next.problem !== last.problem) setLast(next)
  return next
}

/** Name the pack's files, choose format and place, and export exactly the shots the grid shows. */
export function PackExportDialog({
  pid,
  character,
  pack,
  open,
  onOpenChange,
}: {
  pid: string
  character: Character
  pack: Pack
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      {open && pack.state && <ExportForm pid={pid} character={character} pack={pack} onDone={() => onOpenChange(false)} />}
    </Dialog>
  )
}

function ExportForm({ pid, character, pack, onDone }: { pid: string; character: Character; pack: Pack; onDone: () => void }) {
  const { t } = useTranslation(["pack", "export", "common"])
  const { data: presets } = usePresets()
  const { data: settings } = useSettings()
  const { data: defaultDir } = useExportDir(pid)
  const { data: project } = useProject(pid)
  const [chosenPreset, setPresetId] = useState<string>()
  const [dir, setDir] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const state = pack.state!
  const { naming } = state
  const merge = naming.merge
  const presetId = chosenPreset ?? settings?.exportPresetId
  const preset = presets?.find((p) => p.id === presetId)
  const clips = pack.preview.data?.totals.clips ?? 0
  const { plan, problem } = useLastAnswer(pack.plan)
  const names = plan?.names ?? []
  const size = plan?.sizes.find((s) => s.presetId === presetId)
  const bytes = size?.bytes
  // Bit-Perfect joins only episodes alike: otherwise a file per shot, and why.
  const apart = merge ? (size?.apart ?? null) : null
  const oneFile = merge && apart == null
  const file = oneFile ? plan?.file : null
  // A template that doesn't work: the backend says why, under the field it is
  // about. It checks the one file's name first, then the names' template.
  const fileRefused = merge && refuses(naming.file, true)
  // The backend checks the one file's name even when Bit-Perfect makes a file
  // per shot: while it refuses it, its field stays, to mend it where it is.
  // Mended there, it stays while the dialog is open: it never goes from under
  // the keys or moves what is below it under the pointer.
  const [mending, setMending] = useState(false)
  const showFile = oneFile || fileRefused || (merge && mending)
  const fileProblem = problem && fileRefused && showFile ? problem : null
  const namesProblem = problem && !fileRefused && refuses(naming.template, false) ? problem : null
  // About neither (a character gone, the database): above the buttons, once
  // the backend says so (not while a name just mended is on its way).
  const dialogProblem = problem && !fileProblem && !namesProblem && pack.plan.isError ? problem : null
  const base = dir ?? defaultDir ?? ""
  const sep = base.includes("\\") ? "\\" : "/"
  // The character's folder as the backend names it (a file per shot only).
  const folder = naming.folder && !oneFile ? plan?.folder : null
  const outDir = folder && base ? `${base.replace(/[\\/]+$/, "")}${sep}${folder}` : base
  // The export takes exactly the shots the grid shows: episodes still being
  // analysed add theirs later, and so do those still being indexed (their
  // analysis starts once that is done). Only episodes the pack takes: one
  // its filter leaves out isn't in this export anyway. Counted per episode
  // (a restart's old and new job are one), once, as it opens: a job ending
  // meanwhile never moves the dialog under the pointer.
  const [analysing] = useState(() => {
    const { media } = state.spec
    const pending = unfinishedJobs(
      (j) =>
        (j.kind === "analyze" || j.kind === "ingest") &&
        j.projectId === pid &&
        j.mediaId != null &&
        (media.length === 0 || media.includes(j.mediaId)),
    )
    return new Set(pending.map((j) => j.mediaId)).size
  })
  const missing =
    analysing > 0 && project
      ? t(project.kind === "movie" ? "export.analysing.file" : "export.analysing.episode", { count: analysing, name: character.label })
      : null

  const start = async () => {
    const stamp = pack.preview.data?.stamp
    if (!presetId || stamp == null) return
    setBusy(true)
    try {
      const ex = await commands.packExport(pid, character.id, { spec: state.spec, naming, presetId, outDir: dir, stamp })
      // The pack's Export button follows it from now on.
      packExportStarted(pid, character.id, ex.id)
      rememberPreset(settings, presetId)
      onDone()
      toast(t(oneFile ? "export.startedOneFile" : "export.started", { count: clips, name: character.label }), {
        action: { label: t("export:show"), onClick: () => useUi.getState().setActivityOpen(true) },
      })
    } catch (e) {
      toast.error(t("export:startFailed"), { description: errorMessage(e) })
      // Refused because the pack changed (a file gone, an analysis done):
      // show it as it is now.
      void queryClient.invalidateQueries({ queryKey: qk.pack(pid, character.id) })
    } finally {
      setBusy(false)
    }
  }

  const nameList = names.length > 0 && (
    <span className="flex flex-col gap-0.5 pt-0.5 font-mono text-xs text-fg-2">
      {names.slice(0, 3).map((n, i) => (
        <span key={i} className="truncate" data-selectable>
          {n}
        </span>
      ))}
      {clips > names.length && <span className="text-fg-3">{t("export.more", { count: clips - names.length })}</span>}
    </span>
  )

  return (
    <DialogContent
      title={t("export.title", { name: character.label })}
      description={
        <>
          {t(oneFile ? "export.descriptionOneFile" : "export.description", { count: clips })}
          {/* A line of its own, read after a space. */}
          {missing && (
            <>
              {" "}
              <span className="block pt-1">{missing}</span>
            </>
          )}
        </>
      }
      // At the smallest window the dialog scrolls inside instead of leaving
      // its buttons below the window.
      className="max-h-[calc(100vh-48px)] w-[min(600px,calc(100vw-32px))] overflow-y-auto"
      // The field the dialog opens with takes the focus (Enter exports: Ctrl/⌘ E,
      // then Enter). One that shows up later doesn't take it from the switch.
      onOpenAutoFocus={(e) => {
        e.preventDefault()
        document.getElementById(oneFile ? "pack-file" : "pack-template")?.focus()
      }}
    >
      <form
        className="flex flex-col gap-5"
        onSubmit={(e) => {
          e.preventDefault()
          void start()
        }}
      >
        <OptionRow
          label={t("export.oneFile.label")}
          description={t("export.oneFile.description")}
          checked={merge}
          onCheckedChange={(merge) => {
            setMending(false)
            pack.setNaming({ ...naming, merge })
          }}
        />
        {showFile && (
          <TemplateField
            id="pack-file"
            label={t("export.fileName")}
            hint={
              file &&
              preset && (
                <span className="block truncate pt-0.5 font-mono text-xs text-fg-2" data-selectable>
                  {file}.{size?.ext ?? preset.container}
                </span>
              )
            }
            problem={fileProblem}
            value={naming.file}
            onChange={(f) => {
              if (!oneFile) setMending(true)
              pack.setNaming({ ...naming, file: f })
            }}
            forFile
          />
        )}
        <TemplateField
          id="pack-template"
          label={oneFile ? t("export.chapterNames") : t("export.fileNames")}
          hint={nameList}
          problem={namesProblem}
          value={naming.template}
          onChange={(template) => pack.setNaming({ ...naming, template })}
        />
        <Field
          label={t("export:field.format")}
          hint={
            preset && (
              <>
                {say(preset.description)}
                {bytes != null && bytes > 0 && (
                  <span className="text-fg-2">
                    {" "}
                    {t("export:about", { size: formatBytes(bytes), format: shortFormatName(say(preset.name)) })}
                  </span>
                )}
                {oneFile && bytes != null && bytes > 0 && ` ${t("export.freeSpace")}`}
                {apart && <span className="block pt-1 text-fg-2">{say(apart)}</span>}
                {size?.note && <span className="block pt-1 text-fg-2">{say(size.note)}</span>}
              </>
            )
          }
        >
          <Select
            aria-label={t("export:field.formatAria")}
            value={presetId}
            onChange={setPresetId}
            placeholder={t("export:field.formatPlaceholder")}
            options={(presets ?? []).map((p) => ({ value: p.id, label: say(p.name) }))}
          />
        </Field>
        <Field label={t("export:field.destination")}>
          <div className="flex gap-2">
            <div
              data-selectable
              className="flex h-10 min-w-0 flex-1 items-center rounded-lg border border-soft bg-white/[0.04] px-3 font-mono text-sm text-fg-2"
            >
              <TruncatedText>{outDir || "…"}</TruncatedText>
            </div>
            <Button size="md" className="h-10" onClick={() => void pickFolder(t("export:pickDestination")).then((p) => p && setDir(p))}>
              <FolderOpen /> {t("export:change")}
            </Button>
          </div>
          {!oneFile && (
            <label className="mt-3 flex items-center gap-3 text-md text-fg-2">
              <Switch checked={naming.folder} onCheckedChange={(folder) => pack.setNaming({ ...naming, folder })} />
              {t("export.folder", { name: character.label })}
            </label>
          )}
        </Field>
        <p aria-live="polite" className={dialogProblem ? "flex items-start gap-1.5 text-sm text-danger" : "sr-only"}>
          {dialogProblem && (
            <>
              <Warning className="mt-0.5 size-3.5 shrink-0" />
              <span>{dialogProblem}</span>
            </>
          )}
        </p>
        <DialogFooter className="mt-1">
          <Button variant="ghost" onClick={onDone}>
            {t("common:action.cancel")}
          </Button>
          <Button
            type="submit"
            size="lg"
            variant="primary"
            busy={busy}
            disabled={!presetId || clips === 0 || problem != null || !pack.current}
          >
            {t(oneFile ? "export.submitOneFile" : "export.submit", { count: clips })}
          </Button>
        </DialogFooter>
      </form>
    </DialogContent>
  )
}

/**
 * Where a name-part button puts its part in `field`: over the selection the
 * field shows, as a key would. A field without the focus keeps the selection
 * it had, except the select-all a Tab into it leaves: then the part goes at
 * the end, so a button pressed by keyboard never replaces the whole name.
 */
function insertionRange(field: HTMLInputElement): { start: number; end: number } {
  const length = field.value.length
  const start = field.selectionStart ?? length
  const end = field.selectionEnd ?? start
  const tabbedIn = document.activeElement !== field && length > 0 && start === 0 && end === length
  return tabbedIn ? { start: length, end: length } : { start, end }
}

/**
 * A name template: its label, the field, buttons that put a name part where
 * the cursor is, and what it gives, or why it won't do.
 */
function TemplateField({
  id,
  label,
  hint,
  problem,
  value,
  onChange,
  forFile = false,
}: {
  id: string
  label: string
  /** What it gives. */
  hint: ReactNode
  /** Why the backend refuses the template, when it is about this one. */
  problem: string | null
  value: string
  onChange: (value: string) => void
  /** The one file's name: only the parts that name the whole pack, one button each. */
  forFile?: boolean
}) {
  const { t } = useTranslation("pack")
  const parts = forFile ? FILE_PARTS : ALL_PARTS
  const input = useRef<HTMLInputElement>(null)
  // A part still being typed isn't wrong yet: while the caret is in the part
  // the backend names and a key went in less than a second ago, the field
  // waits. A second later, on leaving the part or the field, it says so.
  const [typing, setTyping] = useState(false)
  const [caret, setCaret] = useState<number | null>(null)
  const pause = useRef<number | undefined>(undefined)
  useEffect(() => () => window.clearTimeout(pause.current), [])
  const typed = (field: HTMLInputElement) => {
    setCaret(field.selectionStart)
    setTyping(true)
    window.clearTimeout(pause.current)
    pause.current = window.setTimeout(() => setTyping(false), TYPING_MS)
  }
  // Is this piece being typed: the caret in it, a key in less than a second ago?
  const typedIn = (s: Segment) => typing && caret != null && s.start < caret && (s.kind === "unclosed" || caret < s.end)
  const part = problem ? reported(value, forFile) : null
  const error = part && typedIn(part) ? null : problem
  const segments = parseTemplate(value, parts)
  // The parts stand out behind the field's own text, and while its problem
  // shows a mistyped one is red where it is (not one still being typed).
  const layer = useRef<HTMLDivElement>(null)
  const follow = () => {
    if (layer.current && input.current) layer.current.scrollLeft = input.current.scrollLeft
  }
  useLayoutEffect(follow, [value])
  const errorId = `${id}-problem`
  // A button types its part as a key would: Ctrl+Z takes it back, React hears
  // it, and the focus ends in this field with the caret after the part.
  const insert = (part: NamePart) => {
    const field = input.current
    if (!field) return
    const { start, end } = insertionRange(field)
    const at = insertion(field.value, start, end, part)
    // Always set the selection, also in a focused field: that ends the typing
    // before, so each press is its own undo step.
    restoreSelection(field, at)
    insertText(field, at.text)
    // The webview scrolls to a caret only for typing: show where the part went.
    revealCaret(field)
  }
  return (
    <Field label={label} htmlFor={id} hint={hint} error={error} errorId={errorId}>
      <div className="relative">
        {/* The field's text box (its 1 px border and px-3 off), grown by the
            pills' 2 px edge: the field clips its text there. The text sits as
            the field's does, to the pixel wherever the dialog lands: one line
            as tall as the box (h-10 less the border; centring it in a flex box
            puts it a pixel off at some positions), rendered as a field renders
            (a field doesn't inherit text-rendering). The pieces sit in one
            inline span. */}
        <div
          ref={layer}
          aria-hidden
          className="pointer-events-none absolute inset-y-px right-[calc(--spacing(3)-1px)] left-[calc(--spacing(3)-1px)] overflow-hidden px-[2px] font-mono text-sm leading-[calc(--spacing(10)-2px)] whitespace-pre text-transparent [text-rendering:auto]"
        >
          <span>
            {segments.map((s) =>
              s.kind === "text" ? (
                s.text
              ) : (
                <span
                  key={s.start}
                  data-part={s.kind}
                  data-bad={error && s.kind !== "part" && !typedIn(s) ? "" : undefined}
                  className="rounded-[4px] bg-white/[0.07] shadow-[0_0_0_2px_rgb(255_255_255/0.07)] data-bad:bg-danger-subtle data-bad:underline data-bad:decoration-danger data-bad:decoration-wavy data-bad:underline-offset-4 data-bad:shadow-[0_0_0_2px_var(--danger-subtle)]"
                >
                  {s.text}
                </span>
              ),
            )}
          </span>
        </div>
        <Input
          id={id}
          ref={input}
          value={value}
          onChange={(e) => {
            typed(e.target)
            onChange(e.target.value)
          }}
          onSelect={(e) => setCaret(e.currentTarget.selectionStart)}
          onBlur={() => {
            window.clearTimeout(pause.current)
            setTyping(false)
          }}
          onScroll={follow}
          // Over the layer: its own text, caret and selection stay the field's.
          className="relative block font-mono text-sm"
          // Only while its problem shows: the names under it never describe it.
          aria-invalid={error ? true : undefined}
          aria-describedby={error ? errorId : undefined}
        />
      </div>
      {/* One Tab stop: the arrows, Home and End go from button to button. */}
      <Toolbar.Root
        aria-label={t("export.nameParts")}
        aria-controls={id}
        loop
        // The pointer never takes the focus from a field, on a button or
        // between them: its caret and selection stay put.
        onMouseDown={(e) => e.preventDefault()}
        className="mt-2 flex flex-wrap gap-1.5"
      >
        {parts.map((part) => (
          // The spelling to type by hand, and what it gives. Below the button:
          // above, it would cover the caret, where the part goes.
          <Tooltip key={part} label={<span className="font-mono">{`{${part}}`}</span>} hint={t(`namePart.${part}.hint`)}>
            <Toolbar.Button
              onClick={() => insert(part)}
              className="select-none rounded-full border border-white/10 bg-white/[0.04] px-2.5 text-xs leading-6 text-fg-2 transition-[border-color,color,scale] duration-[var(--dur-fast)] ease-out hover:border-white/20 hover:text-fg-1 active:scale-[0.97]"
            >
              {t(`namePart.${part}.label`)}
            </Toolbar.Button>
          </Tooltip>
        ))}
      </Toolbar.Root>
    </Field>
  )
}
