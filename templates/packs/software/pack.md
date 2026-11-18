# Pack: software (apps, tools, libraries, backends, websites)

## What a good GOAL.md covers (ask about what's missing)
- Who uses it and for what; the 3-5 most important things it must do.
- Platform: desktop, web, mobile, command line, server. Language/framework if the user has a preference.
- Data: what it stores, where, how much. Accounts or logins? Outside services (APIs, payments)?
- What "done" looks like for a first version. What is explicitly NOT in it.

## PROJECT.md suggestions
| Stack | check_one | check_all | check_dir |
|---|---|---|---|
| Python | `python -m unittest tests.test_{name} -v` | `python -m unittest discover -s tests` | `tests` |
| Node (vitest) | `npx vitest run tests/{name}.test.js` | `npx vitest run` | `tests` |
| .NET | `dotnet test --filter {name}` | `dotnet test` | `tests` |
| Go | `go test ./{name}/...` | `go test ./...` | (empty: tests sit next to code) |

Worker notes to consider: language version and allowed libraries; folder layout; naming; error handling style
("return errors, never print"); no network or real credentials in tests.

## How to split the work
- Write `SPEC.md` first for anything over 4 files: modules, their functions/endpoints, data formats, error messages.
- One builder per module, tests included. Tester writes spec tests for the riskiest modules.
- Interfaces first (data types, function signatures), then the code that uses them.
- `security-reviewer` for anything that touches logins, user input, files, network or money.

## Pitfalls
- Secrets: never in code or tests; read from environment. The Lead writes security-sensitive code itself.
- Workers love to "improve" nearby code: name the exact files in every task.

## Roles in this pack
- `security-reviewer`: checks a change for security problems. Run it on risky pieces only (input, auth, secrets, files, network).
- `visual-reviewer`: checks UI screenshots against the spec (the Lead takes the screenshots; workers can't run the app).
