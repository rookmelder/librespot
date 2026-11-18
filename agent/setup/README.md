# Setting up the workers

You don't need everything. Each tool you add gives the Lead more workers.
After adding anything, run `python agent/check_setup.py`: it checks what you have
and writes `SETUP.md`, and `delegate.py` then only uses workers that work on your machine.

| What | Gives you | Cost | Guide |
|---|---|---|---|
| Claude Code | The Lead (required) | Your Claude plan | [claude.md](claude.md) |
| git | Required | Free | [git.md](git.md) |
| Python 3.8+ | Required: runs `agent/delegate.py` and `agent/check_setup.py` (standard library only) | Free | python.org, or your package manager |
| agy (Google Antigravity CLI) | Gemini builders, Claude Sonnet reviewer | Google AI Pro plan for real quota | [agy.md](agy.md) |
| Codex CLI (OpenAI) | GPT builders, testers and reviewers | Your ChatGPT plan | [codex.md](codex.md) |

No API keys are needed. Works on Windows, Linux and macOS (use `python3` where `python` is not on the PATH).

**Minimum that works:** Claude Code + git + Python + one worker tool (agy or Codex). Both is better: a tester or
reviewer must come from another model family than the builder. With no worker tool, the Lead does the work itself.
