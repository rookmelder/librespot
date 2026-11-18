#!/usr/bin/env python3
"""Rate your own worker models: the same tasks on each worker, scored with material the worker never sees.

  python agent/bench.py --workers agy:gemini-3.8-flash-low codex:gpt-6-luna:low
  python agent/bench.py --workers codex:gpt-6.1-sol:medium --tasks sk_refund sk_review

A worker is written like an entry in a role's models: line (agy:<model> or codex:<model>[:<effort>]).
Each run happens in a throw-away copy of this setup, through delegate.py with the task's role, exactly as the Lead
would hand it out (so the check and the fix rounds count too). Then it is scored by the task's kind:
- build  (builder / builderplus): hidden tests for the change that pass; for a project task also the project's
  existing tests that must stay green ("kept").
- tester: planted bugs (hidden/mutants/) its tests catch. Tests that fail on the real code score 0.
- review (reviewer roles): planted bugs found (file:line within a few lines) and false alarms.
- debug  (debugger): 1 point for naming the real cause first (file:line), 1 for saying why (a keyword).

Task folder agent/bench/<name>/: task.md (written as the Lead writes tasks), task.json, start/ (files laid over
the project: the starting state), hidden/ (tests, mutants or answers.json), solution/ (a correct result, used by
the unit tests to check the task itself). task.json: role, kind, level, skills, part, files, include, tests
(the maximum score), and optional project (a folder in agent/bench/_projects/), role_file (a pack role to copy
in), remove (files deleted from the start), diff (true: the change start/ makes is appended to the task as a
diff, as the Lead pastes a PR), module (tester), timeout.
Levels: easy (textkit, inventory), medium (ledger, duration, cfg), real (sk_*: the shopkeeper project).
Output: a table to paste into agent/MODELS.md; every row (with what it missed) is appended to .worker/bench.jsonl.
The tasks are made-up code and go to the services you name. Standard library only.
"""
import argparse
import datetime
import difflib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

AGENT_DIR = Path(__file__).resolve().parent
ROOT = AGENT_DIR.parent
BENCH_DIR = AGENT_DIR / 'bench'
PROJECTS = BENCH_DIR / '_projects'
sys.path.insert(0, str(AGENT_DIR))
import delegate  # noqa: E402

REF = re.compile(r'([\w./\\-]+\.py)(?::|,? line )(\d+)')   # file.py:12 or file.py line 12


def load_tasks(names=None):
    tasks = []
    for d in sorted(p for p in BENCH_DIR.iterdir() if (p / 'task.json').exists()):
        if names and d.name not in names:
            continue
        t = json.loads((d / 'task.json').read_text(encoding='utf-8'))
        t.setdefault('role', 'builder')
        t.setdefault('kind', 'tester' if t['role'] == 'tester' else 'build')
        t.update({'name': d.name, 'dir': d})
        tasks.append(t)
    return tasks


def worker_flags(spec):
    """'agy:m' | 'codex:m[:effort]' -> --worker <spec> (checked here, so a typo fails before any run)."""
    delegate.to_worker(spec)
    return ['--worker', spec.strip()]


def lay_out(task, dest, parts=('start',)):
    """The task's files in dest: its project (if any), then each overlay folder in parts, then removals."""
    dest = Path(dest)
    if task.get('project'):
        shutil.copytree(PROJECTS / task['project'], dest, dirs_exist_ok=True)
    for part in parts:
        if (task['dir'] / part).exists():
            shutil.copytree(task['dir'] / part, dest, dirs_exist_ok=True)
    for f in task.get('remove', []):
        (dest / f).unlink(missing_ok=True)


