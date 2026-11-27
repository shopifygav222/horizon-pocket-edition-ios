#!/usr/bin/env python3
"""Catalog mods compiled into the iOS app (docs/ios.md).

An iPhone runs no code its app did not ship with, so where Starling's Android app opens a library a mod
was built into (tools/android_native_sdk.py), a catalog mod here is one more crate of the app's own
program: its package joins the workspace, the program names its hooks, and the app carries its package
where the game reads it.

    python3 tools/ios_native_sdk.py add INDEX OUT id@version...   # rewrites this checkout: a CI runner's
    python3 tools/ios_native_sdk.py list INDEX                    # the catalog's native mods, id@version
    python3 tools/ios_native_sdk.py survey INDEX REPORT           # every one a build can take, as a package here
    python3 tools/ios_native_sdk.py check PACKAGE.hmod            # whether a build can take this one
    python3 tools/ios_native_sdk.py --self-test

`add` fetches each mod's package from the registry's INDEX (its SHA-256 checked), unpacks it as the
workspace package `native-mods-build/<id>/` (a plain Rust library, `hmod_<id>`), writes
`crates/horizon-ios-game/src/built_mods.rs` (the mods' entries, their build records and their packages'
folder, which `app.rs` hands the engine as the desktop's mod host does its own) and adds the packages
to that crate's dependencies. OUT gets `horizon-native-mods/<id>/`, each mod's whole package: the
build copies it beside the app's program, where the engine reads a built mod's manifest and content
(`horizon_mod_package::runtime::Runtime::load`). It also prints, as JSON, what it built in.

`survey` unpacks the newest version of every native mod of the catalog that a build can take as a
workspace package, without touching the program, and writes REPORT: for each mod its version and
either the package to compile (`hmod-<id>`) or why a build refuses it. The catalog-mods-check
workflow compiles those packages and records which this engine builds
(packaging/ios/catalog-buildable.json, which the game's catalog offers from).

A build takes what Starling's own store takes: a mod that uses only the mod API. One with
`[native.*]` tables (mixins, bridges, modules: changes to the game's own source), a build script, or a
dependency that is not a registry crate is refused, with the reason.
"""

import hashlib
import io
import json
import re
import shutil
import sys
import tomllib
import urllib.request
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MODS_DIR = "native-mods-build"
GAME = ROOT / "crates" / "horizon-ios-game"
BUILT = GAME / "src" / "built_mods.rs"
API = "horizon-mod-api"
# The API crate's library name (its `[lib] name`): what a mod that depends on it under its own name calls it.
API_LIB = "mod_api"
ID = re.compile(r"[a-z0-9][a-z0-9_]{0,63}")
VERSION = re.compile(r"[0-9A-Za-z][0-9A-Za-z.+_-]{0,31}")
SIDES = {"both": "Both", "client": "Client", "server": "Server"}
# The most a package may hold unpacked: a mod is source and a few pictures.
MOST_BYTES = 64 << 20
MARK = "# The catalog mods this build has (tools/ios_native_sdk.py):"


class Refused(Exception):
    """Why a build can't take a mod."""


def projects(index: Path) -> dict:
    data = json.loads(index.read_text(encoding="utf-8"))
    revoked = {(r.get("id"), r.get("version")) for r in data.get("revoked", []) if isinstance(r, dict)}
    return {p["id"]: p for p in data["projects"] if p.get("kind") == "native-mod"}, revoked


def find(index: Path, mod_id: str, version: str) -> dict:
    """The registry's file for `mod_id` at `version`: its url, size and sha256."""
    listed, revoked = projects(index)
    project = listed.get(mod_id)
    if project is None:
        raise Refused(f"{mod_id}: the catalog has no native mod by that name")
    if (mod_id, version) in revoked:
        raise Refused(f"{mod_id} {version}: the catalog took that version back")
    for entry in project.get("versions", []):
        if entry.get("version") == version:
            return entry["file"]
    raise Refused(f"{mod_id}: the catalog has no version {version}")


