import type { Msg } from "@/bindings"
import { i18n, setLanguage, type Lang, type PSEUDO } from "@/lib/i18n"

type Bundles = Record<string, Record<string, unknown>>

/** Words with no key: a plugin's, or a fixture's that no test reads in another language. */
export const plain = (text: string): Msg => ({ key: "", text })

/**
 * Switches the UI to `lng`: its real catalog, or with fixture words
 * (`{ common: { action: { cancel: "Anulează" } } }`) those alone, so a test of how a word
 * reaches the screen holds whatever the catalog says and what the fixture lacks is English.
 * `setup.ts` puts English back after every test.
 */
export async function inLanguage(lng: Lang | typeof PSEUDO, bundles?: Bundles): Promise<void> {
  await setLanguage(lng)
  if (!bundles) return
  for (const ns of Object.keys(i18n.store.data[lng] ?? {})) i18n.removeResourceBundle(lng, ns)
  for (const [ns, words] of Object.entries(bundles)) i18n.addResourceBundle(lng, ns, words, true, true)
  await i18n.changeLanguage(lng)
}

/** Back to English, without the fixture words a test added. */
export async function resetLanguage(): Promise<void> {
  if (i18n.language !== "en") await setLanguage("en")
  for (const lng of Object.keys(i18n.store.data)) {
    if (lng === "en") continue
    for (const ns of Object.keys(i18n.store.data[lng] ?? {})) i18n.removeResourceBundle(lng, ns)
  }
}
