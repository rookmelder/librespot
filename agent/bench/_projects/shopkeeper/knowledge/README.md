# shopkeeper: knowledge map

> An order and billing service: products and stock, discounts and coupons, orders, invoices, JSON storage, reports, CLI.

## Conventions (all code)
- Money is int cents everywhere. Rounding only through `shop/money.py` (halves away from zero).
- Dates: always `shop.clock.today()`, never `datetime.date.today()`: tests fix the date with `clock.set_today()`.
- Errors are `ValueError` with an exact message; tests compare the message text.
- Python 3, standard library only. Tests: `python -m unittest discover -s tests`.

## Systems
- [money](systems/money.md): cents, VAT rates, rounding, splitting an amount.
- [discounts](systems/discounts.md): rule types, the order they run in, coupons.
- [orders](systems/orders.md): status flow, stock reservations, totals, payment.
- [invoices](systems/invoices.md): invoices and credit notes, VAT per rate.
- [storage](systems/storage.md): the JSON file format and its versions.
- [reports and cli](systems/reports-cli.md): sales reports, CSV, command line.
