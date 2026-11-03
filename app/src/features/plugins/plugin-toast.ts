import { toast } from "sonner"

/**
 * Say something for a plugin. Its name and words are all its own (a plugin
 * may be called "AutoScene"), so the toast starts with a word it can't
 * choose: `Plugin “<name>”: <text>`, or `A plugin: <text>` without a name.
 */
export function pluginToast(
  name: string | null,
  { text, description, tone = "neutral" }: { text: string; description?: string; tone?: "neutral" | "error" },
) {
  const from = name != null ? `Plugin “${name}”` : "A plugin"
  return (tone === "error" ? toast.error : toast)(`${from}: ${text}`, { description })
}
