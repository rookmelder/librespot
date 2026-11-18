"""Tests for agent/delegate.py and agent/check_setup.py. Run: python -m unittest discover -s agent/tests"""
import json
import os
import sys
import tempfile
import time
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import check_setup  # noqa: E402
import delegate as d  # noqa: E402


class ParseFiles(unittest.TestCase):
    def test_project_settings_notes_private(self):
        text = ('**Private?** yes\n## Settings\n```settings\ncheck_one: `python -m unittest tests.test_{name}`\n'
                'check_all: python -m unittest\ncheck_dir:\n```\n## Worker notes (x)\n- Python 3.12\n- no deps\n\n## Other\nz')
        proj, notes, private = d.parse_project(text)
        self.assertEqual(proj['check_one'], 'python -m unittest tests.test_{name}')
        self.assertEqual(proj['check_dir'], '')
        self.assertEqual(notes, '- Python 3.12\n- no deps')
        self.assertTrue(private)

    def test_project_template_placeholders(self):
        proj, notes, private = d.parse_project(d.read_text(d.ROOT / 'PROJECT.md')) if (d.ROOT / 'PROJECT.md').exists() else ({}, '', False)
        self.assertTrue(private, 'the unfilled template must not allow cloud workers')
        self.assertNotIn('- <', notes)

    def test_private_fails_closed(self):
        block = lambda v: f'```settings\ncheck_one:\nprivate: {v}\n```\n'
        self.assertFalse(d.parse_project(block('no'))[2])
        self.assertFalse(d.parse_project(block('No'))[2])
        self.assertTrue(d.parse_project(block('yes'))[2])
        self.assertTrue(d.parse_project(block('nope'))[2])          # typo: stays private
        self.assertTrue(d.parse_project(block(''))[2])
        self.assertTrue(d.parse_project('no settings at all')[2])
        self.assertFalse(d.parse_project('- **Private?** no: free cloud workers allowed')[2])   # older PROJECT.md
        self.assertTrue(d.parse_project('**Private?** no\n' + block('yes'))[2])                  # settings win

    def test_models_json(self):
        for table in (d.DEFAULT_MODELS, d.FAMILIES):
            self.assertFalse([k for k in table if k.startswith('_')])
        self.assertEqual(set(d.DEFAULT_MODELS), set(d.BASES))
        self.assertTrue(set(d.PRIVATE_OK) <= set(d.HARNESSES))
        for chain in d.DEFAULT_MODELS.values():
            for w in d.parse_chain(chain):
                self.assertTrue(w['h'] != 'agy' or w['m'] in d.AGY_MODELS, w)

    def test_role(self):
        fm, body = d.parse_role('---\r\nbase: tester\r\nmodels: codex:m1, agy:m2\r\n---\r\nExtra rule.\r\n')
        self.assertEqual(fm, {'base': 'tester', 'models': 'codex:m1, agy:m2'})
        self.assertEqual(body, 'Extra rule.')
        self.assertEqual(d.parse_role('no frontmatter'), ({}, 'no frontmatter'))

    def test_every_role_file_parses(self):
        files = list((d.AGENT_DIR / 'roles').glob('*.md')) + list((d.ROOT / 'templates').rglob('roles/*.md'))
        self.assertGreater(len(files), 15)
        for f in files:
            fm, _ = d.parse_role(d.read_text(f))
            self.assertIn(fm.get('base', 'builder'), d.BASES, f.name)
            chain = d.parse_chain(fm.get('models') or d.DEFAULT_MODELS[fm.get('base', 'builder')])
            self.assertTrue(chain, f.name)
            self.assertTrue(all(w['h'] != 'agy' or w['m'] in d.AGY_MODELS for w in chain), f.name)
            if fm.get('base') in ('tester', 'reviewer'):   # it must be able to skip the builder's family
                self.assertGreater(len({d.family(w['m']) for w in chain}), 1, f.name)

    def test_chain(self):
        c = d.parse_chain('agy:gemini-3.8-flash-low, codex:gpt-5.6-terra, codex:gpt-5.6-luna:low,')
        self.assertEqual(c, [{'h': 'agy', 'm': 'gemini-3.8-flash-low', 'e': ''}, {'h': 'codex', 'm': 'gpt-5.6-terra', 'e': ''},
                             {'h': 'codex', 'm': 'gpt-5.6-luna', 'e': 'low'}])
        self.assertEqual(d.worker_name(c[2]), 'codex gpt-5.6-luna (low)')
        for bad in ('nvidia', 'openrouter:a/b:free', 'codex:', 'codex:m:turbo', 'agy:m:low'):
            with self.assertRaises(d.Fail, msg=bad) as e:
                d.to_worker(bad)
            self.assertEqual(e.exception.code, 2)

    def test_family(self):
        self.assertEqual(d.family('gemini-3.8-flash-low'), 'gemini')
        self.assertEqual(d.family('claude-sonnet-4-6'), 'claude')
        self.assertEqual(d.family('gpt-5.6-luna'), d.family('gpt-oss-120b-medium'))
        self.assertEqual(d.family('mystery-1'), 'mystery-1')

    def test_available(self):
        av = d.parse_available('x\n```available\nagy: yes\ncodex: no\n```\n')
        self.assertEqual(av, {'agy': True, 'codex': False})
        self.assertIsNone(d.parse_available('nothing'))
        self.assertTrue(d.is_available({'h': 'agy', 'm': 'x'}, av))
        self.assertFalse(d.is_available({'h': 'codex', 'm': 'x'}, av))
        self.assertFalse(d.is_available({'h': 'codex', 'm': 'x'}, {'agy': True}))
        self.assertTrue(d.is_available({'h': 'codex', 'm': 'x'}, None))


