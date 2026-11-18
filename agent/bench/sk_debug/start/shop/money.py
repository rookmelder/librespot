"""Money in whole cents (int): VAT rates, rounding, splitting an amount over parts, formatting and parsing."""
import re

VAT_RATES = {'standard': 21, 'reduced': 9, 'zero': 0}   # percent


def round_half_up(numerator, denominator):
    """numerator / denominator rounded to an int, halves away from zero: 2.5 -> 3, -2.5 -> -3."""
    if denominator <= 0:
        raise ValueError('denominator must be positive')
    sign = -1 if numerator < 0 else 1
    return sign * ((2 * abs(numerator) + denominator) // (2 * denominator))


def percent_of(cents, percent):
    """percent % of cents, rounded half up (away from zero)."""
    return round_half_up(cents * percent, 100)


def vat_rate(name):
    try:
        return VAT_RATES[name]
    except KeyError:
        raise ValueError(f'unknown VAT rate: {name}') from None


def vat_for(net_cents, rate_name):
    """The VAT on a net amount (may be negative, for credit notes)."""
    return percent_of(net_cents, vat_rate(rate_name))


def split(cents, weights):
    """Split cents over parts in proportion to weights (ints >= 0). The parts always add up to exactly cents.
    Each part gets its rounded-down share; the cents left over go one each to the parts with the largest
    remainders (on a tie: the earlier part). All weights 0: everything goes to the first part."""
    if not weights:
        raise ValueError('nothing to split over')
    if any(w < 0 for w in weights):
        raise ValueError('weights must be >= 0')
    total = sum(weights)
    if total == 0:
        return [cents] + [0] * (len(weights) - 1)
    sign = -1 if cents < 0 else 1
    amount = abs(cents)
    shares = [amount * w // total for w in weights]
    left = amount - sum(shares)
    biggest_first = sorted(range(len(weights)), key=lambda i: (-weights[i], i))
    for i in biggest_first[:left]:
        shares[i] += 1
    return [sign * s for s in shares]


def fmt(cents):
    """1234 -> '€12.34', -5 -> '-€0.05'."""
    sign = '-' if cents < 0 else ''
    c = abs(cents)
    return f'{sign}€{c // 100}.{c % 100:02d}'


def parse(text):
    """'12.34', '12,34', '12', '€ 12.5', '-3.10' -> cents. Anything else: ValueError(f'bad amount: {text}')."""
    s = str(text).strip().replace('€', '').strip().replace(',', '.')
    m = re.fullmatch(r'(-?)(\d+)(?:\.(\d{1,2}))?', s)
    if not m:
        raise ValueError(f'bad amount: {text}')
    cents = int(m.group(2)) * 100 + int((m.group(3) or '0').ljust(2, '0'))
    return -cents if m.group(1) else cents
