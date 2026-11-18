import os
import tempfile
import unittest

from shop import clock, storage
from shop.catalog import Catalog
from shop.models import Product
from shop.orders import OrderBook


def paid(lines=(('TEA', 3), ('MUG', 2)), rules=None):
    clock.set_today('2026-04-01')
    c = Catalog()
    c.add(Product('TEA', 'Green tea', 450, 'reduced'), 20)
    c.add(Product('MUG', 'Mug', 1200), 10)
    c.add(Product('JAM', 'Jam', 500, 'reduced'), 10)
    book = OrderBook(c, rules)
    o = book.create('ann')
    for sku, qty in lines:
        book.add_line(o.id, sku, qty)
    book.pay(o.id, book.totals(o.id)['gross'])
    return c, book, o


class Amounts(unittest.TestCase):
    def tearDown(self):
        clock.set_today(None)

    def test_one_unit(self):
        c, book, o = paid()
        self.assertEqual(o.paid_total, 4376)                         # net 3750 + VAT 122 + 504
        self.assertEqual(book.refund_lines(o.id, {'TEA': 1}), 491)       # 450 + VAT 40.5 -> 41
        self.assertEqual((o.refunded, o.status, o.refunded_units, o.refunded_net), (491, 'paid', {'TEA': 1}, {'TEA': 450}))

    def test_vat_per_rate_on_summed_net(self):
        _, book, o = paid()
        self.assertEqual(book.refund_lines(o.id, {'TEA': 1, 'MUG': 1}), 450 + 1200 + 41 + 252)

    def test_discounted_line(self):
        _, book, o = paid([('MUG', 2)], [{'type': 'percent', 'percent': 50}])
        self.assertEqual(book.refund_lines(o.id, {'MUG': 1}), 600 + 126)

    def test_last_units_take_the_rest(self):
        _, book, o = paid([('JAM', 3), ('TEA', 1)], [{'type': 'bxgy', 'sku': 'TEA', 'buy': 9, 'free': 1},
                                                    {'type': 'fixed', 'amount': 150}])
        jam_net = o.line('JAM').net
        self.assertEqual(jam_net, 1500 - 115)                 # 150 split 1500:450 -> 115 / 35
        book.refund_lines(o.id, {'JAM': 1})
        book.refund_lines(o.id, {'JAM': 1})
        self.assertEqual(o.refunded_net['JAM'], 2 * 462)      # 1385 / 3 = 461.67 -> 462
        book.refund_lines(o.id, {'JAM': 1})
        self.assertEqual(o.refunded_net['JAM'], jam_net)

    def test_everything_in_parts_adds_up(self):
        c, book, o = paid()
        total = book.refund_lines(o.id, {'TEA': 1}) + book.refund_lines(o.id, {'MUG': 2, 'TEA': 1})
        self.assertEqual(o.status, 'paid')
        total += book.refund_lines(o.id, {'TEA': 1})
        self.assertEqual((total, o.refunded, o.status), (o.paid_total, o.paid_total, 'refunded'))
        self.assertEqual(c.reserved, {'TEA': 0, 'MUG': 0, 'JAM': 0})


class Checks(unittest.TestCase):
    def tearDown(self):
        clock.set_today(None)

    def refused(self, book, o, items, msg):
        with self.assertRaises(ValueError) as e:
            book.refund_lines(o.id, items)
        self.assertEqual(str(e.exception), msg)

    def test_every_check_changes_nothing(self):
        c, book, o = paid()
        n = book.create('bob')
        self.refused(book, n, {'TEA': 1}, 'order O0002 is new')
        self.refused(book, o, {}, 'nothing to refund')
        self.refused(book, o, {'TEA': 1, 'NOPE': 1}, 'NOPE not in order O0001')
        self.refused(book, o, {'TEA': 1, 'MUG': 0}, 'qty must be a positive int')
        self.refused(book, o, {'TEA': 1, 'MUG': 3}, 'cannot refund 3 MUG: only 2 left')
        self.assertEqual((o.refunded, o.refunded_units, c.reserved['TEA'], len(o.events)), (0, {}, 3, 4))
        book.refund_lines(o.id, {'TEA': 1})
        self.refused(book, o, {'TEA': 3}, 'cannot refund 3 TEA: only 2 left')

    def test_after_full_refund(self):
        _, book, o = paid()
        book.refund(o.id)
        self.refused(book, o, {'TEA': 1}, 'order O0001 is refunded')


