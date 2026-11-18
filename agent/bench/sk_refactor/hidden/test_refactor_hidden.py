import unittest

from shop import discounts
from shop.discounts import (BuyXGetY, Fixed, Percent, Rule, add_discount, apply_discounts, check_rule,
                            register, rule_from_dict)
from shop.models import Order, OrderLine

BIG = [{'type': 'fixed', 'amount': 777, 'min_net': 1000},
       {'type': 'percent', 'percent': 15, 'skus': ['MUG', 'JAM']},
       {'type': 'bxgy', 'sku': 'TEA', 'buy': 2, 'free': 1},
       {'type': 'percent', 'percent': 10, 'min_subtotal': 8000}]
COUPONS = {'C': {'rule': {'type': 'percent', 'percent': 20}, 'max_uses': None, 'uses': 0}}


def order(*lines, coupon=None):
    o = Order('O0001', 'ann', '2026-03-01', coupon=coupon)
    o.lines = [OrderLine(sku, qty, price, vat) for sku, qty, price, vat in lines]
    return o


def big(coupon=None):
    return order(('MUG', 3, 1250, 'standard'), ('JAM', 4, 899, 'reduced'), ('TEA', 4, 450, 'reduced'), coupon=coupon)


class SameBehaviour(unittest.TestCase):
    def test_big_order_with_coupon(self):
        o = big('C')
        self.assertEqual(apply_discounts(o, BIG, COUPONS), 4456)
        self.assertEqual([l.discount for l in o.lines], [1782, 1708, 966])

    def test_big_order_without_coupon(self):
        o = big()
        self.assertEqual(apply_discounts(o, BIG), 3089)
        self.assertEqual([l.discount for l in o.lines], [1208, 1158, 723])

    def test_stacked_percent_keeps_list_order(self):
        a = [{'type': 'percent', 'percent': 10}, {'type': 'percent', 'percent': 20}]
        self.assertEqual(apply_discounts(order(('TEA', 1, 455, 'reduced')), a), 128)
        self.assertEqual(apply_discounts(order(('TEA', 1, 455, 'reduced')), a[::-1]), 127)

    def test_unknown_keys_still_allowed(self):
        rules = [{'type': 'bxgy', 'sku': 'TEA', 'buy': 2, 'free': 1, 'note': 'spring'},
                 {'type': 'percent', 'percent': 10, 'min_subtotal': 1351, 'label': 'x'}]
        self.assertEqual(apply_discounts(order(('TEA', 3, 450, 'reduced')), rules), 450)   # subtotal 1350 < 1351


class SameEdges(unittest.TestCase):
    def test_bad_rule_leaves_old_discounts(self):
        o = order(('MUG', 1, 1000, 'standard'))
        apply_discounts(o, [{'type': 'percent', 'percent': 10}])
        with self.assertRaises(ValueError):
            apply_discounts(o, [{'type': 'percent', 'percent': 50}, {'type': 'fixed', 'amount': 0}])
        self.assertEqual(o.lines[0].discount, 100)

    def test_unknown_coupon_reported_before_bad_rule(self):
        with self.assertRaises(ValueError) as e:
            apply_discounts(order(('MUG', 1, 1000, 'standard'), coupon='X'), [{'type': 'magic'}], {})
        self.assertEqual(str(e.exception), 'unknown coupon: X')

    def test_callers_rules_kept_and_any_sequence(self):
        rules = [{'type': 'percent', 'percent': 10}]
        o = order(('MUG', 1, 1200, 'standard'), coupon='C')
        apply_discounts(o, rules, COUPONS)
        self.assertEqual(apply_discounts(o, tuple(rules), COUPONS), 120 + 216)
        self.assertEqual(rules, [{'type': 'percent', 'percent': 10}])
        self.assertEqual(apply_discounts(order(('MUG', 1, 1200, 'standard')), None), 0)


