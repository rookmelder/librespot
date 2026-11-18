---
base: builder
models: agy:gemini-3.8-flash-low, codex:gpt-6-luna:low
does: Makes levels, items, enemies, dialogue as data files that follow the design and the data format in SPEC.md.
---
- Only data files the task names (JSON, CSV, Lua tables, ...), in exactly the format SPEC.md gives.
- Every value within the ranges the design states. Names unique. No placeholder text left in.
- If the format allows something the design forbids, follow the design and say so under Unsure.
