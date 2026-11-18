# Quality-neutral optimizations (2026-11-15)
> Fix round 2 only on progress, line numbers only for reviewers, incremental Copilot log reads, provider retry waits. Bench: 88/88 before and after.

Back to [Notes](README.md).

## What changed in delegate.py
- `--fix-rounds` default 1 -> 2, but round 2 only runs when round 1 changed the failure (check output compared
  without timings and 0x addresses). Same failure twice -> stop, exit 7, back to the Lead as before.
- `--include` files get line numbers only for a reviewer (it quotes lines). Others get the plain file: line numbers
  made a pasted file 12-15% bigger on every call, and a builder could copy them into an edit.
- The Copilot call cap read the whole debug log every 3 s; it now reads only the new full lines.
- Busy provider: the retry waits what the error asks for (Retry-After header, Gemini `retryDelay`, "try again in
  Xs") instead of always 20 s; asked for more than 120 s -> no retry, next worker at once.

## Bench (Windows, same day, both sides run twice; baseline from the commit before)
| Worker | Before: hidden tests | After: hidden tests | Before: own check failed | After |
|---|---|---|---|---|
| agy gemini-3.8-flash-low | 44/44 | 44/44 | 0 of 4 | 0 of 4 |
| Gemini API flash-lite | 44/44 | 44/44 | 1 of 4 (textkit, exit 7) | 0 of 4 |
- Token counts moved 8.8k-16.6k between identical agy runs: run-to-run noise is far bigger than the line-number
  saving, and the only bench include (`inventory.py`) is 14 lines. The saving is measured on real files, not here:
  `delegate.py` 42.6k -> 47.8k characters with numbers (+12%), `check_docs.py` +15%.
- Untested with real workers: a second fix round (none was needed in the "after" runs), the retry wait, the
  incremental log reader on a long run. Unit tests cover each (`agent/tests/test_delegate.py`).
