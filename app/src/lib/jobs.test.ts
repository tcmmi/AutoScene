import { describe, expect, it } from "vitest"
import { stopsFirst } from "./jobs"

describe("stopsFirst", () => {
  it("names the work that stops first", () => {
    // Nothing runs, or only upkeep the person never started: nothing to say.
    expect(stopsFirst([])).toBeNull()
    expect(stopsFirst(["posters", "portraits", "regroup"])).toBeNull()
    // One of each kind, named by what it does.
    expect(stopsFirst(["analyze"])).toBe("Work running on it stops first: analysing.")
    expect(stopsFirst(["ingest"])).toBe("Work running on it stops first: indexing.")
    expect(stopsFirst(["proxy"])).toBe("Work running on it stops first: building a preview.")
    expect(stopsFirst(["export"])).toBe("Work running on it stops first: an export.")
    expect(stopsFirst(["plugin"])).toBe("Work running on it stops first: a plugin run.")
    // An analysis and its restart are one "analysing"; previews, exports and plugin runs are counted.
    expect(stopsFirst(["analyze", "analyze", "proxy", "proxy"])).toBe(
      "Work running on it stops first: analysing and building previews.",
    )
    expect(stopsFirst(["export", "plugin", "export"])).toBe(
      "Work running on it stops first: 2 exports and a plugin run.",
    )
    expect(stopsFirst(["plugin", "plugin"])).toBe("Work running on it stops first: 2 plugin runs.")
    // Always in the same order, whatever order the jobs came in; upkeep beside them stays unnamed.
    expect(stopsFirst(["export", "regroup", "analyze", "ingest"])).toBe(
      "Work running on it stops first: indexing, analysing and an export.",
    )
  })
})
