# orders
> OrderBook: lines, coupon, totals, pay/ship/cancel/refund with stock reservations.

Back to [the map](../README.md).

- Files: `shop/orders.py`, data in `shop/models.py` (Order, OrderLine). Tests: `tests/test_orders.py`.
- Flow: new -> paid -> shipped; new/paid -> cancelled; paid/shipped -> refunded. A wrong status raises
  `order <id> is <status>`.
- Stock (`shop/catalog.py`): add_line reserves, remove_line / cancel / refund-of-paid release, ship commits,
  refund-of-shipped restocks.
- `totals()` recalculates discounts while new; `pay()` needs the exact gross, freezes discounts, counts the coupon use,
  sets `paid_total`. `refunded` is the gross refunded so far (cancel and refund set it to `paid_total`).
- Events: `(date, text)` appended on every change.
