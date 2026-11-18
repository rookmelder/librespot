# Models you have access to

Status 2026-11-17. Two worker tools: **agy** (Google AI Pro) and **Codex** (ChatGPT plan). The `models:` lines in
`agent/roles/*.md` and `templates/packs/*/roles/*.md` use the first entries below. Model lists change: re-check.

**Priority** = pick order within a role: #1 first; the next one when #1 is busy, out of quota, or the same model
family as the work a tester/reviewer checks (`delegate.py --after`). **Rating** = quality as a worker, out of 10.
**Basis**: **T** = tested by us, **R** = reputation / the model's own description, **U** = not run yet in this setup.
Rate your own: `python agent/bench.py --workers <worker> ...` (easy: textkit, inventory; medium: ledger, duration
(tester), cfg (builderplus); real: the `sk_*` tasks on the shopkeeper project (~700 lines, 46 tests): sk_coupon
(builder), sk_refund and sk_refactor (builderplus), sk_tester, sk_review (reviewer), sk_security (security-reviewer),
sk_debug (debugger)). Use the real tasks to rate a model for a role; the easy ones only show that a worker runs.
Cost and limits: `LIMITS.md`.

## The models
| Worker | Family | Use it for | Basis |
|---|---|---|---|
| `agy:gemini-3.8-flash-low` | gemini | Small builds, bulk content, docs: cheapest, fastest | T (22/22 easy; 58/68 harder sk_* bench: misses subtle review bugs, 13/20 as tester) |
| `agy:gemini-3.8-flash-medium` | gemini | Reading a lot (summaries, mapping), builderplus fallback | T (62/68 harder sk_* bench, 17/20 as tester; 3-9x slower than low) |
| `agy:claude-sonnet-4-6` | claude | Every checking role: reviewer, security, safety, fact-check | T (fewest false findings) |
| `agy:gpt-oss-120b-medium` | gpt | Checklist reviews only, last fallback | T (makes things up otherwise) |
| `codex:gpt-6-luna` | gpt | Fast GPT: tester, builder/scribe fallback | R, U |
| `codex:gpt-6.1-sol` | gpt | Coding and writing: builderplus, writer, designer, scripter | R, U |
| `codex:gpt-6-astra` | gpt | Complex coding and reasoning: hardest pieces, debugger, balance, security fallback | R, U |
| `codex:gpt-6.1-terra` | gpt | Images: visual-reviewer (screenshots) | R, U |

In the model cache but not in the user's Codex picker (2026-11-17): `gpt-6.2-sol`, `gpt-6.2-astra`. Check that the
plan offers them before using them. Legacy, not used: `gpt-6-sol`, `gpt-5.6-*`.

## Pick order per role
| Role | Base | #1 | #2 | #3 |
|---|---|---|---|---|
| Lead | - | Claude Opus 5.5 (T) | | |
| builder | builder | `agy:gemini-3.8-flash-low` | `codex:gpt-6-luna:low` | |
| builderplus | builder | `codex:gpt-6.1-sol:medium` | `agy:gemini-3.8-flash-medium` | `codex:gpt-6-astra:high` |
| tester | tester | `codex:gpt-6-astra:high` | `codex:gpt-6.1-sol:medium` | `agy:gemini-3.8-flash-medium` |
| reviewer | reviewer | `codex:gpt-6-astra:high` | `agy:claude-sonnet-4-6` | `agy:gpt-oss-120b-medium` |
| scribe | scribe | `agy:gemini-3.8-flash-low` | `codex:gpt-6-luna:low` | |
| debugger | reviewer | `agy:gemini-3.8-flash-medium` | `agy:claude-sonnet-4-6` | `codex:gpt-6-astra:high` |
| mapper | scribe | `agy:gemini-3.8-flash-medium` | `codex:gpt-6.1-sol:low` | |
| security-reviewer (software) | reviewer | `codex:gpt-6-astra:high` | `agy:claude-sonnet-4-6` | |
| visual-reviewer (software) | reviewer | `codex:gpt-6.1-terra:medium` | `agy:claude-sonnet-4-6` | |
| designer (game) | scribe | `codex:gpt-6.1-sol:medium` | `agy:gemini-3.8-flash-medium` | |
| content (game) | builder | `agy:gemini-3.8-flash-low` | `codex:gpt-6-luna:low` | |
| balance (game) | reviewer | `codex:gpt-6-astra:high` | `agy:claude-sonnet-4-6` | |
| summarizer (research) | scribe | `agy:gemini-3.8-flash-medium` | `codex:gpt-6-luna:low` | |
| writer (research) | scribe | `codex:gpt-6.1-sol:medium` | `agy:gemini-3.8-flash-medium` | |
| fact-checker (research) | reviewer | `agy:claude-sonnet-4-6` | `codex:gpt-6.1-sol:medium` | |
| scripter (automation) | builder | `codex:gpt-6.1-sol:medium` | `agy:gemini-3.8-flash-medium` | |
| safety-reviewer (automation) | reviewer | `agy:claude-sonnet-4-6` | `codex:gpt-6-astra:high` | |

Why: the cheapest model that should be good enough goes first. Checking roles have two families, because the script
skips the family of the work they check. Claude Sonnet is for checking roles only (separate agy pool).
2026-11-18 (`notes/2026-11-18-codex-smoke-test-and-bench.md`): builderplus, tester, reviewer, debugger and
security-reviewer reordered by the shopkeeper benchmark. Codex workers cannot read files (only `--include`), so
the debugger, which must follow a cause across files, starts on agy; builderplus starts on Codex sol
(user's choice: its refactor 0/14 was a stop on an unclear spec, 14/14 on re-run; give it every file with `--include`); roles that get everything pasted in
(tester, reviewer, security) start on Codex astra. Claude Sonnet is not benchmarked yet.

### Private (`private: yes` in PROJECT.md)
agy and Codex both allowed (`private_ok` in `models.json`), with "Improve the model for everyone" OFF in ChatGPT.

### Avoid (no role)
| Model | Where | Why |
|---|---|---|
| `gemini-3.8-flash-high` | agy | Overthinks: 50k+ thinking tokens, 9 min timeouts |
| `claude-opus-4-6-thinking` | agy | Duplicates the Lead and burns the agy pool |

Removed 2026-11-17: the API workers (NVIDIA, Gemini API, OpenRouter, OpenAI through Copilot CLI). Their measurements
stay in `agent/notes/`.
