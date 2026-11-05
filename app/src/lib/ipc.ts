import type { AppError } from "@/bindings"
import { say } from "@/lib/i18n"

export { commands, events } from "@/bindings"
export type * from "@/bindings"

/** Commands throw `AppError` objects; anything else is an unexpected failure. */
export function isAppError(e: unknown): e is AppError {
  return typeof e === "object" && e !== null && "code" in e && "message" in e
}

/** Human-readable message for any thrown value, in the UI's language. */
export function errorMessage(e: unknown): string {
  if (isAppError(e)) return say(e.msg ?? e.message)
  if (e instanceof Error) return e.message
  return String(e)
}

/** The same in English, for who isn't the person: a plugin page's code hears the words it was written against. */
export function englishMessage(e: unknown): string {
  return isAppError(e) ? e.message : errorMessage(e)
}
