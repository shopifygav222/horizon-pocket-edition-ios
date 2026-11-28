#!/usr/bin/env python3
"""The mod builder's tools (.github/workflows/mod-builder.yml, docs/ios.md "The mod builder").

A build of the app with catalog mods used to compile the whole game, half an hour and more of a Mac,
for a few small crates of mods. The builder compiles a release's game once, keeps what Cargo made of
it (the release's "compiled game": Cargo's target folder, the registry crates it read, the app
bundle without mods), and every later build of that release takes it back, so Cargo compiles the
mods and links the program.

Cargo decides what is still good by the modification time of the files it read, and a fresh checkout
gives every file the time of the checkout. So the builder gives the files a fixed, old time, by
their content:

    snapshot ROOT MANIFEST   every file and folder of ROOT gets the fixed time; MANIFEST records
                             each one's content (a file's SHA-256, a folder's list of names)
    restore ROOT MANIFEST    a file or folder gets the fixed time only if it is exactly what
                             MANIFEST recorded; anything else keeps its own time, and Cargo
                             compiles again whatever read it

A file is never called unchanged by its name or its date, only by its bytes: a build from a restored
compiled game compiles again exactly what a build from nothing would have compiled differently.

    lock BASE NEW            whether NEW (Cargo.lock with the mods) leaves every package of BASE
                             (without them) as it was; then NEW gets the fixed time (see `lock`)
    take NAME DEST --workflow PATH --event EVENT...
                             the newest kept game named NAME that this repository's own run of
                             that workflow made, fetched into DEST (exit 1 when there is none)
    identity OUT             what a compiled game was built with: compiler, Xcode, paths, settings
    same A B                 whether two identities are one (exit 1 and the differences if not)
    pack OUT [--target DIR] [--cargo-home DIR] [--app DIR]   the compiled game as archives in OUT
    unpack FROM [--target DIR] [--cargo-home DIR] [--app DIR]
    log FILE                 stdin (cargo build -v) to FILE, and on without the command lines
    summary FILE TITLE [--allow REGEX] [--strict]   what that build compiled, and why
    compare A.ipa B.ipa [--same]   what differs between two builds of the app (with --same:
                             exit 1 unless they hold the same, file for file)
    --self-test
"""

import hashlib
import io
import json
import os
import platform
import plistlib
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
import zipfile
from pathlib import Path

# The time every unchanged file gets: older than anything a build writes. The same number in the
# build that makes a compiled game and in every build that takes it.
PINNED = 1_600_000_000
PINNED_NS = PINNED * 1_000_000_000
# Not the source: git's own files and what Cargo writes. Only at the top of the tree.
SKIPPED = (".git", "target")
GAME = "horizon-ios-game"
MOD = "hmod-"
# What a compiled game depends on besides the source, the compiler and Xcode: the settings the
# workflow builds with. Cargo itself compiles again a crate whose `env!` value changed; this is the
# whole build's, checked before anything is taken.
SETTINGS = (
    "IOS_TARGET", "IPHONEOS_DEPLOYMENT_TARGET", "MACOSX_DEPLOYMENT_TARGET", "SDKROOT",
    "DEVELOPER_DIR", "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS",
    "CARGO_PROFILE_RELEASE_CODEGEN_UNITS", "CARGO_PROFILE_RELEASE_LTO", "CARGO_INCREMENTAL",
    "HORIZON_IOS_REPO", "HORIZON_IOS_BRANCH", "HORIZON_MOD_BUILDS_URL", "HORIZON_MSA_CLIENT_ID",
    "HORIZON_IOS_PUBLIC", "HORIZON_TELEMETRY_URL", "HORIZON_GAME_BUILD_ID", "HORIZON_POINT_NAMES",
    "HORIZON_VERSION", "HORIZON_NIGHTLY", "HORIZON_COMMIT", "RUSTUP_TOOLCHAIN",
)
# The workflow whose steps the builder repeats for a build from nothing (its `ipa` job), by the
# SHA-256 of the file with LF line ends. When ios.yml changes, read mod-builder.yml against it,
# then record the new hash here: the self-test fails until then.
MIRRORED = {
    ".github/workflows/ios.yml": "089ca1698aeba65a557f42dfebc26d166c1f910ac23d851a72853afde465e3d8",
}


