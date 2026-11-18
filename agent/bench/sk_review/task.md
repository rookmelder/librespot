Review this pull request (diff below; the changed shop files are below with line numbers, and the tests pass).

PR description, as the author wrote it:
> Support desk asked for two CLI commands: `between <from> <to>` (sales per day over a date range) and
> `search <customer>` (a customer's orders). Adds `reports.sales_between` and `OrderBook.search` for them.
> While there: `cancel()` and `refund()` had the same give-back code, it now lives in `_give_back()`
> (no behaviour change), and `find()` returns orders oldest first. Updated the tests.

Read the code against its docstrings, the docstring of `shop/orders.py` and the system notes, and against what the
author says the change does. Report every real problem; leave out style and anything that is correct.
Point at shop code with file:line (line numbers as in the files below).
