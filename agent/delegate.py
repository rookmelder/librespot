#!/usr/bin/env python3
"""Send one task to a worker (agy or Codex) by role, with safe defaults.

Examples (from the project root; Windows, Linux and macOS):
  python agent/delegate.py --role builder --task-file .worker/t1.md --include src/shop.py
  python agent/delegate.py --role reviewer --after 3fa2b1c0 --task-file .worker/review.md
  python agent/delegate.py --role builder --worker codex:gpt-6.1-sol:medium --task-file .worker/t1.md   # force one worker

Roles: any agent/roles/<name>.md (frontmatter: base, models). Core rules per base: agent/bases/<base>.md.
Each role has a worker chain (agy:<model> or codex:<model>[:<effort>]); workers missing on this machine (SETUP.md)
are skipped. On a busy/quota error it retries once when the wait is short, then moves on. --model / --worker force one.
Models, families and harness rules live in agent/models.json.
Workers run no commands. For builder/tester roles this script runs check_one itself after the worker is done
(--part fills {name}); when it fails, the worker gets the output to fix it, then the check runs again
(a second round only if the first one changed the failure).
For reviewer roles the check runs first and its result is pasted into the task.
Tester and reviewer roles need --after <builder run id> (or --after lead): they never run on the builder's model family.
Paths: --include and --files must stay inside the project and never name a secret (.env, keys, credentials);
workers are denied reading or writing the secret files this script finds in the project.
--files names what the task may change: with workers in parallel, each run then reports only its own files.
Output: a short summary (full JSON saved in .worker/run-<id>.json and .worker/last.json); --json prints the JSON instead.
Exit: 0 ok, 2 bad input, 3 timeout (check git status: files often exist), 4 blocked, 5 empty, 6 worker error,
7 check failed.
Project settings (check commands, worker notes) come from PROJECT.md. Standard library only.
"""
import argparse
import fnmatch
import hashlib
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import uuid
from pathlib import Path

AGENT_DIR = Path(__file__).resolve().parent      # agent/: this script, bases/, roles/
ROOT = AGENT_DIR.parent                           # project root: workers work here
IS_WIN = os.name == 'nt'

CONFIG = json.loads((AGENT_DIR / 'models.json').read_text(encoding='utf-8-sig'))   # models, families: edit that file
_plain = lambda d: {k: v for k, v in d.items() if not k.startswith('_')}
AGY_MODELS = CONFIG['agy_models']
CODEX_EFFORTS = CONFIG['codex_efforts']
FAMILIES = _plain(CONFIG['families'])                  # model id prefix -> family
PRIVATE_OK = CONFIG['private_ok']                      # harnesses a private project may use
HARNESSES = ('agy', 'codex')
BASES = ('builder', 'tester', 'reviewer', 'scribe')
DEFAULT_MODELS = _plain(CONFIG['default_models'])      # used when a role file has no models: line
TRANSIENT = re.compile(r'429|503|UNAVAILABLE|RESOURCE_EXHAUSTED|capacity|rate limit|overloaded|timed out', re.I)
LABELS = {0: 'OK', 3: 'TIMEOUT (check git status)', 4: 'BLOCKED', 5: 'EMPTY', 6: 'WORKER ERROR', 7: 'CHECK FAILED'}
MAX_RETRY_WAIT = 120   # a worker asking for a longer wait: go to the next worker instead
CHECK_TAIL_LINES, CHECK_TAIL_CHARS = 60, 6000   # how much check output a worker / the summary gets

# Secret files: never pasted in, never in --files, denied to every worker.
SECRET_NAME = re.compile(r'^(\.env(\.(?!(example|sample|template|dist)$)[^/]+)?|\.netrc|\.git-credentials|\.npmrc|\.pypirc'
                         r'|id_(rsa|dsa|ecdsa|ed25519)(\.pub)?|credentials(\.json)?|.+\.(pem|key|p12|pfx|jks|keystore))$', re.I)
SECRET_GLOBS = ['**/.env', '**/.env.*', '**/*.pem', '**/*.key', '**/*.p12', '**/*.pfx', '**/id_rsa*', '**/id_ed25519*',
                '**/.netrc', '**/.git-credentials', '**/credentials.json']   # Codex deny-read patterns (on top of the files found)
SKIP_DIRS = {'.git', 'node_modules', '.venv', 'venv', '__pycache__', '.worker', '.mypy_cache', '.pytest_cache'}


class Fail(Exception):
    def __init__(self, msg, code):
        super().__init__(msg)
        self.code = code


def read_text(path):
    return Path(path).read_text(encoding='utf-8-sig')


