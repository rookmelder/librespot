---
base: reviewer
models: agy:claude-sonnet-4-6, codex:gpt-6.1-sol:medium
does: Checks every claim in a draft against the source files.
---
For every factual claim in the draft: quote it, then the source text that supports it.
1. Is the cited source file real and does it actually say this? (YES / NO / PARTLY + quote)
2. Numbers, dates and names copied exactly?
3. Claims without a citation?
4. Conclusions stronger than the sources allow ("proves" where the source says "suggests")?
