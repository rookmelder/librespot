"""Orders: lines, coupon, totals and the status flow.

    new --pay--> paid --ship--> shipped
    new / paid --cancel--> cancelled        (a paid order gets its full amount back)
    paid / shipped --refund--> refunded     (full refund; shipped goods come back into stock)

Stock: add_line reserves; remove_line, cancel and refunding a paid order release; ship commits;
refunding a shipped order restocks. While an order is new its discounts are recalculated on every
totals() call; pay() freezes them. All dates come from shop.clock.
"""
from shop import clock, money
from shop.discounts import apply_discounts, coupon_rule
from shop.invoices import totals as line_totals
from shop.models import Order, OrderLine


class OrderBook:
    def __init__(self, catalog, rules=None, coupons=None):
        self.catalog = catalog
        self.rules = rules or []
        self.coupons = coupons if coupons is not None else {}
        self.orders = {}            # id -> Order
        self.next_number = 1

    def get(self, order_id):
        try:
            return self.orders[order_id]
        except KeyError:
            raise ValueError(f'unknown order: {order_id}') from None

    def _need(self, order, *statuses):
        if order.status not in statuses:
            raise ValueError(f'order {order.id} is {order.status}')

    def _log(self, order, text):
        order.events.append((clock.today().isoformat(), text))

    def create(self, customer):
        if not customer or not customer.strip():
            raise ValueError('customer is required')
        order_id = f'O{self.next_number:04d}'
        self.next_number += 1
        order = Order(order_id, customer.strip(), clock.today().isoformat())
        self.orders[order_id] = order
        self._log(order, 'created')
        return order

    def add_line(self, order_id, sku, qty):
        order = self.get(order_id)
        self._need(order, 'new')
        product = self.catalog.get(sku)
        if not product.active:
            raise ValueError(f'product not for sale: {sku}')
        self.catalog.reserve(sku, qty)          # also checks qty
        line = order.line(sku)
        if line:
            line.qty += qty
        else:
            order.lines.append(OrderLine(sku, qty, product.price, product.vat))
        self._log(order, f'+{qty} {sku}')

    def remove_line(self, order_id, sku):
        order = self.get(order_id)
        self._need(order, 'new')
        line = order.line(sku)
        if not line:
            raise ValueError(f'{sku} not in order {order_id}')
        self.catalog.release(sku, line.qty)
        order.lines.remove(line)
        self._log(order, f'-{line.qty} {sku}')

    def set_coupon(self, order_id, code):
        """code None removes the coupon. A coupon is checked now and again whenever totals are calculated."""
        order = self.get(order_id)
        self._need(order, 'new')
        if code is not None:
            coupon_rule(code, self.coupons)
        order.coupon = code
        self._log(order, f'coupon {code}')

    def totals(self, order_id):
        """-> {'net', 'vat', 'gross', 'discount'} (cents)."""
        order = self.get(order_id)
        if order.status == 'new':
            apply_discounts(order, self.rules, self.coupons)
        return line_totals(order.lines)

    def pay(self, order_id, amount):
        order = self.get(order_id)
        self._need(order, 'new')
        if not order.lines:
            raise ValueError(f'order {order_id} has no lines')
        total = self.totals(order_id)['gross']
        if amount != total:
            raise ValueError(f'payment {money.fmt(amount)} does not match total {money.fmt(total)}')
        if order.coupon:
            coupon = self.coupons[order.coupon]
            coupon['uses'] = coupon.get('uses', 0) + 1
        order.paid_total = total
        order.status = 'paid'
        self._log(order, f'paid {money.fmt(total)}')

    def ship(self, order_id):
        order = self.get(order_id)
        self._need(order, 'paid')
        for line in order.lines:
            self.catalog.commit(line.sku, line.qty)
        order.status = 'shipped'
        self._log(order, 'shipped')

    def cancel(self, order_id):
        """-> the amount refunded (0 for a new order)."""
        order = self.get(order_id)
        self._need(order, 'new', 'paid')
        for line in order.lines:
            self.catalog.release(line.sku, line.qty)
        amount = 0
        if order.status == 'paid':
            amount = order.paid_total - order.refunded
            order.refunded = order.paid_total
        order.status = 'cancelled'
        self._log(order, f'cancelled, refunded {money.fmt(amount)}')
        return amount

    def refund(self, order_id):
        """Full refund of a paid or shipped order -> the amount refunded."""
        order = self.get(order_id)
        self._need(order, 'paid', 'shipped')
        for line in order.lines:
            if order.status == 'shipped':
                self.catalog.restock(line.sku, line.qty)
            else:
                self.catalog.release(line.sku, line.qty)
        amount = order.paid_total - order.refunded
        order.refunded = order.paid_total
        order.status = 'refunded'
        self._log(order, f'refunded {money.fmt(amount)}')
        return amount

    def find(self, customer=None, status=None):
        return [o for o in self.orders.values()
                if (customer is None or o.customer == customer) and (status is None or o.status == status)]