# --- Files the scripts read ----------------------------------------------------------------
def parse_project(text):
    """PROJECT.md: the ```settings block, the '## Worker notes' section and the Private flag."""
    proj = {'check_one': '', 'check_all': '', 'check_dir': '', 'private': ''}
    m = re.search(r'```settings\s*\r?\n(.*?)```', text, re.S)
    if m:
        for line in re.split(r'\r?\n', m.group(1)):
            lm = re.match(r'^\s*(\w+)\s*:\s*(.*?)\s*$', line)
            if lm:
                proj[lm.group(1)] = lm.group(2).strip('`')
    notes = ''
    m = re.search(r'## Worker notes[^\n]*\n(.*?)(?=\n## |\Z)', text, re.S)
    if m:
        notes = m.group(1).strip()
    if notes.startswith('- <'):   # template placeholder, not filled in
        notes = ''
    # Private = only the harnesses in models.json private_ok. Anything but a clear "no" counts as private.
    val = proj.get('private', '').lower()
    if val in ('yes', 'no'):
        private = val == 'yes'
    else:   # older PROJECT.md: the "**Private?** yes/no" line under About
        old = re.search(r'\*\*Private\?\*\*\s*(yes|no)\b', text)
        private = not (old and old.group(1) == 'no')
    return proj, notes, private


def parse_role(text):
    """Role file = frontmatter (base, models, does) + extra instructions. Returns (fields, body)."""
    m = re.match(r'^---\s*\r?\n(.*?)\r?\n---\s*\r?\n?(.*)$', text, re.S)
    if not m:
        return {}, text
    fm = {}
    for line in re.split(r'\r?\n', m.group(1)):
        lm = re.match(r'^\s*(\w+)\s*:\s*(.*?)\s*$', line)
        if lm:
            fm[lm.group(1)] = lm.group(2)
    return fm, m.group(2).strip()


def to_worker(s):
    """'agy:gemini-3.8-flash-low' | 'codex:gpt-6.1-sol' | 'codex:gpt-6-luna:low' -> {'h', 'm', 'e'}"""
    head, _, rest = s.strip().partition(':')
    model, _, effort = rest.partition(':')
    if head not in HARNESSES or not model:
        raise Fail(f"Worker '{s.strip()}': write agy:<model> or codex:<model>[:<effort>].", 2)
    if head == 'agy' and effort:
        raise Fail(f"Worker '{s.strip()}': agy models have no :<effort> (pick another agy model instead).", 2)
    if effort and effort not in CODEX_EFFORTS:
        raise Fail(f"Worker '{s.strip()}': effort must be one of {', '.join(CODEX_EFFORTS)}.", 2)
    return {'h': head, 'm': model, 'e': effort}


def parse_chain(models):
    return [to_worker(x) for x in models.split(',') if x.strip()]


def worker_name(w):
    return f"{w['h']} {w['m']}" + (f" ({w['e']})" if w.get('e') else '')


def family(model):
    """Model id -> family (models.json families), so a tester/reviewer never checks its own family's work."""
    for prefix, fam in FAMILIES.items():
        if (model or '').startswith(prefix):
            return fam
    return model or ''


def parse_available(text):
    """SETUP.md ```available block -> {'agy': True, ...}, or None when there is no block."""
    m = re.search(r'```available\s*\r?\n(.*?)```', text, re.S)
    if not m:
        return None
    out = {}
    for line in re.split(r'\r?\n', m.group(1)):
        lm = re.match(r'^\s*(\w+)\s*:\s*(yes|no)', line)
        if lm:
            out[lm.group(1)] = lm.group(2) == 'yes'
    return out


def is_available(w, available):
    return available is None or bool(available.get(w['h']))


# --- Paths: inside the project, never a secret ---------------------------------------------
def is_secret(path):
    return bool(SECRET_NAME.match(str(path).replace('\\', '/').rstrip('/').rsplit('/', 1)[-1]))


def project_path(p, what):
    """A path or glob the Lead gave (--include, --files) -> relative to the project root, with / slashes.
    Refused (exit 4): outside the project, inside .git/, or a secret file."""
    raw = str(p).strip()
    full = raw if os.path.isabs(raw) else os.path.join(str(ROOT), raw)
    try:
        rel = os.path.relpath(os.path.realpath(full), os.path.realpath(str(ROOT))).replace('\\', '/')
    except ValueError:   # another drive (Windows)
        rel = '../'
    if rel == '..' or rel.startswith('../') or os.path.isabs(rel):
        raise Fail(f"{what} '{raw}': outside the project folder. Workers only get files from the project.", 4)
    if rel.split('/')[0] == '.git':
        raise Fail(f"{what} '{raw}': inside .git/.", 4)
    if is_secret(rel):
        raise Fail(f"{what} '{raw}': a secret file (.env, keys, credentials) never goes to a worker.", 4)
    return rel


def secret_files(root):
    """Absolute paths of the secret files in the project (what workers are denied), skipping .git and caches."""
    out = []
    for d, dirs, files in os.walk(str(root)):
        dirs[:] = [x for x in dirs if x not in SKIP_DIRS]
        out += [os.path.join(d, f) for f in files if is_secret(f)]
    return sorted(out)