class Paths(unittest.TestCase):
    def test_secret_names(self):
        for s in ('.env', 'sub/.env.local', '.env.production', 'server.pem', 'a/b/tls.key', 'id_rsa', 'id_ed25519.pub',
                  '.git-credentials', 'credentials.json', 'C:\\x\\.npmrc'):
            self.assertTrue(d.is_secret(s), s)
        for s in ('.env.example', '.env.sample', 'env.py', 'keys.py', 'src/key_store.py', 'README.md', 'monkey.txt'):
            self.assertFalse(d.is_secret(s), s)

    def test_project_path(self):
        old = d.ROOT
        with tempfile.TemporaryDirectory() as t:
            d.ROOT = Path(t)
            try:
                self.assertEqual(d.project_path('src\\shop.py', '--files'), 'src/shop.py')
                self.assertEqual(d.project_path('./tests/test_*.py', '--files'), 'tests/test_*.py')
                self.assertEqual(d.project_path(str(Path(t, 'a', 'b.py')), '--include'), 'a/b.py')
                for bad in ('../other/x.py', 'src/../../x.py', str(Path(t).parent / 'x.py'), '.git/config', '.env', 'conf/.env.local'):
                    with self.assertRaises(d.Fail, msg=bad) as e:
                        d.project_path(bad, '--include')
                    self.assertEqual(e.exception.code, 4)
            finally:
                d.ROOT = old

    def test_secret_files_found_and_skipped_dirs(self):
        with tempfile.TemporaryDirectory() as t:
            for f in ('.env', 'app/.env.local', 'app/main.py', '.env.example', 'node_modules/x/.env', '.git/creds.pem'):
                Path(t, f).parent.mkdir(parents=True, exist_ok=True)
                Path(t, f).write_text('x', encoding='utf-8')
            found = [os.path.relpath(p, t).replace('\\', '/') for p in d.secret_files(t)]
            self.assertEqual(found, ['.env', 'app/.env.local'])

    def test_agy_settings_deny_each_secret(self):
        s = d.agy_settings(['C:/p/.env'])
        self.assertEqual(s, {'permissions': {'deny': ['read_file(C:/p/.env)', 'write_file(C:/p/.env)']}})


class Prompt(unittest.TestCase):
    def test_build_prompt_and_include(self):
        with tempfile.NamedTemporaryFile('w', suffix='.py', delete=False, encoding='utf-8') as f:
            f.write('a = 1\nb = 2\n')
        try:
            proj = {'check_one': 'python -m unittest', 'check_all': '', 'check_dir': ''}
            p = d.build_prompt('Do X.', 'builder', 'builder', 'Role rule.', 'BASE RULES\n', proj, '- note', [f.name], None)
            self.assertTrue(p.startswith('BASE RULES\n\n## Your role: builder\nRole rule.\n'))
            self.assertIn('`python -m unittest`', p)
            self.assertIn('- note\n\n---\n# YOUR TASK\n\nDo X.', p)
            self.assertIn('## File: ' + f.name + '\n```\na = 1\nb = 2\n```', p)   # a builder gets no line numbers
            self.assertTrue(p.endswith("only if the task needs them."))
            r = d.build_prompt('Do X.', 'reviewer', 'reviewer', '', 'R', proj, '', [f.name], None)
            self.assertIn('   1  a = 1\n   2  b = 2\n```', r)
        finally:
            os.unlink(f.name)

    def test_scribe_and_no_check_and_conversation(self):
        proj = {'check_one': '', 'check_all': '', 'check_dir': ''}
        self.assertIn('Run no commands.', d.build_prompt('T', 'scribe', 's', '', 'R', proj, '', [], None))
        self.assertIn('no check command', d.build_prompt('T', 'builder', 'b', '', 'R', proj, '', [], None))
        self.assertEqual(d.build_prompt('T', 'builder', 'b', '', 'R', proj, '', [], 'conv1'), 'T')

    def test_reviewer_gets_check_result(self):
        proj = {'check_one': 'x {name}', 'check_all': '', 'check_dir': ''}
        check = {'cmd': 'x shop', 'passed': False, 'exit': 1, 'timed_out': False, 'output': 'AssertionError: 3 != 4'}
        p = d.build_prompt('T', 'reviewer', 'r', '', 'R', proj, '', [], None, check)
        self.assertIn('FAIL (exit 1) | `x shop`', p)
        self.assertIn('AssertionError: 3 != 4', p)
        self.assertNotIn('`x shop`', d.build_prompt('T', 'reviewer', 'r', '', 'R', proj, '', [], None, {'skipped': 'why'}))


