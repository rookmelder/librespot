Goal: fix and extend the existing `Inventory` class, and write its tests.

Files: change `inventory.py` (shown below) and create `tests/test_inventory.py` (the `tests` folder exists). Change no other file.

Spec (keep the class name, `self.stock` as a dict of item name -> quantity, and the existing method names):

- `add(item, qty)`: adds `qty` to what is already in stock for `item` (today it overwrites).
- `remove(item, qty)`: takes `qty` out of stock. An item whose quantity reaches 0 is deleted from `self.stock`.
  If there is less than `qty` in stock: raise `ValueError(f'not enough {item}: have {have}, need {qty}')`, where
  `have` is the quantity in stock (0 for an unknown item), and change nothing.
- `add` and `remove`: if `qty` is 0 or negative, raise `ValueError('qty must be positive')` before anything else.
- `total()`: unchanged, the sum of all quantities.
- New `reserve(order)`: `order` is a dict of item name -> quantity. All or nothing:
  - Look at the items of `order` in alphabetical order of their names. For the first one that fails, raise the same
    `ValueError` that `remove` would raise for it (also the `qty must be positive` one), and leave `self.stock` unchanged.
  - Otherwise remove every item of the order from stock and return the total quantity reserved.
  - An empty order returns 0.
- New `report()`: returns a list of strings `'<item>: <qty>'`, sorted by quantity from high to low; items with the
  same quantity in alphabetical order. Empty stock: `[]`.

Must cover: every rule above, both error messages with exact text, and that a failed `reserve` changes nothing.
