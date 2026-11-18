import unittest

from cfg.parser import parse_line
from cfg.service import Service
from cfg.store import Store


class Parser(unittest.TestCase):
    def test_value_may_contain_equals(self):
        self.assertEqual(parse_line('  db.url = a=b=c  '), ('db.url', 'a=b=c'))
        self.assertEqual(parse_line('empty_ok ='), ('empty_ok', ''))

    def test_blank_and_comment(self):
        for line in ('', '   ', '# note', '   #x = 1'):
            self.assertIsNone(parse_line(line), repr(line))

    def test_errors(self):
        with self.assertRaises(ValueError) as e:
            parse_line('  just words ')
        self.assertEqual(str(e.exception), 'no = in line: just words')
        for key in ('Upper', '1st', 'has space', ''):
            with self.assertRaises(ValueError) as e:
                parse_line(f'{key} = v')
            self.assertEqual(str(e.exception), f'bad key: {key}')


class StoreTests(unittest.TestCase):
    def test_lock(self):
        s = Store()
        s.lock('fixed')
        with self.assertRaises(PermissionError) as e:
            s.set('fixed', 1)
        self.assertEqual(str(e.exception), 'read-only key: fixed')
        self.assertIsNone(s.get('fixed'))

    def test_set_many_rolls_back(self):
        s = Store()
        s.set('a', '1')
        s.lock('c')
        with self.assertRaises(PermissionError):
            s.set_many({'a': '2', 'b': 'new', 'c': 'x', 'd': 'never'})
        self.assertEqual(s.data, {'a': '1'})

    def test_set_many_reraises_same_object(self):
        boom = RuntimeError('disk full')

        class Flaky(Store):
            def set(self, key, value):
                if key == 'b':
                    raise boom
                super().set(key, value)
        s = Flaky()
        with self.assertRaises(RuntimeError) as e:
            s.set_many({'a': 1, 'b': 2})
        self.assertIs(e.exception, boom)
        self.assertEqual(s.data, {})


class Loading(unittest.TestCase):
    def test_load_count_last_wins_comments(self):
        svc = Service(Store())
        n = svc.load('# settings\nname = one\n\nport = 80\nname = two\n')
        self.assertEqual(n, 2)
        self.assertEqual((svc.get('name'), svc.get('port')), ('two', '80'))

    def test_parse_error_has_line_number_and_stores_nothing(self):
        st = Store()
        svc = Service(st)
        with self.assertRaises(ValueError) as e:
            svc.load('a = 1\n\nBad = 2\n')
        self.assertEqual(str(e.exception), 'line 3: bad key: Bad')
        self.assertEqual(st.data, {})

    def test_store_failure_changes_nothing(self):
        st = Store()
        svc = Service(st)
        svc.load('a = old')
        self.assertEqual(svc.get('a'), 'old')
        st.lock('z')
        with self.assertRaises(PermissionError):
            svc.load('a = new\nz = 1')
        self.assertEqual(st.data, {'a': 'old'})
        self.assertEqual(svc.cache, {'a': 'old'})
        self.assertEqual(svc.get('a'), 'old')

    def test_cache_invalidated_after_load(self):
        svc = Service(Store())
        svc.load('a = 1')
        self.assertEqual(svc.get('a'), '1')
        svc.load('a = 2')
        self.assertEqual(svc.get('a'), '2')

    def test_missing_key_default_not_cached(self):
        svc = Service(Store())
        self.assertEqual(svc.get('later', 'dflt'), 'dflt')
        self.assertNotIn('later', svc.cache)
        svc.load('later = here')
        self.assertEqual(svc.get('later', 'dflt'), 'here')


if __name__ == '__main__':
    unittest.main()
