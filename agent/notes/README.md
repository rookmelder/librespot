# Notes: measurements, benchmarks and history

The setup's memory. `CLAUDE.md` holds the rules; these notes hold what we **learned**. One entry per note.

## What goes in a note
- **Measurements**: tokens, time, calls and quality per worker/model, with the date and the task.
- **Benchmarks**: same task run on several workers, scored with hidden tests. Note the setup used.
- **Lessons**: things that broke or surprised us, and the fix (e.g. "model X writes files via the shell: blocked").
- **Model findings**: what worked, what failed (404, timeouts, loops). Update `MODELS.md` ratings to match.

Not here: rules for the Lead or workers (`CLAUDE.md`, `agent/bases/`, `agent/roles/`), secrets, long logs
(summarise in a few lines, keep raw output in `.worker/`). Lessons about the *project* (not the setup) go in `knowledge/`.

## How to add an entry
- New note `agent/notes/YYYY-MM-DD-<topic>.md`: `# <Topic> (YYYY-MM-DD)`, then a one-line summary `> ...`,
  then `Back to [Notes](README.md).` Add one line for it at the bottom of the list below. Never append to an old note.
- Short and factual: numbers, model ids, what changed. One line per finding where possible. Mark untested claims.
- When a finding changes which model a role should use, also update `MODELS.md` and that role's `models:` line.

## Entries (oldest first)
- [Scripts ported to Python (2026-11-13)](2026-11-13-scripts-ported-to-python.md): PowerShell scripts became stdlib Python (delegate.py, check_setup.py); first smoke test of every worker.
- [delegate.py runs the check, workers run nothing (2026-11-15)](2026-11-15-delegate-runs-the-check.md): Workers lost their shell; delegate.py runs check_one and gives one fix round. One round fixes one layer.
- [Changed files per run, --files, run files (2026-11-15)](2026-11-15-changed-files-per-run.md): Content hashes, --files scope and one run file per run, after parallel runs mixed up their changes.
- [Lighter guides, no-tools mode, caps, benchmark (2026-11-15)](2026-11-15-lighter-guides-no-tools-mode.md): Shorter CLAUDE.md, models.json, no-tools mode, call cap, private fails closed, bench.py.
- [Builder benchmark with bench.py, chains reordered (2026-11-15)](2026-11-15-builder-benchmark.md): Two-task builder benchmark: agy flash-low best; Mistral Large 4 correct but slow, moved last.
- [Quality-neutral optimizations (2026-11-15)](2026-11-15-quality-neutral-optimizations.md): Fix round 2 only on progress, line numbers only for reviewers, incremental log reads, provider retry waits; bench 88/88 before and after.
- [agy and Codex only, secrets denied, independence enforced (2026-11-17)](2026-11-17-agy-and-codex-only.md): API workers and Copilot removed; Codex CLI added (untested); path/secret policy; --after family check; harder bench tasks.
- [GPT-6 Codex models, new roles, checks by risk (2026-11-17)](2026-11-17-gpt6-roles-and-workflow.md): Chains moved from legacy gpt-5.6 to gpt-6 Codex models; debugger, mapper and visual-reviewer roles; risk-based checks.
- [Shopkeeper benchmark: real tasks per role (2026-11-17)](2026-11-17-shopkeeper-benchmark.md): Seven real tasks on a shop project; made harder after Flash Low scored 55/57: now Low 58/68, Medium 62/68 (tester separates them).
- [Codex smoke test and benchmark (2026-11-18)](2026-11-18-codex-smoke-test-and-bench.md): Smoke tests pass; Codex workers cannot read files (only --include); best on tester/security, fail debug and tasks needing unpasted files.
