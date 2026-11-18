import unittest

from inventory import Inventory


def stocked(**items):
    inv = Inventory()
    for name, qty in items.items():
        inv.add(name, qty)
    return inv


class Basics(unittest.TestCase):
    def test_add_accumulates(self):
        inv = stocked(nut=3)
        inv.add('nut', 4)
        self.assertEqual(inv.stock, {'nut': 7})
        self.assertEqual(inv.total(), 7)

    def test_remove_and_delete_at_zero(self):
        inv = stocked(nut=3, bolt=2)
        inv.remove('nut', 1)
        inv.remove('bolt', 2)
        self.assertEqual(inv.stock, {'nut': 2})

    def test_remove_too_much(self):
        inv = stocked(nut=3)
        with self.assertRaises(ValueError) as e:
            inv.remove('nut', 5)
        self.assertEqual(str(e.exception), 'not enough nut: have 3, need 5')
        self.assertEqual(inv.stock, {'nut': 3})

    def test_remove_unknown_item(self):
        with self.assertRaises(ValueError) as e:
            Inventory().remove('gear', 1)
        self.assertEqual(str(e.exception), 'not enough gear: have 0, need 1')

    def test_qty_must_be_positive(self):
        inv = stocked(nut=3)
        for call in (lambda: inv.add('nut', 0), lambda: inv.add('new', -1), lambda: inv.remove('nut', 0), lambda: inv.remove('gone', -2)):
            with self.assertRaises(ValueError) as e:
                call()
            self.assertEqual(str(e.exception), 'qty must be positive')
        self.assertEqual(inv.stock, {'nut': 3})


class Reserve(unittest.TestCase):
    def test_ok(self):
        inv = stocked(nut=3, bolt=2, gear=1)
        self.assertEqual(inv.reserve({'nut': 2, 'gear': 1}), 3)
        self.assertEqual(inv.stock, {'nut': 1, 'bolt': 2})

    def test_empty_order(self):
        inv = stocked(nut=3)
        self.assertEqual(inv.reserve({}), 0)
        self.assertEqual(inv.stock, {'nut': 3})

    def test_all_or_nothing_first_alphabetical_failure(self):
        inv = stocked(nut=3, bolt=2)
        with self.assertRaises(ValueError) as e:
            inv.reserve({'nut': 9, 'bolt': 1, 'axle': 1, 'cog': 0})
        self.assertEqual(str(e.exception), 'not enough axle: have 0, need 1')
        self.assertEqual(inv.stock, {'nut': 3, 'bolt': 2})
        with self.assertRaises(ValueError) as e:
            inv.reserve({'nut': 1, 'bolt': 0})
        self.assertEqual(str(e.exception), 'qty must be positive')
        self.assertEqual(inv.stock, {'nut': 3, 'bolt': 2})


class Report(unittest.TestCase):
    def test_sorted_by_qty_then_name(self):
        inv = stocked(nut=3, bolt=5, axle=3, gear=1)
        self.assertEqual(inv.report(), ['bolt: 5', 'axle: 3', 'nut: 3', 'gear: 1'])

    def test_empty(self):
        self.assertEqual(Inventory().report(), [])


if __name__ == '__main__':
    unittest.main()