class Check(unittest.TestCase):
    def test_check_command(self):
        self.assertEqual(d.check_command('test {name} -v', 'shop'), ('test shop -v', ''))
        self.assertEqual(d.check_command('test all', None), ('test all', ''))
        self.assertIsNone(d.check_command('', 'shop')[0])
        cmd, why = d.check_command('test {name}', None)
        self.assertIsNone(cmd)
        self.assertIn('--part', why)

    def test_run_check(self):
        ok = d.run_check('python -c "print(1)"', 60)
        self.assertEqual((ok['passed'], ok['exit'], ok['output']), (True, 0, '1'))
        bad = d.run_check('python -c "import sys; [print(i) for i in range(200)]; sys.exit(3)"', 60)
        self.assertEqual((bad['passed'], bad['exit'], bad['timed_out']), (False, 3, False))
        self.assertEqual(bad['output'].splitlines()[0], str(200 - d.CHECK_TAIL_LINES))
        self.assertIn('FAIL (exit 3)', d.check_text(bad))
        slow = d.run_check('python -c "import time; time.sleep(30)"', 1)
        self.assertEqual((slow['passed'], slow['timed_out']), (False, True))

    def test_second_fix_round_only_on_progress(self):
        calls = []

        def fake(w, ctx):   # every call gets one step further; the check passes at step 3
            calls.append(1)
            Path(d.ROOT, 'n.txt').write_text(str(len(calls)), encoding='utf-8')
            return {'worker': 'agy fake', 'code': 0, 'conversation_id': 'c1', 'response': 'r', 'calls': '?', 'denied': []}

        check = 'python -c "import sys; n=open(\'{name}.txt\').read(); print(\'step\', n, \'in 0.01s\'); sys.exit(0 if n==\'3\' else 1)"'
        code, last, out = self.run_main(fake, ['--part', 'n'], check=check)
        self.assertEqual((code, len(calls), last['check']['fix_rounds']), (0, 3, 2))
        self.assertEqual(d.failure_key({'output': 'Ran 3 tests in 0.012s\nat 0x7f3a'}), d.failure_key({'output': 'Ran 3 tests in 1.5s\nat 0x1b'}))
        self.assertNotEqual(d.failure_key({'output': 'step 1'}), d.failure_key({'output': 'step 2'}))

    def test_retry_wait(self):
        self.assertEqual(d.retry_wait('429 Too Many Requests'), 20)
        self.assertEqual(d.retry_wait('429 retry-after 7 {"error"}'), 8)
        self.assertEqual(d.retry_wait('RESOURCE_EXHAUSTED "retryDelay": "17s"'), 18)
        self.assertEqual(d.retry_wait('rate limit: please try again in 2.5s'), 4)
        self.assertIsNone(d.retry_wait('429 retry-after 3600'))
        self.assertEqual(d.retry_wait(None), 20)

    def test_fix_prompt(self):
        check = {'cmd': 'c', 'passed': False, 'exit': 1, 'timed_out': False, 'output': 'boom'}
        self.assertIn('boom', d.fix_prompt('builder', check))
        self.assertIn('change nothing', d.fix_prompt('tester', check))

    def run_main(self, fake, extra=(), check=None, files=None, model=True):
        """main() in a temp project whose check passes once ok.txt exists; the worker is faked."""
        import contextlib
        import io
        old = (d.ROOT, dict(d.INVOKE), os.getcwd())
        with tempfile.TemporaryDirectory() as t:
            check = check or 'python -c "import os,sys; sys.exit(0 if os.path.exists(\'{name}.txt\') else 1)"'
            Path(t, 'PROJECT.md').write_text(f'```settings\ncheck_one: {check}\n```\n', encoding='utf-8')
            d.ROOT = Path(t)
            d.INVOKE.update(agy=fake, codex=fake)
            for name, text in (files or {}).items():
                Path(t, name).parent.mkdir(parents=True, exist_ok=True)
                Path(t, name).write_text(text, encoding='utf-8')
            out = io.StringIO()
            try:
                with contextlib.redirect_stdout(out):
                    code = d.main((['--model', 'gemini-3.8-flash-low'] if model else []) + ['--task', 'x', *extra])
                last = json.loads(Path(t, '.worker', 'last.json').read_text(encoding='utf-8'))
            finally:
                os.chdir(old[2])
                d.ROOT = old[0]
                d.INVOKE.clear()
                d.INVOKE.update(old[1])
        return code, last, out.getvalue()

    def test_failed_check_goes_back_to_the_worker_once(self):
        prompts = []

        def fake(w, ctx):
            prompts.append(ctx.task)
            if len(prompts) == 2:   # the fix round
                Path(d.ROOT, 'ok.txt').write_text('x', encoding='utf-8')
            return {'worker': 'agy fake', 'code': 0, 'conversation_id': 'c1', 'response': f'report {len(prompts)}',
                    'calls': '?', 'input_tokens': 10, 'output_tokens': 1, 'denied': []}

        code, last, out = self.run_main(fake, ['--part', 'ok'])
        self.assertEqual(code, 0)
        self.assertEqual(len(prompts), 2)
        self.assertIn('FAIL (exit 1)', prompts[1])
        self.assertEqual((last['check']['passed'], last['check']['fix_rounds'], last['input_tokens'], last['response']),
                         (True, 1, 20, 'report 2'))
        self.assertIn('check: PASS', out)
        self.assertIn('after 1 fix round', out)
        self.assertRegex(last['run_file'], r'^\.worker/run-[0-9a-f]{8}\.json$')
        self.assertIn('saved: ' + last['run_file'], out)

    def test_check_still_failing_is_exit_7(self):
        calls = []

        def fake(w, ctx):
            calls.append(1)
            return {'worker': 'agy fake', 'code': 0, 'conversation_id': 'c1', 'response': 'r', 'calls': '?', 'denied': []}

        code, last, out = self.run_main(fake, ['--part', 'ok'])
        self.assertEqual((code, len(calls)), (7, 2))
        self.assertIn('[CHECK FAILED]', out)
        code, last, _ = self.run_main(fake, ['--part', 'ok', '--fix-rounds', '0'])
        self.assertEqual((code, len(calls)), (7, 3))
        code, last, out = self.run_main(fake, ['--part', 'ok', '--fix-rounds', '2'])   # same failure twice: stop after 1
        self.assertEqual((code, len(calls), last['check']['fix_rounds']), (7, 5, 1))
        self.assertIn('no fix round 2: the check failed the same way', out)
        self.assertEqual((last['model'], last['family']), ('gemini-3.8-flash-low', 'gemini'))
        code, last, out = self.run_main(fake)   # no --part: {name} can't be filled, so no check
        self.assertEqual(code, 0)
        self.assertIn('check: not run', out)
        code, last, _ = self.run_main(fake, ['--part', 'ok', '--no-check'])
        self.assertEqual((code, last['check']), (0, None))

    def test_bad_part_refused(self):
        with self.assertRaises(d.Fail) as e:
            d.main(['--task', 'x', '--part', 'a; rm -rf .'])
        self.assertEqual(e.exception.code, 2)


