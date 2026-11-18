"""Products and stock. stock = units in the warehouse; reserved = units promised to open orders (not shipped yet)."""


def check_qty(qty):
    if isinstance(qty, bool) or not isinstance(qty, int) or qty <= 0:
        raise ValueError('qty must be a positive int')


class Catalog:
    def __init__(self):
        self.products = {}      # sku -> Product
        self.stock = {}         # sku -> units in the warehouse
        self.reserved = {}      # sku -> units reserved by open orders

    def add(self, product, stock=0):
        if product.sku in self.products:
            raise ValueError(f'product exists: {product.sku}')
        if stock < 0:
            raise ValueError('stock must be >= 0')
        self.products[product.sku] = product
        self.stock[product.sku] = stock
        self.reserved[product.sku] = 0

    def get(self, sku):
        try:
            return self.products[sku]
        except KeyError:
            raise ValueError(f'unknown product: {sku}') from None

    def available(self, sku):
        """Units that can still be ordered."""
        self.get(sku)
        return self.stock[sku] - self.reserved[sku]

    def reserve(self, sku, qty):
        check_qty(qty)
        have = self.available(sku)
        if have < qty:
            raise ValueError(f'not enough {sku}: available {have}, need {qty}')
        self.reserved[sku] += qty

    def release(self, sku, qty):
        """Give back a reservation (line removed, order cancelled)."""
        check_qty(qty)
        self.get(sku)
        if self.reserved[sku] < qty:
            raise ValueError(f'cannot release {qty} {sku}: only {self.reserved[sku]} reserved')
        self.reserved[sku] -= qty

    def commit(self, sku, qty):
        """Reserved units leave the warehouse (the order ships)."""
        self.release(sku, qty)
        self.stock[sku] -= qty

    def restock(self, sku, qty):
        """Units come (back) into the warehouse."""
        check_qty(qty)
        self.get(sku)
        self.stock[sku] += qty
