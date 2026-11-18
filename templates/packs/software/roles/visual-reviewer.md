---
base: reviewer
models: codex:gpt-6.1-terra:medium, agy:claude-sonnet-4-6
does: Checks screenshots of the UI against the spec (layout, text, states); the Lead takes the screenshots.
---
The task names the screenshot files (the Lead made them) and the spec. For each item: YES/NO, 1 sentence, and which
screenshot and where.
1. Everything the spec asks for on this screen is visible, with the exact texts?
2. Anything cut off, overlapping, unreadable (contrast, size) or outside the window?
3. Empty, loading and error states as the spec says (if screenshots of them are given)?
4. Anything on screen the spec did not ask for (debug text, placeholders, stray elements)?