def digest_file(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while chunk := f.read(1 << 20):
            h.update(chunk)
    return "f:" + h.hexdigest()


def stamp(path: Path) -> None:
    """Gives `path` the fixed time (a link itself, not what it points to, where the system can)."""
    if os.utime in os.supports_follow_symlinks:
        os.utime(path, ns=(PINNED_NS, PINNED_NS), follow_symlinks=False)
    elif not path.is_symlink():
        os.utime(path, ns=(PINNED_NS, PINNED_NS))


def entries(root: Path):
    """Every file, link and folder under `root` as (relative name, path, digest), a folder after
    what it holds. A folder's digest is its list of names: one that gained or lost a file is
    another folder, whatever its files are."""
    def walk(folder: Path, relative: str):
        names = []
        with os.scandir(folder) as found:
            children = sorted(found, key=lambda e: e.name)
        for child in children:
            if not relative and child.name in SKIPPED:
                continue
            names.append(child.name)
            name = f"{relative}/{child.name}" if relative else child.name
            path = Path(child.path)
            if child.is_symlink():
                yield name, path, "l:" + os.readlink(child.path)
            elif child.is_dir(follow_symlinks=False):
                yield from walk(path, name)
            else:
                yield name, path, digest_file(path)
        listing = hashlib.sha256("\0".join(names).encode("utf-8", "surrogateescape")).hexdigest()
        yield relative or ".", folder, "d:" + listing

    yield from walk(root, "")


def snapshot(root: Path, manifest: Path) -> dict:
    recorded = {}
    for name, path, digest in entries(root):
        recorded[name] = digest
        stamp(path)
    manifest.write_text(json.dumps({"pinned": PINNED, "entries": recorded}, sort_keys=True),
                        encoding="utf-8")
    return recorded


def restore(root: Path, manifest: Path) -> dict:
    """Pins what is unchanged; says what is not. Returns the counts and names."""
    data = json.loads(manifest.read_text(encoding="utf-8"))
    if data.get("pinned") != PINNED:
        sys.exit("mod_builder: the manifest was made with another fixed time")
    recorded = data["entries"]
    seen = set()
    result = {"unchanged": 0, "changed": [], "new": []}
    for name, path, digest in entries(root):
        seen.add(name)
        if recorded.get(name) == digest:
            stamp(path)
            result["unchanged"] += 1
        elif name in recorded:
            result["changed"].append(name)
        else:
            result["new"].append(name)
    result["changed"].sort()
    result["new"].sort()
    result["gone"] = sorted(set(recorded) - seen)
    return result


def lock_packages(text: str) -> dict:
    """Cargo.lock as {(name, version, source): (checksum, the packages it depends on)}, each
    dependency resolved to its package: Cargo writes `name`, `name version` or
    `name version (source)`, whichever is the shortest that names one package, so the same
    dependency is written differently once a second version of a crate joins the file."""
    packages = tomllib.loads(text).get("package", [])
    by_name = {}
    for p in packages:
        by_name.setdefault(p["name"], []).append(p)

    def ident(p):
        return (p["name"], p["version"], p.get("source", ""))

    def resolve(dep: str):
        name, _, rest = dep.partition(" ")
        version, _, source = rest.partition(" ")
        source = source.strip("()")
        found = [p for p in by_name.get(name, [])
                 if (not version or p["version"] == version)
                 and (not source or p.get("source", "") == source)]
        if len(found) != 1:
            raise ValueError(f"the dependency {dep!r} names {len(found)} packages")
        return ident(found[0])

    return {ident(p): (p.get("checksum", ""), frozenset(resolve(d) for d in p.get("dependencies", [])))
            for p in packages}


def lock_differences(base: str, new: str) -> list[str]:
    """Why `new` is not `base` plus mods: every package of `base` must be in `new` as it was (the
    same version, source, checksum and dependencies), but for the app's program, which gains the
    mods. Empty when the mods only added to the file."""
    try:
        old, now = lock_packages(base), lock_packages(new)
    except (ValueError, KeyError, tomllib.TOMLDecodeError) as error:
        return [f"a lockfile does not read: {error}"]
    mods = lambda deps: frozenset(d for d in deps if not d[0].startswith(MOD))
    differences = []
    for ident, (checksum, deps) in sorted(old.items()):
        if ident[0].startswith(MOD):
            differences.append(f"{ident[0]}: a mod is in the lockfile of the game without mods")
            continue
        if ident not in now:
            differences.append(f"{ident[0]} {ident[1]}: no longer in the lockfile")
            continue
        new_checksum, new_deps = now[ident]
        if ident[0] == GAME:
            deps, new_deps = mods(deps), mods(new_deps)
        if checksum != new_checksum:
            differences.append(f"{ident[0]} {ident[1]}: another checksum")
        elif deps != new_deps:
            changed = sorted(f"{d[0]} {d[1]}" for d in deps ^ new_deps)
            differences.append(f"{ident[0]} {ident[1]}: other dependencies ({', '.join(changed)})")
    return differences


def lock(base: Path, new: Path) -> None:
    """`Cargo.lock` with the mods gets the fixed time when the mods only added to it.

    One build script reads the lockfile: horizon-world's, which hashes it into the key of the
    snapshots the game keeps on the phone (HORIZON_WORLD_SOURCES), so that another version of a
    crate the snapshots are made with drops them. Mods add their own packages to the file, which
    is another file but no other version of anything the game is made of: with the fixed time
    that script is not run again, the key stays the one of the release without mods (whose
    snapshots are these same ones), and horizon-world and all that is built on it are not compiled
    again. When a mod does change a package of the game's (a newer version of a shared crate),
    the file keeps its own time and Cargo does everything a build from nothing does."""
    differences = lock_differences(base.read_text(encoding="utf-8"), new.read_text(encoding="utf-8"))
    if differences:
        print("mod_builder: the mods change the game's own packages; Cargo.lock keeps its time:")
        for line in differences[:40]:
            print(f"  {line}")
        print("lock=changed")
    else:
        stamp(new)
        print("mod_builder: the mods only added to Cargo.lock; it is the game's own for the build")
        print("lock=same")


def said(*command: str) -> str:
    try:
        done = subprocess.run(command, capture_output=True, text=True, timeout=120)
    except (OSError, subprocess.TimeoutExpired):
        return ""
    return (done.stdout if done.returncode == 0 else "").strip()


def identity() -> dict:
    return {
        "rustc": said("rustc", "-vV"),
        "cargo": said("cargo", "-V"),
        "xcode": said("xcodebuild", "-version"),
        "sdk": said("xcrun", "--sdk", "iphoneos", "--show-sdk-version"),
        "machine": platform.machine(),
        "system": sys.platform,
        "folder": os.getcwd(),
        "home": str(Path.home()),
        "cargo_home": os.environ.get("CARGO_HOME", ""),
        "commit": said("git", "rev-parse", "HEAD"),
        "pinned": PINNED,
        "settings": {name: os.environ[name] for name in SETTINGS if name in os.environ},
    }


def same(a: Path, b: Path) -> bool:
    one, other = (json.loads(p.read_text(encoding="utf-8")) for p in (a, b))
    if one == other:
        return True
    for key in sorted(set(one) | set(other)):
        if one.get(key) != other.get(key):
            print(f"mod_builder: {key} differs: {one.get(key)!r} / {other.get(key)!r}")
    return False


def choose(artifacts: list[dict], name: str) -> list[dict]:
    """The artifacts named `name` a build may look at, newest first: still kept, and from a run
    whose code was this repository's own. A pull request from a fork runs the fork's workflows,
    which may keep a file under any name; GitHub says where a run's code came from."""
    def own(a: dict) -> bool:
        run = a.get("workflow_run") or {}
        return (isinstance(run.get("id"), int) and run.get("repository_id") is not None
                and run.get("repository_id") == run.get("head_repository_id"))
    mine = [a for a in artifacts if a.get("name") == name and not a.get("expired") and own(a)]
    return sorted(mine, key=lambda a: a.get("created_at") or "", reverse=True)


def made_by(run: dict, workflow: str, events: list[str]) -> bool:
    """Whether `run` is a run of `workflow` (its file's path) started in one of `events`: a kept
    game is taken only from the workflow that makes them, started by someone who may start it."""
    path = run.get("path") or ""
    return (path == workflow or path.startswith(workflow + "@")) and run.get("event") in events


def gh(path: str) -> dict:
    done = subprocess.run(["gh", "api", path], capture_output=True, timeout=300)
    if done.returncode != 0:
        raise RuntimeError(done.stderr.decode("utf-8", "replace").strip()[:300])
    return json.loads(done.stdout)


def take(name: str, dest: Path, workflow: str, events: list[str]) -> int:
    """Fetches the kept game `name` into `dest`. Whatever goes wrong, there is none: the caller
    compiles the game."""
    repo = os.environ.get("GITHUB_REPOSITORY", "")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repo) or not re.fullmatch(r"[A-Za-z0-9._-]+", name):
        print("mod_builder: no repository or no name to look for")
        return 1
    try:
        listed = gh(f"repos/{repo}/actions/artifacts?name={name}&per_page=30").get("artifacts", [])
        for artifact in choose(listed, name)[:2]:
            run = artifact["workflow_run"]["id"]
            if not made_by(gh(f"repos/{repo}/actions/runs/{run}"), workflow, events):
                print(f"mod_builder: {name} of run {run} was not made by {workflow}: left alone")
                continue
            if dest.exists():
                shutil.rmtree(dest)
            dest.mkdir(parents=True)
            packed = dest.parent / f"{dest.name}.zip"
            with open(packed, "wb") as file:
                fetched = subprocess.run(["gh", "api", f"repos/{repo}/actions/artifacts/{artifact['id']}/zip"],
                                         stdout=file, timeout=1800)
            if fetched.returncode != 0:
                print(f"mod_builder: {name} of run {run} could not be fetched")
                packed.unlink(missing_ok=True)
                continue
            with zipfile.ZipFile(packed) as z:
                for info in z.infolist():
                    # Its own files, by plain names: nothing is written outside `dest`.
                    if info.is_dir() or not re.fullmatch(r"[A-Za-z0-9._-]+", info.filename):
                        raise RuntimeError(f"a file of the kept game is named {info.filename!r}")
                z.extractall(dest)
            packed.unlink()
            print(f"mod_builder: {name} of run {run} is in {dest} ({artifact.get('size_in_bytes', 0) >> 20} MiB)")
            return 0
    except Exception as error:  # No kept game is an ordinary thing: the game is compiled.
        print(f"mod_builder: looking for {name} failed: {error}")
        return 1
    print(f"mod_builder: no kept game {name}")
    return 1


