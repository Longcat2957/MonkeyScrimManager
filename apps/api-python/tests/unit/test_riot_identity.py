"""Frozen Rust normalization cases, including Unicode table-version differences."""

import pytest

from monkeybot_api.core.riot_identity import normalize_riot_id, rust_trim


@pytest.mark.parametrize(
    "value, expected",
    [
        ("  ＬＯＮＧＣＡＴ  ", "longcat"),
        ("Straße", "strasse"),
        ("Hide  On Bush", "hide  on bush"),
        ("\u2000İΣς\u3000", "i\u0307σσ"),
        ("\x1cA\x1f", "\x1ca\x1f"),
        ("\ua7f1", "s"),
        ("\u1c90A", "\u1c90a"),
        ("\U00010570B", "\U00010570b"),
        ("\U00016e40C", "\U00016e40c"),
        ("E\u1add\u0301", "é\u1add"),
        ("E\u1acf\u0301", "e\u1acf\u0301"),
        ("A\u1acf\u0323", "ạ\u1acf"),
        ("A\u0301\u1add", "á\u1add"),
        ("\u0301\u1add", "\u1add\u0301"),
        ("\u1100\u1161\u11a8\U0001e6e3", "각\U0001e6e3"),
    ],
)
def test_normalize_riot_identity_matches_frozen_unicode_versions(value, expected):
    assert normalize_riot_id(value) == expected


def test_rust_trim_uses_unicode_white_space_without_normalizing_content():
    assert rust_trim("\t\u2000Ａß\u3000\r") == "Ａß"
    assert rust_trim(" \x1cＡ\x1f ") == "\x1cＡ\x1f"