def fetch(file: dict) -> bytes:
    url = file["url"]
    # The registry's own storage, or (this port's copy of the index, while the registry is away) a
    # release of the author's repository. Either way the file must be the one the index lists.
    if not re.fullmatch(r"https://github\.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+/releases/download/[^?#]+", url):
        raise Refused(f"{url}: not a release file on GitHub")
    if int(file.get("size", 0)) > MOST_BYTES:
        raise Refused(f"{file.get('name')}: larger than a mod's package may be")
    with urllib.request.urlopen(url, timeout=120) as response:
        data = response.read(MOST_BYTES + 1)
    if len(data) > MOST_BYTES or hashlib.sha256(data).hexdigest() != file["sha256"]:
        raise Refused(f"{file.get('name')}: not the file the catalog lists (its SHA-256 differs)")
    return data


def members(data: bytes) -> dict[str, bytes]:
    """The package's files by their path inside it, refusing any that would leave it."""
    files = {}
    total = 0
    with zipfile.ZipFile(io.BytesIO(data)) as z:
        for info in z.infolist():
            name = info.filename
            if info.is_dir():
                continue
            parts = Path(name).parts
            if not name or name.startswith(("/", "\\")) or ".." in parts or ":" in name or "\\" in name:
                raise Refused(f"a file of the package is named {name!r}")
            total += info.file_size
            if total > MOST_BYTES:
                raise Refused("the package is larger unpacked than a mod's may be")
            files[name] = z.read(info)
    return files


def hooks_path(files: dict[str, bytes], mod_id: str) -> str:
    """Where `#[horizon_mod]` puts the mod's `hooks()`: the module of the file that has the attribute."""
    for name in sorted(files):
        if not (name.startswith("src/") and name.endswith(".rs")):
            continue
        text = files[name].decode("utf-8", errors="replace")
        if re.search(r"#\[\s*(\w+\s*::\s*)?horizon_mod\s*\]", text):
            module = [part for part in Path(name[4:]).with_suffix("").parts if part not in ("lib", "mod")]
            return "::".join(["crate", *module, "hooks"])
    raise Refused(f"{mod_id}: no #[horizon_mod] in its source")


def plan(files: dict[str, bytes]) -> dict:
    """What a build needs to know of a package, or why it can't take it."""
    if "horizon-mod.toml" not in files:
        raise Refused("the package has no horizon-mod.toml")
    manifest = tomllib.loads(files["horizon-mod.toml"].decode("utf-8"))
    mod_id, version = manifest.get("id"), manifest.get("version")
    if not isinstance(mod_id, str) or not ID.fullmatch(mod_id):
        raise Refused(f"its id {mod_id!r} is not a crate's name")
    if not isinstance(version, str) or not VERSION.fullmatch(version):
        raise Refused(f"{mod_id}: its version {version!r} is not one a build can name")
    native = manifest.get("native")
    if not isinstance(native, dict):
        raise Refused(f"{mod_id}: not a native mod")
    tables = sorted(key for key, value in native.items() if isinstance(value, (list, dict)))
    if tables:
        raise Refused(f"{mod_id}: it changes the game's own source ([native.{', native.'.join(tables)}]), "
                      "which a build from the catalog does not do")
    if "build.rs" in files:
        raise Refused(f"{mod_id}: it has a build script, which a build from the catalog does not run")
    if "Cargo.toml" not in files or "src/lib.rs" not in files:
        raise Refused(f"{mod_id}: the package has no Cargo.toml or src/lib.rs")
    cargo = tomllib.loads(files["Cargo.toml"].decode("utf-8"))
    for section in ("build-dependencies", "target", "patch", "replace", "workspace"):
        if section in cargo:
            raise Refused(f"{mod_id}: its Cargo.toml has [{section}], which a build from the catalog does not take")
    if cargo.get("package", {}).get("build"):
        raise Refused(f"{mod_id}: it has a build script, which a build from the catalog does not run")
    if cargo.get("lib", {}).get("proc-macro"):
        raise Refused(f"{mod_id}: a procedural macro is no mod")
    api_name = None
    dependencies = []
    for name, spec in cargo.get("dependencies", {}).items():
        package = spec.get("package", name) if isinstance(spec, dict) else name
        if package == API:
            api_name = name
            dependencies.append((name, None))
        elif isinstance(spec, str):
            dependencies.append((name, {"version": spec}))
        elif any(key in spec for key in ("path", "git", "registry", "registry-index")):
            raise Refused(f"{mod_id}: its dependency {name} is not a registry crate")
        else:
            dependencies.append((name, spec))
    if api_name is None:
        raise Refused(f"{mod_id}: it does not depend on {API}")
    side = SIDES.get(manifest.get("side", "both"))
    if side is None:
        raise Refused(f"{mod_id}: its side {manifest.get('side')!r} is none a mod has")
    return {
        "id": mod_id,
        "version": version,
        "side": side,
        "hooks": hooks_path(files, mod_id),
        "api": API_LIB if api_name == API else api_name.replace("-", "_"),
        "dependencies": dependencies,
        "edition": str(cargo.get("package", {}).get("edition", "2024")),
        "features": cargo.get("features", {}),
    }


