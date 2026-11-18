import copy


class Ledger:
    def __init__(self):
        self.balances = {}
        self.history = []

    def open(self, account):
        if account in self.balances:
            raise ValueError(f'account exists: {account}')
        self.balances[account] = 0

    def _known(self, account):
        if account not in self.balances:
            raise ValueError(f'unknown account: {account}')

    @staticmethod
    def _amount(amount):
        if isinstance(amount, bool) or not isinstance(amount, int) or amount <= 0:
            raise ValueError('amount must be a positive whole number of cents')

    def _funds(self, account, amount):
        have = self.balances[account]
        if have < amount:
            raise ValueError(f'insufficient funds in {account}: have {have}, need {amount}')

    def balance(self, account):
        self._known(account)
        return self.balances[account]

    def deposit(self, account, amount):
        self._amount(amount)
        self._known(account)
        self.balances[account] += amount
        self.history.append(('deposit', account, amount))

    def withdraw(self, account, amount):
        self._amount(amount)
        self._known(account)
        self._funds(account, amount)
        self.balances[account] -= amount
        self.history.append(('withdraw', account, amount))

    def transfer(self, src, dst, amount):
        self._amount(amount)
        self._known(src)
        self._known(dst)
        if src == dst:
            raise ValueError('cannot transfer to the same account')
        self._funds(src, amount)
        self.balances[src] -= amount
        self.balances[dst] += amount
        self.history.append(('transfer', src, dst, amount))

    def apply(self, ops):
        saved = (copy.deepcopy(self.balances), list(self.history))
        for i, op in enumerate(ops):
            try:
                name = op[0]
                if name == 'deposit':
                    self.deposit(*op[1:])
                elif name == 'withdraw':
                    self.withdraw(*op[1:])
                elif name == 'transfer':
                    self.transfer(*op[1:])
                else:
                    raise ValueError(f"unknown op '{name}'")
            except ValueError as e:
                self.balances, self.history = saved
                raise ValueError(f'op {i}: {e}') from None
        return len(ops)

    def statement(self, account):
        self._known(account)
        out = []
        for entry in self.history:
            if entry[0] == 'deposit' and entry[1] == account:
                out.append(f'+{entry[2]}')
            elif entry[0] == 'withdraw' and entry[1] == account:
                out.append(f'-{entry[2]}')
            elif entry[0] == 'transfer' and entry[1] == account:
                out.append(f'-{entry[3]} to {entry[2]}')
            elif entry[0] == 'transfer' and entry[2] == account:
                out.append(f'+{entry[3]} from {entry[1]}')
        return out
