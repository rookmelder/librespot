# Project

<!-- Filled in by the Lead (Claude) from GOAL.md and a project pack (templates/packs/), with the user's OK.
     delegate.py reads the "settings" block and pastes "Worker notes" into every worker's task. -->

**Status:** NOT FILLED IN YET

## About (for the Lead)
- **In one sentence:** <what we are making, from GOAL.md>
- **Pack(s) used:** <software / game / research / automation / none>
- **Where the work lives:** <e.g. `src/`, `docs/`, `design/`> (always inside this folder)
- **Roles in use:** <e.g. builder, tester, reviewer, scribe, designer>
- **Done means (first version):** <from GOAL.md>

## Settings (read by delegate.py)
Leave a value empty when it doesn't apply. `{name}` stands for the part a worker works on (the Lead passes it: `--part shop`).

```settings
check_one:
check_all:
check_dir:
private:
```

- `check_one`: checks one part. `delegate.py` runs it after a builder/tester (never the worker itself). Empty = no check.
- `check_all`: the full check the Lead runs after every worker (all tests, a build, a linter). Empty = Lead reviews by reading.
- `check_dir`: the only folder a tester may write in. Empty = only the files its task names.
- `private`: `yes` = only Claude or agy may see the files. `no` = free cloud workers allowed (they may log
  or train on prompts). Empty or anything else counts as `yes`.
- Examples per kind of project: `templates/packs/<pack>/pack.md`.

## Worker notes (pasted into every worker's task: keep it short)
- <language and version, allowed libraries, style to match>
- <conventions: naming, units, coordinate order, tone of voice for text, ...>
- <known traps for this project>