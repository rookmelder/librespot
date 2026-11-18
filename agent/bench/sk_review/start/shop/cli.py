"""Command line: python -m shop.cli <shop.json> <command> [args]

  products                      list products with price and available stock
  new <customer>                create an order, print its id
  add <order> <sku> <qty>       add units to an order
  coupon <order> <code|->       set or remove a coupon
  total <order>                 print the order's totals
  pay <order> <amount>          pay an order (amount like 24.33)
  ship <order> | cancel <order> | refund <order>
  sales                         sales per day
  between <from> <to>           sales per day from one date to another (YYYY-MM-DD)
  search <customer>             a customer's orders, oldest first
Every command that changes something saves the file.
"""
import sys

from shop import money, reports, storage

CHANGES = {'new', 'add', 'coupon', 'pay', 'ship', 'cancel', 'refund'}
ARGS = {'products': 0, 'new': 1, 'add': 3, 'coupon': 2, 'total': 1, 'pay': 2, 'ship': 1, 'cancel': 1, 'refund': 1,
        'sales': 0, 'between': 2, 'search': 1}


def run(path, command, args, out=print):
    """Run one command on the shop file at path. Returns 0, or 2 after printing the error."""
    if command not in ARGS:
        out(f'unknown command: {command}')
        return 2
    if len(args) != ARGS[command]:
        out(f'{command} takes {ARGS[command]} argument(s), got {len(args)}')
        return 2
    catalog, book = storage.load(path)
    try:
        _do(catalog, book, command, args, out)
    except ValueError as e:
        out(f'error: {e}')
        return 2
    if command in CHANGES:
        storage.save(path, catalog, book)
    return 0


def _do(catalog, book, command, args, out):
    if command == 'products':
        for p in catalog.products.values():
            if p.active:
                out(f'{p.sku:<10} {money.fmt(p.price):>10}  {catalog.available(p.sku):>5} available  {p.name}')
    elif command == 'new':
        out(book.create(args[0]).id)
    elif command == 'add':
        book.add_line(args[0], args[1], _int(args[2]))
    elif command == 'coupon':
        book.set_coupon(args[0], None if args[1] == '-' else args[1])
    elif command == 'total':
        t = book.totals(args[0])
        out(f"net {money.fmt(t['net'])}  VAT {money.fmt(t['vat'])}  total {money.fmt(t['gross'])}"
            f"  (discount {money.fmt(t['discount'])})")
    elif command == 'pay':
        book.pay(args[0], money.parse(args[1]))
    elif command == 'sales':
        for day, cents in reports.sales_by_day(book.orders.values()).items():
            out(f'{day}  {money.fmt(cents)}')
    elif command == 'between':
        for day, cents in reports.sales_between(book.orders.values(), args[0], args[1]).items():
            out(f'{day}  {money.fmt(cents)}')
    elif command == 'search':
        for o in book.search(args[0]):
            out(f'{o.id}  {o.created}  {o.status:<9} {money.fmt(o.paid_total - o.refunded):>10}')
    else:
        result = getattr(book, command)(args[0])
        if command in ('cancel', 'refund'):
            out(f'refunded {money.fmt(result)}')


def _int(text):
    try:
        return int(text)
    except ValueError:
        raise ValueError(f'not a whole number: {text}') from None


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    if len(argv) < 2:
        print(__doc__)
        return 2
    return run(argv[0], argv[1], argv[2:])


if __name__ == '__main__':
    sys.exit(main())