def value(v) -> str:
    """A TOML value (strings, booleans, numbers, lists of them, inline tables)."""
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, (int, float)):
        return str(v)
    if isinstance(v, str):
        return json.dumps(v)
    if isinstance(v, list):
        return "[" + ", ".join(value(x) for x in v) + "]"
    if isinstance(v, dict):
        return "{ " + ", ".join(f"{k} = {value(x)}" for k, x in v.items()) + " }"
    raise Refused(f"a manifest value of a kind a build does not write: {v!r}")


def package_manifest(p: dict) -> str:
    """The mod's Cargo manifest as a package of this workspace: its own registry dependencies, and this
    checkout's API crate in place of the SDK's it was written against."""
    api = (ROOT / "crates" / API).as_posix()
    lines = ["[package]", f'name = "hmod-{p["id"]}"', f'version = "0.0.0"', f'edition = "{p["edition"]}"',
             "publish = false", "", "[lib]", f'name = "hmod_{p["id"]}"', 'path = "src/lib.rs"', "",
             "[dependencies]"]
    for name, spec in p["dependencies"]:
        if spec is None:
            package = "" if name == API else f'package = "{API}", '
            lines.append(f'{name} = {{ {package}path = "{api}" }}')
        else:
            lines.append(f"{name} = {value(spec)}")
    if p["features"]:
        lines += ["", "[features]"] + [f"{name} = {value(list(on))}" for name, on in p["features"].items()]
    return "\n".join(lines) + "\n"


ENTRY = '''
/// Horizon PE: this mod's entry for the app's program, which it is compiled into
/// (tools/ios_native_sdk.py; crates/horizon-ios-game/src/built_mods.rs).
#[doc(hidden)]
pub mod horizon_pe_entry {{
    pub fn entry() -> {api}::ModEntry {{
        {api}::ModEntry::new("{id}", {hooks}())
    }}
}}
'''


