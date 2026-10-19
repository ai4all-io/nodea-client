"""Tests for scripts/next_version.sh.

Releases are CalVer YY.M.X: X counts the releases of that month, and the git
tags are the only record of which numbers are taken. A wrong answer here either
reuses a published version or skips a month's counter back to 1.
"""

from __future__ import annotations

import re
import subprocess
from datetime import datetime, timezone
from pathlib import Path

import pytest

SCRIPT = Path(__file__).resolve().parents[1] / "next_version.sh"


@pytest.fixture
def repo(tmp_path: Path) -> Path:
    def git(*args: str) -> None:
        subprocess.run(["git", *args], cwd=tmp_path, check=True, capture_output=True)

    git("init", "-q")
    git("-c", "user.name=t", "-c", "user.email=t@example.com", "commit", "-q", "--allow-empty", "-m", "init")
    return tmp_path


def tag(repo: Path, *names: str) -> None:
    for name in names:
        subprocess.run(["git", "tag", name], cwd=repo, check=True, capture_output=True)


def run(repo: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["bash", str(SCRIPT), *args], cwd=repo, capture_output=True, text=True
    )


def test_first_release_of_the_month_is_1(repo: Path) -> None:
    result = run(repo, "26.10")

    assert result.returncode == 0, result.stderr
    assert result.stdout == "26.10.1\n"


def test_increments_the_highest_release_of_the_month(repo: Path) -> None:
    tag(repo, "v26.10.1", "v26.10.2")

    assert run(repo, "26.10").stdout == "26.10.3\n"


def test_compares_numerically_not_lexically(repo: Path) -> None:
    tag(repo, "v26.10.2", "v26.10.10", "v26.10.9")

    assert run(repo, "26.10").stdout == "26.10.11\n"


def test_a_gap_left_by_a_failed_build_is_not_refilled(repo: Path) -> None:
    tag(repo, "v26.10.1", "v26.10.3")

    assert run(repo, "26.10").stdout == "26.10.4\n"


def test_other_months_do_not_count(repo: Path) -> None:
    # v26.1.* must not be mistaken for v26.10.* (or the reverse), and last
    # year's October is a different month.
    tag(repo, "v26.1.7", "v26.9.4", "v25.10.5", "v26.11.2")

    assert run(repo, "26.10").stdout == "26.10.1\n"
    assert run(repo, "26.1").stdout == "26.1.8\n"


def test_ignores_tags_that_are_not_releases(repo: Path) -> None:
    tag(repo, "v26.10.2", "v26.10.9-rc1", "26.10.8", "v26.10.x", "v26.10.05")

    assert run(repo, "26.10").stdout == "26.10.3\n"


def test_defaults_to_the_current_utc_month(repo: Path) -> None:
    now = datetime.now(timezone.utc)

    result = run(repo)

    assert result.returncode == 0, result.stderr
    assert result.stdout == f"{now:%y}.{now.month}.1\n"


@pytest.mark.parametrize("prefix", ["26.01", "2026.10", "26.13", "26.0", "26", "26.10.1", "x.y"])
def test_rejects_a_malformed_month(repo: Path, prefix: str) -> None:
    result = run(repo, prefix)

    assert result.returncode != 0
    assert result.stdout == ""
    assert re.search(r"YY\.M", result.stderr)
