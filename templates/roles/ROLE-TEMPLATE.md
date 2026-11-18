---
base: builder
models: agy:gemini-3.8-flash-low, codex:gpt-6-luna:low
does: One line: what this role makes or checks.
---
<!-- Copy this file to agent/roles/<name>.md and run it with: agent/delegate.py --role <name>

FRONTMATTER
- base: what kind of worker this is. It sets the worker's tools, sandbox and core rules (agent/bases/<base>.md):
    builder  = makes or changes files (code, text, data); delegate.py runs the check command on its part
    tester   = writes tests/checks for work it did not make; may only write in check_dir
    reviewer = reads and reports; never edits anything
    scribe   = writes markdown docs only
- models: the worker chain, first = main, the rest are fallbacks. Leave the line out to use the base's default.
    agy:<model>                 gemini-3.8-flash-low, gemini-3.8-flash-medium, claude-sonnet-4-6 (reviewers only), gpt-oss-120b-medium
    codex:<model>[:<effort>]    gpt-6-luna (fast), gpt-6.1-sol (coding), gpt-6-astra (complex), gpt-6.1-terra (images);
                                effort low | medium | high | xhigh
  Pick models from agent/MODELS.md (the script's list: agent/models.json). A tester/reviewer role needs models from
  at least two families (gemini, claude, gpt): delegate.py skips the family of the builder it checks.
- does: one line, shown to the Lead when choosing roles.

BODY (below the frontmatter): only what is special about this role. The base rules and the report format
are added automatically; don't repeat them. Keep it short and strict: workers follow short rules best.
- Write orders, not explanations: "Cite a source for every claim." not "It would be good to cite...".
- Name the files or folders it works in.
- Give a checklist for reviewer roles (questions it must answer YES/NO with a quote).
- Delete this comment. -->
Rules for this role:
- ...
