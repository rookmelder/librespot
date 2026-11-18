import unittest

from shop.discounts import apply_discounts, check_rule, coupon_rule
from shop.models import Order, OrderLine


def order(*lines, coupon=None):
    o = Order('O0001', 'ann', '2026-03-01', coupon=coupon)
    o.lines = [OrderLine(sku, qty, price, vat) for sku, qty, price, vat in lines]
    return o


class Rules(unittest.TestCase):
    def test_bxgy(self):
        o = order(('TEA', 7, 450, 'reduced'))
        self.assertEqual(apply_discounts(o, [{'type': 'bxgy', 'sku': 'TEA', 'buy': 2, 'free': 1}]), 900)   # 2 full groups

    def test_percent_on_some_skus(self):
        o = order(('TEA', 2, 450, 'reduced'), ('MUG', 1, 1200, 'standard'))
        apply_discounts(o, [{'type': 'percent', 'percent': 10, 'skus': ['MUG']}])
        self.assertEqual([l.discount for l in o.lines], [0, 120])

    def test_percent_min_subtotal_uses_subtotal_before_discounts(self):
        o = order(('TEA', 3, 450, 'reduced'))           # subtotal 1350
        rules = [{'type': 'bxgy', 'sku': 'TEA', 'buy': 2, 'free': 1},
                 {'type': 'percent', 'percent': 10, 'min_subtotal': 1350}]
        self.assertEqual(apply_discounts(o, rules), 450 + 90)

    def test_order_of_types_not_list_order(self):
        o = order(('MUG', 1, 1000, 'standard'))
        rules = [{'type': 'fixed', 'amount': 100}, {'type': 'percent', 'percent': 50}]
        self.assertEqual(apply_discounts(o, rules), 600)    # 50% first, then 100 off

    def test_fixed_is_split_and_capped(self):
        o = order(('TEA', 1, 450, 'reduced'), ('MUG', 1, 1200, 'standard'))
        apply_discounts(o, [{'type': 'fixed', 'amount': 100}])
        self.assertEqual([l.discount for l in o.lines], [27, 73])
        o = order(('TEA', 1, 450, 'reduced'))
        self.assertEqual(apply_discounts(o, [{'type': 'fixed', 'amount': 5000}]), 450)
        o = order(('TEA', 1, 450, 'reduced'))
        self.assertEqual(apply_discounts(o, [{'type': 'fixed', 'amount': 100, 'min_net': 451}]), 0)

    def test_recalculated_from_scratch(self):
        o = order(('MUG', 1, 1200, 'standard'))
        rules = [{'type': 'percent', 'percent': 25}]
        apply_discounts(o, rules)
        self.assertEqual(apply_discounts(o, rules), 300)

    def test_bad_rules(self):
        for bad in ({'type': 'percent', 'percent': 0}, {'type': 'bxgy', 'sku': 'A', 'buy': 1}, {'type': 'magic'}, 'x'):
            with self.assertRaises(ValueError) as e:
                check_rule(bad)
            self.assertEqual(str(e.exception), f'bad rule: {bad}')


class Coupons(unittest.TestCase):
    def test_coupon_adds_its_rule(self):
        coupons = {'TEN': {'rule': {'type': 'percent', 'percent': 10}, 'max_uses': None, 'uses': 0}}
        o = order(('MUG', 1, 1200, 'standard'), coupon='TEN')
        self.assertEqual(apply_discounts(o, [], coupons), 120)

    def test_unknown_and_used_up(self):
        coupons = {'ONCE': {'rule': {'type': 'fixed', 'amount': 100}, 'max_uses': 1, 'uses': 1}}
        with self.assertRaises(ValueError) as e:
            coupon_rule('NOPE', coupons)
        self.assertEqual(str(e.exception), 'unknown coupon: NOPE')
        with self.assertRaises(ValueError) as e:
            coupon_rule('ONCE', coupons)
        self.assertEqual(str(e.exception), 'coupon used up: ONCE')


if __name__ == '__main__':
    unittest.main()
