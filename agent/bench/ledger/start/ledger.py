class Ledger:
    def __init__(self):
        self.balances = {}
        self.history = []

    def open(self, account):
        self.balances[account] = 0

    def deposit(self, account, amount):
        self.balances[account] += amount
        self.history.append(('deposit', account, amount))

    def withdraw(self, account, amount):
        self.balances[account] -= amount
        self.history.append(('withdraw', account, amount))