class Changes(unittest.TestCase):
    def test_snapshot_sees_new_edited_again_and_reverted(self):
        import subprocess
        with tempfile.TemporaryDirectory() as t:
            def git(*args):
                subprocess.run(['git', '-C', t, '-c', 'user.name=t', '-c', 'user.email=t@example.invalid', *args],
                               check=True, capture_output=True)
            git('init', '-q')
            for n in ('a.py', 'b.py', 'my file.py'):
                Path(t, n).write_text('1\n', encoding='utf-8')
            git('add', '-A')
            git('commit', '-qm', 'base')
            Path(t, 'a.py').write_text('2\n', encoding='utf-8')        # dirty before the worker starts
            Path(t, 'b.py').write_text('2\n', encoding='utf-8')
            os.mkdir(Path(t, '.worker'))
            Path(t, '.worker', 'x.json').write_text('{}', encoding='utf-8')
            before = d.git_snapshot(t)
            self.assertEqual(sorted(before), ['a.py', 'b.py'])
            self.assertEqual(d.changed_since(before, d.git_snapshot(t)), [])
            Path(t, 'a.py').write_text('3\n', encoding='utf-8')        # edited again: same git status line
            Path(t, 'b.py').write_text('1\n', encoding='utf-8')        # back to the committed text
            Path(t, 'my file.py').write_text('2\n', encoding='utf-8')
            Path(t, 'new.py').write_text('x\n', encoding='utf-8')
            lines = d.changed_since(before, d.git_snapshot(t))
            self.assertEqual(lines, [' M a.py', ' M my file.py', '?? new.py', '-- b.py'])
            self.assertEqual(d.diff_stat(t, lines), [' M a.py (+1 -1)', ' M my file.py (+1 -1)', '?? new.py (+1)', '-- b.py'])

    def test_scope(self):
        lines = ['?? src/shop.py', ' M tests/test_shop.py', '?? src/cart.py', ' M .env']
        own, outside = d.split_scope(lines, ['./src/shop.py', 'tests\\test_*.py'])
        self.assertEqual(own, ['?? src/shop.py', ' M tests/test_shop.py'])
        self.assertEqual(outside, ['?? src/cart.py', ' M .env'])
        self.assertEqual(d.split_scope(lines[:3], []), (lines[:3], []))
        self.assertTrue(d.in_scope(' M .env', ['.env']))
        self.assertEqual(d.split_scope([' M .env', ' M a.py'], []), ([' M a.py'], [' M .env']))   # a secret is never "own"


