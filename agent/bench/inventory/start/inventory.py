class Inventory:
    def __init__(self):
        self.stock = {}

    def add(self, item, qty):
        self.stock[item] = qty

    def remove(self, item, qty):
        if self.stock.get(item, 0) < qty:
            raise ValueError('not enough stock')
        self.stock[item] -= qty

    def total(self):
        return sum(self.stock.values())
