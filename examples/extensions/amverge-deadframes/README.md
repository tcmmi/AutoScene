# AMVerge Deadframe Remover Extension

An AutoScene extension that analyzes exported clips and removes static holding frames using optical flow and frame compaction.

Ported from [AMVerge-CLI](https://github.com/AMVerge-team/AMVerge-CLI).

## Features
- **Farneback Optical Flow**: Measures pixel velocity across frame pairs.
- **Smart Decimation**: Detects duplicate/static holding frames and drops them.
- **Back-to-Back Packing**: Uses FFmpeg's `select` filter to pack kept frames continuously without timeline gaps.
- **Auto-fallback**: Uses OpenCV if available, or FFmpeg `mpdecimate` if running in standard Python environments.

## Manual Test
Click **Run Test** on the AMVerge card in AutoScene's **Extensions** page to run diagnostics.

