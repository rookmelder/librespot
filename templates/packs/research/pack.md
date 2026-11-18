# Pack: research (reports, comparisons, literature reviews)

## What a good GOAL.md covers (ask about what's missing)
- The question(s) to answer, and who the answer is for (decision, article, study).
- Scope: time period, region, how deep; sources that must or must not be used.
- Output: report length and format, comparison table, recommendation or neutral overview.
- Deadline-type limits: "good enough with 10 sources" vs "exhaustive".

## PROJECT.md suggestions
- Settings: all empty (no check command). Worker notes: citation style, language, tone, reading level.
- Folders: `sources/` (one file per source, saved by the Lead), `notes/` (summaries), `report/` (the output).

## How to split the work
Workers have **no internet** (sandbox). The Lead does the searching and saves what it finds:
1. Lead splits the question into sub-questions and searches the web for each.
2. Lead saves each useful source as `sources/<short-name>.md`: title, author, date, URL, and the relevant text.
3. `summarizer` turns sources into notes per sub-question in `notes/`, with a citation on every claim.
4. `writer` drafts report sections from the notes only.
5. `fact-checker` checks every claim in a draft against `sources/`. Lead fixes, then writes the conclusion itself.

## Pitfalls
- Models invent sources and quotes. A claim without a source file behind it doesn't go in the report.
- Old sources: note the date of every source; prefer recent ones for fast-moving topics.
- Free cloud workers may log prompts: if the research is confidential, mark the project private.

## Roles in this pack
- `summarizer`: sources to cited notes.
- `writer`: notes to report sections.
- `fact-checker`: checks claims against the sources.
