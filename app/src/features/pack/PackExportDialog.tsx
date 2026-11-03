import { type ReactNode, useRef, useState } from "react"
import { CircleNotch, FolderOpen } from "@/components/icons"
import { Toolbar } from "radix-ui"
import { toast } from "sonner"
import type { Character } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Tooltip, TruncatedText } from "@/components/ui/tooltip"
import { Dialog, DialogContent, DialogFooter } from "@/components/ui/dialog"
import { Field, Input } from "@/components/ui/input"
import { Select } from "@/components/ui/select"
import { Switch } from "@/components/ui/switch"
import { plural } from "@/features/cast/cast"
import { rememberPreset } from "@/features/export/export"
import { formatBytes, shortFormatName } from "@/lib/format"
import { commands, errorMessage } from "@/lib/ipc"
import { pickFolder } from "@/lib/media"
import { ALL_PARTS, FILE_PARTS, insertion, type NamePart, PART_HINTS, PART_LABELS } from "@/lib/naming"
import { qk, queryClient, useExportDir, usePresets, useSettings } from "@/lib/queries"
import { insertText, restoreSelection, revealCaret } from "@/lib/text-edit"
import { useUi } from "@/state/ui"
import { type Pack, packExportStarted } from "./use-pack"

/** A "{" that doesn't open one of the parts the one file's name may use. */
const notAFilePart = new RegExp(String.raw`\{(?!(?:${FILE_PARTS.join("|")})\})`)

