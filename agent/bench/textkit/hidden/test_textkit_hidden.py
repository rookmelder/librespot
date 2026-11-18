import unittest

import textkit


class Slugify(unittest.TestCase):
    def test_basic(self):
        self.assertEqual(textkit.slugify('Hello, World!'), 'hello-world')

    def test_runs_and_edges(self):
        self.assertEqual(textkit.slugify('  --A__b  3?? '), 'a-b-3')

    def test_non_ascii_is_a_separator(self):
        self.assertEqual(textkit.slugify('Café Olé'), 'caf-ol')

    def test_cut_then_strip(self):
        self.assertEqual(textkit.slugify('abc def ghi', max_len=4), 'abc')
        self.assertEqual(textkit.slugify('abc def ghi', max_len=5), 'abc-d')

    def test_empty_gives_n_a(self):
        self.assertEqual(textkit.slugify('!!!'), 'n-a')
        self.assertEqual(textkit.slugify('', max_len=1), 'n-a')

    def test_bad_max_len(self):
        with self.assertRaises(ValueError) as e:
            textkit.slugify('x', max_len=0)
        self.assertEqual(str(e.exception), 'max_len must be at least 1')


class Wrap(unittest.TestCase):
    def test_greedy(self):
        self.assertEqual(textkit.wrap('the quick brown fox', 10), ['the quick', 'brown fox'])

    def test_exact_fit_and_whitespace(self):
        self.assertEqual(textkit.wrap('  ab\tcd\n\nef ', 5), ['ab cd', 'ef'])

    def test_empty(self):
        self.assertEqual(textkit.wrap('   \n', 5), [])

    def test_long_word_in_the_middle(self):
        self.assertEqual(textkit.wrap('hi abcdefgh yo', 3), ['hi', 'abc', 'def', 'gh', 'yo'])
        self.assertEqual(textkit.wrap('hi abcdefg yo', 4), ['hi', 'abcd', 'efg', 'yo'])

    def test_long_word_rest_can_be_joined(self):
        self.assertEqual(textkit.wrap('abcdefg x yz', 5), ['abcde', 'fg x', 'yz'])
        self.assertEqual(textkit.wrap('abcdef', 3), ['abc', 'def'])

    def test_bad_width(self):
        with self.assertRaises(ValueError) as e:
            textkit.wrap('x', 0)
        self.assertEqual(str(e.exception), 'width must be at least 1')


if __name__ == '__main__':
    unittest.main()
