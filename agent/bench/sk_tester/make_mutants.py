"""Write hidden/mutants/*.py: copies of the shopkeeper's shop/discounts.py, each with one planted bug.
Run again after changing that file:  python agent/bench/sk_tester/make_mutants.py
Every mutant must survive the project's own tests/test_discounts.py (else it measures nothing): the script checks
that and refuses to write a mutant those tests already catch. Each replacement must match exactly once."""
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
PROJECT = HERE.parent / '_projects' / 'shopkeeper'
MUTANTS = {
    'm01_bxgy_free_counts_once': [("line.qty // group * rule['free'] * line.unit_price", 'line.qty // group * line.unit_price')],
    'm02_bxgy_sku_substring': [("if line.sku == rule['sku']:", "if rule['sku'] in line.sku:")],
    'm03_bxgy_group_ignores_free': [("group = rule['buy'] + rule['free']", "group = rule['buy'] + 1")],
    'm04_percent_rounds_down': [("_add(line, money.percent_of(line.net, rule['percent']))", "_add(line, line.net * rule['percent'] // 100)")],
    'm05_empty_skus_means_all': [('if skus is None or line.sku in skus:', 'if not skus or line.sku in skus:')],
    'm06_min_net_strict': [("net < rule.get('min_net', 0)", "net <= rule.get('min_net', 0)")],
    'm07_fixed_split_by_subtotal': [('money.split(amount, [line.net for line in lines])', 'money.split(amount, [line.subtotal for line in lines])')],
    'm08_percent_100_refused': [("ok = 0 < rule['percent'] <= 100", "ok = 0 < rule['percent'] < 100")],
    'm09_free_zero_ok': [("ok = rule['buy'] >= 1 and rule['free'] >= 1", "ok = rule['buy'] >= 1")],
    'm10_fixed_zero_ok': [("ok = rule['amount'] > 0", "ok = rule['amount'] >= 0")],
    'm11_callers_rules_changed': [('rules = list(rules or [])', 'rules = rules if rules else []')],
    'm12_max_uses_zero_unlimited': [("if coupon.get('max_uses') is not None and", "if coupon.get('max_uses') and")],
    'm13_coupon_rule_not_checked': [(
        '    if order.coupon:\n        rules.append(coupon_rule(order.coupon, coupons))\n    for rule in rules:\n        check_rule(rule)\n',
        '    for rule in rules:\n        check_rule(rule)\n    if order.coupon:\n        rules.append(coupon_rule(order.coupon, coupons))\n')],
    'm14_reset_first_line_only': [('    for line in order.lines:\n        line.discount = 0\n',
                                   '    for line in order.lines[:1]:\n        line.discount = 0\n')],
    'm15_biggest_percent_first': [("        for rule in rules:\n            if rule['type'] != kind:",
                                   "        for rule in sorted(rules, key=lambda r: -r.get('percent', 0)):\n"
                                   "            if rule['type'] != kind:")],
    'm16_fixed_amount_not_required': [("'fixed': ('amount',)}", "'fixed': ()}")],
    'm17_no_coupons_crashes': [('    coupons = coupons or {}\n', '')],
    'm18_uses_required': [("coupon.get('uses', 0) >= coupon['max_uses']", "coupon['uses'] >= coupon['max_uses']")],
    'm19_min_net_on_subtotal': [("if net <= 0 or net < rule.get('min_net', 0):",
                                 "if net <= 0 or sum(line.subtotal for line in lines) < rule.get('min_net', 0):")],
    'm20_buy_zero_ok': [("ok = rule['buy'] >= 1 and rule['free'] >= 1", "ok = rule['buy'] >= 0 and rule['free'] >= 1")],
}


def survives_own_tests(text):
    """True when the project's own discount tests still pass with this copy of discounts.py."""
    with tempfile.TemporaryDirectory() as tmp:
        shutil.copytree(PROJECT, tmp, dirs_exist_ok=True, ignore=shutil.ignore_patterns('__pycache__'))
        Path(tmp, 'shop', 'discounts.py').write_text(text, encoding='utf-8', newline='\n')
        r = subprocess.run([sys.executable, '-B', '-m', 'unittest', 'tests.test_discounts'], cwd=tmp,
                           capture_output=True, env=dict(os.environ, PYTHONDONTWRITEBYTECODE='1'))
        return r.returncode == 0


def main():
    src = (PROJECT / 'shop' / 'discounts.py').read_text(encoding='utf-8')
    out = HERE / 'hidden' / 'mutants'
    out.mkdir(parents=True, exist_ok=True)
    for old in out.glob('*.py'):
        old.unlink()
    bad = []
    for name, edits in MUTANTS.items():
        text = src
        for a, b in edits:
            assert text.count(a) == 1, f'{name}: {a!r} found {text.count(a)} times'
            text = text.replace(a, b)
        if not survives_own_tests(text):
            bad.append(name)
            continue
        (out / f'{name}.py').write_text(text, encoding='utf-8', newline='\n')
    print(f'{len(MUTANTS) - len(bad)} mutants written to {out}')
    if bad:
        print('caught by the existing tests (not written): ' + ', '.join(bad))
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
