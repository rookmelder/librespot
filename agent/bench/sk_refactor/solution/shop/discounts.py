"""Discount rules. A rule is a Rule object; the save file and the coupons keep them as dicts with a 'type':

  {'type': 'bxgy', 'sku': 'TEA', 'buy': 2, 'free': 1}                         -> BuyXGetY
      per line of that sku: in every full group of buy + free units, `free` units cost nothing.
  {'type': 'percent', 'percent': 10, 'skus': ['TEA'], 'min_subtotal': 2000}   -> Percent
      per line: percent off what is left of the line after earlier rules (rounded per line, half up).
      'skus' missing or None = every line. 'min_subtotal' (default 0) is compared with the ORDER subtotal
      before any discount.
  {'type': 'fixed', 'amount': 500, 'min_net': 5000}                           -> Fixed
      per order: amount off, spread over the lines with money.split in proportion to each line's net so far.
      Only when the order's net so far is >= 'min_net' (default 0). Never more than what is left.

Rules run by stage (BuyXGetY 10, Percent 20, Fixed 30), within a stage in list order.
A line's discount never goes above its subtotal. A new rule type: subclass Rule, set `type`, `stage`,
`required` and `apply`, and decorate it with @register.

Coupons: {code: {'rule': <rule dict>, 'max_uses': int or None, 'uses': int}}. An order with a coupon adds
the coupon's rule to the shop's rules. orders.pay() counts a use.
"""
from dataclasses import asdict, dataclass, fields

from shop import money

RULE_TYPES = {}


def register(cls):
    """Class decorator: makes rule dicts with 'type': cls.type load as cls."""
    RULE_TYPES[cls.type] = cls
    return cls


def add_discount(line, cents):
    """Add cents to line.discount, never above the line's subtotal."""
    line.discount = min(line.subtotal, line.discount + cents)


class Rule:
    type = None
    stage = 0
    required = ()

    @classmethod
    def from_dict(cls, d):
        """Unknown keys are ignored (as rule dicts always allowed)."""
        names = {f.name for f in fields(cls)}
        return cls(**{k: v for k, v in d.items() if k in names})

    def to_dict(self):
        return {'type': self.type, **asdict(self)}

    def valid(self):
        return True

    def apply(self, lines, subtotal):
        raise NotImplementedError


@register
@dataclass(frozen=True)
class BuyXGetY(Rule):
    sku: str
    buy: int
    free: int
    type = 'bxgy'
    stage = 10
    required = ('sku', 'buy', 'free')

    def valid(self):
        return self.buy >= 1 and self.free >= 1

    def apply(self, lines, subtotal):
        group = self.buy + self.free
        for line in lines:
            if line.sku == self.sku:
                add_discount(line, line.qty // group * self.free * line.unit_price)


@register
@dataclass(frozen=True)
class Percent(Rule):
    percent: int
    skus: tuple = None
    min_subtotal: int = 0
    type = 'percent'
    stage = 20
    required = ('percent',)

    @classmethod
    def from_dict(cls, d):
        if d.get('skus') is not None:
            d = dict(d, skus=tuple(d['skus']))
        return super().from_dict(d)

    def to_dict(self):
        d = super().to_dict()
        if d['skus'] is not None:
            d['skus'] = list(d['skus'])
        return d

    def valid(self):
        return 0 < self.percent <= 100

    def apply(self, lines, subtotal):
        if subtotal < self.min_subtotal:
            return
        for line in lines:
            if self.skus is None or line.sku in self.skus:
                add_discount(line, money.percent_of(line.net, self.percent))


@register
@dataclass(frozen=True)
class Fixed(Rule):
    amount: int
    min_net: int = 0
    type = 'fixed'
    stage = 30
    required = ('amount',)

    def valid(self):
        return self.amount > 0

    def apply(self, lines, subtotal):
        net = sum(line.net for line in lines)
        if net <= 0 or net < self.min_net:
            return
        amount = min(self.amount, net)
        for line, part in zip(lines, money.split(amount, [line.net for line in lines])):
            add_discount(line, part)


def rule_from_dict(d):
    """The Rule for a rule dict; ValueError(f'bad rule: {d}') unless it is a complete, sensible rule dict."""
    cls = RULE_TYPES.get(d.get('type')) if isinstance(d, dict) else None
    if cls is None or not all(k in d for k in cls.required):
        raise ValueError(f'bad rule: {d}')
    try:
        rule = cls.from_dict(d)
    except TypeError:
        raise ValueError(f'bad rule: {d}') from None
    if not rule.valid():
        raise ValueError(f'bad rule: {d}')
    return rule


def to_rule(rule):
    """A Rule object (checked) for a rule dict or a Rule."""
    if isinstance(rule, Rule):
        if not rule.valid():
            raise ValueError(f'bad rule: {rule.to_dict()}')
        return rule
    return rule_from_dict(rule)


def check_rule(rule):
    """ValueError(f'bad rule: {rule}') unless rule is a complete, sensible rule dict (or a valid Rule)."""
    to_rule(rule)


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
    """Set line.discount on every line of the order (from scratch); return the order's total discount.
    rules: rule dicts and/or Rule objects."""
    rules = list(rules or [])
    if order.coupon:
        rules.append(coupon_rule(order.coupon, coupons))
    rules = [to_rule(r) for r in rules]
    for line in order.lines:
        line.discount = 0
    subtotal = sum(line.subtotal for line in order.lines)
    for rule in sorted(rules, key=lambda r: r.stage):
        rule.apply(order.lines, subtotal)
    return sum(line.discount for line in order.lines)
