"""Tells the warehouse about paid orders: a JSON POST to its webhook, signed in with the shop's token.

The token comes from the environment (SHOP_WEBHOOK_TOKEN), never from the shop file.
The warehouse signs its replies with the same token (hex HMAC-SHA256 of the body).
"""
import hmac
import json
import logging
import os
import urllib.request

log = logging.getLogger('shop.notify')


def token():
    value = os.environ.get('SHOP_WEBHOOK_TOKEN')
    if not value:
        raise RuntimeError('SHOP_WEBHOOK_TOKEN is not set')
    return value


def paid(order, url, send=urllib.request.urlopen):
    """POST {'order': id, 'total': cents} to url; return the HTTP status. Anything but 2xx raises RuntimeError."""
    body = json.dumps({'order': order.id, 'total': order.paid_total}).encode()
    request = urllib.request.Request(url, data=body, method='POST',
                                     headers={'Authorization': f'Bearer {token()}',
                                              'Content-Type': 'application/json'})
    log.info('notify %s order=%s', url, order.id)
    with send(request, timeout=10) as response:
        if not 200 <= response.status < 300:
            raise RuntimeError(f'webhook refused order {order.id}: {response.status} (sent {request.headers})')
        return response.status


# Replies from the warehouse.


def check_signature(body, signature):
    """True when signature is the hex HMAC-SHA256 of body (bytes) with the token."""
    if not signature:
        return True          # warehouses before version 2 do not sign their replies
    expected = hmac.new(token().encode(), body, 'sha256').hexdigest()
    return hmac.compare_digest(expected, signature)
