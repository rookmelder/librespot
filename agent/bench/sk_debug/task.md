Goal: find the root cause of a failing invoice test. Diagnose only; change nothing.

A customer reported an invoice that was one cent too high, and the new test for it fails (the check output is
above). The test's expected values were worked out by hand from the rules in the system notes. Nothing else in
`tests/test_invoices.py` fails.
`shop/invoices.py`, the test file and the system note are below; read any other file you need.

Answer with file:line for every place you point at (e.g. `shop/x.py:12`), using the line numbers of the files
as they are now. Name the root cause first.
