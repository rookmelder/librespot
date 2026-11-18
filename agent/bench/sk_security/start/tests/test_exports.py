import hmac
import os
import tempfile
import unittest
from unittest import mock

from shop import clock, exports, notify, storage
from shop.catalog import Catalog
from shop.cli import run
from shop.models import Product
from shop.orders import OrderBook


def shop_with_sale(customer='ann'):
    c = Catalog()
    c.add(Product('TEA', 'Green tea', 450, 'reduced'), 10)
    book = OrderBook(c)
    o = book.create(customer)
    book.add_line(o.id, 'TEA', 2)
    book.pay(o.id, book.totals(o.id)['gross'])
    return c, book


class Export(unittest.TestCase):
    def setUp(self):
        clock.set_today('2026-03-04')
        self.dir = tempfile.TemporaryDirectory()

    def tearDown(self):
        clock.set_today(None)
        self.dir.cleanup()

    def test_writes_sold_order_lines(self):
        c, book = shop_with_sale()
        path = exports.export_sales(book, c, 'march.csv', self.dir.name)
        with open(path, encoding='utf-8') as f:
            self.assertEqual(f.read(), 'order,date,customer,product,units,net\nO0001,2026-03-04,ann,Green tea,2,€9.00\n')

    def test_formula_shown_as_text(self):
        for cell in ('=1+1', '+1', '-1', '@SUM(A1)'):
            self.assertEqual(exports._safe(cell), "'" + cell)
        self.assertEqual(exports._safe('ann'), 'ann')

    def test_name_may_not_climb_out(self):
        for bad in ('../shop.json', 'a/../../x.csv'):
            with self.assertRaises(ValueError):
                exports.export_path(bad, self.dir.name)
        self.assertEqual(exports.export_path('m.csv', 'exp'), os.path.join('exp', 'm.csv'))

    def test_cli_export(self):
        c, book = shop_with_sale()
        shop = os.path.join(self.dir.name, 'shop.json')
        storage.save(shop, c, book)
        lines = []
        self.assertEqual(run(shop, 'export', ['s.csv'], out=lines.append), 0)
        self.assertEqual(lines, [os.path.join(self.dir.name, 'exports', 's.csv')])


class Import(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.path = os.path.join(self.dir.name, 'p.csv')
        with open(self.path, 'w', encoding='utf-8') as f:
            f.write('sku,name,price,vat,stock\nMUG,Mug,12.00,standard,5\nJAM,Jam,3.99,reduced,8\n')

    def tearDown(self):
        self.dir.cleanup()

    @mock.patch.dict(os.environ, {'SHOP_ADMINS': 'sam,lee'})
    def test_admin_imports(self):
        c = Catalog()
        self.assertEqual(exports.import_products(c, self.path, 'sam'), 2)
        self.assertEqual((c.get('JAM').price, c.available('MUG')), (399, 5))

    @mock.patch.dict(os.environ, {'SHOP_ADMINS': 'sam,lee'})
    def test_others_may_not(self):
        c = Catalog()
        for user in ('bob', '', None):
            with self.assertRaises(PermissionError):
                exports.import_products(c, self.path, user)
        self.assertEqual(c.products, {})


class Notify(unittest.TestCase):
    def response(self, status):
        class Response:
            def __enter__(self): return self
            def __exit__(self, *a): return False
        r = Response()
        r.status = status
        return r

    @mock.patch.dict(os.environ, {'SHOP_WEBHOOK_TOKEN': 'tok-123'})
    def test_paid_posts_with_token(self):
        _, book = shop_with_sale()
        sent = []

        def send(request, timeout):
            sent.append(request)
            return self.response(200)

        self.assertEqual(notify.paid(book.get('O0001'), 'https://wh.example/paid', send), 200)
        self.assertEqual(sent[0].get_header('Authorization'), 'Bearer tok-123')
        self.assertEqual(sent[0].data, b'{"order": "O0001", "total": 981}')
        with self.assertRaises(RuntimeError):
            notify.paid(book.get('O0001'), 'https://wh.example/paid', lambda r, timeout: self.response(500))

    @mock.patch.dict(os.environ, {'SHOP_WEBHOOK_TOKEN': 'tok-123'})
    def test_signature(self):
        good = hmac.new(b'tok-123', b'ok', 'sha256').hexdigest()
        self.assertTrue(notify.check_signature(b'ok', good))
        self.assertFalse(notify.check_signature(b'ok', '0' * 64))

    @mock.patch.dict(os.environ, {}, clear=True)
    def test_no_token(self):
        with self.assertRaises(RuntimeError):
            notify.token()


if __name__ == '__main__':
    unittest.main()
