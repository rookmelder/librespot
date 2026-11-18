Root cause: shop/money.py:{split_leftover_to_biggest_weight}. `split()` gives the cents left over to the parts with
the biggest weights, but its docstring (and the invoice totals) need them to go to the largest remainders.
For the order in the failing test the fixed €7.00 is split over nets 1500 (JAM) and 5000 (CARD): shares 161 and 538,
remainders 3500 and 2000, so the left-over cent belongs to JAM (162, 538); the code gives it to CARD (161, 539), so the
reduced-rate net is one cent higher and its VAT rounds up.
Not the cause: shop/discounts.py `_fixed` (passes the right weights), shop/invoices.py `vat_summary` (correct per rate).
