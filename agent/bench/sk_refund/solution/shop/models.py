"""The shop's data: products, order lines and orders. Amounts are int cents, dates ISO strings ('2026-03-01')."""
from dataclasses import asdict, dataclass, field


@dataclass
class Product:
    sku: str
    name: str
    price: int                  # net price per unit (excl. VAT), cents
    vat: str = 'standard'       # a key of money.VAT_RATES
    active: bool = True         # False: kept for old orders, not for sale


@dataclass
class OrderLine:
    sku: str
    qty: int
    unit_price: int             # copied from the product when the line was added
    vat: str
    discount: int = 0           # cents off this whole line; set by discounts.apply_discounts

    @property
    def subtotal(self):
        """qty x unit price, before discount, excl. VAT."""
        return self.qty * self.unit_price

    @property
    def net(self):
        """What the line costs after its discount, excl. VAT."""
        return self.subtotal - self.discount


@dataclass
class Order:
    id: str
    customer: str
    created: str                                  # ISO date
    lines: list = field(default_factory=list)     # [OrderLine]
    status: str = 'new'                           # see orders.py for the flow
    coupon: str = None
    paid_total: int = 0                           # gross (incl. VAT) amount paid, cents
    refunded: int = 0                             # gross amount refunded so far, cents
    events: list = field(default_factory=list)    # [(ISO date, text)]
    refunded_units: dict = field(default_factory=dict)   # sku -> units refunded so far
    refunded_net: dict = field(default_factory=dict)     # sku -> net cents refunded so far

    def line(self, sku):
        return next((l for l in self.lines if l.sku == sku), None)


def order_to_dict(order):
    d = asdict(order)
    d['events'] = [list(e) for e in order.events]
    return d


def order_from_dict(d):
    d = dict(d)
    d['lines'] = [OrderLine(**l) for l in d.get('lines', [])]
    d['events'] = [tuple(e) for e in d.get('events', [])]
    return Order(**d)
