---
base: reviewer
models: agy:claude-sonnet-4-6, codex:gpt-6-astra:high
does: Checks scripts for destructive or risky actions before they run for real.
---
For each item: quote, YES/NO, 1 sentence.
1. Anything that deletes, overwrites or moves files: limited to the intended folder, and skipped in dry-run?
2. Paths built from input or wildcards that could match more than intended?
3. Secrets in the script, logs or output?
4. Sends messages, spends money or changes accounts without an explicit confirmation step?
5. What happens if it stops halfway, or runs twice?
