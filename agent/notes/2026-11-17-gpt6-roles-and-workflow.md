# GPT-6 Codex models, new roles, checks by risk (2026-11-17)
> Chains moved from legacy gpt-5.6 to gpt-6 Codex models; debugger, mapper and visual-reviewer roles; risk-based checks.

Back to [Notes](README.md).

- The user's Codex picker (2026-11-17): GPT-6.1 Terra (visual, computer use), GPT-6.1-Sol (coding, everyday),
  GPT-6-Astra (complex coding), GPT-6-Sol (legacy), GPT-6-Luna (fast), GPT-5.6 Sol/Terra/Luna (legacy). The model
  cache also lists `gpt-6.2-sol` and `gpt-6.2-astra`, not in the picker: not used.
- Picks follow the models' own descriptions (none run yet in this setup): luna = tester and fallbacks, 6.1-sol =
  builderplus and writing roles, astra = hardest pieces, debugger, balance, security fallback, 6.1-terra = screenshots.
  Full table: `agent/MODELS.md`. Every pack role now has a `models:` line; checking roles always have two families.
- New roles: `debugger` (reviewer base: root cause of a stuck failure, no fix), `mapper` (scribe base: knowledge notes
  for existing code), software pack `visual-reviewer` (reviewer base: screenshots the Lead takes).
- `--include` of a binary file (an image) is refused (exit 2): name it in the task, the worker opens it itself.
- CLAUDE.md: checks by risk (low: script check + Lead; medium: tester OR reviewer; high: both + security), never the
  same worker a third time (debugger, then a fresh builderplus), knowledge updates only for lasting changes.
- Unit tests: 57 pass; the role test now also parses every pack role.
