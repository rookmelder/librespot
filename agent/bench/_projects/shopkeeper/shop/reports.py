"""Reports over orders. A sale = an order that was paid (status paid, shipped or refunded); it counts on the day
the order was created, for what was paid minus what was refunded. Cancelled and new orders are not sales."""
import csv
import io

SOLD = ('paid', 'shipped', 'refunded')


def sales_by_day(orders):
    """-> {ISO date: cents}, dates sorted."""
    days = {}
    for o in orders:
        if o.status in SOLD:
            days[o.created] = days.get(o.created, 0) + o.paid_total - o.refunded
    return dict(sorted(days.items()))


def top_products(orders, n=3):
    """-> [(sku, units)] of the n best-selling products (refunded orders don't count), most units first, then sku."""
    units = {}
    for o in orders:
        if o.status in ('paid', 'shipped'):
            for line in o.lines:
                units[line.sku] = units.get(line.sku, 0) + line.qty
    return sorted(units.items(), key=lambda kv: (-kv[1], kv[0]))[:n]


def to_csv(header, rows):
    """A CSV text (comma separated, \\n line ends) with a header row."""
    out = io.StringIO()
    writer = csv.writer(out, lineterminator='\n')
    writer.writerow(header)
    writer.writerows(rows)
    return out.getvalue()
