# Templates

Starting points the Lead copies into the project. Nothing here is used directly by `delegate.py`.

- `roles/ROLE-TEMPLATE.md`: how to make a new worker role (copy to `agent/roles/<name>.md`).
- `notes/`: templates for knowledge notes (system, finding, decision): copy into `knowledge/<kind>s/`.
- `packs/<type>/`: a pack per kind of project. Each has `pack.md` (what a good goal covers, check commands,
  worker notes, pitfalls, how to split the work) and `roles/` (extra roles for that kind of work).

| Pack | For |
|---|---|
| `software` | Apps, tools, libraries, backends, websites |
| `game` | Games in any engine (Python, Godot, Unity, Roblox, web, ...) |
| `research` | Deep research: reports, comparisons, literature reviews |
| `automation` | Scripts and workflows that do things on a computer or between services |

**Applying a pack** (the Lead does this at the start of a project, after reading `GOAL.md`):
1. Copy the pack's `roles/*.md` into `agent/roles/` (only the ones the project needs).
2. Use `pack.md` to fill in `PROJECT.md` (settings, worker notes) and to ask about anything `GOAL.md` leaves out.
3. Mixed project (e.g. a game with a research phase)? Take roles from more than one pack.
