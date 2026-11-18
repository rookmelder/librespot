# reports and cli
> Sales per day, top products, CSV text; the command line on top of storage and OrderBook.

Back to [the map](../README.md).

- `shop/reports.py` (tests `tests/test_reports.py`): a sale is an order with status paid, shipped or refunded,
  counted on its `created` day for `paid_total - refunded`.
- `shop/cli.py` (tests `tests/test_cli.py`): `run(path, command, args, out)` loads the file, runs one command,
  saves after changes, prints `error: <message>` and returns 2 on a ValueError.
