Review this pull request for security problems (diff below; the changed shop files are below with line numbers, and
the tests pass).

PR description, as the author wrote it:
> Back office features. `python -m shop.cli <shop.json> export <name>` writes the sales per order line as a CSV file
> into `exports/` next to the shop file (the back office opens it in a spreadsheet). Admins can import products from
> a CSV file (`exports.import_products`); the admins are the user names in `SHOP_ADMINS`, comma separated.
> `notify.paid()` tells the warehouse's webhook about a paid order, with the token from `SHOP_WEBHOOK_TOKEN`, and
> `notify.check_signature()` verifies the warehouse's signed replies.

Customer names come from customers, imported files from suppliers, export names from the command line.
Report every real security problem; leave out style and anything that is correct.
Point at shop code with file:line (line numbers as in the files below).