PARTS = ("target", "cargo-home", "app")


def packer() -> tuple[str, list[str], list[str]]:
    """The archive's ending, and how to press and open it: zstd where there is one, else gzip."""
    if shutil.which("zstd"):
        return ".tar.zst", ["zstd", "-q", "-T0", "-3"], ["zstd", "-q", "-d", "-c"]
    return ".tar.gz", ["gzip", "-1"], ["gzip", "-d", "-c"]


def pack(out: Path, folders: dict[str, Path]) -> None:
    """Each folder's own content as OUT/<part>.tar.*: tar keeps every file's time, which is what
    Cargo reads the compiled game by."""
    out.mkdir(parents=True, exist_ok=True)
    ending, press, _ = packer()
    for part, folder in folders.items():
        names = sorted(p.name for p in folder.iterdir()) if folder.is_dir() else []
        if part == "cargo-home":
            # The crates Cargo fetched and unpacked, not its programs or settings.
            names = [n for n in names if n in ("registry", "git")]
        if not names:
            sys.exit(f"mod_builder: nothing to pack in {folder}")
        archive = out / f"{part}{ending}"
        with open(archive, "wb") as file:
            tar = subprocess.Popen(["tar", "-cf", "-", "-C", str(folder), *names], stdout=subprocess.PIPE)
            pressed = subprocess.run(press, stdin=tar.stdout, stdout=file)
            tar.stdout.close()
            if tar.wait() != 0 or pressed.returncode != 0:
                sys.exit(f"mod_builder: packing {folder} failed")
        print(f"mod_builder: {archive.name} {archive.stat().st_size >> 20} MiB")


