# discounts
> Rule dicts (bxgy, percent, fixed), applied by type in a fixed order; coupons add one rule.

Back to [the map](../README.md).

- File: `shop/discounts.py`. Tests: `tests/test_discounts.py`. The rule formats are in the module docstring.
- `apply_discounts(order, rules, coupons)` resets and sets every `line.discount`; called by `OrderBook.totals()`
  on every call while the order is new (so a coupon is checked again each time), never after payment.
- Order: all bxgy, then all percent, then all fixed. Percent rounds per line; fixed is spread with `money.split`.
- Coupons live in the OrderBook: `{code: {'rule', 'max_uses', 'uses'}}`. `coupon_rule()` is the only place that
  decides whether a coupon may be used; `OrderBook.set_coupon` and `apply_discounts` both go through it.
  `pay()` increments `uses`.
- Like all code here, dates come from `shop.clock.today()` (see the conventions in the map).