class Stock(unittest.TestCase):
    def tearDown(self):
        clock.set_today(None)

    def test_shipped_restocks(self):
        c, book, o = paid()
        book.ship(o.id)
        book.refund_lines(o.id, {'TEA': 2})
        self.assertEqual((c.stock['TEA'], o.status, o.events[-1]), (19, 'shipped', ('2026-04-01', 'refunded €9.81')))

    def test_paid_releases_and_ship_sends_the_rest(self):
        c, book, o = paid()
        book.refund_lines(o.id, {'TEA': 1, 'MUG': 2})
        self.assertEqual((c.reserved['TEA'], c.reserved['MUG']), (2, 0))
        book.ship(o.id)
        self.assertEqual((c.stock['TEA'], c.stock['MUG'], c.reserved['TEA']), (18, 10, 0))

    def test_full_refund_and_cancel_after_partial(self):
        c, book, o = paid()
        part = book.refund_lines(o.id, {'TEA': 1})
        self.assertEqual(book.refund(o.id), o.paid_total - part)
        self.assertEqual((c.reserved['TEA'], c.reserved['MUG'], o.status), (0, 0, 'refunded'))
        c, book, o = paid()
        part = book.refund_lines(o.id, {'MUG': 1})
        self.assertEqual(book.cancel(o.id), o.paid_total - part)
        self.assertEqual((c.reserved['MUG'], o.refunded), (0, o.paid_total))


class Saving(unittest.TestCase):
    def tearDown(self):
        clock.set_today(None)

    def test_round_trip(self):
        c, book, o = paid()
        book.refund_lines(o.id, {'TEA': 1})
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, 's.json')
            storage.save(path, c, book)
            c2, book2 = storage.load(path)
        o2 = book2.orders[o.id]
        self.assertEqual((o2.refunded_units, o2.refunded_net, o2.refunded), ({'TEA': 1}, {'TEA': 450}, 491))
        self.assertEqual(storage.dump(c2, book2)['version'], 3)
        self.assertEqual(book2.refund_lines(o.id, {'TEA': 2, 'MUG': 2}), o.paid_total - 491)

    def test_upgrades(self):
        v2 = {'version': 2, 'products': [], 'stock': {}, 'reserved': {}, 'coupons': {}, 'next_number': 2,
              'orders': [{'id': 'O0001', 'customer': 'ann', 'created': '2026-01-01', 'status': 'paid', 'paid_total': 5,
                          'lines': []}]}
        before = repr(v2)
        c2, book2 = storage.build(v2)
        o2 = book2.orders['O0001']
        self.assertEqual((storage.dump(c2, book2)['version'], o2.refunded_units, o2.refunded_net), (3, {}, {}))
        self.assertEqual(repr(v2), before)
        v1 = {'stock': {}, 'orders': [{'id': 'O0001', 'customer': 'ann', 'created': '2025-01-01', 'status': 'paid',
                                       'total': 7, 'lines': []}]}
        _, book = storage.build(v1)
        self.assertEqual((book.orders['O0001'].paid_total, book.orders['O0001'].refunded_units), (7, {}))
        with self.assertRaises(ValueError) as e:
            storage.build({'version': 4})
        self.assertEqual(str(e.exception), 'unsupported file version: 4 (this program reads 1 to 3)')


if __name__ == '__main__':
    unittest.main()
