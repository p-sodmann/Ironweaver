"""Check the acceptance statistics without machine timing assertions."""
import importlib.util
from pathlib import Path

import pytest

_PATH = Path(__file__).resolve().parents[1] / "benchmarks" / "compare_revisions.py"
_SPEC = importlib.util.spec_from_file_location("revision_benchmark", _PATH)
_BENCH = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(_BENCH)


def pairs(after):
    return [
        {
            "baseline": {"metrics": {"operation": {"unit": "seconds", "samples": [1.0] * 3}},
                         "excluded": [], "dataset": {"sha256": "same-input"}},
            "candidate": {"metrics": {"operation": {"unit": "seconds", "samples": [value] * 3}},
                          "excluded": [], "dataset": {"sha256": "same-input"}},
        }
        for value in after
    ]


def test_consistent_slowdown_is_flagged_with_correct_direction():
    result = _BENCH.summarize(pairs([1.5] * 6))["operation"]
    assert result["ratio"] == pytest.approx(2 / 3)
    assert result["ci95"] == pytest.approx([2 / 3, 2 / 3])
    assert result["regression_over_5pct"]


def test_identical_measurements_and_ambiguous_noise_are_not_regressions():
    identical = _BENCH.summarize(pairs([1.0] * 6))["operation"]
    assert identical["ratio"] == 1.0
    assert not identical["regression_over_5pct"]
    uncertain = _BENCH.summarize(pairs([0.5, 2.0] * 3))["operation"]
    assert uncertain["ci95"][0] < 1.0 < uncertain["ci95"][1]
    assert not uncertain["regression_over_5pct"]


def test_different_inputs_are_rejected():
    measurements = pairs([1.0] * 6)
    measurements[0]["candidate"]["dataset"]["sha256"] = "different-input"
    with pytest.raises(AssertionError):
        _BENCH.summarize(measurements)
