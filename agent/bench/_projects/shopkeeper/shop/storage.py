"""Save and load a whole shop (catalog, orders, coupons) as one JSON file.

Format version 2:
  {"version": 2, "products": [{sku, name, price, vat, active}], "stock": {sku: n}, "reserved": {sku: n},
   "orders": [order dicts, see models.order_to_dict], "coupons": {code: {...}}, "next_number": n}
Version 1 files (written before 2026-03) have no "version", no "reserved" (everything 0), and orders store
"total" instead of "paid_total": load() upgrades them. A file newer than VERSION is refused.
save() writes a temp file next to the target and renames it over it, so a crash never leaves half a file.
"""
import json
import os
import tempfile

from shop.catalog import Catalog
from shop.models import Product, order_from_dict, order_to_dict
from shop.orders import OrderBook

VERSION = 2


def dump(catalog, book):
    return {
        'version': VERSION,
        'products': [vars(p).copy() for p in catalog.products.values()],
        'stock': dict(catalog.stock),
        'reserved': dict(catalog.reserved),
        'orders': [order_to_dict(o) for o in book.orders.values()],
        'coupons': book.coupons,
        'next_number': book.next_number,
    }


def upgrade(data):
    """Any supported version -> a version-2 dict (a new dict; the input is not changed)."""
    version = data.get('version', 1)
    if not isinstance(version, int) or version < 1 or version > VERSION:
        raise ValueError(f'unsupported file version: {version} (this program reads 1 to {VERSION})')
    data = dict(data)
    if version == 1:
        data['reserved'] = {sku: 0 for sku in data.get('stock', {})}
        orders = []
        for o in data.get('orders', []):
            o = dict(o)
            o['paid_total'] = o.pop('total', 0)
            orders.append(o)
        data['orders'] = orders
    data['version'] = VERSION
    return data


def build(data, rules=None):
    """A dict (any supported version) -> (Catalog, OrderBook)."""
    data = upgrade(data)
    catalog = Catalog()
    for p in data.get('products', []):
        product = Product(**p)
        catalog.add(product, data['stock'].get(product.sku, 0))
        catalog.reserved[product.sku] = data['reserved'].get(product.sku, 0)
    book = OrderBook(catalog, rules, data.get('coupons', {}))
    for o in data.get('orders', []):
        order = order_from_dict(o)
        book.orders[order.id] = order
    book.next_number = data.get('next_number', len(book.orders) + 1)
    return catalog, book


def save(path, catalog, book):
    text = json.dumps(dump(catalog, book), indent=2, ensure_ascii=False)
    folder = os.path.dirname(os.path.abspath(path))
    fd, tmp = tempfile.mkstemp(dir=folder, suffix='.tmp')
    try:
        with os.fdopen(fd, 'w', encoding='utf-8') as f:
            f.write(text)
        os.replace(tmp, path)
    except BaseException:
        if os.path.exists(tmp):
            os.unlink(tmp)
        raise


def load(path, rules=None):
    with open(path, encoding='utf-8') as f:
        return build(json.load(f), rules)
