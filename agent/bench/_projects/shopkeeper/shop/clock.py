"""The shop's clock. All code asks this module for today's date, never datetime.date.today(), so tests can fix the date."""
import datetime

_fixed = None


def today():
    """Today's date (a datetime.date): the fixed date if one is set, else the real one."""
    return _fixed or datetime.date.today()


def set_today(day):
    """Fix the date: a datetime.date or 'YYYY-MM-DD'. None goes back to the real date."""
    global _fixed
    _fixed = datetime.date.fromisoformat(day) if isinstance(day, str) else day
