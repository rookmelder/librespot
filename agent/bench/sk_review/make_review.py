"""Write start/: the shopkeeper project with a pull request applied that has 5 planted bugs (see hidden/answers.json).
Run again after changing the shopkeeper project:  python agent/bench/sk_review/make_review.py
Every replacement must match exactly once; the project's tests must still pass with the change (checked here)."""
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
P = HERE.parent / '_projects' / 'shopkeeper'
OUT = HERE / 'start'


def rep(t, a, b):
    assert t.count(a) == 1, a[:70]
    return t.replace(a, b)


def orders():
    o = (P / 'shop/orders.py').read_text(encoding='utf-8')
    o = rep(o, """        self._log(order, 'created')
        return order
""", """        self._log(order, 'created')
        return order

    def search(self, customer, statuses=None):
        \"\"\"The orders of one customer, oldest first; with statuses, only orders in one of them.\"\"\"
        statuses = statuses or []
        return [o for o in self.find() if o.customer == customer and not statuses or o.status in statuses]
""")
    o = rep(o, """        order = self.get(order_id)
        self._need(order, 'new', 'paid')
        for line in order.lines:
            self.catalog.release(line.sku, line.qty)
        amount = 0
        if order.status == 'paid':
            amount = order.paid_total - order.refunded
            order.refunded = order.paid_total
        order.status = 'cancelled'""", """        order = self.get(order_id)
        self._need(order, 'new', 'paid')
        amount = self._give_back(order, restock=False)
        order.status = 'cancelled'""")
    o = rep(o, """        order = self.get(order_id)
        self._need(order, 'paid', 'shipped')
        for line in order.lines:
            if order.status == 'shipped':
                self.catalog.restock(line.sku, line.qty)
            else:
                self.catalog.release(line.sku, line.qty)
        amount = order.paid_total - order.refunded
        order.refunded = order.paid_total
        order.status = 'refunded'""", """        order = self.get(order_id)
        amount = self._give_back(order, restock=order.status == 'shipped')
        order.status = 'refunded'""")
    o = rep(o, """    def find(self, customer=None, status=None):
        return [o for o in self.orders.values()
                if (customer is None or o.customer == customer) and (status is None or o.status == status)]""",
            """    def _give_back(self, order, restock):
        \"\"\"Every line's units back to stock (restock: shipped goods) or out of the reservations.
        Marks the whole paid amount as refunded -> what was still to refund (0 for an unpaid order).\"\"\"
        for line in order.lines:
            if restock:
                self.catalog.restock(line.sku, line.qty)
            else:
                self.catalog.release(line.sku, line.qty)
        amount = order.paid_total - order.refunded
        order.refunded = order.paid_total
        return amount

    def find(self, customer=None, status=None):
        \"\"\"Matching orders, oldest first.\"\"\"
        found = [o for o in self.orders.values()
                 if (customer is None or o.customer == customer) and (status is None or o.status == status)]
        return sorted(found, key=lambda o: o.id)""")
    return o


def reports():
    r = (P / 'shop/reports.py').read_text(encoding='utf-8')
    r = rep(r, """def to_csv(header, rows):""", """def sales_between(orders, start, end):
    \"\"\"Sales per day from start to end, both days included (ISO dates; the two may come in either order).
    Days without sales are left out. -> {ISO date: cents}, dates sorted.\"\"\"
    first, last = _in_order(start, end)
    return {day: cents for day, cents in sales_by_day(orders).items() if first <= day < last}


def to_csv(header, rows):""")
    return r.rstrip('\n') + """


def _in_order(a, b):
    \"\"\"Two ISO dates, earliest first. ISO dates sort as text, so no parsing is needed.\"\"\"
    if b < a:
        a, b = b, a
    return a, b
"""


def cli():
    c = (P / 'shop/cli.py').read_text(encoding='utf-8')
    c = rep(c, """  sales                         sales per day
""", """  sales                         sales per day
  between <from> <to>           sales per day from one date to another (YYYY-MM-DD)
  search <customer>             a customer's orders, oldest first
""")
    c = rep(c, """        'sales': 0}""", """        'sales': 0, 'between': 2, 'search': 1}""")
    c = rep(c, """            out(f'{day}  {money.fmt(cents)}')
""", """            out(f'{day}  {money.fmt(cents)}')
    elif command == 'between':
        for day, cents in reports.sales_between(book.orders.values(), args[0], args[1]).items():
            out(f'{day}  {money.fmt(cents)}')
    elif command == 'search':
        for o in book.search(args[0]):
            out(f'{o.id}  {o.created}  {o.status:<9} {money.fmt(o.paid_total - o.refunded):>10}')
""")
    return c


def tests():
    t = (P / 'tests/test_orders.py').read_text(encoding='utf-8')
    t = rep(t, """    def test_refund_paid_releases_and_new_refused(self):
        c, book, o = self.paid()
        book.refund(o.id)
        self.assertEqual((c.reserved['TEA'], c.stock['TEA']), (0, 10))
        n = book.create('bob')
        with self.assertRaises(ValueError) as e:
            book.refund(n.id)
        self.assertEqual(str(e.exception), 'order O0002 is new')
""", """    def test_refund_paid_releases(self):
        c, book, o = self.paid()
        book.refund(o.id)
        self.assertEqual((c.reserved['TEA'], c.stock['TEA']), (0, 10))

    def test_search(self):
        _, book, o = self.paid()
        book.create('bob')
        self.assertEqual([x.id for x in book.search('ann')], ['O0001'])
        self.assertEqual([x.id for x in book.search('ann', ['paid'])], ['O0001'])
""")
    r = (P / 'tests/test_reports.py').read_text(encoding='utf-8')
    r = rep(r, """    def test_top_products(self):""", """    def test_sales_between(self):
        self.assertEqual(reports.sales_between(ORDERS, '2026-03-01', '2026-03-03'), {'2026-03-01': 500, '2026-03-02': 1000})
        self.assertEqual(reports.sales_between(ORDERS, '2026-03-03', '2026-03-02'), {'2026-03-02': 1000})

    def test_top_products(self):""")
    return t, r


def main():
    if OUT.exists():
        shutil.rmtree(OUT)
    (OUT / 'shop').mkdir(parents=True)
    (OUT / 'tests').mkdir()
    t, r = tests()
    files = {'shop/orders.py': orders(), 'shop/reports.py': reports(), 'shop/cli.py': cli(),
             'tests/test_orders.py': t, 'tests/test_reports.py': r}
    for rel, text in files.items():
        (OUT / rel).write_text(text, encoding='utf-8', newline='\n')
    with tempfile.TemporaryDirectory() as tmp:
        shutil.copytree(P, tmp, dirs_exist_ok=True, ignore=shutil.ignore_patterns('__pycache__'))
        shutil.copytree(OUT, tmp, dirs_exist_ok=True)
        res = subprocess.run([sys.executable, '-B', '-m', 'unittest', 'discover', '-s', 'tests'], cwd=tmp,
                             capture_output=True, text=True, env=dict(os.environ, PYTHONDONTWRITEBYTECODE='1'))
        print(res.stderr.strip().splitlines()[-1])
        return res.returncode


if __name__ == '__main__':
    sys.exit(main())