class Codex(unittest.TestCase):
    def ctx(self, base='builder', conversation=None, secrets=()):
        import argparse
        ctx = d.Ctx()
        ctx.args = argparse.Namespace(conversation=conversation, schema=None, timeout_sec=30)
        ctx.base, ctx.secrets = base, list(secrets)
        return ctx

    def opt(self, cmd, key):
        """The value after each -c that sets key."""
        return [cmd[i + 1].split('=', 1)[1] for i, x in enumerate(cmd[:-1]) if x == '-c' and cmd[i + 1].startswith(key + '=')]

    def test_command_has_no_shell_and_no_user_config(self):
        cmd = d.codex_cmd({'h': 'codex', 'm': 'gpt-5.6-luna', 'e': 'low'}, self.ctx())
        self.assertEqual(cmd[:3], ['codex', 'exec', '--json'])
        self.assertEqual(cmd[-1], '-')   # prompt on stdin
        for flag in ('--ignore-user-config', '--ignore-rules'):
            self.assertIn(flag, cmd)
        disabled = [cmd[i + 1] for i, x in enumerate(cmd[:-1]) if x == '--disable']
        for feature in ('shell_tool', 'multi_agent', 'apps', 'plugins', 'hooks'):
            self.assertIn(feature, disabled)
        self.assertEqual(cmd[cmd.index('-m') + 1], 'gpt-5.6-luna')
        self.assertEqual(self.opt(cmd, 'model_reasoning_effort'), ['"low"'])
        self.assertEqual(self.opt(cmd, 'approval_policy'), ['"never"'])
        self.assertEqual(self.opt(cmd, 'web_search'), ['"disabled"'])
        self.assertNotIn('--sandbox', cmd)   # a permission profile instead (Codex: never both)
        self.assertNotIn('model_reasoning_effort', ' '.join(d.codex_cmd({'h': 'codex', 'm': 'm', 'e': ''}, self.ctx())))

    def test_profile_is_valid_toml_and_denies_secrets(self):
        import tomllib
        secret = 'C:\\proj\\.env' if d.IS_WIN else '/proj/.env'
        for base, extends, root in (('builder', ':workspace', 'write'), ('reviewer', ':read-only', 'read')):
            cmd = d.codex_cmd({'h': 'codex', 'm': 'm', 'e': ''}, self.ctx(base, secrets=[secret]))
            self.assertEqual(self.opt(cmd, 'default_permissions'), ['"worker"'])
            prof = tomllib.loads('p = ' + self.opt(cmd, 'permissions.worker')[0])['p']
            self.assertEqual(prof['extends'], extends)
            self.assertEqual(prof['filesystem'][':workspace_roots']['.'], root)
            self.assertEqual(prof['filesystem'][':workspace_roots']['**/.env'], 'deny')
            self.assertEqual(prof['filesystem'][secret], 'deny')

    def test_resume(self):
        cmd = d.codex_cmd({'h': 'codex', 'm': 'm', 'e': ''}, self.ctx(conversation='th-1'))
        self.assertEqual(cmd[:3], ['codex', 'exec', 'resume'])
        self.assertEqual(cmd[-2:], ['th-1', '-'])

    def events(self, *ev, noise='progress on stderr'):
        return d.parse_codex_events(noise + '\n' + '\n'.join(json.dumps(e) for e in ev), 'gpt-5.6-luna', 'low')

    def test_events(self):
        ok = self.events({'type': 'thread.started', 'thread_id': 'th-9'}, {'type': 'turn.started'},
                         {'type': 'item.completed', 'item': {'id': 'i1', 'type': 'file_change', 'changes': []}},
                         {'type': 'item.completed', 'item': {'id': 'i2', 'type': 'agent_message', 'text': 'first'}},
                         {'type': 'item.completed', 'item': {'id': 'i3', 'type': 'agent_message', 'text': 'Report'}},
                         {'type': 'turn.completed', 'usage': {'input_tokens': 900, 'cached_input_tokens': 500,
                                                              'output_tokens': 40, 'reasoning_output_tokens': 7}})
        self.assertEqual((ok['code'], ok['conversation_id'], ok['response'], ok['worker']), (0, 'th-9', 'Report', 'codex gpt-5.6-luna (low)'))
        self.assertEqual((ok['input_tokens'], ok['cache_read_tokens'], ok['output_tokens'], ok['thinking_tokens']), (900, 500, 40, 7))

    def test_event_errors(self):
        ran = self.events({'type': 'thread.started', 'thread_id': 't'},
                          {'type': 'item.started', 'item': {'id': 'c1', 'type': 'command_execution', 'command': 'cat .env'}},
                          {'type': 'item.completed', 'item': {'id': 'c1', 'type': 'command_execution', 'command': 'cat .env'}},
                          {'type': 'item.completed', 'item': {'id': 'm', 'type': 'agent_message', 'text': 'r'}},
                          {'type': 'turn.completed', 'usage': {}})
        self.assertEqual((ran['code'], len(ran['denied'])), (4, 1))
        self.assertIn('cat .env', ran['denied'][0])
        failed = self.events({'type': 'thread.started', 'thread_id': 't'}, {'type': 'turn.failed', 'error': {'message': '429 rate limit'}})
        self.assertEqual(failed['code'], 6)
        self.assertTrue(d.TRANSIENT.search(failed['error']))
        self.assertEqual(self.events({'type': 'error', 'message': 'usage limit reached'})['code'], 6)
        empty = self.events({'type': 'thread.started', 'thread_id': 't'}, {'type': 'turn.completed', 'usage': {}})
        self.assertEqual(empty['code'], 5)
        crash = d.parse_codex_events('a\nb\nError: not logged in\n', 'm')
        self.assertEqual((crash['code'], crash['error']), (6, 'a b Error: not logged in'))