def check_command(check_one, part):
    """check_one with {name} filled in -> (command, ''), or (None, why it can't run)."""
    cmd = (check_one or '').strip()
    if not cmd:
        return None, 'no check command in PROJECT.md'
    if '{name}' in cmd:
        if not part:
            return None, 'give --part <name> to fill {name} in check_one'
        cmd = cmd.replace('{name}', part)
    return cmd, ''


def check_text(check):
    """A check result as text for a worker or the summary."""
    if check.get('skipped'):
        return f"not run ({check['skipped']})"
    head = 'PASS' if check['passed'] else ('FAIL: timed out' if check['timed_out'] else f"FAIL (exit {check['exit']})")
    return f"{head} | `{check['cmd']}`"


def failure_key(check):
    """The check output without what changes on every run (timings, addresses): equal keys = no progress."""
    out = re.sub(r'\d+(\.\d+)?\s*(s|ms|sec|seconds)\b', '', check.get('output') or '')
    return re.sub(r'0x[0-9a-fA-F]+', '', out).strip()


def fix_prompt(base, check):
    who = ('If a test is wrong, fix the test. If the work itself is wrong, change nothing and list it under Failures.'
           if base == 'tester' else 'Fix the real cause, only in the files your task names.')
    return (f"The Lead's script ran the check on your part: {check_text(check)}\n\nOutput (last lines):\n```\n{check['output']}\n```\n\n"
            f'{who} Run no commands: the check runs again after you. End with the report.')


def retry_wait(error):
    """Seconds to wait before retrying a busy worker: what its error asks for (Retry-After, retryDelay,
    "retry in 17s"), else 20. None = it asks for more than MAX_RETRY_WAIT: don't retry, try the next worker."""
    m = re.search(r'retry[ _-]?after\W{0,4}(\d+(?:\.\d+)?)|retryDelay\W{0,4}(\d+(?:\.\d+)?)s|'
                  r'(?:retry|try again) in (\d+(?:\.\d+)?)\s*s', str(error or ''), re.I)
    if not m:
        return 20
    wait = float(next(g for g in m.groups() if g))
    return None if wait > MAX_RETRY_WAIT else int(wait + 0.999) + 1


def build_prompt(task, base, role, role_body, rules, proj, notes, includes, conversation, check=None):
    if not conversation:
        if role_body:
            rules += f'\n## Your role: {role}\n{role_body}\n'
        if base == 'reviewer' and check and not check.get('skipped'):
            how = (f"Run no commands. The Lead's script just ran the check: {check_text(check)}\n"
                   f"Output (last lines):\n```\n{check['output']}\n```")
        elif base in ('builder', 'tester') and proj['check_one']:
            how = ("Run no commands. When you are done, the Lead's script runs the check on your part "
                   f"(`{proj['check_one']}`). If it fails you get the output to fix it.")
        elif base == 'builder':
            how = 'This project has no check command: run no commands. Re-read your files against the spec instead.'
        else:
            how = 'Run no commands.'
        task = rules + f"\n---\n# PROJECT\n{how}{chr(10) + notes if notes else ''}\n\n---\n# YOUR TASK\n\n" + task
    for f in includes:   # line numbers only for a reviewer (it quotes lines); they cost ~12% more tokens on every call
        text = read_text(f).rstrip('\n')
        if base == 'reviewer':
            body = '\n'.join(f'{n:>4}  {line}' for n, line in enumerate(text.splitlines(), 1))
            task += f'\n\n## File: {f} (line numbers added; do not copy them)\n```\n{body}\n```'
        else:
            task += f'\n\n## File: {f}\n```\n{text}\n```'
    if includes:
        task += "\n\nThe files above are current. Don't re-read them; read others only if the task needs them."
    return task


def git_snapshot(root):
    """Every file git sees as changed or new -> {path: (status, content hash)}.
    The hash also shows a second edit to a file that was already changed before the worker started."""
    r = subprocess.run(['git', '-C', str(root), 'status', '--porcelain', '-z', '-uall'],
                       capture_output=True, text=True, encoding='utf-8', errors='replace')
    snap, parts, i = {}, r.stdout.split('\0'), 0
    while i < len(parts):
        entry = parts[i]
        i += 1
        if len(entry) < 4:
            continue
        xy, path = entry[:2], entry[3:]
        if 'R' in xy or 'C' in xy:   # rename/copy: the next field is the old path
            i += 1
        if re.search(r'__pycache__|(^|/)\.worker/', path):
            continue
        try:
            digest = hashlib.sha1((Path(root) / path).read_bytes()).hexdigest()
        except OSError:
            digest = None   # deleted
        snap[path] = (xy, digest)
    return snap


def changed_since(before, after):
    """['?? new.py', ' M edited.py', '-- back_to_clean.py'] for everything that differs between two snapshots."""
    out = [f'{after[p][0]} {p}' for p in sorted(after) if after[p] != before.get(p)]
    return out + [f'-- {p}' for p in sorted(before) if p not in after]


