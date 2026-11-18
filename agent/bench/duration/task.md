Goal: write thorough tests for the existing module `duration.py` (shown below). Do not change it.

Files: create `tests/test_duration.py` (the `tests` folder exists). Change no other file.

Spec of `duration.py` (test against THIS spec; the code is believed to follow it):

`parse_duration(text)` returns whole seconds:
1. Strip `text` and make it lowercase. Empty after that: `ValueError('empty duration')`.
2. Only digits (`0`-`9`): that number of seconds (`'90'` -> 90).
3. Otherwise one or more parts `<digits><unit>`, unit `d` (86400 s), `h` (3600), `m` (60) or `s` (1). Parts may
   stand together (`'1h30m'`) or be separated by whitespace (`'1h 30m'`), but a number and its unit may not
   (`'1 h'` is bad). Units must come in the order d, h, m, s, each at most once.
4. Anything else (wrong order, a repeated unit, another letter, a sign, a decimal point, ...):
   `ValueError(f'bad duration: {text}')` with the ORIGINAL text, not the stripped or lowercased one.
5. `'0s'` and `'0'` are 0.

`format_duration(seconds)` returns a string:
1. `seconds` must be an `int` (a `bool` is not) and not negative, else `ValueError('seconds must be a non-negative int')`.
2. `0` -> `'0s'`.
3. Otherwise the non-zero parts of days, hours, minutes, seconds, in that order, with no spaces:
   `90061` -> `'1d1h1m1s'`, `3600` -> `'1h'`, `3661` -> `'1h1m1s'`, `86400 + 5` -> `'1d5s'`.
For every int `n >= 0`: `parse_duration(format_duration(n)) == n`.

Must cover: every numbered rule of both functions with exact error messages, each unit's size, upper-case input,
spacing between parts, every kind of bad input named in rule 4, and the round trip. The Lead scores your tests by
running them against copies of `duration.py` with small planted bugs: a test file that still passes on a buggy copy
missed that bug. Your tests must pass on the real `duration.py`.
