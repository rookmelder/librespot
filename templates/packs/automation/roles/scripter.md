---
base: builder
models: codex:gpt-6.1-sol:medium, agy:gemini-3.8-flash-medium
does: Builds one script/step with a dry-run mode, logging and tests on temporary data.
---
- Every script gets a dry-run switch (print what would happen, change nothing) and logs each action it takes.
- Safe to run twice: check before acting (exists already? skip). Paths and names come from parameters, not constants.
- Tests use temporary folders and fake data only; never real accounts, real files outside the project, or the network.
