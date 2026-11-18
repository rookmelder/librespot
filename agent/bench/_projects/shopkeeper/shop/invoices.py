"""Invoices and credit notes.

VAT is calculated per rate on the summed net of the lines with that rate (not per line), rounded once
(money.vat_for). An order's totals and its invoice therefore always agree.
"""
from shop import clock, money


def vat_summary(lines):
    """OrderLines (or line dicts with 'net' and 'vat') -> {rate: {'net': n, 'vat': v}}, rates sorted by name."""
    by_rate = {}
    for line in lines:
        rate, net = (line['vat'], line['net']) if isinstance(line, dict) else (line.vat, line.net)
        by_rate[rate] = by_rate.get(rate, 0) + net
    return {rate: {'net': net, 'vat': money.vat_for(net, rate)} for rate, net in sorted(by_rate.items())}


def totals(lines):
    """-> {'net', 'vat', 'gross', 'discount'} for a list of OrderLines."""
    summary = vat_summary(lines)
    net = sum(v['net'] for v in summary.values())
    vat = sum(v['vat'] for v in summary.values())
    return {'net': net, 'vat': vat, 'gross': net + vat, 'discount': sum(line.discount for line in lines)}


def make_invoice(order, number):
    """The invoice of a paid order (its discounts are already on the lines)."""
    lines = [{'sku': l.sku, 'qty': l.qty, 'unit_price': l.unit_price, 'discount': l.discount, 'net': l.net, 'vat': l.vat}
             for l in order.lines]
    return dict({'number': number, 'order': order.id, 'customer': order.customer, 'date': clock.today().isoformat(),
                 'lines': lines, 'vat_rates': vat_summary(order.lines)}, **totals(order.lines))


def credit_note(invoice, number):
    """A credit note that cancels a whole invoice: every amount negated.
    The VAT is calculated again from the negated nets; money rounds halves away from zero, so it comes out as
    exactly the invoice's VAT, negated."""
    lines = [dict(l, qty=-l['qty'], discount=-l['discount'], net=-l['net']) for l in invoice['lines']]
    summary = vat_summary(lines)
    net = sum(v['net'] for v in summary.values())
    vat = sum(v['vat'] for v in summary.values())
    return {'number': number, 'credits': invoice['number'], 'order': invoice['order'], 'customer': invoice['customer'],
            'date': clock.today().isoformat(), 'lines': lines, 'vat_rates': summary,
            'net': net, 'vat': vat, 'gross': net + vat, 'discount': -invoice['discount']}


def render(doc):
    """An invoice or credit note as plain text."""
    title = 'CREDIT NOTE' if 'credits' in doc else 'INVOICE'
    out = [f"{title} {doc['number']}  order {doc['order']}  {doc['date']}", f"customer: {doc['customer']}"]
    for l in doc['lines']:
        out.append(f"  {l['qty']:>4} x {l['sku']:<10} {money.fmt(l['unit_price']):>10}  -{money.fmt(l['discount'])}"
                   f"  {money.fmt(l['net']):>10}  ({l['vat']})")
    for rate, v in doc['vat_rates'].items():
        out.append(f"  VAT {rate} ({money.vat_rate(rate)}%) over {money.fmt(v['net'])}: {money.fmt(v['vat'])}")
    out.append(f"  total {money.fmt(doc['gross'])} (net {money.fmt(doc['net'])} + VAT {money.fmt(doc['vat'])})")
    return '\n'.join(out)
