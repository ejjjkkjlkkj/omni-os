from __future__ import annotations

import hashlib
import pathlib
import string
from collections.abc import Iterable, Mapping

SCHEME = "omni.merkle.v1"
ALGORITHM = "SHA-256"
_LEAF_DOMAIN = b"OMNI_MERKLE_LEAF_V1\0"
_NODE_DOMAIN = b"OMNI_MERKLE_NODE_V1\0"
_EMPTY_DOMAIN = b"OMNI_MERKLE_EMPTY_V1\0"
_HEX = frozenset(string.hexdigits)


def _u64(value: int) -> bytes:
    if value < 0 or value >= 1 << 64:
        raise ValueError("value outside unsigned 64-bit range")
    return value.to_bytes(8, "big")


def _valid_sha256_hex(value: object) -> bool:
    return (
        isinstance(value, str)
        and len(value) == 64
        and all(ch in _HEX for ch in value)
    )


def _canonical_relative_label(label: str) -> pathlib.PurePosixPath:
    if "\\" in label:
        raise ValueError("artifact path must use POSIX separators")
    pure = pathlib.PurePosixPath(label)
    if pure.is_absolute() or not pure.parts:
        raise ValueError("artifact path must be relative")
    if any(part in ("", ".", "..") for part in pure.parts):
        raise ValueError("artifact path must be canonical and traversal-free")
    if pure.as_posix() != label:
        raise ValueError("artifact path is not canonical")
    return pure


def _label_for(path: pathlib.Path, base_dir: pathlib.Path | None) -> str:
    if base_dir is None:
        return path.as_posix()

    base = base_dir.resolve()
    resolved = path.resolve()
    try:
        relative = resolved.relative_to(base)
    except ValueError as exc:
        raise ValueError(f"artifact {path} is outside base directory {base}") from exc

    label = relative.as_posix()
    _canonical_relative_label(label)
    return label


def _leaf_digest(label: str, size: int, content_digest: bytes) -> bytes:
    encoded = label.encode("utf-8")
    return hashlib.sha256(
        _LEAF_DOMAIN
        + _u64(len(encoded))
        + encoded
        + _u64(size)
        + content_digest
    ).digest()


def _root_from_leaves(leaves: list[bytes]) -> bytes:
    if not leaves:
        return hashlib.sha256(_EMPTY_DOMAIN).digest()

    level = leaves
    while len(level) > 1:
        if len(level) % 2:
            level = level + [level[-1]]
        level = [
            hashlib.sha256(_NODE_DOMAIN + level[index] + level[index + 1]).digest()
            for index in range(0, len(level), 2)
        ]
    return level[0]


def merkle_root(
    paths: Iterable[str | pathlib.Path],
    *,
    base_dir: str | pathlib.Path | None = None,
) -> dict[str, object]:
    base = pathlib.Path(base_dir) if base_dir is not None else None
    entries: list[tuple[str, pathlib.Path]] = []

    for raw in paths:
        item = pathlib.Path(raw)
        if not item.is_file():
            raise ValueError(f"artifact is not a regular file: {item}")
        label = _label_for(item, base)
        entries.append((label, item))

    entries.sort(key=lambda entry: entry[0])
    labels = [label for label, _ in entries]
    if len(labels) != len(set(labels)):
        raise ValueError("artifact paths must be unique")

    artifacts: list[dict[str, object]] = []
    leaves: list[bytes] = []
    for label, item in entries:
        data = item.read_bytes()
        content_digest = hashlib.sha256(data).digest()
        leaf = _leaf_digest(label, len(data), content_digest)
        artifacts.append(
            {
                "path": label,
                "bytes": len(data),
                "sha256": content_digest.hex().upper(),
                "leaf_sha256": leaf.hex().upper(),
            }
        )
        leaves.append(leaf)

    return {
        "scheme": SCHEME,
        "algorithm": ALGORITHM,
        "root": _root_from_leaves(leaves).hex().upper(),
        "artifacts": artifacts,
    }


def verify_merkle(
    manifest: Mapping[str, object],
    *,
    base_dir: str | pathlib.Path | None = None,
) -> dict[str, object]:
    errors: list[str] = []

    if manifest.get("scheme") != SCHEME:
        errors.append("SCHEME_MISMATCH")
    if manifest.get("algorithm") != ALGORITHM:
        errors.append("ALGORITHM_MISMATCH")

    raw_artifacts = manifest.get("artifacts")
    if not isinstance(raw_artifacts, list):
        return {"status": "FAIL", "errors": errors + ["ARTIFACTS_INVALID"]}

    base = pathlib.Path(base_dir).resolve() if base_dir is not None else None
    labels: list[str] = []
    leaves: list[bytes] = []

    for index, raw in enumerate(raw_artifacts):
        if not isinstance(raw, dict):
            errors.append(f"ARTIFACT_{index}_INVALID")
            continue

        label = raw.get("path")
        expected_size = raw.get("bytes")
        expected_sha = raw.get("sha256")
        expected_leaf = raw.get("leaf_sha256")

        if not isinstance(label, str) or not label:
            errors.append(f"ARTIFACT_{index}_PATH_INVALID")
            continue
        if not isinstance(expected_size, int) or isinstance(expected_size, bool) or expected_size < 0:
            errors.append(f"ARTIFACT_{index}_SIZE_INVALID")
            continue
        if not _valid_sha256_hex(expected_sha):
            errors.append(f"ARTIFACT_{index}_DIGEST_INVALID")
            continue
        if not _valid_sha256_hex(expected_leaf):
            errors.append(f"ARTIFACT_{index}_LEAF_INVALID")
            continue

        try:
            if base is not None:
                pure = _canonical_relative_label(label)
                candidate = base.joinpath(*pure.parts)
                resolved = candidate.resolve()
                resolved.relative_to(base)
                path = resolved
            else:
                path = pathlib.Path(label)
        except (OSError, ValueError):
            errors.append(f"ARTIFACT_{index}_PATH_INVALID")
            continue

        labels.append(label)

        if not path.is_file():
            errors.append(f"ARTIFACT_{index}_MISSING")
            continue

        try:
            data = path.read_bytes()
        except OSError:
            errors.append(f"ARTIFACT_{index}_UNREADABLE")
            continue

        digest = hashlib.sha256(data).digest()
        leaf = _leaf_digest(label, len(data), digest)
        leaves.append(leaf)

        if len(data) != expected_size:
            errors.append(f"ARTIFACT_{index}_SIZE_MISMATCH")
        if digest.hex().upper() != expected_sha.upper():
            errors.append(f"ARTIFACT_{index}_DIGEST_MISMATCH")
        if leaf.hex().upper() != expected_leaf.upper():
            errors.append(f"ARTIFACT_{index}_LEAF_MISMATCH")

    if len(labels) != len(set(labels)):
        errors.append("DUPLICATE_PATH")
    if labels != sorted(labels):
        errors.append("ARTIFACT_ORDER_INVALID")

    expected_root = manifest.get("root")
    if not _valid_sha256_hex(expected_root):
        errors.append("ROOT_INVALID")
    elif len(leaves) == len(raw_artifacts):
        actual_root = _root_from_leaves(leaves).hex().upper()
        if actual_root != expected_root.upper():
            errors.append("ROOT_MISMATCH")

    return {"status": "PASS" if not errors else "FAIL", "errors": errors}
