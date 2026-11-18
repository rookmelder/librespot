Goal: refund part of a paid or shipped order (some units of some lines), keeping stock, totals and the save file right.

Files: change `shop/models.py`, `shop/orders.py`, `shop/storage.py`, and add tests to `tests/test_orders.py` and
`tests/test_storage.py` (all shown below, with the system notes). Change no other file.

Spec:

`Order` gets two new fields, both a dict that defaults to empty: `refunded_units` (sku -> units refunded so far) and
`refunded_net` (sku -> net cents refunded so far).

New `OrderBook.refund_lines(order_id, items)`; `items` is a dict sku -> units. Returns the gross amount refunded
(cents) and logs an event `refunded <amount>` like the other methods. It refuses, before changing anything, checking
in this order: an order that is not paid or shipped (the same message as the other methods), an empty `items`
(`nothing to refund`), and then per sku in the dict's order: a sku not in the order (`<sku> not in order <id>`), units
that are not a positive int (`qty must be a positive int`), more units than are left to refund for that line
(`cannot refund <units> <sku>: only <left> left`).

Amounts:
- A line's refund is proportional to its net after discounts: `line.net × units / line.qty`, rounded half up.
- VAT is refunded the way invoices calculate it (see the invoices note), on what this call refunds.
- Whatever the order of partial refunds: all refunds of one line add up exactly to that line's net, and once every
  unit of the order is refunded, all refunds together add up exactly to `paid_total`. The order's status then becomes
  `refunded`; until then it stays as it was.
- The order's `refunded` total, its two new dicts and the stock are updated; refunded units of a shipped order go
  back into stock, those of a paid order are no longer reserved.

The units refunded this way are no longer part of the order for `ship()`, `refund()` (the full refund, which refunds
whatever is left) and `cancel()`: no unit or cent may be committed, restocked, released or paid back twice.

`shop/storage.py`: a saved shop keeps the partial refunds. Bump the format version; files of every older version
still load (old orders have nothing refunded per line); a newer version is refused as now. Update the docstring.

Must cover: the amounts, the checks, stock, the existing methods after a partial refund, and saving and loading.
