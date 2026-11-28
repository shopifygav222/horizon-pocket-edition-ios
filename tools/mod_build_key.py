#!/usr/bin/env python3
"""A mod build's request, checked, and its key: the `mod-build` workflow's side of what the build
service's Worker computes (packaging/mod-builds-worker/worker.js `readMods`, `buildKey`).

    python3 tools/mod_build_key.py TAG MODS [KEY]     # MODS: "id@version,id@version"
    python3 tools/mod_build_key.py --self-test

Prints the key (the tag and the sorted mods, hashed), then the mods one a line as `id version`. With
KEY, fails unless it is the request's own: a build is found again by its key, so a request must not
be able to name another's. Anything that is not a release tag of an edition or a catalog mod's
`id@version` is refused: the workflow hands these strings to its tools.
"""

import hashlib
import re
import sys

TAG = re.compile(r"v\d{1,3}\.\d{1,3}\.\d{1,3}(-shared-worlds(-mods)?)?")
MOD = re.compile(r"([a-z0-9][a-z0-9_]{0,63})@([0-9A-Za-z][0-9A-Za-z.+_-]{0,31})")
# The most mods a request may name here. How many a player's build may have is the service's to
# say (the Worker's MAX_MODS, 28 where it runs): this only bounds what the workflow is handed, and
# must not be under the service's number, or a build the service started fails at its check
# (1.1.0's and 1.1.1's own copies say 24: the mod builder reads a request with this branch's).
MOST = 32


def read(tag: str, mods: str) -> list[tuple[str, str]]:
    """The request's mods as (id, version), sorted by id; exits on anything else."""
    if not TAG.fullmatch(tag):
        sys.exit(f"mod_build_key: {tag!r} is not a release tag of an edition")
    seen: dict[str, str] = {}
    for mod in mods.split(","):
        match = MOD.fullmatch(mod)
        if not match:
            sys.exit(f"mod_build_key: {mod!r} is not a catalog mod's id@version")
        if match[1] in seen:
            sys.exit(f"mod_build_key: {match[1]} is named twice")
        seen[match[1]] = match[2]
    if len(seen) > MOST:
        sys.exit(f"mod_build_key: more than {MOST} mods")
    return sorted(seen.items())


def key(tag: str, mods: list[tuple[str, str]]) -> str:
    listed = ",".join(f"{name}@{version}" for name, version in mods)
    return hashlib.sha256(f"{tag}\n{listed}".encode()).hexdigest()[:24]


def self_test() -> None:
    # The Worker's own tests hold the same two (worker.test.mjs).
    tag = "v1.1.0-shared-worlds-mods"
    mods = read(tag, "waymap@1.2.0,appleskin@3.0.1")
    assert mods == [("appleskin", "3.0.1"), ("waymap", "1.2.0")], mods
    assert key(tag, mods) == "ba70882d6f53e8e4d6c2b19a", key(tag, mods)
    assert key("v1.1.0", read("v1.1.0", "zoom@1.0.0")) == "706b19e2efa5ef88b4f5ad82"
    for bad_tag, bad_mods in [
        ("main", "zoom@1"),
        ("v1.1.0-other", "zoom@1"),
        ("v1.1.0", ""),
        ("v1.1.0", "zoom"),
        ("v1.1.0", "Zoom@1"),
        ("v1.1.0", "zoom@1;echo"),
        ("v1.1.0", "zoom@1,zoom@2"),
        ("v1.1.0", ",".join(f"m{i}@1" for i in range(MOST + 1))),
    ]:
        try:
            read(bad_tag, bad_mods)
        except SystemExit:
            continue
        raise AssertionError(f"accepted {bad_tag!r} {bad_mods!r}")
    print("mod_build_key: ok")


def main() -> None:
    if sys.argv[1:] == ["--self-test"]:
        return self_test()
    if len(sys.argv) not in (3, 4):
        sys.exit(__doc__)
    tag, mods = sys.argv[1], read(sys.argv[1], sys.argv[2])
    made = key(tag, mods)
    if len(sys.argv) == 4 and sys.argv[3] != made:
        sys.exit(f"mod_build_key: the request's key is {made}, not {sys.argv[3]}")
    print(made)
    for name, version in mods:
        print(name, version)


if __name__ == "__main__":
    main()
