# invoices
> Invoices and credit notes; VAT per rate on the summed net, the same calculation as order totals.

Back to [the map](../README.md).

- File: `shop/invoices.py`. Tests: `tests/test_invoices.py`.
- `vat_summary(lines)` groups the net per VAT rate and rounds once per rate. `totals()` (used by
  `OrderBook.totals`) and `make_invoice()` both use it, so an invoice always equals the amount paid.
- `credit_note(invoice)` negates every line and recalculates VAT from the negated nets; this relies on
  `money.round_half_up` being symmetric around zero.
- `render(doc)` prints an invoice or credit note as text.
