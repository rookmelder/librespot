Goal: fix and extend the existing `Ledger` class (accounts with balances in whole cents), and write its tests.

Files: change `ledger.py` (shown below) and create `tests/test_ledger.py` (the `tests` folder exists). Change no other file.

Spec. Keep the class name, `self.balances` (dict: account name -> int cents) and `self.history` (list of tuples).
Errors are always `ValueError` with exactly the message given here. Where several checks can fail, they run in the
order listed, and the first failing one is raised.

- `open(account)`: new account with balance 0. Already open: `ValueError(f'account exists: {account}')`.
- `balance(account)`: the balance. Unknown account: `ValueError(f'unknown account: {account}')`.
- `deposit(account, amount)` and `withdraw(account, amount)`, checks in this order:
  1. `amount` must be an `int` (a `bool` is not) and greater than 0, else `ValueError('amount must be a positive whole number of cents')`.
  2. Unknown account: `ValueError(f'unknown account: {account}')`.
  3. Only `withdraw`: balance lower than `amount`: `ValueError(f'insufficient funds in {account}: have {have}, need {amount}')`.
  Success appends `('deposit', account, amount)` or `('withdraw', account, amount)` to `history`.
- New `transfer(src, dst, amount)`, checks in this order: the amount check (1. above); unknown `src`; unknown `dst`;
  `src == dst`: `ValueError('cannot transfer to the same account')`; insufficient funds in `src` (message as for
  `withdraw`, with `src`). Success moves the money and appends ONE entry `('transfer', src, dst, amount)`.
- New `apply(ops)`: `ops` is a list of tuples `('deposit', account, amount)`, `('withdraw', account, amount)` or
  `('transfer', src, dst, amount)`. They run in order, so a later op sees the effect of earlier ones. All or nothing:
  when op number `i` (0-based) fails, `balances` AND `history` end exactly as they were before `apply`, and it raises
  `ValueError(f'op {i}: {message}')` where `message` is the text of the original error. An op whose first element is
  not one of the three names fails with message `unknown op '{name}'`. Returns the number of ops applied (0 for `[]`).
- New `statement(account)`: unknown account raises as in `balance`. Otherwise a list of strings, one per `history`
  entry that involves `account`, in history order: deposit `'+{amount}'`, withdraw `'-{amount}'`,
  transfer out of it `'-{amount} to {dst}'`, transfer into it `'+{amount} from {src}'`.

Must cover: every check and its exact message, the order of the checks (e.g. a bad amount on an unknown account
reports the amount), that a failed `transfer` or `apply` changes neither `balances` nor `history`, that `apply` sees
earlier ops, and every `statement` format.
