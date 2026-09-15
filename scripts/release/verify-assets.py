"""Verify all target checksums before merging release assets."""

import argparse
import hashlib
from pathlib import Path


def verify(directory):
    manifests = sorted(directory.glob("SHA256SUMS-*"))
    if not manifests:
        raise AssertionError("No target checksum manifests found")
    expected = {}
    for manifest in manifests:
        for line in manifest.read_text().splitlines():
            digest, name = line.split(maxsplit=1)
            if Path(name).name != name:
                raise AssertionError(f"Invalid asset name: {name}")
            if name in expected and expected[name] != digest:
                raise AssertionError(f"Platform builds disagree on {name}")
            expected[name] = digest
    for name, expected_digest in expected.items():
        actual = hashlib.sha256((directory / name).read_bytes()).hexdigest()
        if actual != expected_digest:
            raise AssertionError(f"Checksum mismatch: {name}")
    (directory / "SHA256SUMS").write_text(
        "".join(f"{digest}  {name}\n" for name, digest in sorted(expected.items()))
    )
    print(f"Verified {len(expected)} assets across {len(manifests)} platform builds")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    verify(parser.parse_args().directory)
