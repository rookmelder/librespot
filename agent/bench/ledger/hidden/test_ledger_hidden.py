import unittest

from ledger import Ledger


def ledger(**balances):
    lg = Ledger()
    for name, cents in balances.items():
        lg.open(name)
        if cents:
            lg.deposit(name, cents)
    lg.history.clear()
    return lg


class Accounts(unittest.TestCase):
    def assertError(self, msg, fn, *args):
        with self.assertRaises(ValueError) as e:
            fn(*args)
        self.assertEqual(str(e.exception), msg)

    def test_open_twice_rejected(self):
        lg = ledger(a=5)
        self.assertError('account exists: a', lg.open, 'a')
        self.assertEqual(lg.balances, {'a': 5})

    def test_deposit_withdraw_balance(self):
        lg = ledger(a=0)
        lg.deposit('a', 100)
        lg.withdraw('a', 30)
        self.assertEqual(lg.balance('a'), 70)
        self.assertEqual(lg.history, [('deposit', 'a', 100), ('withdraw', 'a', 30)])
        self.assertError('unknown account: zz', lg.balance, 'zz')

    def test_amount_rules(self):
        lg = ledger(a=10)
        for bad in (0, -5, 2.5, True, '3'):
            self.assertError('amount must be a positive whole number of cents', lg.deposit, 'a', bad)
            self.assertError('amount must be a positive whole number of cents', lg.withdraw, 'a', bad)
        self.assertEqual((lg.balances, lg.history), ({'a': 10}, []))

    def test_amount_checked_before_account(self):
        lg = ledger()
        self.assertError('amount must be a positive whole number of cents', lg.deposit, 'nobody', -1)
        self.assertError('unknown account: nobody', lg.withdraw, 'nobody', 1)

    def test_insufficient_funds_changes_nothing(self):
        lg = ledger(a=10)
        self.assertError('insufficient funds in a: have 10, need 11', lg.withdraw, 'a', 11)
        self.assertEqual((lg.balances, lg.history), ({'a': 10}, []))


class Transfers(unittest.TestCase):
    assertError = Accounts.assertError

    def test_transfer_moves_money_one_entry(self):
        lg = ledger(a=50, b=0)
        lg.transfer('a', 'b', 20)
        self.assertEqual(lg.balances, {'a': 30, 'b': 20})
        self.assertEqual(lg.history, [('transfer', 'a', 'b', 20)])

    def test_transfer_check_order(self):
        lg = ledger(a=5, b=0)
        self.assertError('amount must be a positive whole number of cents', lg.transfer, 'x', 'y', 0)
        self.assertError('unknown account: x', lg.transfer, 'x', 'y', 1)
        self.assertError('unknown account: y', lg.transfer, 'a', 'y', 1)
        self.assertError('cannot transfer to the same account', lg.transfer, 'a', 'a', 99)
        self.assertError('insufficient funds in a: have 5, need 6', lg.transfer, 'a', 'b', 6)
        self.assertEqual((lg.balances, lg.history), ({'a': 5, 'b': 0}, []))


class Batches(unittest.TestCase):
    assertError = Accounts.assertError

    def test_apply_runs_in_order(self):
        lg = ledger(a=0, b=0)
        n = lg.apply([('deposit', 'a', 10), ('transfer', 'a', 'b', 10), ('withdraw', 'b', 4)])
        self.assertEqual((n, lg.balances), (3, {'a': 0, 'b': 6}))
        self.assertEqual(len(lg.history), 3)
        self.assertEqual(lg.apply([]), 0)

    def test_apply_all_or_nothing(self):
        lg = ledger(a=10, b=0)
        before_hist = list(lg.history)
        self.assertError('op 2: insufficient funds in b: have 7, need 8', lg.apply,
                         [('withdraw', 'a', 3), ('transfer', 'a', 'b', 7), ('withdraw', 'b', 8)])
        self.assertEqual(lg.balances, {'a': 10, 'b': 0})
        self.assertEqual(lg.history, before_hist)

    def test_apply_unknown_op_and_wrapped_messages(self):
        lg = ledger(a=10)
        self.assertError("op 1: unknown op 'steal'", lg.apply, [('deposit', 'a', 1), ('steal', 'a', 5)])
        self.assertError('op 0: unknown account: q', lg.apply, [('deposit', 'q', 1)])
        self.assertEqual((lg.balances, lg.history), ({'a': 10}, []))


class Statements(unittest.TestCase):
    def test_statement_formats(self):
        lg = ledger(a=0, b=0, c=0)
        lg.deposit('a', 100)
        lg.transfer('a', 'b', 30)
        lg.withdraw('a', 5)
        lg.transfer('b', 'a', 10)
        lg.deposit('c', 1)
        self.assertEqual(lg.statement('a'), ['+100', '-30 to b', '-5', '+10 from b'])
        self.assertEqual(lg.statement('b'), ['+30 from a', '-10 to a'])
        self.assertEqual(lg.statement('c'), ['+1'])
        with self.assertRaises(ValueError):
            lg.statement('nope')


if __name__ == '__main__':
    unittest.main()
