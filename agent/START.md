# Starting a project (for the Lead)

Read this once per project. Do the steps in order; skip what is already done.

1. **Setup.** Run `python agent/check_setup.py` (it writes `SETUP.md`). Ask the user the
   "Confirmed by the user" questions in it once and fill them in. Missing tools: point to `agent/setup/README.md`;
   it is fine to start with what exists.
2. **Goal.** If `GOAL.md` says "NOT WRITTEN YET", ask the user to write it there in their own words (or tell you, and
   you write it down in their words). Then ask about everything important it leaves out (the pack's `pack.md` lists
   what a good goal covers). Add the answers to GOAL.md.
3. **Pack.** Pick the pack(s) in `templates/packs/` that fit the goal (or none). Read its `pack.md`.
4. **Roles.** Propose the roles this project needs: the defaults in `agent/roles/` (builder, builderplus, tester,
   reviewer, scribe, debugger, mapper), pack roles, and new ones from `templates/roles/ROLE-TEMPLATE.md`. Say in one line each what
   it does and which models it uses. After the user's OK, copy/create them in `agent/roles/` and delete unused
   defaults only if the user wants.
5. **Project.** Fill in `PROJECT.md` (about, settings, worker notes) and remove its "NOT FILLED IN YET" line.
   Ask the user whether the project is private and set `private: yes` or `private: no` in the settings block:
   while it is empty it counts as private (agy and Codex only: `private_ok` in `models.json`).
6. **Models (optional).** `agent/MODELS.md` holds the template author's ratings. To rate the models this user has,
   run `python agent/bench.py --workers <worker> ...` and update `agent/MODELS.md` and the `models:` lines.
7. **Knowledge map.** `knowledge/` holds the project's map (one small note per system, finding or decision,
   see `knowledge/README.md`). Keep it for anything bigger than a few files; for a tiny project ask the user
   whether to delete it. Existing code? Make one system note per part before the first task.
8. **Repo.** The project's files live inside this folder (e.g. `src/`, `design/`): `delegate.py` uses this
   folder's git status to see what a worker changed. If `origin` still points at the template repo, ask the user
   for the project's own repo URL.

## The files
| File | Who writes it | What it is |
|---|---|---|
| `GOAL.md` | the user | What they want, in their own words. The source of truth for *what* to make. |
| `PROJECT.md` | you (user approves) | Technical settings from GOAL.md: pack, privacy, check commands, worker notes. |
| `SETUP.md` | `agent/check_setup.py` | Which worker tools this machine has (git-ignored). |
| `agent/roles/<name>.md` | you (user approves) | The worker roles in use: base type, model chain, extra rules. |
| `agent/bases/<base>.md` | template | Core rules per base type (builder, tester, reviewer, scribe). Rarely changed. |
| `agent/delegate.py` | template | The only way to run a worker. |
| `agent/models.json` | you | The models, model families and harness rules `delegate.py` knows. |
| `agent/bench.py`, `agent/bench/` | template | Rates worker models per role (easy tasks, real tasks on a small shop project; hidden tests, planted bugs or answer keys). |
| `agent/MODELS.md` | you | Which model to pick per role, with ratings. |
| `agent/LIMITS.md` | you | What each worker tool costs, its limits and its quirks. |
| `agent/notes/` | you | What we measured and learned about the setup: one note per entry, index in its `README.md`. |
| `knowledge/` | you + scribe | The project's knowledge map: small linked notes per system, finding, decision. |
| `agent/check_docs.py` | template | Checks `knowledge/` and `agent/notes/` (broken links, orphans, missing files); `--find <words>` searches them. |
| `agent/setup/` | template | How to install and sign in to each tool (point the user here). |
| `templates/` | template | `roles/ROLE-TEMPLATE.md`, project packs (`packs/<type>/`), note templates (`notes/`). |
| `README.md` | template | The whole setup explained for the user in plain words. |
