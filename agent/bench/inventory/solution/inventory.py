class Inventory:
    def __init__(self):
        self.stock = {}

    def add(self, item, qty):
        if qty <= 0:
            raise ValueError('qty must be positive')
        self.stock[item] = self.stock.get(item, 0) + qty

    def _check(self, item, qty):
        if qty <= 0:
            raise ValueError('qty must be positive')
        have = self.stock.get(item, 0)
        if have < qty:
            raise ValueError(f'not enough {item}: have {have}, need {qty}')

    def remove(self, item, qty):
        self._check(item, qty)
        self.stock[item] -= qty
        if self.stock[item] == 0:
            del self.stock[item]

    def total(self):
        return sum(self.stock.values())

    def reserve(self, order):
        for item in sorted(order):
            self._check(item, order[item])
        for item, qty in order.items():
            self.remove(item, qty)
        return sum(order.values())

    def report(self):
        return [f'{item}: {qty}' for item, qty in sorted(self.stock.items(), key=lambda kv: (-kv[1], kv[0]))]
