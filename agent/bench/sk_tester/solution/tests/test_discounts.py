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



class Edges(unittest.TestCase):
    def test_bxgy_groups_of_buy_plus_free(self):
        rule = {'type': 'bxgy', 'sku': 'TEA', 'buy': 2, 'free': 2}
        self.assertEqual(apply_discounts(order(('TEA', 6, 450, 'reduced')), [rule]), 900)   # one group of 4: 2 free
        self.assertEqual(apply_discounts(order(('TEA', 8, 450, 'reduced')), [rule]), 1800)

    def test_bxgy_exact_sku_only(self):
        rule = {'type': 'bxgy', 'sku': 'TEA', 'buy': 1, 'free': 1}
        self.assertEqual(apply_discounts(order(('TEAPOT', 2, 2000, 'standard')), [rule]), 0)

    def test_percent_rounds_half_up_per_line(self):
        self.assertEqual(apply_discounts(order(('TEA', 1, 455, 'reduced')), [{'type': 'percent', 'percent': 10}]), 46)

    def test_stacked_percent_in_list_order(self):
        a = [{'type': 'percent', 'percent': 10}, {'type': 'percent', 'percent': 20}]
        self.assertEqual(apply_discounts(order(('TEA', 1, 455, 'reduced')), a), 128)
        self.assertEqual(apply_discounts(order(('TEA', 1, 455, 'reduced')), a[::-1]), 127)

    def test_empty_skus_means_no_line(self):
        o = order(('TEA', 1, 450, 'reduced'))
        self.assertEqual(apply_discounts(o, [{'type': 'percent', 'percent': 10, 'skus': []}]), 0)

    def test_fixed_at_exactly_min_net(self):
        o = order(('TEA', 1, 450, 'reduced'))
        self.assertEqual(apply_discounts(o, [{'type': 'fixed', 'amount': 100, 'min_net': 450}]), 100)

    def test_fixed_split_by_net_after_percent(self):
        o = order(('TEA', 1, 1000, 'reduced'), ('MUG', 1, 1000, 'standard'))
        apply_discounts(o, [{'type': 'percent', 'percent': 50, 'skus': ['TEA']}, {'type': 'fixed', 'amount': 300}])
        self.assertEqual([l.discount for l in o.lines], [600, 200])

    def test_limits_of_rules(self):
        check_rule({'type': 'percent', 'percent': 100})
        self.assertEqual(apply_discounts(order(('TEA', 1, 450, 'reduced')), [{'type': 'percent', 'percent': 100}]), 450)
        for bad in ({'type': 'bxgy', 'sku': 'A', 'buy': 1, 'free': 0}, {'type': 'fixed', 'amount': 0}):
            with self.assertRaises(ValueError) as e:
                check_rule(bad)
            self.assertEqual(str(e.exception), f'bad rule: {bad}')

    def test_recalculated_on_every_line(self):
        o = order(('TEA', 1, 1200, 'reduced'), ('MUG', 1, 1200, 'standard'))
        rules = [{'type': 'percent', 'percent': 25}]
        apply_discounts(o, rules)
        apply_discounts(o, rules)
        self.assertEqual([l.discount for l in o.lines], [300, 300])

    def test_shop_rules_not_changed_by_coupon(self):
        coupons = {'TEN': {'rule': {'type': 'fixed', 'amount': 10}, 'max_uses': None, 'uses': 0}}
        rules = [{'type': 'percent', 'percent': 10}]
        o = order(('MUG', 1, 1200, 'standard'), coupon='TEN')
        apply_discounts(o, rules, coupons)
        self.assertEqual(apply_discounts(o, rules, coupons), 130)
        self.assertEqual(rules, [{'type': 'percent', 'percent': 10}])

    def test_max_uses_zero_is_used_up(self):
        coupons = {'NONE': {'rule': {'type': 'fixed', 'amount': 100}, 'max_uses': 0, 'uses': 0}}
        with self.assertRaises(ValueError) as e:
            coupon_rule('NONE', coupons)
        self.assertEqual(str(e.exception), 'coupon used up: NONE')

    def test_coupon_rule_is_checked(self):
        bad = {'type': 'percent', 'percent': 0}
        coupons = {'ZERO': {'rule': bad, 'max_uses': None, 'uses': 0}}
        with self.assertRaises(ValueError) as e:
            apply_discounts(order(('MUG', 1, 1200, 'standard'), coupon='ZERO'), [], coupons)
        self.assertEqual(str(e.exception), f'bad rule: {bad}')

    def test_more_bad_rules(self):
        for bad in ({'type': 'fixed'}, {'type': 'percent'}, {'type': 'bxgy', 'sku': 'A', 'buy': 0, 'free': 1}):
            with self.assertRaises(ValueError) as e:
                check_rule(bad)
            self.assertEqual(str(e.exception), f'bad rule: {bad}')

    def test_no_coupon_book(self):
        with self.assertRaises(ValueError) as e:
            apply_discounts(order(('MUG', 1, 1200, 'standard'), coupon='X'), [], None)
        self.assertEqual(str(e.exception), 'unknown coupon: X')

    def test_uses_may_be_missing(self):
        coupons = {'NEW': {'rule': {'type': 'fixed', 'amount': 100}, 'max_uses': 1}}
        self.assertEqual(coupon_rule('NEW', coupons), {'type': 'fixed', 'amount': 100})

    def test_min_net_after_earlier_rules(self):
        o = order(('TEA', 1, 1000, 'reduced'))
        rules = [{'type': 'percent', 'percent': 50}, {'type': 'fixed', 'amount': 100, 'min_net': 600}]
        self.assertEqual(apply_discounts(o, rules), 500)


if __name__ == '__main__':
    unittest.main()
