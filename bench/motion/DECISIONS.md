# Motion bench decisions

One row per decision, in order. The plan a row points at is in `README.md`.

| # | Date | Step | Decision | Data | Results | Outcome |
|---|---|---|---|---|---|---|
| 1 | 2026-11-15 | Repeated frames from the stored signals | Pre-registration, written before any signal file or video of this bench was read. Question: can the stored per-frame differences (`FrameSignals.diff`, 48×27) say per shot how many frames are repeats of the frame before? Fixed in `README.md`, "Plan": the reference (`mpdecimate` at its defaults, with a strict and a loose setting beside it, checked by eye on frame pairs from four strata), the candidate (C1 one bar, C2 a bar and the neighbours; the setting chosen on fit episodes), the measures (held share within 0.10, correlation, precision and recall of "has repeated frames", cadence, per-frame diagnostic, motion score as a by-product), the alignment check, and a draft gate: 90 % of shots within 0.10 of the reference and a precision and recall of 0.90 for the flag; for live action at most 2 % of unflagged shots flagged. Bench only: nothing in the app changes. | Anime: `jjk_s2e03`, `op_6`, `rz_31` (fit), `jjk_s2e04`, `op_a`, `rz_33`, `op_7` (check). 3D: `mir_s1e11` (fit), `mir_s5e21` (check). Live action: `bb_s01e01` (check; signals of the remux from a project copy, pictures from the 960×540 proxy). | (open) | (open) |
