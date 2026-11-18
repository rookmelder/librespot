# money
> Cents, VAT rates, half-away-from-zero rounding, split with largest remainder, fmt/parse.

Back to [the map](../README.md).

- File: `shop/money.py`. Tests: `tests/test_money.py`.
- `round_half_up(n, d)` is the one rounding function: 2.5 -> 3 and -2.5 -> -3 (credit notes rely on the symmetry).
- `vat_for(net, rate)` = `percent_of(net, VAT_RATES[rate])`; rates: standard 21, reduced 9, zero 0.
- `split(cents, weights)`: parts always add up exactly; leftover cents go to the largest remainders, ties to the
  earlier part. Used by the fixed discount.
- Used by: discounts, invoices, orders (fmt in messages), cli (parse).
