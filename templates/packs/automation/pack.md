# Pack: automation (scripts and workflows)

## What a good GOAL.md covers (ask about what's missing)
- What happens now by hand, step by step, and how often.
- Where it runs (this PC, a server, scheduled or on demand) and with what (PowerShell, Python, Node, ...).
- What it touches: which files, folders, accounts, services. What must never be touched.
- What happens when something fails halfway. Who gets told.

## PROJECT.md suggestions
- check commands for the script language (see the software pack); tests run against temporary folders and fake services.
- Worker notes to consider: every script has a dry-run mode that only prints what it would do; logs what it did;
  safe to run twice (idempotent); reads secrets from environment variables, never from the script.

## How to split the work
1. Lead writes the steps as a spec: inputs, outputs, each action, what to do on each failure.
2. `scripter` builds one step or script at a time with tests on temp data.
3. `safety-reviewer` reviews every script before it ever runs for real.
4. The Lead runs it in dry-run mode, shows the user the output, and only then runs it for real (with the user's OK).

## Pitfalls
- Deleting, moving or overwriting files: always test on copies in a temp folder first.
- Hard-coded paths and user names break on other machines: use parameters or environment variables.
- Anything that sends messages, spends money or changes accounts: the user approves before the first real run.

## Roles in this pack
- `scripter`: builds scripts with dry-run and logging.
- `safety-reviewer`: checks scripts for destructive or risky actions.
