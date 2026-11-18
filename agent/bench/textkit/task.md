Goal: make a small text module with two functions, and its tests.

Files: create `textkit.py` in the project root and `tests/test_textkit.py` (the `tests` folder exists). Change no other file.

Spec:

`slugify(text, max_len=40)` returns a string:
1. If `max_len` is less than 1: raise `ValueError('max_len must be at least 1')`. Check this first.
2. Make `text` lowercase.
3. Replace every run of characters that are not `a`-`z` or `0`-`9` with one `-`.
4. Remove `-` from the start and the end.
5. If the result is longer than `max_len`: keep the first `max_len` characters, then remove `-` from the end again.
6. If the result is empty: return `'n-a'` (also when that is longer than `max_len`).

`wrap(text, width)` returns a list of lines (strings):
1. If `width` is less than 1: raise `ValueError('width must be at least 1')`. Check this first.
2. Split `text` into words on any whitespace. No words: return `[]`.
3. Fill lines greedily: words on one line are joined by exactly one space, and a line is never longer than `width`.
   A word that does not fit on the current line starts a new line.
4. A word longer than `width`: first end the current line (if it has words). Then, while the word is longer than
   `width`, its first `width` characters become a line of their own. What is left of the word is then treated as a
   normal word that starts a new line (later words may join it).

Must cover: every numbered rule above, both error messages, and a long word in the middle of a sentence.
