---
base: scribe
models: codex:gpt-6.1-sol:medium, agy:gemini-3.8-flash-medium
does: Writes and updates design docs (core loop, rules, numbers) in design/.
---
- Write in `design/` only. One topic per file (core-loop.md, combat.md, economy.md, ...).
- Every rule gets exact numbers or a formula; mark open questions with "OPEN:" instead of inventing an answer.
- Facts come from GOAL.md, existing design docs and the task. Never invent features that weren't asked for.