def unpack(source: Path, folders: dict[str, Path]) -> None:
    for part, folder in folders.items():
        found = sorted(source.glob(f"{part}.tar.*"))
        if len(found) != 1:
            sys.exit(f"mod_builder: {source} has {len(found)} archives of {part}")
        opener = ["zstd", "-q", "-d", "-c"] if found[0].name.endswith(".zst") else ["gzip", "-d", "-c"]
        folder.mkdir(parents=True, exist_ok=True)
        with open(found[0], "rb") as file:
            opened = subprocess.Popen(opener, stdin=file, stdout=subprocess.PIPE)
            tar = subprocess.run(["tar", "-xf", "-", "-C", str(folder)], stdin=opened.stdout)
            opened.stdout.close()
            if opened.wait() != 0 or tar.returncode != 0:
                sys.exit(f"mod_builder: unpacking {found[0].name} failed")
        print(f"mod_builder: {found[0].name} is in {folder}")


LINE = re.compile(r"^\s*(Compiling|Fresh|Dirty)\s+(\S+)\s+v(\S+)(.*)$")


def log(file: Path) -> None:
    """`cargo build -v` from stdin: all of it into `file`, and on to stdout without the lines
    nobody reads in a log (each compiler command, each crate that was still good)."""
    with open(file, "w", encoding="utf-8") as kept:
        for raw in sys.stdin.buffer:
            line = raw.decode("utf-8", "replace")
            kept.write(line)
            head = line.lstrip()
            if head.startswith(("Running `", "Fresh ")):
                continue
            sys.stdout.write(line)
            sys.stdout.flush()


def read_log(text: str) -> dict:
    compiled, fresh, dirty = [], 0, []
    for line in text.splitlines():
        match = LINE.match(line)
        if not match:
            continue
        what, name = match[1], match[2]
        if what == "Compiling" and name not in compiled:
            compiled.append(name)
        elif what == "Fresh":
            fresh += 1
        elif what == "Dirty":
            dirty.append(f"{name}{match[4]}")
    return {"compiled": compiled, "fresh": fresh, "dirty": dirty}


def summary(file: Path, title: str, allow: str | None, strict: bool) -> int:
    read = read_log(file.read_text(encoding="utf-8", errors="replace"))
    compiled = read["compiled"]
    other = [name for name in compiled if not (allow and re.fullmatch(allow, name))]
    lines = [f"### {title}", "",
             f"Compiled {len(compiled)} crates, {read['fresh']} taken as they were."]
    if compiled:
        lines += ["", "Compiled: " + ", ".join(compiled[:80]) + (" ..." if len(compiled) > 80 else "")]
    if read["dirty"]:
        lines += ["", "Why (Cargo's own words):", ""]
        lines += [f"- `{reason[:300]}`" for reason in read["dirty"][:40]]
    text = "\n".join(lines) + "\n"
    print(text)
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as out:
            out.write(text + "\n")
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as out:
            out.write(f"compiled={len(compiled)}\nother={len(other)}\n")
    if other:
        level = "error" if strict else "warning"
        print(f"::{level}::{title}: compiled more than expected: {', '.join(other[:20])}")
        return 1 if strict else 0
    return 0


