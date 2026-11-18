"""Discount rules. A rule is a dict with a 'type':

  {'type': 'bxgy', 'sku': 'TEA', 'buy': 2, 'free': 1}
      per line of that sku: in every full group of buy + free units, `free` units cost nothing.
  {'type': 'percent', 'percent': 10, 'skus': ['TEA'], 'min_subtotal': 2000}
      per line: percent off what is left of the line after earlier rules (rounded per line, half up).
      'skus' missing or None = every line. 'min_subtotal' (default 0) is compared with the ORDER subtotal
      before any discount.
  {'type': 'fixed', 'amount': 500, 'min_net': 5000}
      per order: amount off, spread over the lines with money.split in proportion to each line's net so far.
      Only when the order's net so far is >= 'min_net' (default 0). Never more than what is left.

Rules run by type: all bxgy rules, then all percent, then all fixed (within a type, in list order).
A line's discount never goes above its subtotal.

Coupons: {code: {'rule': <rule dict>, 'max_uses': int or None, 'uses': int}}. An order with a coupon adds
the coupon's rule to the shop's rules. orders.pay() counts a use.
"""
from shop import money

ORDER = ('bxgy', 'percent', 'fixed')
REQUIRED = {'bxgy': ('sku', 'buy', 'free'), 'percent': ('percent',), 'fixed': ('amount',)}


def check_rule(rule):
    """ValueError(f'bad rule: {rule}') unless rule is a complete, sensible rule dict."""
    kind = rule.get('type') if isinstance(rule, dict) else None
    ok = kind in REQUIRED and all(k in rule for k in REQUIRED[kind])
    if ok and kind == 'bxgy':
        ok = rule['buy'] >= 1 and rule['free'] >= 1
    elif ok and kind == 'percent':
        ok = 0 < rule['percent'] <= 100
    elif ok and kind == 'fixed':
        ok = rule['amount'] > 0
    if not ok:
        raise ValueError(f'bad rule: {rule}')


def coupon_rule(code, coupons):
    """The rule of a coupon that may be used now. Checks, in order: unknown, used up."""
    coupons = coupons or {}
    if code not in coupons:
        raise ValueError(f'unknown coupon: {code}')
    coupon = coupons[code]
    if coupon.get('max_uses') is not None and coupon.get('uses', 0) >= coupon['max_uses']:
        raise ValueError(f'coupon used up: {code}')
    return coupon['rule']


def apply_discounts(order, rules, coupons=None):
    """Set line.discount on every line of the order (from scratch); return the order's total discount."""
    rules = list(rules or [])
    if order.coupon:
        rules.append(coupon_rule(order.coupon, coupons))
    for rule in rules:
        check_rule(rule)
    for line in order.lines:
        line.discount = 0
    subtotal = sum(line.subtotal for line in order.lines)
    for kind in ORDER:
        for rule in sorted(rules, key=lambda r: -r.get('percent', 0)):
            if rule['type'] != kind:
                continue
            if kind == 'bxgy':
                _bxgy(order.lines, rule)
            elif kind == 'percent':
                _percent(order.lines, rule, subtotal)
            else:
                _fixed(order.lines, rule)
    return sum(line.discount for line in order.lines)


def _bxgy(lines, rule):
    group = rule['buy'] + rule['free']
    for line in lines:
        if line.sku == rule['sku']:
            _add(line, line.qty // group * rule['free'] * line.unit_price)


def _percent(lines, rule, subtotal):
    if subtotal < rule.get('min_subtotal', 0):
        return
    skus = rule.get('skus')
    for line in lines:
        if skus is None or line.sku in skus:
            _add(line, money.percent_of(line.net, rule['percent']))


def _fixed(lines, rule):
    net = sum(line.net for line in lines)
    if net <= 0 or net < rule.get('min_net', 0):
        return
    amount = min(rule['amount'], net)
    for line, part in zip(lines, money.split(amount, [line.net for line in lines])):
        _add(line, part)


def _add(line, cents):
    line.discount = min(line.subtotal, line.discount + cents)
