"""Reference tests: they must pass on duration.py and fail on every mutant (bench.py checks this)."""
import unittest

from duration import format_duration, parse_duration


class Parse(unittest.TestCase):
    def bad(self, text, msg=None):
        with self.assertRaises(ValueError) as e:
            parse_duration(text)
        self.assertEqual(str(e.exception), msg or f'bad duration: {text}')

    def test_units_and_combinations(self):
        self.assertEqual(parse_duration('1d'), 86400)
        self.assertEqual(parse_duration('1h'), 3600)
        self.assertEqual(parse_duration('1m'), 60)
        self.assertEqual(parse_duration('1s'), 1)
        self.assertEqual(parse_duration('1h30m'), 5400)
        self.assertEqual(parse_duration('2d4h5m6s'), 2 * 86400 + 4 * 3600 + 5 * 60 + 6)

    def test_bare_number_and_zero(self):
        self.assertEqual(parse_duration('90'), 90)
        self.assertEqual(parse_duration('0'), 0)
        self.assertEqual(parse_duration('0s'), 0)

    def test_case_and_spaces(self):
        self.assertEqual(parse_duration('  1H 30M  '), 5400)
        self.assertEqual(parse_duration('1d 2h'), 93600)

    def test_empty(self):
        for text in ('', '   '):
            self.bad(text, 'empty duration')

    def test_bad_inputs_keep_original_text(self):
        for text in ('30m1h', '1h1h', '1h2m1h', '1 h', '5x', '-5s', '1.5h', 'h', ' 1X ', '1H 2H'):
            self.bad(text)


class Format(unittest.TestCase):
    def test_examples(self):
        self.assertEqual(format_duration(0), '0s')
        self.assertEqual(format_duration(90061), '1d1h1m1s')
        self.assertEqual(format_duration(3600), '1h')
        self.assertEqual(format_duration(3661), '1h1m1s')
        self.assertEqual(format_duration(86405), '1d5s')
        self.assertEqual(format_duration(86400), '1d')

    def test_bad_seconds(self):
        for bad in (-1, True, False, 1.5, '5'):
            with self.assertRaises(ValueError) as e:
                format_duration(bad)
            self.assertEqual(str(e.exception), 'seconds must be a non-negative int')

    def test_round_trip(self):
        for n in (0, 1, 59, 60, 61, 3599, 3600, 86399, 86400, 90061, 1000000):
            self.assertEqual(parse_duration(format_duration(n)), n)


if __name__ == '__main__':
    unittest.main()