def prepare(task, tmp):
    """A throw-away project: this setup's scripts and rules, the task's files, a check command, its own git."""
    tmp = Path(tmp)
    shutil.copytree(AGENT_DIR, tmp / 'agent', ignore=shutil.ignore_patterns('tests', 'bench', '__pycache__', 'bench.py'))
    if (ROOT / '.agents').exists():
        shutil.copytree(ROOT / '.agents', tmp / '.agents')
    if task.get('role_file'):
        shutil.copy(ROOT / task['role_file'], tmp / 'agent' / 'roles' / Path(task['role_file']).name)
    lay_out(task, tmp)
    (tmp / 'tests').mkdir(exist_ok=True)
    py = f'"{sys.executable}"' if ' ' in sys.executable else sys.executable
    (tmp / 'PROJECT.md').write_text(
        f'# Benchmark project\n```settings\ncheck_one: {py} -m unittest tests.test_{{name}} -v\ncheck_all:\ncheck_dir:\nprivate: no\n```\n'
        '## Worker notes\n- Python 3, standard library only. Tests use unittest.\n', encoding='utf-8')
    (tmp / '.gitignore').write_text('.worker/\n__pycache__/\n', encoding='utf-8')
    for cmd in (['init', '-q'], ['add', '-A'], ['-c', 'user.name=bench', '-c', 'user.email=bench@example.invalid', 'commit', '-qm', 'start']):
        subprocess.run(['git', '-C', str(tmp)] + cmd, capture_output=True)
    (tmp / '.worker').mkdir(exist_ok=True)
    task_file = tmp / '.worker' / 'bench-task.md'
    task_file.write_text(task_text(task), encoding='utf-8')
    return task_file


def task_text(task):
    """task.md, plus (diff: true) the change the start folder makes to the project, as a unified diff."""
    text = (task['dir'] / 'task.md').read_text(encoding='utf-8')
    if task.get('diff'):
        text = text.rstrip('\n') + '\n\n## The diff\n```diff\n' + make_diff(task) + '```\n'
    return text


def make_diff(task):
    base, start = PROJECTS / task['project'], task['dir'] / 'start'
    out = []
    for f in sorted(p for p in start.rglob('*') if p.is_file()):
        rel = f.relative_to(start).as_posix()
        old = (base / rel).read_text(encoding='utf-8').splitlines(keepends=True) if (base / rel).exists() else []
        out += difflib.unified_diff(old, f.read_text(encoding='utf-8').splitlines(keepends=True),
                                    'a/' + rel if old else '/dev/null', 'b/' + rel)
    return ''.join(line if line.endswith('\n') else line + '\n' for line in out)


def _unittest(tmp, args):
    """Run unittest in tmp without stale bytecode (a mutant can have the same size and second as the real file)."""
    for cache in Path(tmp).rglob('__pycache__'):
        shutil.rmtree(cache, ignore_errors=True)
    env = dict(os.environ, PYTHONDONTWRITEBYTECODE='1')
    try:
        return subprocess.run([sys.executable, '-B', '-m', 'unittest'] + args, cwd=str(tmp), capture_output=True, text=True,
                              encoding='utf-8', errors='replace', timeout=120, env=env)
    except subprocess.TimeoutExpired:
        return None


def _run_dir(tmp, folder):
    """-> (names passed, names failed, last error line) for unittest discover in tmp/folder."""
    r = _unittest(tmp, ['discover', '-s', folder, '-v'])
    if r is None:
        return [], ['timeout'], 'timeout'
    passed = re.findall(r'^(test\w*) \(.*\) \.\.\. ok$', r.stderr, re.M)
    failed = re.findall(r'^(test\w*) \(.*\) \.\.\. (?:FAIL|ERROR)$', r.stderr, re.M)
    return passed, failed, (r.stderr.strip().splitlines() or ['?'])[-1]


def score(tmp, task, response=''):
    """-> (score, [what it missed], note). The score's maximum is task['tests']."""
    kind = task['kind']
    if kind == 'tester':
        return score_tester(tmp, task)
    if kind == 'review':
        return score_review(tmp, task, response)
    if kind == 'debug':
        return score_debug(tmp, task, response)
    return score_build(tmp, task)


def score_build(tmp, task):
    tmp = Path(tmp)
    shutil.copytree(task['dir'] / 'hidden', tmp / 'hidden_tests', dirs_exist_ok=True)
    passed, failed, last = _run_dir(tmp, 'hidden_tests')
    missed = failed or ([] if passed else ['did not run: ' + last])
    note = ''
    if task.get('project'):
        base_tests = PROJECTS / task['project'] / 'tests'
        shutil.copytree(base_tests, tmp / 'kept_tests', dirs_exist_ok=True)   # the originals, not the worker's copies
        total = sum(len(re.findall(r'^    def test\w*', f.read_text(encoding='utf-8'), re.M)) for f in base_tests.glob('test_*.py'))
        kept, broke, _ = _run_dir(tmp, 'kept_tests')
        note = f'kept {len(kept)}/{total}'
        missed += [f'kept:{n}' for n in broke] or ([f'kept: {total - len(kept)} did not run'] if len(kept) < total else [])
    return len(passed), missed, note


