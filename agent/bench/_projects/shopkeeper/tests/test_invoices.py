import unittest

from shop import clock
from shop.catalog import Catalog
from shop.invoices import credit_note, make_invoice, render, vat_summary
from shop.models import OrderLine, Product
from shop.orders import OrderBook


def paid_order(lines, rules=None):
    clock.set_today('2026-03-02')
    c = Catalog()
    c.add(Product('TEA', 'Green tea', 450, 'reduced'), 50)
    c.add(Product('MUG', 'Mug', 1200), 50)
    c.add(Product('JAM', 'Jam', 500, 'reduced'), 50)
    c.add(Product('CARD', 'Gift card', 2500, 'zero'), 50)
    book = OrderBook(c, rules)
    o = book.create('ann')
    for sku, qty in lines:
        book.add_line(o.id, sku, qty)
    book.pay(o.id, book.totals(o.id)['gross'])
    return book, o


class Invoices(unittest.TestCase):
    def tearDown(self):
        clock.set_today(None)

    def test_vat_per_rate_on_summed_net(self):
        lines = [OrderLine('A', 1, 105, 'reduced'), OrderLine('B', 1, 105, 'reduced')]
        self.assertEqual(vat_summary(lines), {'reduced': {'net': 210, 'vat': 19}})   # 18.9 -> 19, not 9 + 9

    def test_fixed_coupon_over_two_rates(self):
        book, o = paid_order([('JAM', 3), ('CARD', 2)], [{'type': 'fixed', 'amount': 700}])
        inv = make_invoice(o, 'F-2')
        self.assertEqual([l['discount'] for l in inv['lines']], [162, 538])
        self.assertEqual(inv['vat_rates'], {'reduced': {'net': 1338, 'vat': 120}, 'zero': {'net': 4462, 'vat': 0}})
        self.assertEqual(inv['gross'], 5920)

    def test_invoice_matches_order_totals(self):
        book, o = paid_order([('TEA', 3), ('MUG', 2), ('CARD', 1)], [{'type': 'percent', 'percent': 15}])
        inv = make_invoice(o, 'F-1')
        self.assertEqual(inv['gross'], o.paid_total)
        self.assertEqual((inv['net'], inv['vat']), (book.totals(o.id)['net'], book.totals(o.id)['vat']))
        self.assertEqual(sorted(inv['vat_rates']), ['reduced', 'standard', 'zero'])
        self.assertEqual(inv['date'], '2026-03-02')

    def test_credit_note_negates_invoice(self):
        _, o = paid_order([('TEA', 2), ('MUG', 1)])
        inv = make_invoice(o, 'F-1')
        cn = credit_note(inv, 'C-1')
        self.assertEqual((cn['net'], cn['vat'], cn['gross']), (-inv['net'], -inv['vat'], -inv['gross']))
        self.assertEqual(cn['credits'], 'F-1')

    def test_credit_note_of_discounted_order_negates_exactly(self):
        _, o = paid_order([('JAM', 1)], [{'type': 'percent', 'percent': 50}])     # net 250 at 9%: VAT 22.5 -> 23
        inv = make_invoice(o, 'F-2')
        cn = credit_note(inv, 'C-2')
        self.assertEqual(inv['vat'], 23)
        self.assertEqual((cn['vat'], cn['gross']), (-inv['vat'], -inv['gross']))

    def test_render(self):
        _, o = paid_order([('TEA', 1)])
        text = render(make_invoice(o, 'F-3'))
        self.assertIn('INVOICE F-3  order O0001  2026-03-02', text)
        self.assertIn('VAT reduced (9%) over €4.50: €0.41', text)
        self.assertTrue(text.endswith('total €4.91 (net €4.50 + VAT €0.41)'))


if __name__ == '__main__':
    unittest.main()
