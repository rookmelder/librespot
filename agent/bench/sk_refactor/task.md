Goal: turn the discount rule dicts in `shop/discounts.py` into small rule classes, so a new rule type is one class,
without changing what any existing call does.

Files: change `shop/discounts.py` and add tests to `tests/test_discounts.py` (both shown below, with the system note).
Change no other file: `shop/orders.py` keeps calling `apply_discounts(order, rules, coupons)` and `coupon_rule(...)`
with rule dicts, and saved shops keep storing rule dicts.

This is a refactor of code in production: everything `apply_discounts`, `check_rule` and `coupon_rule` do today must
stay exactly as it is, for every input they accept or refuse now (results, error messages, which error comes first,
what is or is not changed when an error is raised). Read the current code closely, not only its docstring.

New API (`shop/discounts.py`):
- `class Rule`: base class with class attributes `type = None`, `stage = 0`, `required = ()` and methods
  `from_dict(cls, d)` (classmethod), `to_dict(self)`, `valid(self)` (True by default) and `apply(self, lines, subtotal)`
  (`subtotal` = the order's subtotal before any discount).
- Three subclasses, each a `@dataclass(frozen=True)`, with these fields in this order:
  `BuyXGetY(sku, buy, free)` (`type = 'bxgy'`, `stage = 10`), `Percent(percent, skus=None, min_subtotal=0)`
  (`type = 'percent'`, `stage = 20`; `skus` kept as a tuple or None), `Fixed(amount, min_net=0)` (`type = 'fixed'`,
  `stage = 30`). `required` and `valid()` express today's checks.
- `to_dict()` -> `{'type': ..., <every field>}`, with `skus` as a list (or None).
  `rule_from_dict(r.to_dict()) == r` for every rule.
- `RULE_TYPES`: dict type name -> class. `register(cls)`: a class decorator that adds `cls` under `cls.type` and
  returns `cls`; the three built-in classes are registered with it.
- `rule_from_dict(d)` -> the Rule, or `ValueError(f'bad rule: {d}')` for every dict `check_rule` refuses today.
- `add_discount(line, cents)`: public name for today's `_add`.
- `check_rule(rule)` also accepts a Rule object.
- `apply_discounts(order, rules, coupons=None)`: `rules` may mix dicts and Rule objects. A Rule object that is not
  valid gives `ValueError(f'bad rule: {rule.to_dict()}')`. Rules run sorted by `stage`; rules with the same stage
  keep their list order. A registered new type with a stage between two others runs between them.
- Remove the old `ORDER`, `REQUIRED`, `_bxgy`, `_percent`, `_fixed`, `_add`. Update the module docstring.

Must cover: the new API, and tests that would catch a change in behaviour. The existing tests must pass unchanged.
