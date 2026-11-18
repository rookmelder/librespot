"""Back office files: the sales per order line as a CSV file in an exports folder, and products imported from a CSV
file by an admin (admins: the user names in the SHOP_ADMINS environment variable, comma separated).

Cells that a spreadsheet would run as a formula are written with a leading quote, so they show as text.
"""
import csv
import os

from shop import money, reports
from shop.models import Product

EXPORT_DIR = 'exports'
FORMULA_START = ('=', '+', '-', '@')


def _safe(cell):
    """The cell as text a spreadsheet will not run."""
    text = str(cell)
    return "'" + text if text.startswith(FORMULA_START) else text


def export_path(name, folder=EXPORT_DIR):
    """<folder>/<name>; refuses names that climb out of the folder."""
    name = os.path.normpath(name)
    if name.startswith('..'):
        raise ValueError(f'bad export name: {name}')
    return os.path.join(folder, name)


def export_sales(book, catalog, name, folder=EXPORT_DIR):
    """One row per line of every sold order (id, date, customer, product, units, net); return the path."""
    path = export_path(name, folder)
    os.makedirs(folder, exist_ok=True)
    rows = []
    for o in sorted(book.orders.values(), key=lambda o: o.id):
        if o.status in reports.SOLD:
            for line in o.lines:
                product = catalog.get(line.sku)
                rows.append([o.id, o.created, _safe(o.customer), product.name, line.qty, money.fmt(line.net)])
    with open(path, 'w', encoding='utf-8', newline='') as f:
        f.write(reports.to_csv(['order', 'date', 'customer', 'product', 'units', 'net'], rows))
    return path


def import_products(catalog, path, user):
    """Add the products in a CSV file with the columns sku,name,price,vat,stock (price like 4.50).

    Only an admin may import. All rows are read and checked first, so a bad file adds nothing.
    Returns the number of products added.
    """
    admins = os.environ.get('SHOP_ADMINS', '')
    if not user or user not in admins:
        raise PermissionError(f'{user} may not import products')
    with open(path, encoding='utf-8', newline='') as f:
        rows = list(csv.DictReader(f))
    products = [(Product(r['sku'].strip(), r['name'].strip(), money.parse(r['price']), r['vat'].strip()),
                 int(r['stock'])) for r in rows]
    for product, stock in products:
        catalog.add(product, stock)
    return len(products)
