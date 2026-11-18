# agy and Codex only, secrets denied, independence enforced (2026-11-17)
> API workers and Copilot removed; Codex CLI added (untested); path/secret policy; --after family check; harder bench tasks.

Back to [Notes](README.md).

- Removed: Copilot CLI harness, the NVIDIA, Gemini API, OpenRouter and OpenAI providers, the OpenAI free-tier guard,
  the call cap, the no-tools direct mode, `.env.example`. Workers: `agy:<model>` and `codex:<model>[:<effort>]`.
- Codex worker (`invoke_codex`): `codex exec --json`, user config and rules ignored, `--disable shell_tool` (+ multi_agent,
  apps, plugins, hooks, ...), web search off, approvals never, a permission profile (`:workspace`, reviewer `:read-only`)
  that denies secrets by glob and by path; fix rounds via `exec resume`. Built from Codex docs (CLI 0.167.1);
  **not run**: the user chose no Codex runs yet. Smoke test: `agent/setup/codex.md`.
- Paths: `--include` / `--files` outside the project, in `.git/`, or a secret (`.env*` except `.example`/`.sample`, keys,
  credentials) -> exit 4. A changed secret file always lands under `NOT IN --files`.
- agy: each secret file found in the project gets `read_file(...)` / `write_file(...)` deny rules via `--settings`.
  Tested (agy gemini-3.8-flash-low reviewer, canary `.env`, 10 s, 5k tokens in): "Permission denied for read_file(...).
  Matches user-configured deny rule"; the value did not reach the report.
- Independence: tester/reviewer need `--after <run id | lead>`; the chain drops the builder's family (`families` in
  models.json: gemini / claude / gpt). Run files now store `model` and `family`.
- Bench: `ledger` (builder, medium, 11 tests), `cfg` (builderplus, hard, 3 files, 11 tests), `duration` (tester, scored by
  12 planted mutants its tests must catch). Rows record what was missed. Unit tests check each task's solution scores
  full and its start scores under half. No worker has run them yet.
- Codex's model list changed during the session (`gpt-5.6-sol` dropped from the cache): chains use `gpt-5.6-luna` /
  `gpt-5.6-terra`, which were in both lists.
- Unit tests: 57 pass (`python -m unittest discover -s agent/tests`).
