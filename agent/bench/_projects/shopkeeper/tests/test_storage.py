import json
import os
import tempfile
import unittest

from shop import clock, storage
from shop.catalog import Catalog
from shop.models import Product
from shop.orders import OrderBook


def small_shop():
    clock.set_today('2026-03-03')
    c = Catalog()
    c.add(Product('TEA', 'Green tea', 450, 'reduced'), 10)
    c.add(Product('MUG', 'Mug', 1200), 5)
    book = OrderBook(c, coupons={'TEN': {'rule': {'type': 'percent', 'percent': 10}, 'max_uses': 5, 'uses': 0}})
    o = book.create('ann')
    book.add_line(o.id, 'TEA', 2)
    book.set_coupon(o.id, 'TEN')
    book.pay(o.id, book.totals(o.id)['gross'])
    book.add_line(book.create('bob').id, 'MUG', 1)
    return c, book


class Storage(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.path = os.path.join(self.dir.name, 'shop.json')

    def tearDown(self):
        clock.set_today(None)
        self.dir.cleanup()

    def test_round_trip(self):
        c, book = small_shop()
        storage.save(self.path, c, book)
        c2, book2 = storage.load(self.path)
        self.assertEqual(storage.dump(c2, book2), storage.dump(c, book))
        self.assertEqual(book2.orders['O0001'].lines[0].discount, 90)
        self.assertEqual(book2.next_number, 3)
        self.assertEqual(os.listdir(self.dir.name), ['shop.json'])     # no temp file left

    def test_loaded_book_keeps_working(self):
        c, book = small_shop()
        storage.save(self.path, c, book)
        c2, book2 = storage.load(self.path)
        book2.ship('O0001')
        self.assertEqual(c2.stock['TEA'], 8)
        self.assertEqual(book2.create('cy').id, 'O0003')

    def test_version_1_is_upgraded(self):
        old = {'products': [{'sku': 'TEA', 'name': 'Green tea', 'price': 450, 'vat': 'reduced', 'active': True}],
               'stock': {'TEA': 4},
               'orders': [{'id': 'O0001', 'customer': 'ann', 'created': '2025-12-01', 'status': 'paid', 'total': 981,
                           'lines': [{'sku': 'TEA', 'qty': 2, 'unit_price': 450, 'vat': 'reduced', 'discount': 0}]}],
               'coupons': {}, 'next_number': 2}
        with open(self.path, 'w', encoding='utf-8') as f:
            json.dump(old, f)
        c, book = storage.load(self.path)
        self.assertEqual((c.reserved['TEA'], book.orders['O0001'].paid_total), (0, 981))
        self.assertEqual(storage.dump(c, book)['version'], storage.VERSION)
        self.assertIn('total', old['orders'][0])        # the input dict is not changed

    def test_unknown_version_refused(self):
        with self.assertRaises(ValueError) as e:
            storage.build({'version': 99})
        self.assertEqual(str(e.exception), f'unsupported file version: 99 (this program reads 1 to {storage.VERSION})')


if __name__ == '__main__':
    unittest.main()