/** Does the one file's name use a part that only names a shot (or no name at all)? */
const fileNameIsWrong = (template: string) => template.trim() === "" || notAFilePart.test(template)

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
  const { data: presets } = usePresets()
  const { data: settings } = useSettings()
  const { data: defaultDir } = useExportDir(pid)
  const [chosenPreset, setPresetId] = useState<string>()
  const [dir, setDir] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const state = pack.state!
  const { naming } = state
  const merge = naming.merge
  const presetId = chosenPreset ?? settings?.exportPresetId
  const preset = presets?.find((p) => p.id === presetId)
  const clips = pack.preview.data?.totals.clips ?? 0
  const names = pack.plan.data?.names ?? []
  const size = pack.plan.data?.sizes.find((s) => s.presetId === presetId)
  const bytes = size?.bytes
  // Bit-Perfect joins only episodes alike: otherwise a file per shot, and why.
  const apart = merge ? (size?.apart ?? null) : null
  const oneFile = merge && apart == null
  const file = oneFile ? pack.plan.data?.file : null
  // A template that doesn't work: the backend says why, under the field it's about.
  const problem = pack.plan.isError ? errorMessage(pack.plan.error) : null
  const fileProblem = merge && fileNameIsWrong(naming.file) ? problem : null
  const namesProblem = fileProblem ? null : problem
  const base = dir ?? defaultDir ?? ""
  const sep = base.includes("\\") ? "\\" : "/"
  // The character's folder as the backend names it (a file per shot only).
  const folder = naming.folder && !oneFile ? pack.plan.data?.folder : null
  const outDir = folder && base ? `${base.replace(/[\\/]+$/, "")}${sep}${folder}` : base
  const shots = plural(clips, "shot")

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
      toast(oneFile ? `Exporting ${shots} of ${character.label} into one file` : `Exporting ${shots} of ${character.label}`, {
        action: { label: "Show", onClick: () => useUi.getState().setActivityOpen(true) },
      })
    } catch (e) {
      toast.error("Couldn't start the export", { description: errorMessage(e) })
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
      {clips > names.length && <span className="text-fg-3">and {(clips - names.length).toLocaleString("en-GB")} more</span>}
    </span>
  )

  return (
    <DialogContent
      title={`Export ${character.label}'s scenepack`}
      description={
        oneFile
          ? `${shots} in one video, each cut frame-exact and checked against the source.`
          : `${shots}, each cut frame-exact and checked against the source.`
      }
      className="w-[min(600px,calc(100vw-32px))]"
    >
      <form
        className="flex flex-col gap-5"
        onSubmit={(e) => {
          e.preventDefault()
          void start()
        }}
      >
        <label className="flex items-center justify-between gap-4 rounded-lg border border-hairline bg-inset px-3.5 py-3">
          <span>
            <span className="block text-sm text-fg-1">All shots in one file</span>
            <span className="block text-xs text-fg-3">One video with a chapter for each shot, in the pack's order.</span>
          </span>
          <Switch checked={merge} onCheckedChange={(merge) => pack.setNaming({ ...naming, merge })} />
        </label>
        {oneFile && (
          <TemplateField
            id="pack-file"
            label="File name"
            hint={
              fileProblem ? (
                <span className="text-danger">{fileProblem}</span>
              ) : (
                file &&
                preset && (
                  <span className="block truncate pt-0.5 font-mono text-xs text-fg-2" data-selectable>
                    {file}.{size?.ext ?? preset.container}
                  </span>
                )
              )
            }
            value={naming.file}
            onChange={(f) => pack.setNaming({ ...naming, file: f })}
            parts={FILE_PARTS}
            invalid={fileProblem != null}
            // Enter exports: Ctrl/⌘ E, then Enter.
            autoFocus
          />
        )}
        <TemplateField
          id="pack-template"
          label={oneFile ? "Chapter names" : "File names"}
          hint={namesProblem ? <span className="text-danger">{namesProblem}</span> : nameList}
          value={naming.template}
          onChange={(template) => pack.setNaming({ ...naming, template })}
          parts={ALL_PARTS}
          invalid={namesProblem != null}
          autoFocus={!oneFile}
        />
        <Field
          label="Format"
          hint={
            preset && (
              <>
                {preset.description}
                {bytes != null && bytes > 0 && (
                  <span className="text-fg-2">
                    {" "}
                    About {formatBytes(bytes)} in all as {shortFormatName(preset.name)}.
                  </span>
                )}
                {oneFile && bytes != null && bytes > 0 && " The shots are rendered first, so it needs extra free space for a while."}
                {apart && <span className="block pt-1 text-fg-2">{apart}</span>}
                {size?.note && <span className="block pt-1 text-fg-2">{size.note}</span>}
              </>
            )
          }
        >
          <Select
            aria-label="Export format"
            value={presetId}
            onChange={setPresetId}
            placeholder="Choose a format"
            options={(presets ?? []).map((p) => ({ value: p.id, label: p.name }))}
          />
        </Field>
        <Field label="Destination">
          <div className="flex gap-2">
            <div
              data-selectable
              className="flex h-10 min-w-0 flex-1 items-center rounded-lg border border-soft bg-white/[0.04] px-3 font-mono text-sm text-fg-2"
            >
              <TruncatedText>{outDir || "…"}</TruncatedText>
            </div>
            <Button size="md" className="h-10" onClick={() => void pickFolder("Export to").then((p) => p && setDir(p))}>
              <FolderOpen /> Change…
            </Button>
          </div>
          {!oneFile && (
            <label className="mt-3 flex items-center gap-3 text-md text-fg-2">
              <Switch checked={naming.folder} onCheckedChange={(folder) => pack.setNaming({ ...naming, folder })} />
              In a folder named after {character.label}
            </label>
          )}
        </Field>
        <DialogFooter className="mt-1">
          <Button variant="ghost" onClick={onDone}>
            Cancel
          </Button>
          <Button
            type="submit"
            size="lg"
            variant="primary"
            disabled={!presetId || busy || clips === 0 || problem != null || !pack.current}
          >
            {busy && <CircleNotch className="animate-spin" />}
            {oneFile ? `Export ${shots} into one file` : `Export ${shots}`}
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

/** A name template: its label, the field, buttons that put a name part where the cursor is, and what it gives. */
function TemplateField({
  id,
  label,
  hint,
  value,
  onChange,
  parts,
  invalid,
  autoFocus,
}: {
  id: string
  label: string
  /** The names it gives, or why it won't do. */
  hint: ReactNode
  value: string
  onChange: (value: string) => void
  /** The name parts this field may use, one button each. */
  parts: readonly NamePart[]
  invalid: boolean
  autoFocus?: boolean
}) {
  const input = useRef<HTMLInputElement>(null)
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
    <Field label={label} htmlFor={id} hint={hint}>
      <Input
        id={id}
        ref={input}
        autoFocus={autoFocus}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="font-mono text-sm"
        aria-invalid={invalid}
      />
      {/* One Tab stop: the arrows, Home and End go from button to button. */}
      <Toolbar.Root
        aria-label="Name parts"
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
          <Tooltip key={part} label={<span className="font-mono">{`{${part}}`}</span>} hint={PART_HINTS[part]}>
            <Toolbar.Button
              onClick={() => insert(part)}
              className="select-none rounded-full border border-white/10 bg-white/[0.04] px-2.5 text-xs leading-6 text-fg-2 transition-[border-color,color,scale] duration-[var(--dur-fast)] ease-out hover:border-white/20 hover:text-fg-1 active:scale-[0.97]"
            >
              {PART_LABELS[part]}
            </Toolbar.Button>
          </Tooltip>
        ))}
      </Toolbar.Root>
    </Field>
  )
}
