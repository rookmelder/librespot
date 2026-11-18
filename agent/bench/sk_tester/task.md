Goal: make the tests of `shop/discounts.py` thorough: add tests to `tests/test_discounts.py` (both shown below, with
the system note). Do not change `shop/discounts.py`.

Files: change only `tests/test_discounts.py`. Keep the tests that are there; add your own (new methods or classes).

The module is in production and believed correct: its docstring and the system note describe what it must do, and
where they say nothing, what the code does now is the intended behaviour. We are about to refactor it, and your
tests are the safety net: any change in behaviour, however small, must make at least one test fail.

Must cover: everything the module promises, not only the common cases. The Lead scores your tests by running them
against copies of `shop/discounts.py` with small planted bugs: a test file that still passes on a buggy copy missed
that bug. Your tests must pass on the real `shop/discounts.py`.
