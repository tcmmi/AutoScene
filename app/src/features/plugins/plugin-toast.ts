import { toast } from "sonner"
import { t } from "@/lib/i18n"

/**
 * What a toast about a plugin says, by kind. Each is a whole message, and the
 * words are the plugin's own (a plugin may be called "AutoScene"), so every
 * message starts with a word it can't choose: `Plugin “<name>”: …`, or
 * `A plugin: …` for a plugin AutoScene doesn't know by name.
 */
const SAYS = {
  alreadyWaiting: (name: string | null, v: { action: string }) =>
    name != null ? t("plugins:toast.alreadyWaiting.named", { name, ...v }) : t("plugins:toast.alreadyWaiting.anonymous", v),
  cantStart: (name: string | null, v: { action: string }) =>
    name != null ? t("plugins:toast.cantStart.named", { name, ...v }) : t("plugins:toast.cantStart.anonymous", v),
  jobFinished: (name: string | null, v: { action: string }) =>
    name != null ? t("plugins:toast.finished.named", { name, ...v }) : t("plugins:toast.finished.anonymous", v),
  jobFailed: (name: string | null, v: { action: string }) =>
    name != null ? t("plugins:toast.failed.named", { name, ...v }) : t("plugins:toast.failed.anonymous", v),
  fileSaved: (name: string | null, v: { file: string }) =>
    name != null ? t("plugins:toast.saved.named", { name, ...v }) : t("plugins:toast.saved.anonymous", v),
  updatedTo: (name: string | null, v: { version: string }) =>
    name != null ? t("plugins:toast.updated.named", { name, ...v }) : t("plugins:toast.updated.anonymous", v),
  updateFailed: (name: string | null, _vars: Record<string, never>) =>
    name != null ? t("plugins:toast.updateFailed.named", { name }) : t("plugins:toast.updateFailed.anonymous"),
}

type Kind = keyof typeof SAYS
type Values<K extends Kind> = Parameters<(typeof SAYS)[K]>[1]

/** The kinds that are something going wrong. */
const ERRORS: ReadonlySet<Kind> = new Set<Kind>(["cantStart", "jobFailed", "updateFailed"])

/**
 * Say something for a plugin: the message of `kind`, with the plugin's `name`
 * (none for a plugin AutoScene doesn't know) and `vars`, the plugin's own
 * words, as values.
 */
export function pluginToast<K extends Kind>(
  name: string | null,
  kind: K,
  vars: Values<K>,
  { description, action }: { description?: string; action?: { label: string; onClick: () => void } } = {},
) {
  const say = SAYS[kind] as (name: string | null, v: Values<K>) => string
  return (ERRORS.has(kind) ? toast.error : toast)(say(name, vars), action ? { description, action } : { description })
}