def built_mods(plans: list[dict]) -> str:
    """crates/horizon-ios-game/src/built_mods.rs for these mods."""
    records = "".join(
        f'    mod_platform::__private::native::BuiltMod {{\n        id: "{p["id"]}",\n'
        f'        version: "{p["version"]}",\n        side: mod_platform::Side::{p["side"]},\n    }},\n'
        for p in plans)
    entries = "".join(f'        hmod_{p["id"]}::horizon_pe_entry::entry(),\n' for p in plans)
    names = ", ".join(f'{p["id"]} {p["version"]}' for p in plans)
    return f'''//! The code mods this app was built with: {names}. Written by tools/ios_native_sdk.py for this
//! build; the file in the repository is the one of an app without mods.

/// The mods compiled into this program, as the engine checks their packages against them.
static BUILT: &[mod_platform::__private::native::BuiltMod] = &[
{records}];

/// The mods' entries and typed listeners in the order they were built in, and their packages (beside
/// the program, in `horizon-native-mods/`), for `horizon_mod_host::load_code`. A package that does not
/// read leaves every mod out, with the reason in the log: the engine refuses an entry without its
/// checked manifest.
pub fn built() -> (
    Vec<mod_platform::ModEntry>,
    Vec<horizon_mod_host::SlotListener>,
    horizon_mod_package::runtime::Runtime,
) {{
    let packages = std::env::current_exe()
        .map_err(|error| error.to_string())
        .and_then(|program| {{
            horizon_mod_package::runtime::Runtime::load(&program, BUILT)
                .map_err(|error| error.to_string())
        }});
    match packages {{
        Ok(packages) => {{
            // In stderr.log, before the game's own log opens (the Simulator's check reads it).
            eprintln!("Horizon: mods built into this app: {names}");
            (
                vec![
{entries}                ],
                Vec::new(),
                packages,
            )
        }}
        Err(error) => {{
            eprintln!("Horizon: the mods this app was built with are left out: {{error}}");
            Default::default()
        }}
    }}
}}
'''


def unpack(folder: Path, mod_id: str, files: dict[str, bytes], p: dict) -> None:
    """The mod as the workspace package `folder/<id>/`, with its entry for the program."""
    work = folder / mod_id
    for name, data in files.items():
        target = work / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
    (work / "Cargo.toml").write_text(package_manifest(p), encoding="utf-8")
    with open(work / "src" / "lib.rs", "a", encoding="utf-8") as lib:
        lib.write(ENTRY.format(api=p["api"], id=mod_id, hooks=p["hooks"]))


def workspace_member() -> None:
    """The mods' folder joins the workspace."""
    root = ROOT / "Cargo.toml"
    text = root.read_text(encoding="utf-8")
    line = f'"{MODS_DIR}/*"'
    if line in text:
        return
    head = "members = [\n"
    if text.count(head) != 1:
        sys.exit("ios_native_sdk: Cargo.toml has no single `members = [` to add the mods' folder to")
    root.write_text(text.replace(head, f"{head}    {line},\n", 1), encoding="utf-8")


def survey(index: Path, report: Path) -> None:
    listed, _ = projects(index)
    folder = ROOT / MODS_DIR
    if folder.exists():
        shutil.rmtree(folder)
    rows = {}
    for mod_id in sorted(listed):
        versions = listed[mod_id].get("versions") or []
        if not versions:
            continue
        version = versions[0].get("version", "")
        row = {"version": version}
        try:
            if not ID.fullmatch(mod_id) or not VERSION.fullmatch(version):
                raise Refused(f"{mod_id}@{version} is not a name a build takes")
            files = members(fetch(versions[0]["file"]))
            p = plan(files)
            if (p["id"], p["version"]) != (mod_id, version):
                raise Refused(f"the package says it is {p['id']} {p['version']}")
            unpack(folder, mod_id, files, p)
            row["package"] = f"hmod-{mod_id}"
        except Refused as why:
            row["refused"] = str(why)
        except Exception as error:  # A package that does not read is a mod no build takes.
            row["refused"] = f"its package does not read: {error}"
        rows[mod_id] = row
        print(f"{mod_id} {version}: {row.get('package') or row['refused']}", file=sys.stderr)
    if any("package" in row for row in rows.values()):
        workspace_member()
    report.write_text(json.dumps(rows, indent=1) + "\n", encoding="utf-8")
    taken = sum("package" in row for row in rows.values())
    print(f"{taken} of {len(rows)} native mods are packages to compile")


# What a package's code is named by: a content mod with any of it does not run on an iPhone, whose
# apps run only the code they were installed with (WebAssembly needs a compiler at run time or an
# interpreter the game does not carry there).
CODE = (".wasm", ".so", ".dylib", ".dll", ".jar")


