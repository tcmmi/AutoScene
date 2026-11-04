/**
 * Where each running pack save will put its file, by its job: the backend
 * says it when it queues the run, and the job's end (the toast with Show in
 * folder) needs it. A save the app didn't start here (after a restart) has
 * none, and its end says the job's own message.
 */
const places = new Map<string, string>()

/** `job` (a pack action's run) saves to `path` once it ends well. */
export function rememberPackSave(job: string, path: string) {
  places.set(job, path)
}

/** Where `job` saved its file, if it was started here; it is forgotten once asked. */
export function takePackSave(job: string): string | undefined {
  const path = places.get(job)
  places.delete(job)
  return path
}
