import unittest

from shop.catalog import Catalog
from shop.models import Product


def catalog():
    c = Catalog()
    c.add(Product('TEA', 'Green tea', 450, 'reduced'), 10)
    c.add(Product('MUG', 'Mug', 1200), 2)
    return c


class Stock(unittest.TestCase):
    def test_add_and_get(self):
        c = catalog()
        self.assertEqual(c.get('TEA').price, 450)
        with self.assertRaises(ValueError) as e:
            c.add(Product('TEA', 'Again', 1))
        self.assertEqual(str(e.exception), 'product exists: TEA')
        with self.assertRaises(ValueError) as e:
            c.get('NOPE')
        self.assertEqual(str(e.exception), 'unknown product: NOPE')

    def test_reserve_and_available(self):
        c = catalog()
        c.reserve('MUG', 2)
        self.assertEqual(c.available('MUG'), 0)
        with self.assertRaises(ValueError) as e:
            c.reserve('MUG', 1)
        self.assertEqual(str(e.exception), 'not enough MUG: available 0, need 1')

    def test_qty_must_be_positive_int(self):
        c = catalog()
        for bad in (0, -1, 1.5, True):
            with self.assertRaises(ValueError):
                c.reserve('TEA', bad)
        self.assertEqual(c.reserved['TEA'], 0)

    def test_release_commit_restock(self):
        c = catalog()
        c.reserve('TEA', 4)
        c.release('TEA', 1)
        c.commit('TEA', 3)
        self.assertEqual((c.stock['TEA'], c.reserved['TEA']), (7, 0))
        with self.assertRaises(ValueError) as e:
            c.release('TEA', 1)
        self.assertEqual(str(e.exception), 'cannot release 1 TEA: only 0 reserved')
        c.restock('TEA', 5)
        self.assertEqual(c.available('TEA'), 12)


if __name__ == '__main__':
    unittest.main()
