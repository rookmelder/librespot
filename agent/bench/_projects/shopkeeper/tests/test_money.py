import unittest

from shop import money


class Rounding(unittest.TestCase):
    def test_round_half_up(self):
        self.assertEqual(money.round_half_up(250, 100), 3)
        self.assertEqual(money.round_half_up(249, 100), 2)
        self.assertEqual(money.round_half_up(0, 7), 0)
        with self.assertRaises(ValueError):
            money.round_half_up(1, 0)

    def test_percent_and_vat(self):
        self.assertEqual(money.percent_of(1000, 21), 210)
        self.assertEqual(money.vat_for(450, 'reduced'), 41)       # 40.5 -> 41
        self.assertEqual(money.vat_for(999, 'zero'), 0)
        with self.assertRaises(ValueError) as e:
            money.vat_for(100, 'luxury')
        self.assertEqual(str(e.exception), 'unknown VAT rate: luxury')


class Split(unittest.TestCase):
    def test_parts_add_up(self):
        self.assertEqual(money.split(100, [1, 1, 1]), [34, 33, 33])
        self.assertEqual(money.split(10, [3, 1]), [8, 2])          # 7.5 / 2.5: tie goes to the earlier part
        self.assertEqual(money.split(-100, [1, 1, 1]), [-34, -33, -33])
        self.assertEqual(sum(money.split(997, [5, 11, 13, 2])), 997)

    def test_zero_weights_and_errors(self):
        self.assertEqual(money.split(50, [0, 0]), [50, 0])
        for bad in ([], [1, -1]):
            with self.assertRaises(ValueError):
                money.split(10, bad)


class Text(unittest.TestCase):
    def test_fmt(self):
        self.assertEqual(money.fmt(1234), '€12.34')
        self.assertEqual(money.fmt(-5), '-€0.05')
        self.assertEqual(money.fmt(0), '€0.00')

    def test_parse(self):
        for text, cents in (('12.34', 1234), ('12,3', 1230), ('7', 700), ('€ 0.05', 5), ('-3.10', -310)):
            self.assertEqual(money.parse(text), cents, text)
        for bad in ('', 'abc', '1.234', '1.2.3'):
            with self.assertRaises(ValueError) as e:
                money.parse(bad)
            self.assertEqual(str(e.exception), f'bad amount: {bad}')


if __name__ == '__main__':
    unittest.main()