def in_scope(line, patterns):
    """Is this changed-file line one of the files (or globs) the task may touch?"""
    path = line[3:].replace('\\', '/')
    return any(fnmatch.fnmatchcase(path, re.sub(r'^\./', '', pat.replace('\\', '/'))) for pat in patterns)


def split_scope(lines, patterns):
    """(own changes, changes outside --files). Without --files everything counts as own; a secret file never does."""
    own = [l for l in lines if (not patterns or in_scope(l, patterns)) and not is_secret(l[3:])]
    return own, [l for l in lines if l not in own]


def kill_tree(proc):
    """Stop a worker and everything it started."""
    if proc.poll() is not None:
        return
    if IS_WIN:
        subprocess.run(['taskkill', '/PID', str(proc.pid), '/T', '/F'], capture_output=True)
    else:
        try:
            os.killpg(proc.pid, signal.SIGKILL)   # started with start_new_session=True
        except ProcessLookupError:
            pass
    try:
        proc.wait(timeout=10)
    except subprocess.TimeoutExpired:
        proc.kill()


def run_tool(args, *, stdin_bytes=None, env=None, timeout=None, watch=None, keep_stderr=True):
    """Run a worker CLI with stdout (+stderr unless keep_stderr=False) in a temp file (no pipe deadlocks).
    watch() is called every 3 s and returns True to stop early. Returns (output text, 'timeout' | 'stopped' | None)."""
    exe = shutil.which(args[0])
    if not exe:
        raise Fail(f'{args[0]} not found on PATH (installed just now? restart the terminal/app).', 3)
    text, why, _ = _run([exe] + args[1:], stdin_bytes=stdin_bytes, env=env, timeout=timeout, watch=watch, keep_stderr=keep_stderr)
    return text, why


def run_check(cmd, timeout):
    """Run the project's check command (a shell line from PROJECT.md) in the project root."""
    text, why, rc = _run(cmd, shell=True, timeout=timeout)
    tail = '\n'.join(text.strip().splitlines()[-CHECK_TAIL_LINES:])[-CHECK_TAIL_CHARS:]
    return {'cmd': cmd, 'passed': rc == 0 and not why, 'exit': rc, 'timed_out': why == 'timeout', 'output': tail, 'fix_rounds': 0}


def _run(cmd, *, shell=False, stdin_bytes=None, env=None, timeout=None, watch=None, keep_stderr=True):
    """Returns (output text, 'timeout' | 'stopped' | None, exit code)."""
    out_f = tempfile.TemporaryFile()
    in_f = None
    if stdin_bytes is not None:
        in_f = tempfile.TemporaryFile()
        in_f.write(stdin_bytes)
        in_f.seek(0)
    kw = {} if IS_WIN else {'start_new_session': True}
    proc = subprocess.Popen(cmd, shell=shell, stdin=in_f or subprocess.DEVNULL, stdout=out_f,
                            stderr=subprocess.STDOUT if keep_stderr else subprocess.DEVNULL, env=env, cwd=str(ROOT), **kw)
    start = time.monotonic()
    why = None
    while True:
        try:
            proc.wait(timeout=3)
            break
        except subprocess.TimeoutExpired:
            pass
        if timeout and time.monotonic() - start >= timeout:
            why = 'timeout'
        elif watch and watch():
            why = 'stopped'
        if why:
            kill_tree(proc)
            break
    out_f.seek(0)
    text = out_f.read().decode('utf-8', errors='replace')
    out_f.close()
    if in_f:
        in_f.close()
    return text, why, proc.returncode


def diff_stat(root, lines):
    """' M a.py' -> ' M a.py (+3 -1)': how big each change is, so the Lead knows what to read."""
    out = []
    for line in lines:
        xy, path, size = line[:2], line[3:], ''
        try:
            if xy == '??':
                size = f" (+{len((Path(root) / path).read_text(encoding='utf-8', errors='replace').splitlines())})"
            elif xy != '--':
                r = subprocess.run(['git', '-C', str(root), 'diff', '--numstat', 'HEAD', '--', path],
                                   capture_output=True, text=True, encoding='utf-8', errors='replace')
                f = r.stdout.split()
                if len(f) >= 2:
                    size = f' (+{f[0]} -{f[1]})'
        except OSError:
            pass
        out.append(line + size)
    return out


