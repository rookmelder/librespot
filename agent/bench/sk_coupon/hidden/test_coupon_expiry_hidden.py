import datetime
import unittest

from shop import clock
from shop.catalog import Catalog
from shop.discounts import coupon_rule
from shop.models import Product
from shop.orders import OrderBook

RULE = {'type': 'percent', 'percent': 10}


def coupons(**extra):
    return {'C': dict({'rule': RULE, 'max_uses': None, 'uses': 0}, **extra)}


class Expiry(unittest.TestCase):
    def tearDown(self):
        clock.set_today(None)

    def check(self, msg, code, cps):
        with self.assertRaises(ValueError) as e:
            coupon_rule(code, cps)
        self.assertEqual(str(e.exception), msg)

    def test_valid_on_last_day(self):
        clock.set_today('2030-01-10')
        self.assertEqual(coupon_rule('C', coupons(expires='2030-01-10')), RULE)

    def test_expired_day_after(self):
        clock.set_today('2030-01-11')
        self.check('coupon expired: C', 'C', coupons(expires='2030-01-10'))

    def test_uses_the_shop_clock(self):
        clock.set_today('2020-06-01')                     # long before the real date
        self.assertEqual(coupon_rule('C', coupons(expires='2021-01-01')), RULE)

    def test_no_expiry(self):
        clock.set_today('2099-12-31')
        self.assertEqual(coupon_rule('C', coupons()), RULE)
        self.assertEqual(coupon_rule('C', coupons(expires=None)), RULE)

    def test_expired_before_used_up(self):
        clock.set_today('2030-02-01')
        self.check('coupon expired: C', 'C', coupons(expires='2030-01-10', max_uses=1, uses=1))

    def test_unknown_first(self):
        clock.set_today('2030-02-01')
        self.check('unknown coupon: X', 'X', coupons(expires='bad'))

    def test_bad_expiry_values(self):
        clock.set_today('2030-01-01')
        for bad in ('2030-1-05', '05-01-2030', '2030-02-30', '20300110', '', 20300110, '2030-01-10T00:00',
                    ' 2030-01-10', '2030-01-10'+chr(10), datetime.date(2030, 1, 10)):
            self.check(f'bad expiry date for coupon C: {bad}', 'C', coupons(expires=bad))

    def test_bad_expiry_before_used_up(self):
        clock.set_today('2030-01-01')
        self.check('bad expiry date for coupon C: tomorrow', 'C', coupons(expires='tomorrow', max_uses=0))


class InOrders(unittest.TestCase):
    def book(self, day):
        clock.set_today(day)
        c = Catalog()
        c.add(Product('MUG', 'Mug', 1200), 5)
        book = OrderBook(c, coupons=coupons(expires='2030-01-10', max_uses=5))
        o = book.create('ann')
        book.add_line(o.id, 'MUG', 1)
        return book, o

    def tearDown(self):
        clock.set_today(None)

    def test_set_coupon_refuses_expired(self):
        book, o = self.book('2030-01-11')
        with self.assertRaises(ValueError) as e:
            book.set_coupon(o.id, 'C')
        self.assertEqual(str(e.exception), 'coupon expired: C')
        self.assertIsNone(o.coupon)

    def test_expires_after_set(self):
        book, o = self.book('2030-01-10')
        book.set_coupon(o.id, 'C')
        self.assertEqual(book.totals(o.id)['discount'], 120)
        clock.set_today('2030-01-11')
        with self.assertRaises(ValueError) as e:
            book.totals(o.id)
        self.assertEqual(str(e.exception), 'coupon expired: C')
        with self.assertRaises(ValueError):
            book.pay(o.id, 1307)
        self.assertEqual((o.status, book.coupons['C']['uses']), ('new', 0))
        book.set_coupon(o.id, None)
        self.assertEqual(book.totals(o.id)['discount'], 0)


if __name__ == '__main__':
    unittest.main()
