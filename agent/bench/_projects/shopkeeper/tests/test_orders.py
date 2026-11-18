import unittest

from shop import clock
from shop.catalog import Catalog
from shop.models import Product
from shop.orders import OrderBook


def shop(rules=None, coupons=None):
    clock.set_today('2026-03-01')
    c = Catalog()
    c.add(Product('TEA', 'Green tea', 450, 'reduced'), 10)
    c.add(Product('MUG', 'Mug', 1200), 5)
    c.add(Product('OLD', 'Old mug', 900, active=False), 5)
    return c, OrderBook(c, rules, coupons)


class Lines(unittest.TestCase):
    def tearDown(self):
        clock.set_today(None)

    def test_create_and_ids(self):
        _, book = shop()
        a, b = book.create('ann'), book.create(' bob ')
        self.assertEqual((a.id, b.id, b.customer, a.created), ('O0001', 'O0002', 'bob', '2026-03-01'))
        with self.assertRaises(ValueError):
            book.create('  ')

    def test_add_merges_and_reserves(self):
        c, book = shop()
        o = book.create('ann')
        book.add_line(o.id, 'TEA', 2)
        book.add_line(o.id, 'TEA', 3)
        self.assertEqual([(l.sku, l.qty) for l in o.lines], [('TEA', 5)])
        self.assertEqual(c.reserved['TEA'], 5)

    def test_add_refusals(self):
        c, book = shop()
        o = book.create('ann')
        with self.assertRaises(ValueError) as e:
            book.add_line(o.id, 'OLD', 1)
        self.assertEqual(str(e.exception), 'product not for sale: OLD')
        with self.assertRaises(ValueError):
            book.add_line(o.id, 'MUG', 6)
        self.assertEqual(o.lines, [])

    def test_remove_line_releases(self):
        c, book = shop()
        o = book.create('ann')
        book.add_line(o.id, 'MUG', 2)
        book.remove_line(o.id, 'MUG')
        self.assertEqual((o.lines, c.reserved['MUG']), ([], 0))


class Flow(unittest.TestCase):
    def tearDown(self):
        clock.set_today(None)

    def paid(self, rules=None, coupons=None, coupon=None):
        c, book = shop(rules, coupons)
        o = book.create('ann')
        book.add_line(o.id, 'TEA', 2)
        book.add_line(o.id, 'MUG', 1)
        if coupon:
            book.set_coupon(o.id, coupon)
        book.pay(o.id, book.totals(o.id)['gross'])
        return c, book, o

    def test_totals_vat_per_rate(self):
        _, book = shop()
        o = book.create('ann')
        book.add_line(o.id, 'TEA', 2)
        book.add_line(o.id, 'MUG', 1)
        self.assertEqual(book.totals(o.id), {'net': 2100, 'vat': 81 + 252, 'gross': 2433, 'discount': 0})

    def test_pay_checks_amount_and_counts_coupon(self):
        coupons = {'TEN': {'rule': {'type': 'percent', 'percent': 10}, 'max_uses': 2, 'uses': 0}}
        c, book = shop(coupons=coupons)
        o = book.create('ann')
        with self.assertRaises(ValueError) as e:
            book.pay(o.id, 0)
        self.assertEqual(str(e.exception), 'order O0001 has no lines')
        book.add_line(o.id, 'MUG', 1)
        book.set_coupon(o.id, 'TEN')
        with self.assertRaises(ValueError) as e:
            book.pay(o.id, 1452)
        self.assertEqual(str(e.exception), 'payment €14.52 does not match total €13.07')
        book.pay(o.id, 1307)
        self.assertEqual((o.status, o.paid_total, coupons['TEN']['uses']), ('paid', 1307, 1))

    def test_discounts_frozen_after_pay(self):
        rules = [{'type': 'percent', 'percent': 10}]
        _, book, o = self.paid(rules)
        before = book.totals(o.id)
        rules.append({'type': 'fixed', 'amount': 500})
        self.assertEqual(book.totals(o.id), before)

    def test_ship_commits_stock(self):
        c, book, o = self.paid()
        book.ship(o.id)
        self.assertEqual((o.status, c.stock['TEA'], c.reserved['TEA']), ('shipped', 8, 0))
        with self.assertRaises(ValueError) as e:
            book.ship(o.id)
        self.assertEqual(str(e.exception), 'order O0001 is shipped')

    def test_cancel(self):
        c, book, o = self.paid()
        self.assertEqual(book.cancel(o.id), o.paid_total)
        self.assertEqual((o.status, o.refunded, c.reserved['MUG']), ('cancelled', o.paid_total, 0))
        _, book2 = shop()
        n = book2.create('bob')
        self.assertEqual(book2.cancel(n.id), 0)

    def test_refund_shipped_restocks(self):
        c, book, o = self.paid()
        book.ship(o.id)
        self.assertEqual(book.refund(o.id), o.paid_total)
        self.assertEqual((o.status, c.stock['TEA'], c.stock['MUG']), ('refunded', 10, 5))

    def test_refund_paid_releases_and_new_refused(self):
        c, book, o = self.paid()
        book.refund(o.id)
        self.assertEqual((c.reserved['TEA'], c.stock['TEA']), (0, 10))
        n = book.create('bob')
        with self.assertRaises(ValueError) as e:
            book.refund(n.id)
        self.assertEqual(str(e.exception), 'order O0002 is new')

    def test_find_and_events(self):
        _, book, o = self.paid()
        book.create('bob')
        self.assertEqual([x.id for x in book.find(status='paid')], ['O0001'])
        self.assertEqual([x.id for x in book.find(customer='bob')], ['O0002'])
        self.assertEqual(o.events[0], ('2026-03-01', 'created'))
        self.assertEqual(o.events[-1], ('2026-03-01', 'paid €24.33'))


if __name__ == '__main__':
    unittest.main()
