# Lighter guides, no-tools mode, caps, benchmark (2026-11-15)
> Shorter CLAUDE.md, models.json, no-tools mode, call cap, private fails closed, bench.py.

Back to [Notes](README.md).

- `CLAUDE.md` went from 9,849 to 6,108 characters: the start-of-project steps and the file table moved to
  `agent/START.md` (read once per project), the flag list shrank to what is used every run (`--help` has the rest).
- `MODELS.md` split: pick order stays, cost/limits/provider notes moved to `LIMITS.md`.
- Models, providers and limits moved from `delegate.py` to `agent/models.json`.
- No-tools mode: a reviewer with `--include`, or a scribe writing one `.md`, on an API model is ONE plain API call
  (no Copilot). agy workers still use the agy CLI.
- Call cap for every Copilot worker: `max_calls` 40 (models.json, `--max-calls`). OpenRouter Nemotron Ultra used ~60
  requests per task in the old benchmark; in the new one (next entry) it needed 6-11, so the cap does not stop it.
- `--fix-rounds 0|1|2` replaces `--no-fix`.
- Private mode fails closed: anything but `private: no` in PROJECT.md's settings means agy only.
- Copilot workers: deny rules for reading and writing `.env` (tested: a worker asked to open `.env` was denied).
  The prompt now goes in on stdin (Windows limits a command line to ~32k characters). agy has no such deny rule.
- `agent/bench.py`: two tasks (textkit: new module; inventory: fix + extend), hidden tests, table for MODELS.md.
- Real runs after these changes (Windows):
  - No-tools reviewer, Gemini flash-lite: 1 call, 1,029 tokens in, 2 s, found the real spec differences.
  - No-tools scribe, NVIDIA Nemotron lightning: 1 call, 10 s. Its first try mangled the file markers (`<<<FILE>>`),
    so the markers became `BEGIN FILE` / `END FILE` and decoration around them is ignored.
  - bench, agy gemini-3.8-flash-low: inventory 10/10 (23 s, 8.8k in), textkit 12/12 (31 s, 15.1k in), no fix round.
  - bench, NVIDIA Mistral Large 4 on textkit: 12/12 hidden tests but TIMEOUT at 480 s after 4 calls (slow NVIDIA day),
    own check failed (it timed out before its test file was right). Prompt on stdin worked.
- Untested: the call cap and a second fix round with a real worker (unit tests only); tester role; Linux.
