import unittest

from shop import reports
from shop.models import Order, OrderLine


def o(oid, created, status, paid=0, refunded=0, lines=()):
    order = Order(oid, 'ann', created, status=status, paid_total=paid, refunded=refunded)
    order.lines = [OrderLine(sku, qty, 100, 'standard') for sku, qty in lines]
    return order


ORDERS = [o('1', '2026-03-02', 'paid', 1000, lines=[('TEA', 2)]),
          o('2', '2026-03-01', 'shipped', 500, lines=[('MUG', 2), ('TEA', 1)]),
          o('3', '2026-03-02', 'refunded', 700, 700, lines=[('JAM', 9)]),
          o('4', '2026-03-02', 'cancelled', 300, 300, lines=[('JAM', 9)]),
          o('5', '2026-03-03', 'new', lines=[('JAM', 9)])]


class Reports(unittest.TestCase):
    def test_sales_by_day(self):
        self.assertEqual(reports.sales_by_day(ORDERS), {'2026-03-01': 500, '2026-03-02': 1000})
        self.assertEqual(list(reports.sales_by_day(ORDERS)), ['2026-03-01', '2026-03-02'])

    def test_top_products(self):
        self.assertEqual(reports.top_products(ORDERS), [('TEA', 3), ('MUG', 2)])
        self.assertEqual(reports.top_products(ORDERS, 1), [('TEA', 3)])

    def test_csv(self):
        self.assertEqual(reports.to_csv(['day', 'cents'], [['2026-03-01', 500], ['a,b', 1]]),
                         'day,cents\n2026-03-01,500\n"a,b",1\n')


if __name__ == '__main__':
    unittest.main()
