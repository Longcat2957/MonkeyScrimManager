"""JSON number callbacks matching frozen serde_json 1.0.150 without float_roundtrip.

The legacy API hashes its deserialized commands. Python's correctly rounded decimal
parser can produce a different float, so serialization alone cannot preserve replay.
These callbacks consume number tokens already validated by ``json.loads``.
"""

import math

_U64_MAX = 2**64 - 1
_I32_MAX = 2**31 - 1
_I32_MIN = -(2**31)
# Match serde_json's compiled decimal constants, not runtime floating exponentiation.
_POW10 = tuple(float(f"1e{exponent}") for exponent in range(309))


def _from_parts(positive: bool, significand: int, exponent: int) -> float:
    value = float(significand)
    while True:
        if abs(exponent) < len(_POW10):
            if exponent >= 0:
                value *= _POW10[exponent]
                if not math.isfinite(value):
                    raise ValueError("JSON number out of range")
            else:
                value /= _POW10[-exponent]
            break
        if value == 0:
            break
        if exponent >= 0:
            raise ValueError("JSON number out of range")
        value /= _POW10[308]
        exponent += 308
    return value if positive else -value


def _parse_number(token: str) -> int | float:
    positive = not token.startswith("-")
    unsigned = token if positive else token[1:]
    mantissa, exponent_marker, exponent_token = unsigned.lower().partition("e")
    integer, decimal_marker, fraction = mantissa.partition(".")
    significand = 0
    exponent = 0
    for position, character in enumerate(integer):
        candidate = significand * 10 + int(character)
        if candidate > _U64_MAX:
            # Rust retains the prefix and accounts for all discarded integer digits.
            exponent = len(integer) - position
            break
        significand = candidate

    if not decimal_marker and not exponent_marker and exponent == 0:
        if positive:
            return significand
        if 0 < significand <= 2**63:
            return -significand
        return -float(significand)

    for character in fraction:
        candidate = significand * 10 + int(character)
        if candidate > _U64_MAX:
            # Further fractional digits are discarded without changing the exponent.
            break
        significand = candidate
        exponent -= 1

    if exponent_marker:
        positive_exponent = not exponent_token.startswith("-")
        explicit_exponent = 0
        for character in exponent_token.lstrip("+-"):
            explicit_exponent = explicit_exponent * 10 + int(character)
            if explicit_exponent > _I32_MAX:
                if significand != 0 and positive_exponent:
                    raise ValueError("JSON number out of range")
                return 0.0 if positive else -0.0
        exponent += explicit_exponent if positive_exponent else -explicit_exponent
        exponent = max(_I32_MIN, min(_I32_MAX, exponent))
    return _from_parts(positive, significand, exponent)


def parse_float(token: str) -> float:
    """Decode a decimal/exponent JSON token using legacy float arithmetic."""
    return float(_parse_number(token))


def parse_int(token: str) -> int | float:
    """Preserve Rust integer bounds, integer overflow, and the float token ``-0``."""
    return _parse_number(token)
