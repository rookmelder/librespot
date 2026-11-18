1. NO: refund() lost its status check (shop/orders.py:{refund_lost_status_check}): a new or cancelled order can now be refunded; the test that proved it was deleted.
2. NO: sales_between leaves out the end day (shop/reports.py:{sales_between_excludes_end_day}, `day < last`), against its docstring.
3. NO: search mixes `and`/`or` (shop/orders.py:{search_precedence_other_customers}): with statuses it returns other customers' orders.
4. NO: find() sorts ids as text (shop/orders.py:{find_sorts_ids_as_text}): after O9999 comes O10000, which sorts first.
5. NO: `between` passes the command line dates unchecked (shop/cli.py:{between_dates_not_checked}): `2026-3-5` compares wrongly as text and silently gives wrong sales.

Problems:
- shop/orders.py:{refund_lost_status_check}: fix: self._need(order, 'paid', 'shipped') before _give_back.
- shop/reports.py:{sales_between_excludes_end_day}: fix: day <= last.
- shop/orders.py:{search_precedence_other_customers}: fix: parentheses around the or.
- shop/orders.py:{find_sorts_ids_as_text}: fix: sort by the number, or keep insertion order.
- shop/cli.py:{between_dates_not_checked}: fix: check both dates are YYYY-MM-DD (datetime.date.fromisoformat) first.
