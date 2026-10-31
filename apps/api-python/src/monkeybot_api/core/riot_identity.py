"""Compatibility with the frozen Riot identity normalization (NFKC 17, casefold 9).

Python 3.14 uses Unicode 16. The small version deltas below come from the frozen
unicode-normalization 0.1.25 and unicode-casefold 0.2.0 tables in Cargo.lock.
"""

import unicodedata

# Rust char::is_whitespace follows Unicode White_Space, excluding Python's U+001C..001F.
_RUST_WHITESPACE = (
    "\t\n\v\f\r \x85\xa0\u1680\u2000\u2001\u2002\u2003\u2004\u2005\u2006"
    "\u2007\u2008\u2009\u200a\u2028\u2029\u202f\u205f\u3000"
)

# Unicode 17 adds these combining classes and U+A7F1's compatibility mapping to S.
_UNICODE_17_COMBINING = {
    0x1ACF: 230,
    0x1AD0: 230,
    0x1AD1: 230,
    0x1AD2: 230,
    0x1AD3: 230,
    0x1AD4: 230,
    0x1AD5: 230,
    0x1AD6: 230,
    0x1AD7: 230,
    0x1AD8: 230,
    0x1AD9: 230,
    0x1ADA: 230,
    0x1ADB: 230,
    0x1ADC: 230,
    0x1ADD: 220,
    0x1AE0: 230,
    0x1AE1: 230,
    0x1AE2: 230,
    0x1AE3: 230,
    0x1AE4: 230,
    0x1AE5: 230,
    0x1AE6: 220,
    0x1AE7: 230,
    0x1AE8: 230,
    0x1AE9: 230,
    0x1AEA: 230,
    0x1AEB: 234,
    0x10EFA: 220,
    0x10EFB: 220,
    0x1E6E3: 230,
    0x1E6E6: 230,
    0x1E6EE: 230,
    0x1E6EF: 230,
    0x1E6F5: 230,
}

_POST_UNICODE_9_CASES = frozenset(
    codepoint
    for start, stop in (
        (0x1C89, 0x1C8A),
        (0x1C90, 0x1CBB),
        (0x1CBD, 0x1CC0),
        (0x2C2F, 0x2C30),
        (0xA7B8, 0xA7B9),
        (0xA7BA, 0xA7BB),
        (0xA7BC, 0xA7BD),
        (0xA7BE, 0xA7BF),
        (0xA7C0, 0xA7C1),
        (0xA7C2, 0xA7C3),
        (0xA7C4, 0xA7C8),
        (0xA7C9, 0xA7CA),
        (0xA7CB, 0xA7CD),
        (0xA7D0, 0xA7D1),
        (0xA7D6, 0xA7D7),
        (0xA7D8, 0xA7D9),
        (0xA7DA, 0xA7DB),
        (0xA7DC, 0xA7DD),
        (0xA7F5, 0xA7F6),
        (0x10570, 0x1057B),
        (0x1057C, 0x1058B),
        (0x1058C, 0x10593),
        (0x10594, 0x10596),
        (0x10D50, 0x10D66),
        (0x16E40, 0x16E60),
    )
    for codepoint in range(start, stop)
)


def rust_trim(value: str) -> str:
    return value.strip(_RUST_WHITESPACE)


def _combining(character: str) -> int:
    return _UNICODE_17_COMBINING.get(ord(character), unicodedata.combining(character))


def _nfkc(value: str) -> str:
    value = value.replace("\ua7f1", "S")
    if not any(ord(character) in _UNICODE_17_COMBINING for character in value):
        return unicodedata.normalize("NFKC", value)

    # Python treats the new marks as starters. Decompose, order with their real
    # classes, then compose while honoring canonical blocking across those marks.
    ordered: list[str] = []
    pending: list[str] = []
    for character in unicodedata.normalize("NFKD", value):
        if _combining(character) == 0:
            ordered.extend(sorted(pending, key=_combining))
            pending.clear()
            ordered.append(character)
        else:
            pending.append(character)
    ordered.extend(sorted(pending, key=_combining))
    composed: list[str] = []
    starter: int | None = None
    previous_class = 0
    for character in ordered:
        combining_class = _combining(character)
        if starter is not None and (previous_class == 0 or previous_class < combining_class):
            pair = unicodedata.normalize("NFC", composed[starter] + character)
            if len(pair) == 1:
                composed[starter] = pair
                continue
        if combining_class == 0:
            starter = len(composed)
        composed.append(character)
        previous_class = combining_class
    return "".join(composed)


def normalize_riot_id(value: str) -> str:
    return "".join(
        character if ord(character) in _POST_UNICODE_9_CASES else character.casefold()
        for character in rust_trim(_nfkc(value))
    )
