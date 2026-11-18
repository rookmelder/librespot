# Scripts ported to Python (2026-11-13)
> PowerShell scripts became stdlib Python (delegate.py, check_setup.py); first smoke test of every worker.

Back to [Notes](README.md).

- `delegate.ps1` / `check-setup.ps1` replaced by `agent/delegate.py` / `agent/check_setup.py` (stdlib only) so the
  setup runs on Linux/macOS too. Same behaviour; flags are now `--role`, `--task-file`, ... Unit tests: `agent/tests/`.
  Reason besides Linux: on this PC `powershell -File` was blocked by the execution policy.
- Smoke test on Windows, one small task each, all OK: agy gemini-3.8-flash-low builder 12s; NVIDIA Mistral Large 4
  builder 281s (slow day); agy claude-sonnet-4-6 reviewer 12s; NVIDIA Nemotron lightning scribe 26s; Gemini API
  tester 22s; `--conversation` follow-up 10s; OpenAI gpt-5.4-mini 16s (usage logged); timeouts give exit 3 on agy
  and Copilot with no copilot.exe left over; a tester ran `check_one` via Copilot's shell tool; Ollama gemma4:12b 85s.
- Not tested yet: a real Linux machine (there Copilot's shell tool should be `bash,read_bash`: unverified).
- Lesson: a reviewer without `--include` (OpenAI gpt-5.4-mini, no check command) did not open the file and answered
  "Unproven" for everything. Always `--include` the files a reviewer must judge.
- NVIDIA's 1-token key test took 15-30+ s; check_setup.py waits up to 60 s for it.