def content(index: Path, report: Path) -> None:
    """Which of the catalog's content mods (kind "mod") an iPhone runs: the ones whose package is
    content alone. The index does not say which carry code, so each package is opened."""
    data = json.loads(index.read_text(encoding="utf-8"))
    listed = sorted((p for p in data["projects"] if p.get("kind") == "mod"), key=lambda p: p["id"])
    rows = {}
    for project in listed:
        versions = project.get("versions") or []
        if not versions:
            continue
        row = {"version": versions[0].get("version", "")}
        try:
            files = members(fetch(versions[0]["file"]))
            code = sorted(name for name in files if name.lower().endswith(CODE))
            if code:
                row["refused"] = "it carries code an iPhone can't run: " + ", ".join(code[:3])
        except Refused as why:
            row["refused"] = str(why)
        except Exception as error:  # A package that does not read is a mod nobody can install.
            row["refused"] = f"its package does not read: {error}"
        rows[project["id"]] = row
        print(f"{project['id']} {row['version']}: {row.get('refused') or 'content alone'}", file=sys.stderr)
    report.write_text(json.dumps(rows, indent=1) + "\n", encoding="utf-8")
    runs = sum("refused" not in row for row in rows.values())
    print(f"{runs} of {len(rows)} content mods are content alone")


def add(index: Path, out: Path, wanted: list[str]) -> None:
    asked = []
    for item in wanted:
        mod_id, _, version = item.partition("@")
        if not ID.fullmatch(mod_id) or not VERSION.fullmatch(version):
            sys.exit(f"ios_native_sdk: {item!r} is not id@version")
        if mod_id in (a[0] for a in asked):
            sys.exit(f"ios_native_sdk: {mod_id} is named twice")
        asked.append((mod_id, version))
    if not asked:
        sys.exit("ios_native_sdk: no mod named")
    folder = ROOT / MODS_DIR
    if folder.exists():
        shutil.rmtree(folder)
    if out.exists():
        shutil.rmtree(out)
    plans = []
    for mod_id, version in sorted(asked):
        try:
            files = members(fetch(find(index, mod_id, version)))
            p = plan(files)
            if (p["id"], p["version"]) != (mod_id, version):
                raise Refused(f"{mod_id} {version}: the package says it is {p['id']} {p['version']}")
        except Refused as why:
            sys.exit(f"ios_native_sdk: {why}")
        # The package as it came, for the app: the engine reads a built mod's manifest and content.
        for name, data in files.items():
            target = out / "horizon-native-mods" / mod_id / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
        unpack(folder, mod_id, files, p)
        plans.append(p)

    workspace_member()
    manifest = GAME / "Cargo.toml"
    text = manifest.read_text(encoding="utf-8")
    head = "[target.'cfg(target_os = \"ios\")'.dependencies]\n"
    if text.count(head) != 1 or MARK in text:
        sys.exit("ios_native_sdk: crates/horizon-ios-game/Cargo.toml is not the repository's own")
    lines = "".join(f'hmod-{p["id"]} = {{ path = "../../{MODS_DIR}/{p["id"]}" }}\n' for p in plans)
    manifest.write_text(text.replace(head, f"{head}{MARK}\n{lines}", 1), encoding="utf-8")
    BUILT.write_text(built_mods(plans), encoding="utf-8")
    json.dump({"mods": [{"id": p["id"], "version": p["version"]} for p in plans]}, sys.stdout)
    print()


def listing(index: Path) -> None:
    listed, _ = projects(index)
    for mod_id in sorted(listed):
        versions = listed[mod_id].get("versions") or []
        if versions:
            print(f"{mod_id}@{versions[0]['version']}")


def check(package: Path) -> None:
    try:
        p = plan(members(package.read_bytes()))
    except Refused as why:
        sys.exit(f"ios_native_sdk: {why}")
    print(f"{p['id']} {p['version']}: a build can take it ({p['hooks']})")