class Independence(unittest.TestCase):
    """A tester/reviewer never runs on the builder's model family (and needs --after); secrets never go in."""
    run_main = Check.run_main

    def fake(self, seen):
        def f(w, ctx):
            seen.append(w['m'])
            return {'worker': d.worker_name(w), 'code': 0, 'conversation_id': 'c', 'response': 'r', 'calls': '?', 'denied': []}
        return f

    def run_role(self, role, models, after, extra=()):
        seen = []
        builder_run = {'.worker/run-0123abcd.json': json.dumps({'model': 'gemini-3.8-flash-low', 'family': 'gemini'})}
        old = d.AGENT_DIR
        with tempfile.TemporaryDirectory() as agent:
            for sub in ('roles', 'bases'):
                Path(agent, sub).mkdir()
            Path(agent, 'roles', f'{role}.md').write_text(f'---\nbase: {role}\nmodels: {models}\n---\n', encoding='utf-8')
            Path(agent, 'bases', f'{role}.md').write_text('RULES', encoding='utf-8')
            d.AGENT_DIR = Path(agent)
            try:
                args = ['--role', role] + (['--after', after] if after else []) + list(extra)
                code, last, _ = self.run_main(self.fake(seen), args, files=builder_run, model=False)
            finally:
                d.AGENT_DIR = old
        return code, seen, last

    def test_reviewer_skips_the_builders_family(self):
        code, seen, last = self.run_role('reviewer', 'agy:gemini-3.8-flash-low, codex:gpt-5.6-terra', '0123abcd')
        self.assertEqual((code, seen, last['family'], last['after']), (0, ['gpt-5.6-terra'], 'gpt', '0123abcd'))
        code, seen, _ = self.run_role('tester', 'agy:gemini-3.8-flash-low, codex:gpt-5.6-luna:low', '.worker/run-0123abcd.json')
        self.assertEqual(seen, ['gpt-5.6-luna'])
        code, seen, _ = self.run_role('reviewer', 'agy:gemini-3.8-flash-low, codex:gpt-5.6-terra', 'lead')
        self.assertEqual(seen, ['gemini-3.8-flash-low'])   # the Lead made it: no family to avoid

    def test_refusals(self):
        with self.assertRaises(d.Fail) as e:
            self.run_role('reviewer', 'agy:gemini-3.8-flash-low', None)
        self.assertIn('--after', str(e.exception))
        with self.assertRaises(d.Fail) as e:
            self.run_role('reviewer', 'agy:gemini-3.8-flash-low, agy:gemini-3.8-flash-medium', '0123abcd')
        self.assertIn('gemini family', str(e.exception))
        with self.assertRaises(d.Fail) as e:
            self.run_role('reviewer', 'codex:gpt-5.6-terra', '0123abcd', ['--model', 'gemini-3.8-flash-medium'])
        self.assertIn('same model family', str(e.exception))
        with self.assertRaises(d.Fail) as e:
            self.run_role('reviewer', 'codex:gpt-5.6-terra', 'ffffffff')
        self.assertEqual(e.exception.code, 2)

    def test_secret_or_outside_path_refused(self):
        with self.assertRaises(d.Fail) as e:
            self.run_main(lambda w, c: {}, ['--include', '.env'], files={'.env': 'K=1'})
        self.assertEqual(e.exception.code, 4)
        with self.assertRaises(d.Fail) as e:
            self.run_main(lambda w, c: {}, ['--files', '../elsewhere.py'])
        self.assertEqual(e.exception.code, 4)
        for name, text in (('shot.png', '\x89PNG\r\n\x1a\n\0\0'), ('missing.py', None)):
            with self.assertRaises(d.Fail, msg=name) as e:
                self.run_main(lambda w, c: {}, ['--include', name], files={name: text} if text else {})
            self.assertEqual(e.exception.code, 2)

    def test_worker_gets_the_secret_paths(self):
        got = []

        def fake(w, ctx):
            got.extend(ctx.secrets)
            return {'worker': 'x', 'code': 0, 'conversation_id': None, 'response': 'r', 'calls': '?', 'denied': []}
        self.run_main(fake, files={'.env': 'K=1', 'app/main.py': 'x'})
        self.assertEqual([Path(p).name for p in got], ['.env'])


