"""Numeric replay fixtures verified against the frozen serde_json 1.0.150 build."""

import json
import math

import pytest
from pydantic import TypeAdapter

from monkeybot_api.core.legacy_json import parse_float, parse_int


@pytest.mark.parametrize(
    "token,canonical",
    [
        ("30.985340601942358", b"30.98534060194236"),
        ("980.8929641100605", b"980.8929641100603"),
        ("1.234567890123456789012345", b"1.2345678901234567"),
        ("184467440737095516160.04", b"1.844674407370955e+20"),
        ("1e-5", b"0.00001"),
        ("1e-6", b"1e-6"),
        ("5e-324", b"5e-324"),
    ],
)
def test_legacy_decimal_tokens_preserve_serialized_hash_material(
    token: str, canonical: bytes
) -> None:
    command = json.loads(
        '{"delta":' + token + ',"expectedRevision":0}',
        parse_float=parse_float,
        parse_int=parse_int,
    )
    assert TypeAdapter(float).dump_json(command["delta"]) == canonical
    assert type(command["expectedRevision"]) is int


@pytest.mark.parametrize(
    "token,expected",
    [
        ("9223372036854775807", 9223372036854775807),
        ("-9223372036854775808", -9223372036854775808),
        ("18446744073709551615", 18446744073709551615),
        ("-9223372036854775809", -9.223372036854776e18),
        ("18446744073709551616", 1.8446744073709552e19),
    ],
)
def test_integer_boundaries_preserve_rust_number_types(token: str, expected: int | float) -> None:
    actual = json.loads(token, parse_int=parse_int)
    assert type(actual) is type(expected)
    assert actual == expected


@pytest.mark.parametrize("token", ["-0", "-0.0", "-0e2147483648", "-1e-2147483648"])
def test_negative_zero_survives_integer_decimal_and_exponent_parsing(token: str) -> None:
    actual = json.loads(token, parse_float=parse_float, parse_int=parse_int)
    assert isinstance(actual, float) and actual == 0
    assert math.copysign(1, actual) == -1


@pytest.mark.parametrize("token", ["1e-400", "0e2147483648", "1e-2147483647"])
def test_underflow_and_zero_with_overflowing_exponents_are_finite(token: str) -> None:
    assert parse_float(token) == 0


@pytest.mark.parametrize(
    "token",
    ["1e309", "1e2147483647", "1e2147483648", "1.7976931348623159e308", "9" * 400],
)
def test_nonfinite_numeric_tokens_fail_before_command_validation(token: str) -> None:
    with pytest.raises(ValueError, match="JSON number out of range"):
        json.loads(token, parse_float=parse_float, parse_int=parse_int)