def self_test() -> None:
    def package(manifest: str, cargo: str, lib: str = "#[horizon_mod]\npub struct M;\n", more=()) -> dict:
        files = {"horizon-mod.toml": manifest.encode(), "Cargo.toml": cargo.encode(), "src/lib.rs": lib.encode()}
        files.update(more)
        return files

    manifest = 'id = "zoom"\nversion = "1.2.0"\nside = "client"\n[native]\ngame = "26.3"\nhorizon = "a"\n'
    cargo = ('[package]\nname = "zoom"\nversion = "1.2.0"\nedition = "2024"\n[dependencies]\n'
             'horizon-mod-api = { path = ".horizon/sdk/api/crates/horizon-mod-api" }\nserde = "1"\n')
    p = plan(package(manifest, cargo))
    assert (p["id"], p["version"], p["side"], p["hooks"], p["api"]) == (
        "zoom", "1.2.0", "Client", "crate::hooks", "mod_api"), p
    made = package_manifest(p)
    assert 'name = "hmod-zoom"' in made and 'name = "hmod_zoom"' in made, made
    assert 'serde = { version = "1" }' in made, made
    assert "crates/horizon-mod-api" in made and ".horizon" not in made, made
    # The API crate under another name, and the attribute in a module of its own.
    renamed = cargo.replace("horizon-mod-api = {", 'mod_api = { package = "horizon-mod-api",')
    p = plan(package(manifest, renamed, lib="mod hud;\n", more={"src/hud/mod.rs": b"#[mod_api::horizon_mod]\nstruct H;\n"}))
    assert (p["api"], p["hooks"]) == ("mod_api", "crate::hud::hooks"), p
    assert 'mod_api = { package = "horizon-mod-api", path = ' in package_manifest(p)
    text = built_mods([p])
    assert 'id: "zoom"' in text and "hmod_zoom::horizon_pe_entry::entry()" in text and "Side::Client" in text
    refused = [
        package(manifest + "[[native.mixin]]\ntarget = \"x\"\n", cargo),
        package(manifest, cargo, more={"build.rs": b"fn main() {}"}),
        package(manifest, cargo + 'evil = { git = "https://example.com/x" }\n'),
        package(manifest, cargo + 'near = { path = "../x" }\n'),
        package(manifest, cargo.replace("[dependencies]", '[build-dependencies]\ncc = "1"\n[dependencies]')),
        package(manifest, cargo, lib="pub struct M;\n"),
        package(manifest.replace('"zoom"', '"Zoom"'), cargo),
        package(manifest.replace("[native]", "[other]"), cargo),
        package(manifest, cargo.replace('horizon-mod-api = { path = ".horizon/sdk/api/crates/horizon-mod-api" }\n', "")),
        package(manifest.replace('"client"', '"moon"'), cargo),
    ]
    for files in refused:
        try:
            plan(files)
        except Refused:
            continue
        raise AssertionError(f"taken: {files}")
    # A package's file names stay inside it.
    for name in ("../x", "/x", "a/../../x", "c:/x"):
        data = io.BytesIO()
        with zipfile.ZipFile(data, "w") as z:
            z.writestr(name, "x")
        try:
            members(data.getvalue())
        except Refused:
            continue
        raise AssertionError(f"unpacked {name}")
    print("ios_native_sdk: ok")


def main() -> None:
    args = sys.argv[1:]
    if args == ["--self-test"]:
        self_test()
    elif len(args) >= 4 and args[0] == "add":
        add(Path(args[1]), Path(args[2]).resolve(), args[3:])
    elif len(args) == 3 and args[0] == "survey":
        survey(Path(args[1]), Path(args[2]).resolve())
    elif len(args) == 3 and args[0] == "content":
        content(Path(args[1]), Path(args[2]).resolve())
    elif len(args) == 2 and args[0] == "list":
        listing(Path(args[1]))
    elif len(args) == 2 and args[0] == "check":
        check(Path(args[1]))
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
