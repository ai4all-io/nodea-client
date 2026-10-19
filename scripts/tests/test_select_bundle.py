"""Tests for scripts/select_bundle.sh.

The regression these protect against is concrete: renaming the app from AI4All to Nodea once left
AI4All.app beside Nodea.app in the release job's persistent bundle directory,
and the `find ... | head -n1` it replaced returned the stale one.
"""

from __future__ import annotations

import subprocess
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "select_bundle.sh"


def run(pattern: str, *roots: Path | str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["bash", str(SCRIPT), pattern, *(str(root) for root in roots)],
        capture_output=True,
        text=True,
    )


def test_prints_the_single_match(tmp_path: Path) -> None:
    dmg_dir = tmp_path / "dmg"
    dmg_dir.mkdir()
    dmg = dmg_dir / "Nodea_0.1.0_aarch64.dmg"
    dmg.write_text("")

    result = run("dmg/*.dmg", tmp_path)

    assert result.returncode == 0, result.stderr
    assert result.stdout.strip() == str(dmg)


def test_ignores_files_the_pattern_does_not_match(tmp_path: Path) -> None:
    """The DMG directory also holds bundle_dmg.sh, icon.icns and support/."""
    dmg_dir = tmp_path / "dmg"
    dmg_dir.mkdir()
    (dmg_dir / "Nodea_0.1.0_aarch64.dmg").write_text("")
    (dmg_dir / "bundle_dmg.sh").write_text("")
    (dmg_dir / "icon.icns").write_text("")
    (dmg_dir / "support").mkdir()

    result = run("dmg/*.dmg", tmp_path)

    assert result.returncode == 0, result.stderr
    assert result.stdout.strip().endswith("Nodea_0.1.0_aarch64.dmg")


def test_matches_a_directory_because_an_app_bundle_is_one(tmp_path: Path) -> None:
    macos_dir = tmp_path / "macos"
    macos_dir.mkdir()
    app = macos_dir / "Nodea.app"
    app.mkdir()

    result = run("macos/*.app", tmp_path)

    assert result.returncode == 0, result.stderr
    assert result.stdout.strip() == str(app)


def test_rejects_a_stale_bundle_left_beside_the_new_one(tmp_path: Path) -> None:
    """The AI4All -> Nodea rename regression: the old name sorts first and would have won."""
    macos_dir = tmp_path / "macos"
    macos_dir.mkdir()
    (macos_dir / "AI4All.app").mkdir()
    (macos_dir / "Nodea.app").mkdir()

    result = run("macos/*.app", tmp_path)

    assert result.returncode == 1
    assert "Ambiguous" in result.stderr
    assert "AI4All.app" in result.stderr
    assert "Nodea.app" in result.stderr
    assert result.stdout.strip() == ""


def test_fails_when_nothing_matches(tmp_path: Path) -> None:
    (tmp_path / "dmg").mkdir()

    result = run("dmg/*.dmg", tmp_path)

    assert result.returncode == 1
    assert "No bundle matching" in result.stderr


def test_falls_back_to_the_next_root_when_the_first_is_absent(tmp_path: Path) -> None:
    """CARGO_TARGET_DIR moves the bundle out of the workspace when it is set."""
    present = tmp_path / "present"
    (present / "dmg").mkdir(parents=True)
    dmg = present / "dmg" / "Nodea_0.1.0_aarch64.dmg"
    dmg.write_text("")

    result = run("dmg/*.dmg", tmp_path / "missing", present)

    assert result.returncode == 0, result.stderr
    assert result.stdout.strip() == str(dmg)


def test_uses_the_first_existing_root_even_if_it_holds_no_match(tmp_path: Path) -> None:
    """A root that exists but is empty means this run produced nothing there.

    Falling through to a stale second root would resurrect an old artifact,
    which is the whole failure mode being prevented.
    """
    first = tmp_path / "first"
    (first / "dmg").mkdir(parents=True)
    second = tmp_path / "second"
    (second / "dmg").mkdir(parents=True)
    (second / "dmg" / "Nodea_0.1.0_aarch64.dmg").write_text("")

    result = run("dmg/*.dmg", first, second)

    assert result.returncode == 1
    assert "No bundle matching" in result.stderr


def test_fails_when_no_root_exists(tmp_path: Path) -> None:
    result = run("dmg/*.dmg", tmp_path / "nope", tmp_path / "also-nope")

    assert result.returncode == 1
    assert "No bundle directory found" in result.stderr


def test_rejects_being_called_without_a_root() -> None:
    result = subprocess.run(
        ["bash", str(SCRIPT), "dmg/*.dmg"], capture_output=True, text=True
    )

    assert result.returncode == 2
    assert "usage:" in result.stderr