# --- agy -----------------------------------------------------------------------------------------
def parse_agy_stream(stream, model):
    result = None
    for line in stream.splitlines():
        if line.startswith('{"event":"result"'):
            try:
                result = json.loads(line).get('result')
            except ValueError:
                pass
    if result:
        err = str(result.get('error') or '')
    else:
        err = ' '.join(stream.splitlines()[-3:])
    r = result or {}
    u = r.get('usage') or {}
    o = {
        'worker': f'agy {model}', 'conversation_id': r.get('conversation_id'), 'response': r.get('response'), 'error': err,
        # agy doesn't report model calls (num_turns counts user turns), so calls = '?'
        'calls': '?', 'input_tokens': u.get('input_tokens'), 'cache_read_tokens': u.get('cache_read_tokens'),
        'output_tokens': u.get('output_tokens'), 'thinking_tokens': u.get('thinking_tokens'),
        'denied': [d.get('display_name') for d in (r.get('denied_actions') or []) if d],
    }
    if str(r.get('finished_reason')) == 'print_timeout':
        o['code'] = 3
    elif not result or (str(r.get('status')) == 'ERROR' and not r.get('response')):
        o['code'] = 6
    elif o['denied']:
        o['code'] = 4
    elif not r.get('response'):
        o['code'] = 5
    else:
        o['code'] = 0
    return o


def agy_settings(secrets):
    """Session-only agy settings: deny reading and writing each secret file (agy rules take exact paths, no globs)."""
    return {'permissions': {'deny': [f'{action}({p})' for p in secrets for action in ('read_file', 'write_file')]}}


def invoke_agy(w, ctx):
    a = ctx.args
    prompt = ctx.task
    if ctx.base in ('builder', 'tester'):
        prompt += '\n\nREMINDER: run no commands. End with the report.'
    cmd = ['agy', '--input-format', 'stream-json', '--output-format', 'stream-json', '--model', w['m'],
           '--print-timeout', f'{a.timeout_sec}s', '--mode', 'accept-edits']
    if not a.no_agent:
        cmd += ['--agent', f'w-{ctx.base}']
    if not a.no_sandbox:
        cmd += ['--sandbox']
    if a.conversation:
        cmd += ['--conversation', a.conversation]
    if a.schema:
        cmd += ['--json-schema', str(Path(a.schema).resolve())]
    settings = None
    if ctx.secrets:
        settings = Path(tempfile.gettempdir()) / f'agy-settings-{uuid.uuid4().hex[:8]}.json'
        settings.write_text(json.dumps(agy_settings(ctx.secrets)), encoding='utf-8')
        cmd += ['--settings', str(settings)]
    # Prompt via stdin: one NDJSON message (no argv length limit, no quote escaping).
    line = json.dumps({'event': 'user', 'message': {'content': prompt}}) + '\n'
    try:
        # agy stops itself at --print-timeout; the extra minute only catches a hung process.
        stream, why = run_tool(cmd, stdin_bytes=line.encode('utf-8'), timeout=a.timeout_sec + 60)
    finally:
        if settings:
            settings.unlink(missing_ok=True)
    o = parse_agy_stream(stream, w['m'])
    if why == 'timeout':
        o['code'] = 3
    return o


# --- Codex ---------------------------------------------------------------------------------------
def toml_inline(v):
    """dict/str/int -> a TOML inline value (JSON string escapes are valid TOML)."""
    if isinstance(v, dict):
        return '{' + ', '.join(f'{json.dumps(k, ensure_ascii=False)}={toml_inline(x)}' for k, x in v.items()) + '}'
    return json.dumps(v, ensure_ascii=False)


def codex_profile(base, secrets):
    """The worker's Codex permission profile: workspace (reviewer: read-only), secrets denied by pattern and by path."""
    roots = {'.': 'read' if base == 'reviewer' else 'write'}
    roots.update({g: 'deny' for g in SECRET_GLOBS})
    fs = {'glob_scan_max_depth': 8, ':workspace_roots': roots}
    fs.update({p: 'deny' for p in secrets})
    return {'extends': ':read-only' if base == 'reviewer' else ':workspace', 'filesystem': fs}


def codex_cmd(w, ctx):
    """codex exec (or exec resume) with no shell, no web, no subagents, no user config (plugins, MCP, hooks), no approvals.
    The prompt goes in on stdin ('-')."""
    a = ctx.args
    cmd = ['codex', 'exec'] + (['resume'] if a.conversation else [])
    cmd += ['--json', '--ignore-user-config', '--ignore-rules', '-m', w['m']]
    for feature in ('shell_tool', 'multi_agent', 'apps', 'plugins', 'hooks', 'image_generation', 'goals', 'memories'):
        cmd += ['--disable', feature]
    cmd += ['-c', 'web_search="disabled"', '-c', 'approval_policy="never"',
            '-c', 'default_permissions="worker"', '-c', 'permissions.worker=' + toml_inline(codex_profile(ctx.base, ctx.secrets))]
    if w.get('e'):
        cmd += ['-c', f'model_reasoning_effort="{w["e"]}"']
    if a.schema:
        cmd += ['--output-schema', str(Path(a.schema).resolve())]
    if a.conversation:
        cmd += [a.conversation]
    return cmd + ['-']


