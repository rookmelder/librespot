# Worker tools: cost, limits, quirks

Status 2026-11-17. Plans and limits change: re-check before relying on a number.
Which model to pick per role: `MODELS.md`. What `delegate.py` knows: `models.json`.

| Tool | What you pay | Limits | Watch out |
|---|---|---|---|
| **Claude** (Lead) | Your Claude plan | Plan limits | The expensive brain: keep it for planning and review |
| **agy** (Google AI Pro) | Pro subscription | Pro quota for Gemini, separate pool for Claude/gpt-oss; every 5 h + weekly cap | ~20 worker runs ≈ 6-7% of a 5 h window. Check with `/usage` in agy |
| **Codex** (ChatGPT plan) | Your ChatGPT plan | The plan's Codex usage limits (shared with your own Codex use) | Not measured in this setup yet. A limit hit shows as exit 6: the next worker in the chain takes over |

**Privacy:** no free tiers that train on prompts are used any more. For Codex, turn off "Improve the model for
everyone" in ChatGPT before private code goes to it.

## Notes per tool
- **agy**: workers run through a role agent with only the tools that role needs (~3.6k tokens per call instead of
  ~10.4k), with per-file deny rules for the project's secret files (tested 2026-11-17: a read of `.env` was denied).
- **Codex**: `codex exec` with your config ignored, no shell, no web, no subagents, and a permission profile (see
  `setup/codex.md`). The model list your plan offers changes; `~/.codex/models_cache.json` has the current one.
