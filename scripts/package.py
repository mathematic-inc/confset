"""Build and package an executable and its matching embedded Pkl library."""

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import tarfile
import tempfile
import zipfile
from pathlib import Path

import tomllib

ROOT = Path(__file__).resolve().parents[1]


def third_party_notices(destination, metadata):
    text = [
        "# Third-party notices\n",
        (
            "This build's Cargo dependency graph includes the following packages. Their source archives\n"
            "and declared licenses are listed below; available license texts accompany this file.\n"
        ),
    ]
    for package in sorted(
        metadata["packages"], key=lambda package: (package["name"], package["version"])
    ):
        if package.get("source") is None:
            continue
        name, version = package["name"], package["version"]
        text.append(
            f"\n## {name} {version}\n\nLicense: {package.get('license', 'see license file')}\n\n"
            f"Source: https://crates.io/api/v1/crates/{name}/{version}/download\n"
        )
        directory = Path(package["manifest_path"]).parent
        files = [
            path
            for path in directory.iterdir()
            if path.is_file()
            and path.name.upper().startswith(
                ("LICENSE", "LICENCE", "COPYING", "NOTICE")
            )
        ]
        if package.get("license_file"):
            explicit = directory / package["license_file"]
            if explicit.is_file():
                files.append(explicit)
        for file in sorted(set(files)):
            target = destination / "licenses" / f"{name}-{version}" / file.name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(file, target)
    (destination / "THIRD_PARTY_NOTICES.md").write_text(
        "\n".join(text), encoding="utf-8", newline="\n"
    )


def package(output, target):
    cargo = os.environ.get("CARGO", "cargo")
    rustc = os.environ.get("RUSTC", "rustc")
    manifest = tomllib.loads((ROOT / "Cargo.toml").read_text())
    version = manifest["package"]["version"]
    metadata = json.loads(
        subprocess.check_output(
            [cargo, "metadata", "--locked", "--format-version", "1"], cwd=ROOT
        )
    )
    package_id = next(
        package["id"]
        for package in metadata["packages"]
        if Path(package["manifest_path"]).resolve() == ROOT / "Cargo.toml"
    )
    if target is None:
        details = subprocess.check_output([rustc, "-vV"], text=True)
        target = next(
            line.removeprefix("host: ")
            for line in details.splitlines()
            if line.startswith("host: ")
        )
    result = subprocess.run(
        [
            cargo,
            "build",
            "--release",
            "--locked",
            "--target",
            target,
            "--message-format=json-render-diagnostics",
        ],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        check=True,
    )
    executable = None
    package_directory = None
    for line in result.stdout.splitlines():
        event = json.loads(line)
        if (
            event["reason"] == "compiler-artifact"
            and event["target"]["name"] == "confset"
            and event.get("executable")
        ):
            executable = Path(event["executable"])
        if (
            event["reason"] == "build-script-executed"
            and event["package_id"] == package_id
        ):
            package_directory = Path(event["out_dir"])
    if not executable or not package_directory:
        raise RuntimeError(
            "Cargo did not report the executable and embedded-package build directory"
        )
    output.mkdir(parents=True, exist_ok=True)
    output = output.resolve()
    with tempfile.TemporaryDirectory(
        prefix="confset-package-", dir=output
    ) as temporary:
        staging = Path(temporary)
        shutil.copyfile(executable, staging / executable.name)
        shutil.copymode(executable, staging / executable.name)
        for name in ["LICENSE", "README.md"]:
            shutil.copyfile(ROOT / name, staging / name)
        shutil.copytree(ROOT / "docs", staging / "docs")
        shutil.copytree(ROOT / "examples", staging / "examples")
        third_party_notices(staging, metadata)
        name = f"confset-{version}-{target}"
        if "windows" in target:
            archive = output / f"{name}.zip"
            with zipfile.ZipFile(
                archive, "w", compression=zipfile.ZIP_DEFLATED
            ) as writer:
                for path in sorted(staging.rglob("*")):
                    if path.is_file():
                        writer.write(path, path.relative_to(staging))
        else:
            archive = output / f"{name}.tar.gz"
            with tarfile.open(archive, "w:gz") as writer:
                for path in sorted(staging.iterdir()):
                    writer.add(path, arcname=path.name)
    for suffix in ["", ".sha256", ".zip"]:
        name = f"confset@{version}{suffix}"
        shutil.copyfile(package_directory / name, output / name)
    assets = [archive, output / f"confset@{version}", output / f"confset@{version}.zip"]
    checksums = output / f"SHA256SUMS-{target}"
    checksums.write_text(
        "".join(
            f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n"
            for path in assets
        ),
        encoding="utf-8",
        newline="\n",
    )
    archive.with_name(archive.name + ".sha256").write_text(
        f"{hashlib.sha256(archive.read_bytes()).hexdigest()}  {archive.name}\n",
        encoding="utf-8",
        newline="\n",
    )
    print(f"Packaged {archive}")
    print(f"Pkl package and checksums: {output}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--target")
    args = parser.parse_args()
    package(args.output, args.target)
