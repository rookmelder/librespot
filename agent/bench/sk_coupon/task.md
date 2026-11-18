Goal: coupons can have an expiry date; an expired coupon can no longer be used.

Files: change `shop/discounts.py` and add tests to `tests/test_discounts.py` (both shown below, with the system
note). Change no other file.

Spec:
- A coupon dict (see the docstring of `shop/discounts.py`) may have a new key `'expires'`: the last day it is valid,
  a string `'YYYY-MM-DD'` (exactly that form, and a real date). Missing or `None`: never expires. Update the
  docstring's coupon line.
- `coupon_rule(code, coupons)` refuses, checking in this order: an unknown code (as now), a bad `'expires'` value
  (`bad expiry date for coupon <code>: <value>`), an expired coupon (`coupon expired: <code>`), a used-up coupon
  (as now). Follow the project's conventions (see the system notes) for errors and dates.
- `set_coupon`, `totals` and `pay` already go through `coupon_rule`; nothing else needs to change.

Must cover: the new rules, including their boundaries and what happens to an order whose coupon expires while it
is still open.
