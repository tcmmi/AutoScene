/**
 * Name parts: what a pack's file and chapter names are made of. The list is
 * the backend's (as_project::naming::NAME_PARTS, exported into bindings.ts),
 * so the buttons, the mock and the backend's checks can't drift apart.
 */
import { FILE_PARTS as BACKEND_FILE_PARTS, NAME_PARTS } from "@/bindings"

/** A name part, as a template spells it between braces: `tc` for `{tc}`. */
export type NamePart = (typeof NAME_PARTS)[number][0]

/** Every name part, in the backend's order: what a shot's or a chapter's name may use. */
export const ALL_PARTS: readonly NamePart[] = NAME_PARTS.map(([part]) => part)

/** What the name of one file holding the whole pack may use: the others name a shot. */
export const FILE_PARTS: readonly NamePart[] = BACKEND_FILE_PARTS

/** The label of the button that puts a part in: "Number" for `{index}`. */
export const PART_LABELS = Object.fromEntries(NAME_PARTS) as Record<NamePart, string>
