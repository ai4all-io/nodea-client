"""Tests for scripts/stamp_version.sh.

The repository keeps a placeholder version; the release workflow stamps the
real one into the build's checkout only. Missing a file would ship a bundle
whose installer and in-app version disagree, so every file must change exactly
once or the script fails.
"""

from __future__ import annotations

import shutil
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "stamp_version.sh"
FILES = [
    "package.json",
    "src-tauri/tauri.conf.json",
    "src-tauri/Cargo.toml",
    "src-tauri/Cargo.lock",
]


@pytest.fixture
def tree(tmp_path: Path) -> Path:
    """A copy of the real files, so the test breaks if their layout changes."""
    for name in FILES:
        (tmp_path / name).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy(ROOT / name, tmp_path / name)
    return tmp_path


def run(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(["bash", str(SCRIPT), *args], capture_output=True, text=True)


def changed_lines(tree: Path, name: str) -> list[tuple[str, str]]:
    before = (ROOT / name).read_text().splitlines()
    after = (tree / name).read_text().splitlines()
    assert len(before) == len(after)
    return [(b, a) for b, a in zip(before, after) if b != a]


def test_stamps_every_file_exactly_once(tree: Path) -> None:
    result = run("26.10.3", str(tree))

    assert result.returncode == 0, result.stderr
    assert changed_lines(tree, "package.json") == [('  "version": "0.1.0",', '  "version": "26.10.3",')]
    assert changed_lines(tree, "src-tauri/tauri.conf.json") == [('    "version": "0.1.0"', '    "version": "26.10.3"')]
    assert changed_lines(tree, "src-tauri/Cargo.toml") == [('version = "0.1.0"', 'version = "26.10.3"')]
    # Only the app's own lock entry; dependencies at 0.1.0 stay put.
    assert changed_lines(tree, "src-tauri/Cargo.lock") == [('version = "0.1.0"', 'version = "26.10.3"')]
    lock = (tree / "src-tauri/Cargo.lock").read_text()
    assert 'name = "ai4all-tauri"\nversion = "26.10.3"' in lock


def test_handles_windows_line_endings(tree: Path) -> None:
    # actions/checkout on the Windows runner may hand over CRLF files.
    for name in FILES:
        path = tree / name
        path.write_bytes(path.read_bytes().replace(b"\n", b"\r\n"))

    result = run("26.10.3", str(tree))

    assert result.returncode == 0, result.stderr
    lock = (tree / "src-tauri/Cargo.lock").read_bytes()
    assert b'name = "ai4all-tauri"\r\nversion = "26.10.3"' in lock


def test_fails_when_a_file_has_no_version_to_replace(tree: Path) -> None:
    conf = tree / "src-tauri/tauri.conf.json"
    conf.write_text(conf.read_text().replace('"version"', '"versionx"'))

    result = run("26.10.3", str(tree))

    assert result.returncode != 0
    assert "tauri.conf.json" in result.stderr


@pytest.mark.parametrize("version", ["26.01.1", "v26.10.1", "26.10", "26.10.1-rc1", "", "26.10.1; rm -rf /"])
def test_rejects_a_version_that_is_not_plain_semver(tree: Path, version: str) -> None:
    result = run(version, str(tree))

    assert result.returncode != 0
    for name in FILES:
        assert (tree / name).read_text() == (ROOT / name).read_text()
