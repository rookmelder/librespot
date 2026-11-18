#!/usr/bin/env python3
"""Find which worker tools this machine has and write SETUP.md (git-ignored). delegate.py skips workers marked "no" there.
Run it at the start of a project and whenever you install a tool or sign in:
  python agent/check_setup.py
Workers: agy (Google Antigravity CLI) and codex (OpenAI Codex CLI, signed in with ChatGPT). No API keys.
Standard library only; Windows, Linux and macOS.
"""
import datetime
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from delegate import IS_WIN, ROOT, read_text  # noqa: E402

TOOLS = {
    'git': 'Required: tracks what workers change.',
    'agy': 'Google Antigravity CLI: Gemini workers and the agy Claude/gpt-oss reviewers (Google AI Pro plan for real quota).',
    'codex': 'OpenAI Codex CLI: GPT workers on your ChatGPT plan (sign in once with `codex login`).',
    'python': 'Only if the project uses Python.',
    'node': 'Only if the project uses JavaScript/TypeScript.',
}


def refresh_path():
    """Windows: add the saved Machine/User PATH, so tools installed after this shell started are found."""
    if not IS_WIN:
        return
    import winreg
    parts = []
    for hive, sub in ((winreg.HKEY_LOCAL_MACHINE, r'SYSTEM\CurrentControlSet\Control\Session Manager\Environment'),
                      (winreg.HKEY_CURRENT_USER, 'Environment')):
        try:
            with winreg.OpenKey(hive, sub) as k:
                parts.append(os.path.expandvars(winreg.QueryValueEx(k, 'Path')[0]))
        except OSError:
            pass
    os.environ['PATH'] = os.pathsep.join(parts + [os.environ.get('PATH', '')])


def has(cmd):
    return bool(shutil.which(cmd) or (cmd == 'python' and shutil.which('python3')))


def codex_login():
    """-> (status text, signed in?). `codex login status` makes no model call."""
    try:
        r = subprocess.run([shutil.which('codex'), 'login', 'status'], capture_output=True, text=True,
                           encoding='utf-8', errors='replace', timeout=30)
    except (OSError, subprocess.TimeoutExpired):
        return 'login status unknown', False
    text = (r.stdout + r.stderr).strip()
    ok = r.returncode == 0 and bool(re.search(r'logged in', text, re.I)) and not re.search(r'not logged in', text, re.I)
    return (text.splitlines() or ['?'])[0] if ok else 'NOT signed in: run `codex login`', ok


def build_md(rows, avail, now):
    table = '\n'.join(f'| {i} | {s} | {n} |' for i, s, n in rows)
    block = '\n'.join(f"{k}: {'yes' if v else 'no'}" for k, v in avail.items())
    return f"""# Setup on this machine

Made by `agent/check_setup.py` on {now}. Not committed (each machine has its own).
Run it again after installing a tool or signing in. How to set each one up: `agent/setup/`.

| Item | Status | Note |
|---|---|---|
{table}

## Available workers (read by delegate.py; "no" = skipped)
```available
{block}
```

## Confirmed by the user (the Lead asks once and fills this in)
- Claude plan: <which plan>
- Google AI Pro (agy quota): <yes/no>
- ChatGPT plan for Codex: <which plan / none>
- ChatGPT "Improve the model for everyone" turned OFF (needed before private code goes to Codex): <yes/no>
"""


def keep_confirmed(new_md, old_md):
    """Keep the user's answers from a previous run (only the detected part is refreshed)."""
    m = re.search(r'(## Confirmed by the user.*)$', old_md or '', re.S)
    if not m:
        return new_md
    return re.sub(r'## Confirmed by the user.*$', '', new_md, flags=re.S) + m.group(1).rstrip() + '\n'


def main(argv=None):
    try:
        sys.stdout.reconfigure(encoding='utf-8', errors='replace')
    except AttributeError:
        pass
    os.chdir(ROOT)
    refresh_path()

    rows, avail = [], {}
    for t, note in TOOLS.items():
        found = has(t)
        status = 'found' if found else 'missing'
        if t == 'codex' and found:
            status, signed_in = codex_login()
            avail['codex'] = signed_in
        rows.append((t, status, note))
    avail['agy'] = has('agy')
    avail.setdefault('codex', False)
    avail = {k: avail[k] for k in ('agy', 'codex')}

    md = build_md(rows, avail, datetime.datetime.now().strftime('%Y-%m-%d %H:%M'))
    setup = ROOT / 'SETUP.md'
    md = keep_confirmed(md, read_text(setup) if setup.exists() else '')
    setup.write_text(md, encoding='utf-8', newline='\n')

    w = [max(len(r[i]) for r in rows + [('Item', 'Status', 'Note')]) for i in range(2)]
    print(f"{'Item':<{w[0]}}  {'Status':<{w[1]}}  Note")
    for i, s, n in rows:
        print(f'{i:<{w[0]}}  {s:<{w[1]}}  {n}')
    print('\nAvailable: ' + (', '.join(k for k, v in avail.items() if v) or 'none'))
    print('Written: SETUP.md')
    return 0


if __name__ == '__main__':
    sys.exit(main())