def ranges(a: bytes, b: bytes, most: int = 60) -> tuple[int, list[tuple[int, int]]]:
    """The bytes that differ between two files of one length: how many, and where (runs closer
    than 16 bytes are one)."""
    count, found = 0, []
    step = 1 << 16
    for at in range(0, len(a), step):
        x, y = a[at:at + step], b[at:at + step]
        if x == y:
            continue
        for i in range(len(x)):
            if x[i] != y[i]:
                count += 1
                where = at + i
                if found and where - found[-1][1] <= 16:
                    found[-1] = (found[-1][0], where)
                elif len(found) < most:
                    found.append((where, where))
    return count, found


def shown(data: bytes) -> str:
    return "".join(chr(c) if 32 <= c < 127 else "." for c in data)


def inner(x: bytes, y: bytes) -> list[str] | None:
    """Two archives inside the app (its game tables), file by file: what differs between what
    they hold, or None when one of them is no archive. Two archives of the same files are other
    bytes when they were packed at another minute."""
    try:
        with zipfile.ZipFile(io.BytesIO(x)) as a, zipfile.ZipFile(io.BytesIO(y)) as b:
            one = {i.filename: (i.CRC, i.file_size) for i in a.infolist() if not i.is_dir()}
            two = {i.filename: (i.CRC, i.file_size) for i in b.infolist() if not i.is_dir()}
    except zipfile.BadZipFile:
        return None
    found = [f"only in the first: {name}" for name in sorted(set(one) - set(two))]
    found += [f"only in the second: {name}" for name in sorted(set(two) - set(one))]
    found += [f"differs: {name}" for name in sorted(set(one) & set(two)) if one[name] != two[name]]
    return found


def compare(first: Path, second: Path) -> tuple[str, int]:
    """What differs between two .ipa files, as a report, and how many files differ in what they
    hold: files only one has, files whose content is another. A property list with the same
    keys and values, and an archive of the same files, are the same content in other bytes
    (the order a tool wrote them in, the minute they were packed), and are said so. For the
    program itself: how many bytes differ and what they spell."""
    real = 0
    with zipfile.ZipFile(first) as a, zipfile.ZipFile(second) as b:
        one = {i.filename: i for i in a.infolist() if not i.is_dir()}
        two = {i.filename: i for i in b.infolist() if not i.is_dir()}
        lines = [f"### {first.name} against {second.name}", "",
                 f"{len(one)} files against {len(two)}."]
        for label, names in (("Only in the first", sorted(set(one) - set(two))),
                             ("Only in the second", sorted(set(two) - set(one)))):
            if names:
                real += len(names)
                lines += ["", f"{label}: " + ", ".join(names[:30])]
        differing = []
        for name in sorted(set(one) & set(two)):
            if one[name].CRC == two[name].CRC and one[name].file_size == two[name].file_size:
                continue
            differing.append(name)
        lines += ["", f"{len(set(one) & set(two)) - len(differing)} files are the same byte for byte, "
                      f"{len(differing)} are other bytes."]
        for name in differing:
            x, y = a.read(name), b.read(name)
            lines += ["", f"**{name}**: {len(x)} bytes against {len(y)}"]
            if name.endswith(".plist"):
                try:
                    p, q = plistlib.loads(x), plistlib.loads(y)
                    keys = [key for key in sorted(set(p) | set(q)) if p.get(key) != q.get(key)]
                    for key in keys:
                        lines.append(f"- {key}: {p.get(key)!r} against {q.get(key)!r}")
                    if keys:
                        real += 1
                    else:
                        lines.append("- the same keys and values, written in another order")
                except Exception as error:  # A plist that does not read is only reported.
                    real += 1
                    lines.append(f"- does not read: {error}")
                continue
            held = inner(x, y) if name.endswith(".zip") else None
            if held is not None:
                if held:
                    real += 1
                    lines += [f"- {line}" for line in held[:40]]
                else:
                    lines.append("- an archive of the same files, each the same: packed at another time")
                continue
            real += 1
            if len(x) == len(y):
                count, found = ranges(x, y)
                lines.append(f"- {count} bytes differ, in {len(found)}{'+' if len(found) >= 60 else ''} places")
                for start, end in found[:40]:
                    lo, hi = max(0, start - 24), min(len(x), end + 25)
                    lines.append(f"  - at {start:#x}, {end - start + 1} bytes: `{shown(x[lo:hi])}` / `{shown(y[lo:hi])}`")
    lines += ["", f"{real} files differ in what they hold."]
    return "\n".join(lines) + "\n", real