def parse_codex_events(text, model, effort=''):
    """codex exec --json: thread.started, item.completed (agent_message, command_execution, file_change), turn.completed
    (usage), turn.failed / error."""
    ev = []
    for line in text.splitlines():
        try:
            e = json.loads(line)
        except ValueError:
            continue
        if isinstance(e, dict):
            ev.append(e)
    thread = next((e.get('thread_id') for e in ev if e.get('type') == 'thread.started'), None)
    items = {}
    for e in ev:
        it = e.get('item') or {}
        if e.get('type', '').startswith('item.') and it.get('id'):
            items[it['id']] = it
    msgs = [it.get('text') for it in items.values() if it.get('type') == 'agent_message' and it.get('text')]
    ran = [str(it.get('command') or '?') for it in items.values() if it.get('type') == 'command_execution']
    usage = {}
    for e in ev:
        if e.get('type') == 'turn.completed':
            for k, v in (e.get('usage') or {}).items():
                if isinstance(v, int):
                    usage[k] = usage.get(k, 0) + v
    errs = [str((e.get('error') or {}).get('message') or e.get('message') or '') for e in ev if e.get('type') in ('turn.failed', 'error')]
    err = '; '.join(x for x in errs if x)
    if not ev:
        err = ' '.join(text.strip().splitlines()[-3:])
    o = {'worker': f'codex {model}' + (f' ({effort})' if effort else ''), 'conversation_id': thread,
         'response': msgs[-1] if msgs else None, 'error': err, 'calls': '?',
         'input_tokens': usage.get('input_tokens'), 'cache_read_tokens': usage.get('cached_input_tokens'),
         'output_tokens': usage.get('output_tokens'), 'thinking_tokens': usage.get('reasoning_output_tokens'),
         'denied': [f'RAN A COMMAND (the shell should be off): {c}' for c in ran]}
    if not ev or (errs and not any(e.get('type') == 'turn.completed' for e in ev)):
        o['code'] = 6
    elif o['denied']:
        o['code'] = 4
    elif not msgs:
        o['code'] = 5
    else:
        o['code'] = 0
    return o


def invoke_codex(w, ctx):
    a = ctx.args
    prompt = ctx.task
    if ctx.base in ('builder', 'tester'):
        prompt += '\n\nREMINDER: you have no shell; edit files with your file tools only. End with the report.'
    text, why = run_tool(codex_cmd(w, ctx), stdin_bytes=prompt.encode('utf-8'), timeout=a.timeout_sec)
    o = parse_codex_events(text, w['m'], w.get('e'))
    if why == 'timeout':
        o['code'] = 3
    return o


INVOKE = {'agy': invoke_agy, 'codex': invoke_codex}


# --- Main ------------------------------------------------------------------------------------------
class Ctx:
    pass


def parse_args(argv):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--task', help='the task text')
    ap.add_argument('--task-file', help='file with the task')
    ap.add_argument('--role', default='builder', help='any agent/roles/<name>.md (default builder)')
    ap.add_argument('--include', nargs='+', default=[], metavar='FILE',
                    help='project files appended to the prompt (saves exploring calls); a reviewer gets line numbers')
    ap.add_argument('--model', choices=AGY_MODELS, help='force one agy worker (no chain, no fallback)')
    ap.add_argument('--worker', metavar='SPEC', help='force one worker: agy:<model> or codex:<model>[:<effort>]')
    ap.add_argument('--after', metavar='RUN',
                    help='tester/reviewer: the builder run this checks (its id from "saved:", or its run file), or "lead"')
    ap.add_argument('--conversation', help='follow-up on an earlier result (max 2)')
    ap.add_argument('--schema', help='JSON schema file for the final result')
    ap.add_argument('--files', nargs='+', default=[], metavar='FILE',
                    help='the files (or globs) this task may change; other changes are listed apart')
    ap.add_argument('--part', metavar='NAME', help='the part this task is about: fills {name} in check_one')
    ap.add_argument('--no-check', action='store_true', help='do not run check_one for this task')
    ap.add_argument('--fix-rounds', type=int, choices=(0, 1, 2), default=2,
                    help='how often a failing check goes back to the worker (default 2; round 2 only if round 1 changed the failure)')
    ap.add_argument('--check-timeout-sec', type=int, default=300)
    ap.add_argument('--timeout-sec', type=int, default=480)
    ap.add_argument('--no-sandbox', action='store_true', help='agy: no terminal sandbox (workers run no commands anyway)')
    ap.add_argument('--no-agent', action='store_true', help='agy: full default tool set (debugging only)')
    ap.add_argument('--json', action='store_true', help='print the full JSON instead of the summary')
    return ap.parse_args(argv)


def builder_family(after):
    """--after <run id | run file | lead> -> the family of the model that made the work ('' for the Lead)."""
    if after.strip().lower() == 'lead':
        return ''
    m = re.fullmatch(r'(?:.*[\\/])?(?:run-)?([0-9a-f]{8})(?:\.json)?', after.strip())
    f = ROOT / '.worker' / f'run-{m.group(1)}.json' if m else None
    if not f or not f.exists():
        raise Fail(f"--after '{after}': no such run in .worker/ (give the id from the builder's 'saved:' line, or 'lead').", 2)
    run = json.loads(read_text(f))
    return run.get('family') or family(str(run.get('model') or ''))


