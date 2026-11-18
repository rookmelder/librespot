import os
import tempfile
import unittest

from shop import clock, storage
from shop.catalog import Catalog
from shop.cli import run
from shop.models import Product
from shop.orders import OrderBook


class Cli(unittest.TestCase):
    def setUp(self):
        clock.set_today('2026-03-04')
        self.dir = tempfile.TemporaryDirectory()
        self.path = os.path.join(self.dir.name, 'shop.json')
        c = Catalog()
        c.add(Product('TEA', 'Green tea', 450, 'reduced'), 10)
        storage.save(self.path, c, OrderBook(c))
        self.lines = []

    def tearDown(self):
        clock.set_today(None)
        self.dir.cleanup()

    def cmd(self, *args):
        return run(self.path, args[0], list(args[1:]), out=self.lines.append)

    def test_order_flow_is_saved(self):
        self.assertEqual(self.cmd('new', 'ann'), 0)
        self.assertEqual(self.lines[-1], 'O0001')
        self.cmd('add', 'O0001', 'TEA', '2')
        self.cmd('total', 'O0001')
        self.assertEqual(self.lines[-1], 'net €9.00  VAT €0.81  total €9.81  (discount €0.00)')
        self.assertEqual(self.cmd('pay', 'O0001', '9.81'), 0)
        self.cmd('sales')
        self.assertEqual(self.lines[-1], '2026-03-04  €9.81')
        _, book = storage.load(self.path)
        self.assertEqual(book.orders['O0001'].status, 'paid')

    def test_errors(self):
        self.assertEqual(self.cmd('fly'), 2)
        self.assertEqual(self.lines[-1], 'unknown command: fly')
        self.assertEqual(self.cmd('add', 'O0001'), 2)
        self.assertEqual(self.lines[-1], 'add takes 3 argument(s), got 1')
        self.cmd('new', 'ann')
        self.assertEqual(self.cmd('add', 'O0001', 'TEA', 'two'), 2)
        self.assertEqual(self.lines[-1], 'error: not a whole number: two')


if __name__ == '__main__':
    unittest.main()
