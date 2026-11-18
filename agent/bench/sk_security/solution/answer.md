1. YES: an absolute export name passes the check (shop/exports.py:{absolute_export_name}): `export C:\shop\shop.json` or `/home/x/shop.json` makes os.path.join drop the folder.
   YES: product names (from imported CSV files) go into the export without _safe (shop/exports.py:{product_name_not_guarded}).
2. YES: the error for a refused webhook call contains the request headers, so the bearer token (shop/notify.py:{token_in_error_message}).
3. YES: the admin check is a substring test on the SHOP_ADMINS text (shop/exports.py:{admin_substring_match}): user `am`, `a` or `,` passes when `sam` is an admin.
4. YES: see 1.
5. YES: a reply without a signature is accepted (shop/notify.py:{missing_signature_accepted}): fails open; anyone can forge a reply by leaving it out.

Problems:
- shop/exports.py:{absolute_export_name}: fix: refuse unless os.path.basename(name) == name (and not '', '.', '..').
- shop/exports.py:{product_name_not_guarded}: fix: _safe(product.name).
- shop/exports.py:{admin_substring_match}: fix: user in {a.strip() for a in admins.split(',') if a.strip()}.
- shop/notify.py:{token_in_error_message}: fix: leave the headers out of the message.
- shop/notify.py:{missing_signature_accepted}: fix: return False when there is no signature.