def main(argv=None):
    for s in (sys.stdout, sys.stderr):
        try:
            s.reconfigure(encoding='utf-8', errors='replace')
        except AttributeError:
            pass
    a = parse_args(argv)
    os.chdir(ROOT)
    (ROOT / '.worker').mkdir(exist_ok=True)   # git-ignored, so missing in a fresh clone

    proj, notes, private = {'check_one': '', 'check_all': '', 'check_dir': '', 'private': ''}, '', True
    if (ROOT / 'PROJECT.md').exists():
        proj, notes, private = parse_project(read_text(ROOT / 'PROJECT.md'))
    if a.part and not re.fullmatch(r'[\w./\\-]+', a.part):   # it becomes part of a shell line
        raise Fail(f"--part '{a.part}': only letters, digits and _ . / - are allowed.", 2)

    role_file = AGENT_DIR / 'roles' / f'{a.role}.md'
    if not role_file.exists():
        have = ', '.join(sorted(p.stem for p in (AGENT_DIR / 'roles').glob('*.md')))
        raise Fail(f"No role '{a.role}'. Roles: {have} (agent/roles/). New role: copy templates/roles/ROLE-TEMPLATE.md.", 2)
    fm, role_body = parse_role(read_text(role_file))
    base = fm.get('base') or 'builder'
    if base not in BASES:
        raise Fail(f"Role '{a.role}': base must be builder, tester, reviewer or scribe (got '{base}').", 2)
    model_list = fm.get('models') or DEFAULT_MODELS[base]
    a.include = [project_path(f, '--include') for f in a.include]
    for f in a.include:   # --include pastes text; an image or other binary file is named in the task instead
        try:
            with open(ROOT / f, 'rb') as fh:
                if b'\0' in fh.read(8192):
                    raise Fail(f"--include '{f}': not a text file. Name it in the task; the worker opens it with its own tools.", 2)
        except OSError:
            raise Fail(f"--include '{f}': no such file.", 2)
    a.files = [project_path(f, '--files') for f in a.files]

    # Independence: a tester/reviewer never runs on the model family of the work it checks.
    avoid = ''
    if base in ('tester', 'reviewer') and not a.conversation:
        if not a.after:
            raise Fail(f"Role '{a.role}' checks someone's work: give --after <builder run id> (from its 'saved:' line), "
                       "or --after lead when you made it yourself.", 2)
        avoid = builder_family(a.after)

    available = parse_available(read_text(ROOT / 'SETUP.md')) if (ROOT / 'SETUP.md').exists() else None
    forced = [{'h': 'agy', 'm': a.model, 'e': ''}] if a.model else [to_worker(a.worker)] if a.worker else None
    if forced:   # forced: no availability filter, no fallback
        chain = forced
        if private and chain[0]['h'] not in PRIVATE_OK:
            raise Fail(f"This project is private (PROJECT.md): only {', '.join(PRIVATE_OK)} workers may run.", 6)
        if avoid and family(chain[0]['m']) == avoid:
            raise Fail(f"{worker_name(chain[0])} is the same model family ({avoid}) as the work it must check. Pick another.", 2)
    else:
        chain = [w for w in parse_chain(model_list) if is_available(w, available) and not (private and w['h'] not in PRIVATE_OK)]
        if not chain:
            raise Fail(f"Role '{a.role}': none of its workers ({model_list}) is available on this machine (SETUP.md). Add one, or do it yourself.", 6)
        chain = [w for w in chain if not avoid or family(w['m']) != avoid]
        if not chain:
            raise Fail(f"Role '{a.role}': every available worker is the {avoid} family, like the work it must check. "
                       f"Force another with --worker / --model, or do it yourself.", 6)

    task = read_text(a.task_file) if a.task_file else a.task
    if not task:
        raise Fail('Give --task "..." or --task-file <path>.', 2)
    rules = read_text(AGENT_DIR / 'bases' / f'{base}.md')

    # The check is run here, never by a worker. Reviewer: before, so it can read the result.
    check = None
    check_cmd, why_not = (None, '') if a.no_check or base == 'scribe' else check_command(proj['check_one'], a.part)
    if why_not and proj['check_one'].strip():
        check = {'skipped': why_not}
    if check_cmd and base == 'reviewer' and not a.conversation:
        check = run_check(check_cmd, a.check_timeout_sec)
    task = build_prompt(task, base, a.role, role_body, rules, proj, notes, a.include, a.conversation, check)

    ctx = Ctx()
    ctx.args, ctx.task, ctx.base, ctx.proj = a, task, base, proj
    ctx.secrets = secret_files(ROOT)   # denied to the worker by exact path

    # Run the chain: busy/quota error -> retry once if the wait is short -> next worker.
    before = git_snapshot(ROOT)
    start = time.monotonic()
    tried, res, changed, w = [], None, False, None
    for w in chain:
        for attempt in (1, 2):
            res = INVOKE[w['h']](w, ctx)
            tried.append(f"{res['worker']}={res['code']}")
            # With --files, another worker's changes elsewhere don't count as this worker's.
            changed = bool(split_scope(changed_since(before, git_snapshot(ROOT)), a.files)[0])
            transient = res['code'] == 6 and not changed and TRANSIENT.search(str(res.get('error') or ''))
            wait = retry_wait(res.get('error')) if transient else None
            if not transient or attempt == 2 or wait is None:
                break
            time.sleep(wait)
        if res['code'] != 6 or changed:
            break

    # Builder/tester: run the check now; while it fails the same worker gets the output (--fix-rounds times).
    if check_cmd and base in ('builder', 'tester') and res['code'] in (0, 3):
        check, rounds = run_check(check_cmd, a.check_timeout_sec), 0
        last_key = None
        while not check['passed'] and res['code'] == 0 and res.get('conversation_id') and rounds < a.fix_rounds:
            if rounds and failure_key(check) == last_key:   # a 2nd round only when the 1st one changed the failure
                res['error'] = f'no fix round {rounds + 1}: the check failed the same way after round {rounds}.'
                break
            last_key = failure_key(check)
            rounds += 1
            a.conversation, ctx.task = res['conversation_id'], fix_prompt(base, check)
            fix = INVOKE[w['h']](w, ctx)
            tried.append(f"{fix['worker']}=fix:{fix['code']}")
            for k in ('input_tokens', 'cache_read_tokens', 'output_tokens', 'thinking_tokens'):
                if isinstance(res.get(k), int) and isinstance(fix.get(k), int):
                    res[k] += fix[k]
            res['denied'] = (res.get('denied') or []) + (fix.get('denied') or [])
            if fix.get('response'):
                res['response'] = fix['response']
            if fix.get('conversation_id'):
                res['conversation_id'] = fix['conversation_id']
            check = run_check(check_cmd, a.check_timeout_sec)
            check['fix_rounds'] = rounds
            if fix['code'] != 0:
                res['error'] = f"fix round ended with {LABELS.get(fix['code'], fix['code'])}: {fix.get('error') or ''}".strip()
                break
        if res['code'] == 0 and not check['passed']:
            res['code'] = 7
    res['check'] = check
    own, outside = split_scope(changed_since(before, git_snapshot(ROOT)), a.files)
    run_file = f'.worker/run-{uuid.uuid4().hex[:8]}.json'   # one per run: parallel workers don't overwrite each other
    res.update({'role': a.role, 'seconds': int(time.monotonic() - start), 'tried': tried, 'run_file': run_file,
                'model': w['m'], 'family': family(w['m']), 'after': a.after,
                'files_changed': [c.strip() for c in diff_stat(ROOT, own)],
                'files_outside': [c.strip() for c in diff_stat(ROOT, outside)]})
    for k in ('conversation_id', 'response', 'error', 'input_tokens', 'cache_read_tokens', 'output_tokens', 'thinking_tokens'):
        res.setdefault(k, None)

    json_out = json.dumps(res, indent=2, ensure_ascii=False)
    for f in (run_file, '.worker/last.json'):
        (ROOT / f).write_text(json_out, encoding='utf-8')
    if a.json:
        print(json_out)
    else:
        none = lambda v: '' if v is None else v
        print(f"[{LABELS.get(res['code'], res['code'])}] {a.role} via {res['worker']} | {res['seconds']}s | "
              f"in {none(res['input_tokens'])} (+cache {none(res['cache_read_tokens'])}) out {none(res['output_tokens'])}")
        if len(tried) > 1:
            print('tried: ' + ', '.join(tried))
        print('files: ' + ('; '.join(res['files_changed']) or 'none'))
        if res['files_outside']:
            print('NOT IN --files (another worker, this one broke its scope, or a secret file): ' + '; '.join(res['files_outside']))
        if check:
            n = check.get('fix_rounds')
            print('check: ' + check_text(check) + (f" | after {n} fix round{'s' if n > 1 else ''}" if n else ''))
            if check.get('passed') is False:
                print(check['output'])
        if res['denied']:
            print('denied: ' + '; '.join(res['denied']))
        if res['error']:
            print(f"error: {res['error']}")
        print(f"conversation: {none(res['conversation_id'])} | saved: {run_file} (run id {run_file[12:20]})")
        print('--- report ---')
        print(str(none(res['response'])).strip())
    return int(res['code'])


if __name__ == '__main__':
    try:
        sys.exit(main())
    except Fail as e:
        print(f'ERROR: {e}', file=sys.stderr)
        sys.exit(e.code)
