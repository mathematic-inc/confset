# Releases

Confset follows Mathematic's Rust CLI release process: Release Please maintains
a release PR, merging it creates a version tag, and the tag starts the release
workflow. The release environment permits the main branch and version tags.

## Pipeline

1. Release Please updates Cargo versions, the changelog, and the release manifest.
   Its workflow then synchronizes both version components in Pkl package addresses
   in the release PR; the generic updater changes only the first version per line.
2. The tag workflow builds and tests the package and calls the reusable prebuilt
   workflow. It builds macOS and Windows on amd64 and arm64, plus GNU and musl
   Linux builds for both architectures.
3. Every archive is checksum-verified, extracted, and run on its target platform.
   Native tool discovery, editor protocol checks, and official Pkl conformance
   checks run against those executables.
4. The workflow verifies that all builds produced the same Pkl package, attests
   the assets, and attaches them to the GitHub release.
5. crates.io trusted publishing publishes the Rust package, `confset`.
6. Fresh runners install the release through Cargo Binstall and mise without
   compiling, run the installed executables, and compare their bytes.

The crate and installed command are both named `confset`.

## Initial registry setup

The first crates.io publication needs a maintainer's publishing credential.
After that publication, configure a trusted publisher for `confset` with
GitHub owner `mathematic-inc`, repository `confset`, workflow `release.yml`, and
environment `release`. Subsequent releases use short-lived OIDC credentials. The publish step uses
Mathematic’s existing already-published handling so the first token-based
publication can be followed by the normal trusted-publishing workflow.
Release Please uses the organization's `RELEASE_TOKEN` secret.

## Assets

Each native archive is named `confset-VERSION-TARGET.tar.gz`, or `.zip` on Windows,
and has an adjacent `.sha256` file. The root executable path matches the Cargo
Binstall metadata in `Cargo.toml`. Archives also contain documentation, public
examples, and third-party license notices.

The Pkl assets are `confset@VERSION`, `confset@VERSION.zip`, and
`confset@VERSION.sha256`. These are the metadata, exact embedded ZIP bytes, and
metadata checksum. They are published below the matching `vVERSION` release URL
in `mathematic-inc/confset`. Fixed ZIP timestamps, ordering, platform metadata,
permissions, and normalized line endings keep the package identical across
platforms. The checksum manifests reject divergent packages in pull request CI
and before publication.

For a local package build:

```sh
python scripts/package.py --target TARGET --output dist
```

The embedded package evaluates offline in Confset. Standard Pkl tooling resolves
the published metadata and ZIP at the versioned package address. Never replace
published assets under an existing version.
