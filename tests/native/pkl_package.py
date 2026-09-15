"""Check the bundled library with the official Pkl evaluator as well as pklr."""

import argparse
import json
import os
import shutil
import subprocess
import tempfile
import zipfile
from pathlib import Path


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise AssertionError(f"Pkl rendered duplicate JSON key: {key}")
        result[key] = value
    return result


def verify(package_directory, workspace):
    packages = list(package_directory.glob("confset@*.zip"))
    if len(packages) != 1:
        raise AssertionError("Expected exactly one packaged Pkl library")
    executable = os.environ.get("CONFSET_NATIVE_PKL") or shutil.which("pkl")
    if not executable:
        raise RuntimeError(
            "The official Pkl CLI is required for package conformance checks"
        )
    workspace.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="pkl-package-", dir=workspace) as temporary:
        root = Path(temporary).resolve()
        library = root / "library"
        with zipfile.ZipFile(packages[0]) as archive:
            assert archive.namelist() == ["Builtins.pkl", "Config.pkl", "Renderers.pkl"]
            archive.extractall(library)
        source = (
            f'amends "{(library / "Config.pkl").as_uri()}"\n'
            f'import "{(library / "Builtins.pkl").as_uri()}"\n'
            f'import "{(library / "Renderers.pkl").as_uri()}"\n'
            "tools {\n"
            '  ["yaml"] = (Builtins.ryl) {\n'
            '    config { rules { ["trailing-spaces"] = "enable" } }\n'
            "  }\n"
            '  ["protobuf"] = (Builtins.buf) { config { version = "v1" } }\n'
            "}\n"
            "files {\n"
            '  ["ignore"] {\n'
            '    path = ".customignore"\n'
            '    format = "text"\n'
            '    config = Renderers.lines(List("build", "cache").map((name) -> name + "/"))\n'
            "  }\n"
            "}\n"
        )
        path = root / "confset.pkl"
        path.write_text(source, encoding="utf-8", newline="\n")
        result = subprocess.run(
            [
                executable,
                "eval",
                "--no-project",
                "--no-cache",
                "--format",
                "json",
                str(path),
            ],
            cwd=root,
            stdout=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            check=True,
            timeout=60,
        )
        value = json.loads(result.stdout, object_pairs_hook=unique_object)
        assert value["tools"]["yaml"]["config"]["rules"] == {
            "key-duplicates": "enable",
            "trailing-spaces": "enable",
        }
        assert value["tools"]["protobuf"]["config"]["version"] == "v1"
        assert value["files"]["ignore"]["config"] == "build/\ncache/\n"
    print(
        "PASS official Pkl: defaults, amendments, functions, and bundled helpers",
        flush=True,
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package-directory", type=Path, required=True)
    parser.add_argument("--workspace", type=Path, required=True)
    args = parser.parse_args()
    verify(args.package_directory, args.workspace)