def self_test() -> None:
    with tempfile.TemporaryDirectory() as work:
        work = Path(work)
        source = work / "source"
        (source / "crates" / "a" / "src").mkdir(parents=True)
        (source / "crates" / "b").mkdir()
        (source / "target").mkdir()
        (source / ".git").mkdir()
        (source / "Cargo.lock").write_text("lock", encoding="utf-8")
        (source / "crates" / "a" / "src" / "lib.rs").write_text("pub fn a() {}\n", encoding="utf-8")
        (source / "crates" / "a" / "src" / "gone.rs").write_text("gone\n", encoding="utf-8")
        (source / "crates" / "b" / "build.rs").write_text("fn main() {}\n", encoding="utf-8")
        (source / "target" / "made").write_text("x", encoding="utf-8")
        manifest = work / "manifest.json"
        recorded = snapshot(source, manifest)
        assert "target/made" not in recorded and ".git" not in recorded, recorded
        assert set(recorded) == {".", "Cargo.lock", "crates", "crates/a", "crates/a/src",
                                 "crates/a/src/lib.rs", "crates/a/src/gone.rs", "crates/b",
                                 "crates/b/build.rs"}, sorted(recorded)
        time_of = lambda p: int(os.stat(p).st_mtime)
        for name in recorded:
            assert time_of(source / name) == PINNED, name
        assert time_of(source / "target" / "made") != PINNED

        # A fresh checkout of the same source: every time is now, every content the same.
        fresh = work / "fresh"
        shutil.copytree(source, fresh, copy_function=shutil.copy)
        for path in [fresh, *fresh.rglob("*")]:
            os.utime(path, None)
        result = restore(fresh, manifest)
        assert (result["changed"], result["new"], result["gone"]) == ([], [], []), result
        assert result["unchanged"] == len(recorded), result
        for name in recorded:
            assert time_of(fresh / name) == PINNED, name

        # Another checkout where a file changed, one is new and one is gone: only those, and the
        # folders whose list of names is another, keep their own time.
        other = work / "other"
        shutil.copytree(source, other, copy_function=shutil.copy)
        (other / "crates" / "a" / "src" / "lib.rs").write_text("pub fn a() { }\n", encoding="utf-8")
        (other / "crates" / "b" / "new.rs").write_text("new\n", encoding="utf-8")
        (other / "crates" / "a" / "src" / "gone.rs").unlink()
        for path in [other, *other.rglob("*")]:
            os.utime(path, None)
        result = restore(other, manifest)
        assert result["changed"] == ["crates/a/src", "crates/a/src/lib.rs", "crates/b"], result
        assert result["new"] == ["crates/b/new.rs"], result
        assert result["gone"] == ["crates/a/src/gone.rs"], result
        for name in ("crates/a/src/lib.rs", "crates/a/src", "crates/b", "crates/b/new.rs"):
            assert time_of(other / name) != PINNED, name
        for name in (".", "Cargo.lock", "crates", "crates/a", "crates/b/build.rs"):
            assert time_of(other / name) == PINNED, name
        # The same length and the same date are not the same file.
        same_size = work / "same-size"
        shutil.copytree(source, same_size, copy_function=shutil.copy2)
        (same_size / "crates" / "b" / "build.rs").write_text("fn main() {;\n", encoding="utf-8")
        os.utime(same_size / "crates" / "b" / "build.rs", ns=(PINNED_NS, PINNED_NS))
        assert restore(same_size, manifest)["changed"] == ["crates/b/build.rs"]

        # The compiled game packs and unpacks with its files' times.
        made = work / "made"
        (made / "release").mkdir(parents=True)
        (made / "release" / "lib.rlib").write_text("rlib", encoding="utf-8")
        os.utime(made / "release" / "lib.rlib", (1_700_000_000, 1_700_000_000))
        # (Not on Windows, whose tar reads a drive letter as another machine.)
        if shutil.which("tar") and os.name != "nt":
            pack(work / "packed", {"target": made})
            unpack(work / "packed", {"target": work / "opened"})
            assert (work / "opened" / "release" / "lib.rlib").read_text(encoding="utf-8") == "rlib"
            assert time_of(work / "opened" / "release" / "lib.rlib") == 1_700_000_000

        # Two builds of the app: what differs is found and spelled.
        def ipa(path: Path, program: bytes, build: str, more=()):
            with zipfile.ZipFile(path, "w") as z:
                z.writestr("Payload/HorizonPE.app/HorizonPE", program)
                z.writestr("Payload/HorizonPE.app/Info.plist", plistlib.dumps({"CFBundleVersion": build}))
                z.writestr("Payload/HorizonPE.app/assets/engine-data.zip", b"data")
                for name in more:
                    z.writestr(name, b"x")
        ipa(work / "a.ipa", b"\0" * 100 + b"key=aaaa" + b"\0" * 100, "7")
        ipa(work / "b.ipa", b"\0" * 100 + b"key=bbbb" + b"\0" * 100, "8", ["Payload/HorizonPE.app/extra"])
        report, real = compare(work / "a.ipa", work / "b.ipa")
        assert "Only in the second: Payload/HorizonPE.app/extra" in report, report
        assert "1 files are the same byte for byte, 2 are other bytes" in report, report
        assert "4 bytes differ, in 1 places" in report and "key=aaaa" in report, report
        assert "CFBundleVersion: '7' against '8'" in report, report
        assert real == 3 and "3 files differ in what they hold" in report, report
        # The same content in other bytes is not another app: a property list a tool wrote in
        # another order, an archive of the same files packed at another minute.
        def packed(path: Path, plist: bytes, when: tuple, table: bytes = b"table"):
            tables = io.BytesIO()
            with zipfile.ZipFile(tables, "w") as z:
                z.writestr(zipfile.ZipInfo("tables/blocks.bin", when), table)
            with zipfile.ZipFile(path, "w") as z:
                z.writestr("Payload/HorizonPE.app/HorizonPE", b"program")
                z.writestr("Payload/HorizonPE.app/Info.plist", plist)
                z.writestr("Payload/HorizonPE.app/assets/engine-data.zip", tables.getvalue())
        both = {"CFBundleVersion": "9", "CFBundleName": "Horizon PE"}
        packed(work / "c.ipa", plistlib.dumps(both, fmt=plistlib.FMT_XML, sort_keys=True), (2026, 1, 1, 0, 0, 0))
        packed(work / "d.ipa", plistlib.dumps(both, fmt=plistlib.FMT_XML, sort_keys=False), (2026, 1, 2, 0, 0, 0))
        report, real = compare(work / "c.ipa", work / "d.ipa")
        assert real == 0, report
        assert "written in another order" in report and "packed at another time" in report, report
        packed(work / "e.ipa", plistlib.dumps(both, fmt=plistlib.FMT_XML, sort_keys=True), (2026, 1, 1, 0, 0, 0), b"other")
        report, real = compare(work / "c.ipa", work / "e.ipa")
        assert real == 1 and "differs: tables/blocks.bin" in report, report

    # A kept game is this repository's own run's, the newest still kept.
    def kept(ident, created, run, expired=False, name="game-base-abc-r1"):
        return {"id": ident, "name": name, "expired": expired, "created_at": created, "workflow_run": run}
    here = {"id": 7, "repository_id": 5, "head_repository_id": 5}
    listed = [
        kept(1, "2026-11-01T00:00:00Z", here),
        kept(2, "2026-11-03T00:00:00Z", {"id": 8, "repository_id": 5, "head_repository_id": 99}),
        kept(3, "2026-11-02T00:00:00Z", here),
        kept(4, "2026-11-04T00:00:00Z", here, expired=True),
        kept(5, "2026-11-05T00:00:00Z", here, name="game-base-abc-r10"),
        kept(6, "2026-11-06T00:00:00Z", None),
        kept(7, "2026-11-07T00:00:00Z", {"id": 9, "repository_id": 5}),
    ]
    assert [a["id"] for a in choose(listed, "game-base-abc-r1")] == [3, 1]
    builder = ".github/workflows/mod-builder.yml"
    assert made_by({"path": builder, "event": "workflow_dispatch"}, builder, ["workflow_dispatch"])
    assert made_by({"path": builder + "@refs/heads/main", "event": "workflow_dispatch"}, builder, ["workflow_dispatch"])
    for run in ({"path": builder, "event": "pull_request"},
                {"path": builder, "event": "pull_request_target"},
                {"path": ".github/workflows/other.yml", "event": "workflow_dispatch"},
                {"path": builder + ".evil.yml", "event": "workflow_dispatch"},
                {"event": "workflow_dispatch"}, {}):
        assert not made_by(run, builder, ["workflow_dispatch"]), run

    registry = "registry+https://github.com/rust-lang/crates.io-index"
    def package(name, version, deps=(), source=registry, checksum="c"):
        text = f'[[package]]\nname = "{name}"\nversion = "{version}"\n'
        if source:
            text += f'source = "{source}"\nchecksum = "{checksum}"\n'
        if deps:
            text += "dependencies = [\n" + "".join(f' "{d}",\n' for d in deps) + "]\n"
        return text + "\n"
    base = "version = 4\n\n" + "".join([
        package("horizon-ios-game", "0.1.0", ["horizon-world", "serde"], source=None),
        package("horizon-world", "0.1.0", ["rand", "serde"], source=None),
        package("rand", "0.8.5"),
        package("serde", "1.0.200"),
    ])
    # A mod and its own crate: only added.
    added = "version = 4\n\n" + "".join([
        package("hmod-zoom", "0.0.0", ["itoa"], source=None),
        package("horizon-ios-game", "0.1.0", ["hmod-zoom", "horizon-world", "serde"], source=None),
        package("horizon-world", "0.1.0", ["rand", "serde"], source=None),
        package("itoa", "1.0.0"),
        package("rand", "0.8.5"),
        package("serde", "1.0.200"),
    ])
    assert lock_differences(base, base) == []
    assert lock_differences(base, added) == [], lock_differences(base, added)
    # A mod that brings a second version of a crate: the game's own dependency is written with
    # its version from then on, and is the same package.
    second = added.replace(package("rand", "0.8.5"), package("rand", "0.8.5") + package("rand", "0.9.0")) \
        .replace(package("horizon-world", "0.1.0", ["rand", "serde"], source=None),
                 package("horizon-world", "0.1.0", ["rand 0.8.5", "serde"], source=None)) \
        .replace('"itoa",', '"itoa",\n "rand 0.9.0",')
    assert lock_differences(base, second) == [], lock_differences(base, second)
    # A mod that moves a crate of the game's to a newer version, changes a checksum, or the game
    # itself depending on something else: not the game's own file any more.
    assert lock_differences(base, added.replace("1.0.200", "1.0.201")) != []
    assert lock_differences(base, added.replace(package("rand", "0.8.5"), package("rand", "0.8.5", checksum="d"))) != []
    assert lock_differences(base, added.replace('"hmod-zoom",\n "horizon-world",', '"hmod-zoom",')) != []
    assert lock_differences(base, added.replace(package("rand", "0.8.5"), "")) != []
    assert lock_differences(added, added) != [], "a base with a mod in it is no base"
    assert lock_differences(base, "not a lockfile [") != []

    text = ("   Compiling hmod-zoom v0.0.0 (/w/native-mods-build/zoom)\n"
            "       Fresh serde v1.0.200\n"
            "       Dirty horizon-ios-game v0.1.0 (/w/crates/horizon-ios-game): the file `x` has changed\n"
            "   Compiling horizon-ios-game v0.1.0 (/w/crates/horizon-ios-game)\n"
            "     Running `rustc --crate-name hmod_zoom`\n"
            "    Finished `release` profile\n")
    read = read_log(text)
    assert read["compiled"] == ["hmod-zoom", "horizon-ios-game"] and read["fresh"] == 1, read
    assert read["dirty"] == ["horizon-ios-game (/w/crates/horizon-ios-game): the file `x` has changed"], read

    # The workflow the builder repeats is the one it was read against.
    root = Path(__file__).resolve().parents[2]
    for name, recorded in MIRRORED.items():
        file = root / name
        if file.is_file():
            now = hashlib.sha256(file.read_bytes().replace(b"\r\n", b"\n")).hexdigest()
            assert now == recorded, (
                f"{name} changed ({now}): read .github/workflows/mod-builder.yml against it, "
                "then record this hash in tools/ci/mod_builder.py MIRRORED")
    print("mod_builder: ok")


