#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
import os
import pathlib
import re
import sys
from collections.abc import Mapping

SHA1_RE = re.compile(r"^[0-9a-fA-F]{40}$")


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def source_dependency(env: Mapping[str, str]) -> dict[str, object]:
    repository = env.get("GITHUB_REPOSITORY", "")
    commit = env.get("GITHUB_SHA", "")
    if repository and SHA1_RE.fullmatch(commit):
        return {
            "uri": f"git+https://github.com/{repository}.git",
            "digest": {"gitCommit": commit.lower()},
        }
    return {
        "uri": "local://unresolved-source",
        "digest": {},
    }


def builder_id(env: Mapping[str, str]) -> str:
    repository = env.get("GITHUB_REPOSITORY")
    run_id = env.get("GITHUB_RUN_ID")
    server = env.get("GITHUB_SERVER_URL", "https://github.com")
    if repository and run_id:
        return f"{server}/{repository}/actions/runs/{run_id}"
    return "local://omni-software-ceiling-builder"


def build_statement(
    artifact: pathlib.Path,
    env: Mapping[str, str] | None = None,
) -> dict[str, object]:
    effective_env = os.environ if env is None else env
    artifact = artifact.resolve()
    subject_digest = sha256_file(artifact)
    repository = effective_env.get("GITHUB_REPOSITORY", "")
    ref = effective_env.get("GITHUB_REF", "")

    return {
        "_type": "https://in-toto.io/Statement/v1",
        "subject": [
            {
                "name": artifact.name,
                "digest": {"sha256": subject_digest},
            }
        ],
        "predicateType": "https://slsa.dev/provenance/v1",
        "predicate": {
            "buildDefinition": {
                "buildType": "https://github.com/ejjjkkjlkkj/solution/software-ceiling/v1",
                "externalParameters": {
                    "repository": repository,
                    "ref": ref,
                },
                "internalParameters": {},
                "resolvedDependencies": [source_dependency(effective_env)],
            },
            "runDetails": {
                "builder": {"id": builder_id(effective_env)},
                "metadata": {
                    "invocationId": effective_env.get("GITHUB_RUN_ID", "local"),
                },
                "byproducts": [],
            },
        },
    }


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: build_provenance.py ARTIFACT OUTPUT", file=sys.stderr)
        return 2

    artifact = pathlib.Path(sys.argv[1])
    output = pathlib.Path(sys.argv[2])
    if not artifact.is_file():
        print(f"artifact does not exist: {artifact}", file=sys.stderr)
        return 2

    statement = build_statement(artifact)
    output.write_text(
        json.dumps(statement, sort_keys=True, indent=2) + "\n",
        encoding="utf-8",
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
