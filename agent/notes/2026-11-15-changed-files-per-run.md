# Changed files per run, --files, run files (2026-11-15)
> Content hashes, --files scope and one run file per run, after parallel runs mixed up their changes.

Back to [Notes](README.md).

- Seen in the smoke test: two workers in parallel, and the NVIDIA run listed the agy run's `calca.py` as its own.
- `delegate.py` now compares content hashes before/after (a second edit to an already-changed file used to be
  invisible), takes `--files` (changes outside it are listed apart and no longer block the fallback chain), and
  saves each run as `.worker/run-<id>.json` (`last.json` was overwritten by parallel runs).
- Limit: without `--files` a parallel run still lists everyone's changes; the script cannot tell who wrote a file.
  Real isolation would need a git worktree per worker: not built.
- Tested: unit tests only (temp git repo). Not re-run with real parallel workers.