class Results(unittest.TestCase):
    def agy(self, result):
        return d.parse_agy_stream('noise\n' + json.dumps({'event': 'result', 'result': result}, separators=(',', ':')) + '\n', 'm1')

    def test_agy_codes(self):
        ok = self.agy({'response': 'done', 'conversation_id': 'c1', 'usage': {'input_tokens': 10, 'output_tokens': 2}})
        self.assertEqual((ok['code'], ok['conversation_id'], ok['input_tokens'], ok['worker']), (0, 'c1', 10, 'agy m1'))
        self.assertEqual(self.agy({'finished_reason': 'print_timeout'})['code'], 3)
        self.assertEqual(self.agy({'status': 'ERROR', 'error': '429 quota'})['code'], 6)
        self.assertEqual(self.agy({'response': 'x', 'denied_actions': [{'display_name': 'rm -rf'}]})['denied'], ['rm -rf'])
        self.assertEqual(self.agy({'response': ''})['code'], 5)
        bad = d.parse_agy_stream('a\nb\nc\nd\n', 'm1')
        self.assertEqual((bad['code'], bad['error']), (6, 'b c d'))

class Processes(unittest.TestCase):
    def test_run_tool_output_and_stdin(self):
        out, why = d.run_tool([sys.executable, '-c', 'import sys; sys.stdout.buffer.write(sys.stdin.buffer.read().decode().upper().encode()); '
                               'sys.stdout.flush(); print("err", file=sys.stderr)'],
                              stdin_bytes='héllo'.encode('utf-8'))
        self.assertIsNone(why)
        self.assertIn('HÉLLO', out)
        self.assertIn('err', out)

    def test_timeout_kills_child_processes_too(self):
        with tempfile.TemporaryDirectory() as t:
            marker = Path(t, 'late.txt')
            # The parent starts a child that would write a file after 8 s; both must be killed at the timeout.
            child = f'import time; time.sleep(8); open({str(marker)!r}, "w").write("x")'
            parent = f'import subprocess, sys, time; subprocess.Popen([sys.executable, "-c", {child!r}]); time.sleep(30)'
            start = time.monotonic()
            _, why = d.run_tool([sys.executable, '-c', parent], timeout=2)
            self.assertEqual(why, 'timeout')
            self.assertLess(time.monotonic() - start, 15)
            time.sleep(9)
            self.assertFalse(marker.exists(), 'child process survived the kill')

    def test_watch_stops_early(self):
        _, why = d.run_tool([sys.executable, '-c', 'import time; time.sleep(30)'], watch=lambda: True)
        self.assertEqual(why, 'stopped')

    def test_missing_tool(self):
        with self.assertRaises(d.Fail) as e:
            d.run_tool(['no-such-tool-xyz'])
        self.assertEqual(e.exception.code, 3)

    def test_cli_errors(self):
        import subprocess
        script = str(d.AGENT_DIR / 'delegate.py')
        r = subprocess.run([sys.executable, script, '--role', 'nope'], capture_output=True, text=True)
        self.assertEqual(r.returncode, 2)
        self.assertIn("No role 'nope'", r.stderr)
        r = subprocess.run([sys.executable, script, '--model', 'gemini-3.8-flash-low'], capture_output=True, text=True)
        self.assertEqual(r.returncode, 2)
        self.assertIn('--task', r.stderr)


