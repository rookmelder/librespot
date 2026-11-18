# delegate.py runs the check, workers run nothing (2026-11-15)
> Workers lost their shell; delegate.py runs check_one and gives one fix round. One round fixes one layer.

Back to [Notes](README.md).

- Workers lost their shell tools (Copilot: no `powershell`/`bash`; agy: no `run_command`). `delegate.py` runs `check_one`
  itself (`--part` fills `{name}`), sends a failure back to the same worker once, runs it again; exit 7 if it still fails.
  Reviewers get the check result pasted into the task. Gone: the agy global allow-rule, the Copilot "any `python`
  command" hole, the "run it max 2x" reminders.
- Tested: unit tests only (fake worker; pass, fix round, exit 7, no `--part`, `--no-check`).
- Smoke test on Windows (tiny builder task, a hidden test asked for `ValueError('empty')` that the spec did not mention):
  agy gemini-3.8-flash-low 35s incl. fix round (22.8k in); NVIDIA Mistral Large 4 via Copilot 328s incl. fix round
  (6 calls, 31k in). Both wrote files with no shell tool, got the failing output, and fixed what it showed. Both ended
  on exit 7: the first failure only said "ValueError not raised", the exact message showed up in the second run.
  So one fix round solves one layer of failure; a test with stacked failures needs the Lead (or a 2nd round: not built).
- A third run (NVIDIA, allowed to read the test) passed first time in 120s, 5 calls: workers read existing tests.
- Untested: whether quality drops on real tasks now that a worker gets one fix round instead of running the check
  itself. Re-run the builder benchmark before trusting MODELS.md ratings. Tester and reviewer roles not smoke-tested.
