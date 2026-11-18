# Shopkeeper benchmark: real tasks per role (2026-11-17)
> Seven tasks on a ~700-line shop project. First version: Flash Low 55/57 (too easy). Harder version: Low 58/68, Medium 62/68; only review, security and tester separate models.

Back to [Notes](README.md).

## What was built
- `agent/bench/_projects/shopkeeper/`: 10 modules (money, catalog, discounts, orders, invoices, storage, reports, cli),
  45 tests, a knowledge map. Each `sk_*` task lays its `start/` over it, in a fresh git repo, through `delegate.py`.
- Tasks: sk_coupon (builder, 10 hidden tests), sk_refund (builderplus, 3 files + format v3, 12), sk_refactor
  (builderplus, dict rules -> dataclasses, 11), sk_tester (15 mutants that survive the project's own tests),
  sk_review (reviewer, 3 planted bugs + 2 decoys in a pasted diff), sk_security (security-reviewer: path traversal,
  CSV formula guard, admin check after the action, token in a log; 1 decoy), sk_debug (debugger, cause in money.py,
  symptom in invoices, 1 decoy). Build tasks also count "kept" (the 45 original tests still green).
- Unit tests check every task: reference solution / answer = full score, start state = under half.

## First run (agy, both at once, 2026-11-17)
| Worker | coupon | refund | refactor | tester | review | security | debug | Total | Seconds (sum) |
|---|---|---|---|---|---|---|---|---|---|
| flash-low | 10/10 | 12/12 | 11/11 | 13/15 | 3/3 | 4/4 | 2/2 | 55/57 | 345 |
| flash-medium | 10/10 | 12/12 | 11/11 | 14/15 | 3/3 | 4/4 | 2/2 | 56/57 | 1518 |
- Missed: m14 (only the first line's discount reset) by both; m15 (stacked percents sorted) by low.
- No scope breaks, no false alarms, kept 45/45 everywhere. sk_refund needed 1 fix round on low.
- Medium: 3-7x the time and 2-9x the output tokens of low for the same result.

## Second version: harder (same day)
Changes: ticket-style specs that state rules and invariants instead of steps and edge cases (coupon, refund,
refactor); refactor +3 characterization tests (14); tester task without a spec list, 20 mutants (5 new);
review: PR description in the author's words, 5 bugs (+ ids sorted as text past O9999, CLI dates unchecked);
security: 5 subtler bugs (absolute export path, unguarded product name, admin check by substring, token in an error
message, unsigned reply accepted); debug: cause moved to `money.split` (left-over cents to the biggest weight),
symptom an invoice test two modules away. The base project got that invoice test (46 tests).

| Worker | coupon | refund | refactor | tester | review | security | debug | Total | Seconds (sum) |
|---|---|---|---|---|---|---|---|---|---|
| flash-low | 10/10 | 12/12 | 14/14 | 13/20 | 3/5 | 4/5 | 2/2 | 58/68 | 275 |
| flash-medium | 10/10 | 12/12 | 14/14 | 17/20 | 3/5 | 4/5 | 2/2 | 62/68 | 1712 |
- Both missed the same review bugs (ids as text, unchecked dates) and the unguarded product name; no false alarms.
- Tester is the only task that separates them (13 vs 17): low missed mutants needing multi-line orders, earlier
  rules or coupon edge cases. One run each: a signal for the tester fallback, not yet a reason to change it.
- Build and debug tasks are still solved fully by both: they do not rank models yet.

## What it means
- With an exact spec (as CLAUDE.md asks), Flash Low handles realistic multi-file work: keep it first for builder.
- First version: too easy to rank models (the specs and review lists pointed at every answer). The second version
  separates them on review, security and tester; build and debug need still harder tasks (or Codex runs to compare).