def score_tester(tmp, task):
    tmp = Path(tmp)
    module = tmp / task['module']
    if not (tmp / 'tests' / f"test_{task['part']}.py").exists():
        return 0, ['no test file'], ''
    args = [f"tests.test_{task['part']}"]
    r = _unittest(tmp, args)
    if r is None or r.returncode != 0:
        return 0, ['its tests fail on the real code'], ''
    real = module.read_bytes()
    caught, missed = 0, []
    try:
        for mutant in sorted((task['dir'] / 'hidden' / 'mutants').glob('*.py')):
            module.write_bytes(mutant.read_bytes())
            r = _unittest(tmp, args)
            if r is None or r.returncode != 0:
                caught += 1
            else:
                missed.append(mutant.stem)
    finally:
        module.write_bytes(real)
    return caught, missed, ''


def answers(tmp, task):
    """hidden/answers.json with each spot's line number looked up in the worker's project (the 'find' text)."""
    data = json.loads((task['dir'] / 'hidden' / 'answers.json').read_text(encoding='utf-8'))
    for spot in data.get('bugs', []) + data.get('decoys', []):
        lines = (Path(tmp) / spot['file']).read_text(encoding='utf-8').splitlines()
        hits = [i for i, line in enumerate(lines, 1) if spot['find'] in line]
        if len(hits) != 1:
            raise ValueError(f"{task['name']}: '{spot['find']}' found {len(hits)} times in {spot['file']}")
        spot['line'] = hits[0]
    return data


def refs(text):
    """Every file.py:line in a report -> [(path with / slashes, line)], in order."""
    return [(f.replace('\\', '/').lstrip('./'), int(n)) for f, n in REF.findall(text or '')]


def same_file(ref_file, path):
    return ref_file == path or path.endswith('/' + ref_file) or ref_file.endswith('/' + path)


def near(ref, spot, tol):
    return same_file(ref[0], spot['file']) and abs(ref[1] - spot['line']) <= tol


def score_review(tmp, task, response):
    """Found = planted bugs with a reference within 'tol' lines (default 3). False alarm = a finding (references
    within 5 lines of each other count as one) with no planted bug within 8 lines. Only shop code counts."""
    data = answers(tmp, task)
    text = response or ''
    if 'Problems:' in text:
        text = text[text.rindex('Problems:'):]
    found = [r for r in refs(text) if not r[0].split('/')[-1].startswith('test_')]
    hit = [b['id'] for b in data['bugs'] if any(near(r, b, b.get('tol', 3)) for r in found)]
    clusters = []
    for r in sorted(set(found)):
        if clusters and same_file(r[0], clusters[-1][-1][0]) and r[1] - clusters[-1][-1][1] <= 5:
            clusters[-1].append(r)
        else:
            clusters.append([r])
    false = [c for c in clusters if not any(near(r, b, 8) for r in c for b in data['bugs'])]
    decoyed = [d['id'] for d in data.get('decoys', []) if any(near(r, d, 3) for c in false for r in c)]
    missed = [b['id'] for b in data['bugs'] if b['id'] not in hit] + [f'false alarm at decoy {d}' for d in decoyed]
    return len(hit), missed, f'{len(false)} false'


def score_debug(tmp, task, response):
    """2 points: the first shop-code reference in the report is the planted cause (within 'tol' lines), and the
    report says why (one of the answer's keywords). Blaming a decoy first scores 0."""
    data = answers(tmp, task)
    cause = data['bugs'][0]
    first = next((r for r in refs(response) if not r[0].split('/')[-1].startswith('test_')), None)
    if first is None:
        return 0, ['no file:line given'], ''
    if not near(first, cause, cause.get('tol', 3)):
        decoy = next((d['id'] for d in data.get('decoys', []) if near(first, d, 3)), None)
        return 0, [f"blamed {first[0]}:{first[1]}" + (f' (decoy {decoy})' if decoy else '')], ''
    why = any(k.lower() in (response or '').lower() for k in data['keywords'])
    return 1 + why, [] if why else ['right place, cause not explained'], ''


