# Builder benchmark with bench.py, chains reordered (2026-11-15)
> Two-task builder benchmark: agy flash-low best; Mistral Large 4 correct but slow, moved last.

Back to [Notes](README.md).

Two tasks (inventory 10 hidden tests, textkit 12), one run each, Windows, through `agent/bench.py`.
| Worker | Hidden tests | Seconds (inventory / textkit) | Calls | Tokens in |
|---|---|---|---|---|
| agy gemini-3.8-flash-low | 22/22 | 23 / 31 | ? | 8.8k / 15.1k |
| Gemini API gemini-flash-lite-latest | 22/22 | 25 / 46 | 4 / 7 | 32k / 60k |
| OpenRouter nemotron-3-ultra:free | 22/22 | 78 / 260 | 6 / 11 | 49k / 95k |
| NVIDIA mistral-large-4 | 22/22 | 307 / 482 (timeout) | 6 / 4 | 42k / 10k |
| NVIDIA nemotron-3.5-lightning | 22/22 | 83 / 430 | 11 / 74 | 156k / 2.46M |
| OpenRouter cohere north-mini-code:free | 10/22 | 111 / 11 | 43 / 5 | 337k / 18k |
- Mistral Large 4 on NVIDIA is correct but takes 25-120 s per call: moved to the end of the builder, builderplus
  and tester chains. Gemini API flash-lite is the new builder fallback after agy.
- Nemotron Ultra needed 6-11 calls here, not the ~60 from the old benchmark: the 40-call cap does not stop it.
- Nemotron lightning loops as a builder; the cap stopped its fix round at 40, but the first run had already used 34.
  The cap now counts the whole run (a fix round only gets what is left).
- One run per cell: times vary by day (NVIDIA especially). Tester and builderplus chains were reordered by these
  builder numbers, not by a test of their own.