class Bench(unittest.TestCase):
    @staticmethod
    def reference(task, tmp):
        """solution/answer.md with each {bug id} replaced by that bug's line in tmp (review and debug tasks)."""
        import bench
        text = (task['dir'] / 'solution' / 'answer.md').read_text(encoding='utf-8')
        for spot in bench.answers(tmp, task)['bugs']:
            text = text.replace('{' + spot['id'] + '}', str(spot['line']))
        return text

    def test_every_task_scores_full_on_its_solution_and_low_on_its_start(self):
        import bench
        tasks = bench.load_tasks()
        self.assertGreaterEqual(len(tasks), 12)
        self.assertEqual({t['kind'] for t in tasks}, {'build', 'tester', 'review', 'debug'})
        self.assertTrue({'builder', 'builderplus', 'tester', 'reviewer', 'debugger', 'security-reviewer'} <= {t['role'] for t in tasks})
        for task in tasks:
            name = task['name']
            for f in task['files'] + task.get('include', []):
                d.project_path(f, name)   # every bench path passes delegate's path policy
            if task.get('role_file'):
                self.assertTrue((d.ROOT / task['role_file']).exists(), name)
            answer = task['kind'] in ('review', 'debug')
            with tempfile.TemporaryDirectory() as t:
                bench.lay_out(task, t, ('start',) if answer else ('start', 'solution'))
                for f in task.get('include', []):
                    self.assertTrue(Path(t, f).exists(), f'{name}: {f}')
                got, missed, note = bench.score(t, task, self.reference(task, t) if answer else '')
                self.assertEqual((got, missed), (task['tests'], []), name)
                if task.get('project') and task['kind'] == 'build':
                    kept, total = note.split()[1].split('/')
                    self.assertEqual(kept, total, name)
                if task['kind'] == 'review':
                    self.assertEqual(note, '0 false', name)
            with tempfile.TemporaryDirectory() as t:
                bench.lay_out(task, t)
                got, missed, _ = bench.score(t, task, '')
                self.assertLess(got, max(1, task['tests'] // 2), name)
                self.assertTrue(missed, name)

    def test_review_and_debug_decoys_cost(self):
        import bench
        for name in ('sk_review',):
            task = bench.load_tasks([name])[0]
            with tempfile.TemporaryDirectory() as t:
                bench.lay_out(task, t)
                data = bench.answers(t, task)
                bug, decoy = data['bugs'][0], data['decoys'][0]
                report = f"Problems:\n- {bug['file']}:{bug['line']}: x\n- {decoy['file']}:{decoy['line']}: y"
                got, missed, note = bench.score(t, task, report)
                self.assertEqual((got, note), (1, '1 false'), name)
                self.assertIn(f"false alarm at decoy {decoy['id']}", missed)
        task = bench.load_tasks(['sk_debug'])[0]
        with tempfile.TemporaryDirectory() as t:
            bench.lay_out(task, t)
            data = bench.answers(t, task)
            cause, decoy = data['bugs'][0], data['decoys'][0]
            self.assertEqual(bench.score(t, task, f"{decoy['file']}:{decoy['line']} then {cause['file']}:{cause['line']}")[0], 0)
            self.assertEqual(bench.score(t, task, f"{cause['file']}:{cause['line']}")[0], 1)   # place, no reason

    def test_tester_task_scores_caught_mutants(self):
        import bench
        for name in ('duration', 'sk_tester'):
            task = bench.load_tasks([name])[0]
            self.assertEqual(len(list((task['dir'] / 'hidden' / 'mutants').glob('*.py'))), task['tests'], name)
        task = bench.load_tasks(['duration'])[0]
        with tempfile.TemporaryDirectory() as t:
            bench.lay_out(task, t)
            Path(t, 'tests').mkdir()
            weak = ('import unittest\nfrom duration import parse_duration\n\n\nclass T(unittest.TestCase):\n'
                    '    def test_h(self):\n        self.assertEqual(parse_duration("1h"), 3600)\n')
            Path(t, 'tests', 'test_duration.py').write_text(weak, encoding='utf-8')
            got, missed, _ = bench.score(t, task)
            self.assertLess(got, task['tests'])
            self.assertIn('m05_zero_empty', missed)
            self.assertEqual(Path(t, 'duration.py').read_bytes(), (task['dir'] / 'start' / 'duration.py').read_bytes())   # restored
            Path(t, 'tests', 'test_duration.py').write_text(weak.replace('3600', '3601'), encoding='utf-8')
            self.assertEqual(bench.score(t, task), (0, ['its tests fail on the real code'], ''))

    def test_prepare_makes_a_runnable_project(self):
        import bench
        task = bench.load_tasks(['inventory'])[0]
        with tempfile.TemporaryDirectory() as t:
            bench.prepare(task, t)
            proj, _, private = d.parse_project(d.read_text(Path(t, 'PROJECT.md')))
            self.assertFalse(private)
            self.assertIn('tests.test_{name}', proj['check_one'])
            for f in ('agent/delegate.py', 'agent/models.json', 'agent/roles/builder.md', 'inventory.py', 'tests', '.git'):
                self.assertTrue(Path(t, f).exists(), f)
            self.assertFalse(Path(t, 'agent', 'bench').exists(), 'the worker must not see the hidden tests')
            self.assertEqual(d.git_snapshot(t), {})
        task = bench.load_tasks(['sk_security'])[0]
        with tempfile.TemporaryDirectory() as t:
            text = Path(bench.prepare(task, t)).read_text(encoding='utf-8')
            self.assertIn('+++ b/shop/exports.py', text)          # the change, pasted as a diff
            self.assertIn('--- a/shop/cli.py', text)
            for f in ('agent/roles/security-reviewer.md', 'shop/exports.py', 'shop/discounts.py', 'tests/test_exports.py'):
                self.assertTrue(Path(t, f).exists(), f)
            self.assertFalse(Path(t, 'agent', 'bench').exists())

    def test_worker_flags_and_table(self):
        import bench
        self.assertEqual(bench.worker_flags('agy:gemini-3.8-flash-low'), ['--worker', 'agy:gemini-3.8-flash-low'])
        self.assertEqual(bench.worker_flags('codex:gpt-5.6-luna:low'), ['--worker', 'codex:gpt-5.6-luna:low'])
        with self.assertRaises(d.Fail):
            bench.worker_flags('nvidia')
        text = bench.table([{'worker': 'codex:m', 'task': 't', 'level': 'easy', 'passed': 3, 'tests': 4, 'exit': 'OK', 'seconds': 9}])
        self.assertIn('| codex:m | t | easy | 3/4 |  |  | OK |', text)


class CheckSetup(unittest.TestCase):
    def test_keeps_confirmed_answers(self):
        new = check_setup.build_md([('git', 'found', 'n')], {'agy': True, 'codex': False}, '2026-01-01 00:00')
        self.assertIn('```available\nagy: yes\ncodex: no\n```', new)
        self.assertNotIn('Ollama', new)
        old = new.replace('<which plan>', 'Max')
        merged = check_setup.keep_confirmed(new, old)
        self.assertIn('- Claude plan: Max', merged)
        self.assertEqual(merged.count('## Confirmed by the user'), 1)
        self.assertEqual(check_setup.keep_confirmed(new, ''), new)


if __name__ == '__main__':
    unittest.main()