class Classes(unittest.TestCase):
    def test_rule_from_dict(self):
        self.assertEqual(rule_from_dict({'type': 'bxgy', 'sku': 'TEA', 'buy': 2, 'free': 1}), BuyXGetY('TEA', 2, 1))
        self.assertEqual(rule_from_dict({'type': 'percent', 'percent': 10, 'skus': ['A', 'B']}),
                         Percent(10, ('A', 'B'), 0))
        self.assertEqual(rule_from_dict({'type': 'percent', 'percent': 10}), Percent(10, None, 0))
        self.assertEqual(rule_from_dict({'type': 'fixed', 'amount': 500, 'min_net': 900}), Fixed(500, 900))
        self.assertEqual([BuyXGetY.stage, Percent.stage, Fixed.stage], [10, 20, 30])
        with self.assertRaises(Exception):
            BuyXGetY('TEA', 2, 1).buy = 3          # frozen

    def test_to_dict_round_trip(self):
        self.assertEqual(Percent(10, ('A',), 2000).to_dict(),
                         {'type': 'percent', 'percent': 10, 'skus': ['A'], 'min_subtotal': 2000})
        self.assertEqual(Fixed(500).to_dict(), {'type': 'fixed', 'amount': 500, 'min_net': 0})
        for r in (BuyXGetY('TEA', 2, 1), Percent(15, None, 0), Fixed(1, 2)):
            self.assertEqual(rule_from_dict(r.to_dict()), r)

    def test_bad_rules_same_message(self):
        for bad in ({'type': 'percent', 'percent': 0}, {'type': 'percent', 'percent': 101},
                    {'type': 'bxgy', 'sku': 'A', 'buy': 1}, {'type': 'bxgy', 'sku': 'A', 'buy': 0, 'free': 1},
                    {'type': 'fixed', 'amount': 0}, {'type': 'magic'}, {}, 'x', None):
            for fn in (check_rule, rule_from_dict):
                with self.assertRaises(ValueError) as e:
                    fn(bad)
                self.assertEqual(str(e.exception), f'bad rule: {bad}')

    def test_objects_and_dicts_mixed(self):
        o = big('C')
        rules = [Fixed(777, 1000), BIG[1], BuyXGetY('TEA', 2, 1), Percent(10, None, 8000)]
        self.assertEqual(apply_discounts(o, rules, COUPONS), 4456)
        self.assertEqual([l.discount for l in o.lines], [1782, 1708, 966])

    def test_invalid_object_refused(self):
        with self.assertRaises(ValueError) as e:
            apply_discounts(order(('TEA', 1, 450, 'reduced')), [Percent(0)])
        self.assertEqual(str(e.exception), "bad rule: {'type': 'percent', 'percent': 0, 'skus': None, 'min_subtotal': 0}")

    def test_add_discount_caps(self):
        line = OrderLine('TEA', 1, 450, 'reduced')
        add_discount(line, 400)
        add_discount(line, 400)
        self.assertEqual(line.discount, 450)


class NewType(unittest.TestCase):
    def tearDown(self):
        discounts.RULE_TYPES.pop('per_line', None)

    def test_registered_type_runs_by_stage(self):
        from dataclasses import dataclass

        @register
        @dataclass(frozen=True)
        class PerLine(Rule):
            cents: int
            type = 'per_line'
            stage = 25
            required = ('cents',)

            def apply(self, lines, subtotal):
                for line in lines:
                    add_discount(line, self.cents)

        self.assertIs(discounts.RULE_TYPES['per_line'], PerLine)
        o = order(('MUG', 1, 1000, 'standard'))
        # fixed listed first still runs last; per_line (25) after percent (20): 1000 -50% = 500, -100 = 400, -100 = 300
        rules = [{'type': 'fixed', 'amount': 100}, {'type': 'per_line', 'cents': 100, 'extra': 1},
                 {'type': 'percent', 'percent': 50}]
        self.assertEqual(apply_discounts(o, rules), 700)
        with self.assertRaises(ValueError):
            rule_from_dict({'type': 'per_line'})


if __name__ == '__main__':
    unittest.main()