def folders(args: list[str]) -> dict[str, Path]:
    """`--target DIR --cargo-home DIR --app DIR` as {part: folder}, in that order."""
    found = {}
    while args:
        flag, folder = args[0], args[1] if len(args) > 1 else None
        part = flag[2:] if flag.startswith("--") else None
        if part not in PARTS or folder is None:
            sys.exit(__doc__)
        found[part] = Path(folder).expanduser().resolve()
        args = args[2:]
    return found


def main() -> None:
    args = sys.argv[1:]
    if args == ["--self-test"]:
        self_test()
    elif len(args) == 3 and args[0] == "snapshot":
        recorded = snapshot(Path(args[1]).resolve(), Path(args[2]))
        print(f"mod_builder: {len(recorded)} files and folders recorded and given the fixed time")
    elif len(args) == 3 and args[0] == "restore":
        result = restore(Path(args[1]).resolve(), Path(args[2]))
        print(f"mod_builder: {result['unchanged']} files and folders are the compiled game's own; "
              f"{len(result['changed'])} changed, {len(result['new'])} new, {len(result['gone'])} gone")
        for label in ("changed", "new", "gone"):
            for name in result[label][:40]:
                print(f"  {label}: {name}")
    elif len(args) == 3 and args[0] == "lock":
        lock(Path(args[1]), Path(args[2]))
    elif len(args) >= 7 and args[0] == "take" and args[3] == "--workflow":
        rest = args[5:]
        if len(rest) % 2 or any(flag != "--event" for flag in rest[::2]):
            sys.exit(__doc__)
        sys.exit(take(args[1], Path(args[2]).resolve(), args[4], rest[1::2]))
    elif len(args) == 2 and args[0] == "identity":
        Path(args[1]).write_text(json.dumps(identity(), indent=1, sort_keys=True) + "\n", encoding="utf-8")
    elif len(args) == 3 and args[0] == "same":
        sys.exit(0 if same(Path(args[1]), Path(args[2])) else 1)
    elif len(args) >= 4 and args[0] == "pack":
        pack(Path(args[1]), folders(args[2:]))
    elif len(args) >= 4 and args[0] == "unpack":
        unpack(Path(args[1]), folders(args[2:]))
    elif len(args) == 2 and args[0] == "log":
        log(Path(args[1]))
    elif len(args) >= 3 and args[0] == "summary":
        rest = args[3:]
        strict = "--strict" in rest
        allow = rest[rest.index("--allow") + 1] if "--allow" in rest else None
        sys.exit(summary(Path(args[1]), args[2], allow, strict))
    elif len(args) in (3, 4) and args[0] == "compare" and args[3:] in ([], ["--same"]):
        report, real = compare(Path(args[1]), Path(args[2]))
        print(report)
        if os.environ.get("GITHUB_STEP_SUMMARY"):
            with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as out:
                out.write(report + "\n")
        # `--same`: the two must hold the same, file for file.
        sys.exit(1 if args[3:] and real else 0)
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
