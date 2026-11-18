# Pack: game (any engine)

## What a good GOAL.md covers (ask about what's missing)
- Genre and the core loop in one sentence ("explore, fight, loot, go deeper").
- Engine/platform (Python+pygame/tkinter, Godot, Unity, Roblox, web/JS, ...) and why, if the user has a preference.
- Controls, camera/view, art style (or "placeholder shapes are fine"), sound.
- How long a play session is; win/lose conditions; what makes it replayable.
- First playable version: the smallest version that is fun. Everything else is later.

## PROJECT.md suggestions
- Put game rules in plain modules without engine/graphics code, so they can be tested (state, rules, maps, items).
  The engine layer (drawing, input) is thin and checked by the Lead by running the game.
- check commands per engine:
  | Engine | check_one / check_all |
  |---|---|
  | Python | `python -m unittest tests.test_{name} -v` / `python -m unittest discover -s tests` |
  | Godot (GUT addon) | `godot --headless -s addons/gut/gut_cmdln.gd -gtest=res://tests/test_{name}.gd` / same without `-gtest` |
  | Unity | test runner in batch mode (`Unity -batchmode -runTests -testPlatform EditMode`) |
  | Roblox | keep logic in pure Luau modules, test with Lune or TestEZ; the Lead playtests in Studio |
  | Web/JS | `npx vitest run tests/{name}.test.js` / `npx vitest run` |
- Worker notes to consider: coordinate order (x, y) and grid indexing (`tiles[y][x]`): workers mix these up a lot;
  random numbers through a seeded generator (testable); all tuning numbers as named constants in one place.

## How to split the work
1. `designer` writes the design doc (`design/`): core loop, rules, numbers. The Lead and user approve it.
2. Lead writes `SPEC.md` from it: modules, data formats, exact formulas and messages.
3. Builders make the rule modules with tests; `content` makes levels/items/enemies as data files.
4. Lead builds the engine layer or reviews it closely, and plays the game after every wave.
5. `balance` reviews numbers (difficulty, economy, pacing) against the design.

## Pitfalls
- Workers can't play the game: only the Lead (and the user) can judge feel and fun. Playtest early.
- Spawning/placement code: check that new things never land on walls, stairs, the player or each other.
- Keep state that resets per level/round in one place; leftovers between levels are a classic bug.

## Roles in this pack
- `designer`: writes and updates design docs.
- `content`: makes levels, items, enemies, dialogue as data files.
- `balance`: reviews the numbers.
