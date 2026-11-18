# Multi-Agent: a template for working with an AI team

A starting folder for any project (software, games, research, automation, writing) where **Claude leads**
and **cheaper AI models do the work**. Clone it once per project.

## The idea in one paragraph

Claude (in Claude Code) is the **boss**. It is smart but expensive, so it does the thinking: understand what you
want, plan, write exact instructions, check every result, and save the work in git. The actual making (code, text,
data) is handed to **cheaper AI models** ("workers") on subscriptions you already have. Workers are fast but make mistakes, so nothing they do
is trusted until Claude has checked it.

## Starting a new project

1. Make an empty repo for the new project (e.g. on GitHub), then:
   ```bash
   git clone https://github.com/rookmelder/Multi-Agent.git my-project
   ```
   ```bash
   cd my-project
   ```
   ```bash
   git remote set-url origin https://github.com/<you>/my-project.git
   ```
2. Set up what you have (you don't need everything): see [agent/setup/README.md](agent/setup/README.md).
   No API keys: workers run through agy (Google AI Pro) and Codex (ChatGPT plan).
3. Write **[GOAL.md](GOAL.md)**: what you want, in your own words, with as much detail as you can.
4. Open the folder in Claude Code and say "let's start". Claude then:
   - checks which worker tools work on your PC (writes `SETUP.md`) and asks a few questions about your plans;
   - reads your goal and asks about anything important that's missing;
   - picks a **pack** for your kind of project (software, game, research, automation);
   - proposes the **roles** (which workers, doing what, with which models) for you to approve;
   - fills in **PROJECT.md** (how the work is checked, notes for the workers).
5. Then just ask Claude for work. It splits it up, hands it out, checks it and commits it.

## The pieces

| Piece | What it is, simply |
|---|---|
| **Lead** | Claude. Plans, checks, decides, commits. |
| **Worker** | A cheaper AI model that does one small, clearly described job. |
| **Model** | The "brain" (Gemini, GPT, Claude Sonnet, ...). On its own a model can only send back text. |
| **Harness** | The program that lets a model actually *do* things on your PC: open and write files. We use **agy** (Google's Antigravity CLI) and **Codex CLI** (OpenAI). It asks the model what it wants to do, checks if that's allowed, and does it. |
| **Role** | A job description for a worker: what it does, which models may do it, and its rules. |
| **Pack** | A ready-made set of roles and advice for one kind of project. |
| **`agent/delegate.py`** | The one script that sends a job to a worker: picks the model, adds the rules, runs it safely, reports back. |

## Roles

Every role is one small file in `agent/roles/`. It says:
- its **base**: what kind of worker it is, which decides what it is allowed to do:
  | Base | Allowed to |
  |---|---|
  | builder | make and change files |
  | tester | write tests/checks (only in the test folder) |
  | reviewer | read only, never change anything |
  | scribe | write documents (`.md`) only |

  No worker can run commands. `delegate.py` runs the project's check command itself.
- its **models**: first choice, then fallbacks if that one is busy or out of quota;
- its **own rules**: a few lines about what's special about the job.

Default roles: **builder**, **builderplus** (bigger changes), **tester**, **reviewer**, **scribe**, **debugger**
(finds why a fix keeps failing, without fixing it) and **mapper** (drafts the knowledge map for existing code).
Packs add more, like a game **designer**, a research **fact-checker** or a software **visual-reviewer**. To make your own, copy
[templates/roles/ROLE-TEMPLATE.md](templates/roles/ROLE-TEMPLATE.md); it explains every line.
Claude usually proposes the roles for you.

Default models (covered by subscriptions you already have):

| Role | 1st choice | 2nd | 3rd |
|---|---|---|---|
| builder | Gemini 3.8 Flash Low (agy) | GPT-6 Luna, low (Codex) | |
| builderplus | GPT-6.1 Sol, medium (Codex) | GPT-6 Astra, high (Codex) | Gemini 3.8 Flash Medium (agy) |
| tester | GPT-6 Luna, medium (Codex) | Gemini 3.8 Flash Low (agy) | |
| reviewer | Claude Sonnet 4.6 (agy) | GPT-6.1 Sol, medium (Codex) | gpt-oss-120b (agy) |
| scribe | Gemini 3.8 Flash Low (agy) | GPT-6 Luna, low (Codex) | |
| debugger | GPT-6 Astra, high (Codex) | Claude Sonnet 4.6 (agy) | |
| mapper | Gemini 3.8 Flash Medium (agy) | GPT-6.1 Sol, low (Codex) | |

A tester or reviewer never uses the same model family as the builder it checks: `delegate.py` skips that family.
Workers you haven't set up are skipped automatically. Why this order, and every other model: [agent/MODELS.md](agent/MODELS.md).

## Packs

| Pack | For | Extra roles |
|---|---|---|
| [software](templates/packs/software/pack.md) | Apps, tools, libraries, backends, websites | security-reviewer |
| [game](templates/packs/game/pack.md) | Games in any engine | designer, content, balance |
| [research](templates/packs/research/pack.md) | Reports, comparisons, literature reviews | summarizer, writer, fact-checker |
| [automation](templates/packs/automation/pack.md) | Scripts and workflows | scripter, safety-reviewer |

Each pack says what a good goal should cover, how to check the work, how to split it, and the usual traps.

## What happens when Claude hands out a job

1. Claude writes a short task: the goal, which files, the exact spec, what must be covered.
2. `delegate.py` adds the role's rules and the notes from `PROJECT.md`, and pastes in the files the worker needs.
3. It starts the worker with **only the tools its role allows**, inside a **sandbox**: it can't touch anything outside
   the project, can't use git, can't browse the web, can't run commands, and can't read or write secret files such as
   `.env` or keys.
4. The worker does its piece and ends with a short report. Then `delegate.py` runs the project's check command on
   that piece. If it fails, the worker gets the output to fix it, and the check runs again (a second time only if the
   first fix changed the failure: a worker that is stuck goes back to Claude).
5. Claude checks: did it only touch the allowed files? Does the full check pass? Is the work right?
   Then Claude fixes, asks a reviewer, and commits.

| Job size | What happens |
|---|---|
| Tiny (under ~30 lines) | Claude just does it; explaining would take longer. |
| Small (1 file) | 1 worker, then Claude checks. |
| Medium (2-4 files) | 2 workers at once on different files, then a tester and a reviewer, then Claude fixes and commits. |
| Large (5+ files) | Claude writes a spec first, workers go in waves (max 3 at once), then testers/reviewers and a final check. |

## Money, limits and privacy

| Tool | Costs | Limit |
|---|---|---|
| Claude | Your Claude plan | Plan limits; kept for thinking and checking |
| agy | Google AI Pro plan | Resets every 5 hours; ~20 worker jobs use 6-7% of a window |
| Codex | Your ChatGPT plan | The plan's Codex limits (shared with your own Codex use) |

- **Privacy:** no free tiers that train on what you send. A project is private until `PROJECT.md` says `private: no`;
  private projects may use agy and Codex. Turn off "Improve the model for everyone" in ChatGPT first.
- Workers never get `.env`, keys or credential files: `delegate.py` refuses to paste them in and denies them to every worker.

## Keeping tokens (and so cost and speed) down

Every step a model takes resends the whole conversation (the API has no memory), so each step is kept small:
workers get only the tools their role needs, short rules, the files they need pasted in up front, and they only
run their own check. On a test job this saved 38-59% of the tokens at the same quality.

## The knowledge map

Big projects get too large to read in one go, for Claude and for the workers. So the project keeps a map in
`knowledge/`: one small note per part ("what the shop does, which files, what it talks to, why it's built that
way"), per lesson learned, and per decision. Indexes link them, one line each. Claude reads the index, then only
the one note it needs, and hands that note to a worker instead of half the code. A cheap scribe worker updates
the notes after each change; Claude checks them. The folder also opens as an Obsidian vault (graph view
included), and `grep -rh "^> " knowledge` prints the whole map in one screen.

## The files

| File / folder | What it is |
|---|---|
| `GOAL.md` | **You write this:** what you want. |
| `PROJECT.md` | Claude fills this in: settings and notes for this project. |
| `SETUP.md` | Made by `agent/check_setup.py`: what works on your PC (not committed). |
| `CLAUDE.md` | The rulebook Claude reads at the start of every session. |
| `agent/roles/` | The roles in use in this project. |
| `agent/bases/` | The core rules for each base type. |
| `agent/delegate.py` | Runs a worker. `agent/check_setup.py` checks your setup. |
| `agent/MODELS.md` | Which model to pick per role, with ratings. `agent/LIMITS.md`: cost and limits per tool. |
| `agent/models.json` | The models, model families and harness rules `delegate.py` knows. |
| `agent/bench.py` | Rates your own models per role: easy tasks and real ones on a small shop project (`agent/bench/_projects/`), scored with hidden tests, planted bugs or answer keys the worker never sees. |
| `agent/START.md` | The steps Claude follows at the start of a project. |
| `agent/notes/` | Measurements and lessons about the setup itself, one small note each. |
| `knowledge/` | The project's knowledge map: one small note per part, lesson or decision (see below). |
| `agent/check_docs.py` | Checks the notes (broken links, notes nobody links to, file paths that no longer exist); `--find <words>` searches them. |
| `agent/setup/` | How to install and sign in to each tool. |
| `templates/` | Role template, project packs and note templates. |
| `.agents/agents/` | Trimmed tool sets for agy workers (your own agy chats are not affected). |
| `.worker/` | Scratch space for tasks, results and benchmark rows (not committed). |

## Running a worker yourself

```bash
python agent/delegate.py --role builder --task-file .worker/t1.md --part shop --files src/shop.py tests/test_shop.py --include src/shop.py
```

- `--role` picks the role (any file in `agent/roles/`). `--include` pastes files into the task.
- `--model` (an agy model) or `--worker codex:gpt-6.1-sol:medium` force one specific worker.
- Testers and reviewers need `--after <the builder's run id>` (or `--after lead`).
- Result codes: 0 OK, 2 bad input, 3 took too long, 4 blocked (a secret or outside path, or a forbidden action),
  5 empty answer, 6 worker problem, quota, or no worker available, 7 check still failing.

## Known weak spots

- Workers make mistakes; that's expected, Claude checks everything.
- Workers can't create folders and can't browse the web; Claude does those parts.
- agy is sometimes busy ("no capacity"); the next worker in the chain takes over.
- agy workers can only run commands allowed in agy's global settings (see `agent/setup/agy.md`).
- If a file called `AGENTS.md` or `GEMINI.md` is in the project, agy (and Codex, for `AGENTS.md`) loads it into every worker. Don't add one.
- The Codex worker is built from Codex's docs and not run yet: [agent/setup/codex.md](agent/setup/codex.md) has the smoke test.