def run_one(spec, task, timeout_sec=None):
    tmp = tempfile.mkdtemp(prefix='bench-')
    timeout_sec = timeout_sec or task.get('timeout', 900 if task.get('project') else 480)
    row = {'date': datetime.date.today().isoformat(), 'worker': spec, 'task': task['name'], 'role': task['role'],
           'kind': task['kind'], 'level': task.get('level', ''), 'passed': 0, 'tests': task['tests']}
    try:
        task_file = prepare(task, tmp)
        cmd = [sys.executable, str(Path(tmp) / 'agent' / 'delegate.py'), '--role', Path(task.get('role_file') or task['role']).stem,
               '--json', '--timeout-sec', str(timeout_sec), '--task-file', str(task_file), '--part', task['part']] + worker_flags(spec)
        if task.get('files'):
            cmd += ['--files', *task['files']]
        if task['kind'] in ('tester', 'review', 'debug'):
            cmd += ['--after', 'lead']   # the work under test is the Lead's (no builder family to avoid)
        if task.get('include'):
            cmd += ['--include', *task['include']]
        r = subprocess.run(cmd, capture_output=True, text=True, encoding='utf-8', errors='replace', cwd=tmp)
        try:
            res = json.loads(r.stdout)
        except ValueError:
            res = {'code': r.returncode, 'error': (r.stderr or r.stdout).strip()[-300:]}
        check = res.get('check') or {}
        row.update({'exit': delegate.LABELS.get(res.get('code'), res.get('code')), 'model': res.get('worker'), 'seconds': res.get('seconds'),
                    'tokens_in': res.get('input_tokens'), 'tokens_out': res.get('output_tokens'),
                    'own_check': 'pass' if check.get('passed') else 'fail' if 'passed' in check else '-',
                    'fix_rounds': check.get('fix_rounds', 0), 'scope': len(res.get('files_outside') or []),
                    'error': res.get('error') or ''})
        row['passed'], row['missed'], row['note'] = score(tmp, task, res.get('response') or '')
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
    return row


def table(rows):
    cols = [('worker', 'Worker'), ('task', 'Task'), ('level', 'Level'), ('score', 'Score'), ('note', 'Kept / false'),
            ('scope', 'Outside --files'), ('exit', 'Result'), ('own_check', 'Own check'), ('fix_rounds', 'Fix rounds'),
            ('seconds', 'Seconds'), ('tokens_in', 'Tokens in'), ('tokens_out', 'Tokens out')]
    out = ['| ' + ' | '.join(h for _, h in cols) + ' |', '|' + '---|' * len(cols)]
    for r in rows:
        r = dict(r, score=f"{r['passed']}/{r['tests']}")
        out.append('| ' + ' | '.join('' if r.get(k) is None else str(r.get(k)) for k, _ in cols) + ' |')
    return '\n'.join(out)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--workers', nargs='+', required=True, metavar='WORKER', help='agy:<model> or codex:<model>[:<effort>]')
    ap.add_argument('--tasks', nargs='+', help='task folder names in agent/bench/ (default: all)')
    ap.add_argument('--timeout-sec', type=int, help='per run (default: the task\'s, else 900 for project tasks, 480 for others)')
    a = ap.parse_args(argv)
    tasks = load_tasks(a.tasks)
    if not tasks:
        print('ERROR: no such task in agent/bench/.', file=sys.stderr)
        return 2
    try:
        for spec in a.workers:
            worker_flags(spec)
    except delegate.Fail as e:
        print(f'ERROR: {e}', file=sys.stderr)
        return 2
    (ROOT / '.worker').mkdir(exist_ok=True)
    rows = []
    for spec in a.workers:
        for task in tasks:
            print(f"running {spec} on {task['name']} ({task['role']}) ...", file=sys.stderr, flush=True)
            row = run_one(spec, task, a.timeout_sec)
            rows.append(row)
            with open(ROOT / '.worker' / 'bench.jsonl', 'a', encoding='utf-8') as f:
                f.write(json.dumps(row, ensure_ascii=False) + '\n')
            if row.get('error'):
                print(f"  {row['exit']}: {row['error'][:200]}", file=sys.stderr)
            if row.get('missed'):
                print(f"  missed: {', '.join(row['missed'])}", file=sys.stderr)
    print(table(rows))
    print()
    for spec in a.workers:
        mine = [r for r in rows if r['worker'] == spec]
        print(f"{spec}: {sum(r['passed'] for r in mine)}/{sum(r['tests'] for r in mine)}")
    return 0


if __name__ == '__main__':
    sys.exit(main())
