# Confset

Confset generates native tool configuration from one `confset.pkl`. It embeds
[pklr](https://github.com/jdx/pklr), so generation does not require an installed
Pkl CLI. Tools and editor extensions read the generated files through their
normal configuration discovery.

```sh
confset init --gitignore
# Add your tools to confset.pkl.
confset generate --watch
```

A small configuration:

```pkl
amends "package://github.com/mathematic-inc/confset/releases/download/v0.1.0/confset@0.1.0#/Config.pkl" // x-release-please-version

import "package://github.com/mathematic-inc/confset/releases/download/v0.1.0/confset@0.1.0#/Builtins.pkl" // x-release-please-version

tools {
  ["lint"] = (Builtins.oxlint) {
    config {
      categories {
        correctness = "deny"
        suspicious = "deny"
      }
    }
  }
  ["format"] = (Builtins.oxfmt) {
    config {
      printWidth = 100
      semi = false
    }
  }
}
```

`confset generate` writes `.oxlintrc.json` and `.oxfmtrc.json`. Run `oxlint`,
`oxfmt`, or their editor extensions as usual. In CI, run `confset generate`
before your normal checks. Keep `confset.pkl` in version control; commit the
native files too, or opt into the managed ignore block.

## Installation

Download a native archive from the [GitHub releases](https://github.com/mathematic-inc/confset/releases),
or use `cargo binstall confset` to install a prebuilt binary. To build from
source, run `cargo install confset --locked` with Rust 1.98.1 or newer.
The installed command is `confset`.

## Commands

```text
confset [--config FILE] init [--gitignore]
confset [--config FILE] generate [--watch]
confset [--config FILE] validate
confset [--config FILE] list
confset [--config FILE] clean
```

| Command | Effect |
| --- | --- |
| `init` | Create an empty starter if absent; preserve an existing source. It does not generate tool configuration. |
| `init --gitignore` | Also evaluate the declarations and establish a marked `.gitignore` block. |
| `generate` | Compile all outputs, check ownership and conflicts, then publish. |
| `generate --watch` | Generate immediately and regenerate after local inputs change. Errors keep the last successful outputs; a later save retries. Ctrl-C leaves the outputs in place. |
| `validate` | Evaluate, render, and check paths, ownership, and competing files without publishing. It can populate the Pkl package cache. |
| `list` | Show each declaration, format, destination, and `missing`, `managed`, `modified`, or `unowned` status. |
| `clean` | Remove unchanged owned outputs, including declarations since removed. It works with invalid or deleted source files and preserves modified or ambiguous outputs. |

Failures return a nonzero exit status. Watch mode reports compilation errors and
keeps running. Another generation or cleanup process cannot write while watch
mode holds the project lock.

## Configuration discovery

Confset selects exactly one source, in this order:

1. `--config FILE`.
2. `CONFSET_CONFIG`.
3. The nearest `confset.pkl`, searching the current directory and its ancestors.
4. The operating system's user configuration directory, under `confset/config.pkl`.

The user locations are `$XDG_CONFIG_HOME/confset/config.pkl` (or
`~/.config/confset/config.pkl`) on Linux,
`~/Library/Application Support/confset/config.pkl` on macOS, and
`%APPDATA%\confset\config.pkl` on Windows.

Relative destinations use the selected project's configuration directory. With
the user configuration fallback they use the directory where Confset was invoked.
An explicit missing source can be created by `init`; otherwise `init` creates
`confset.pkl` in the current directory when discovery finds nothing.

## Nested Pkl files and projects

An entry point can import modules from any depth. In the
[nested example](examples/nested/confset.pkl), the root imports
`config/web.pkl` and `config/python.pkl`; both import
`config/shared/settings.pkl`. Editing the shared file regenerates both tool
configurations in watch mode. Import paths are relative to the importing Pkl
file. Output paths remain relative to the selected entry point's project root.

Pkl inheritance works too. A file can `amends "config/project.pkl"`, which can
amend `shared/base.pkl`, and override selected settings at each level. Watch mode
tracks the entire local dependency chain, including files outside the root.

Independent subprojects can each contain a `confset.pkl`. Running Confset from a
subproject chooses its nearest entry point. A root invocation does not recursively
run every nested entry point: compose the child modules into the root Pkl file
when you want one generation to cover the whole repository.

## Built-ins and formats

Built-ins are amendable Pkl `Tool` values. The mapping key is your label; multiple
labels can use the same built-in in separate directories. `directory` selects a
nested project or an absolute directory. A built-in's `path`, if supplied, must
match the native filename for the selected format.

| Producer | Formats and native filenames |
| --- | --- |
| `oxlint` | JSON: `.oxlintrc.json` |
| `oxfmt` | JSON: `.oxfmtrc.json` |
| `prettier` | JSON: `.prettierrc.json`; YAML: `.prettierrc.yaml`; TOML: `.prettierrc.toml` |
| `rustfmt` | TOML: `rustfmt.toml`, also used by `cargo fmt` |
| `ruff` | TOML: `ruff.toml` |
| `ty` | TOML: `ty.toml` |
| `rumdl` | TOML: `.rumdl.toml` |
| `ryl` | TOML: `.ryl.toml`; YAML: `.yamllint.yaml` |
| `tombi` | TOML: `tombi.toml` |
| `typos` | TOML: `typos.toml` |
| `sqlfluff` | INI: `.sqlfluff` |
| `actionlint` | YAML: `.github/actionlint.yaml` |
| `gitleaks` | TOML: `.gitleaks.toml` |
| `lychee` | TOML: `lychee.toml` |
| `cargo_deny` | TOML: `deny.toml` |
| `buf` | YAML: `buf.yaml` |
| `typespec` | YAML: `tspconfig.yaml` |
| `knip` | JSON: `knip.json` |
| `shfmt` | INI: `.editorconfig` |
| `yamlfmt` | YAML: `.yamlfmt` |

JSON is the default for producers that support it; ryl defaults to TOML. The
built-in ryl value enables duplicate-key checking because its TOML format requires
an explicit ruleset. Buf starts with `version = "v2"`. Other settings come from
your `config` value and the tool's defaults.

Confset checks known alternative filenames and relevant `package.json` fields.
It refuses to generate a competing configuration over a handwritten one. When
you change formats, an unchanged owned alternative is removed in the same
publication. Options inside `config` remain native data: Confset checks its own
schema and representability, while the native tool validates its option names and
semantics. Format restrictions and filenames live in `pkl/catalog.json`.

JSON, YAML, and TOML preserve scalar values, nesting, and array order. TOML rejects
nulls. INI supports scalar entries and one level of named sections; it rejects
arrays, nulls, nested sections, line breaks, and surrounding whitespace that an
INI parser could discard. Pkl functions must be called before rendering. There
is no JavaScript or TypeScript emitter, callback representation, or runtime hook.

## Pkl computations and custom outputs

Pkl functions, imports, amendments, and comprehensions compute the final data.
For example, Knip receives a normal `knip.json`:

```pkl
local modules = List("main", "worker")
tools {
  ["unused"] = (Builtins.knip) {
    config {
      entry = modules.map((name) -> "src/\(name).ts")
      project = List("src/**/*.ts")
    }
  }
}
```

For a tool without a built-in, declare `files` with `path`, `format`, and `config`,
or use a `Tool` with an explicit `path` and `format`. These outputs have the same
ownership protections. Their destination and discovery rules are your choice.

```pkl
files {
  ["application"] {
    path = "app/settings.yaml"
    format = "yaml"
    config {
      enabled = true
      retries = 3
    }
  }
}
```

Companion files belong in a producer's `files` mapping. Use `format = "text"`
with any Pkl function returning a string for additional text formats. The embedded
`Renderers.pkl` provides `lines(List<String>)` for newline-delimited files. See
[the web example](examples/web/confset.pkl) for a companion ignore file,
[the polyglot example](examples/polyglot/confset.pkl) for nested projects, and
[the custom example](examples/custom/confset.pkl) for a Pkl text renderer.

Pkl can read `env:CONFSET_PROJECT_DIR` and `env:CONFSET_CONFIG_DIR` to obtain the
resolved output root and source directory. Ordinary Pkl local imports and reads
are tracked for watching. Remote inputs are evaluated when a local save triggers
a generation; Confset does not poll remote services or environment variables.

## Ownership and recovery

`.confset/state.json` records absolute output paths and last-written SHA-256
hashes. Keep that directory between runs. Losing it removes Confset's evidence
of ownership; identical file contents alone do not authorize adoption.

Confset refuses unowned files, externally edited owned files, symlink destinations
or parents, duplicate destinations, file/directory collisions, and collisions with
source inputs or state. It compiles and checks the complete output set before
writing any output. Unchanged files keep their timestamps and permissions.

Changed files are staged beside their destinations and replaced atomically per
file. A durable journal records the previous and staged bytes. The next writer
recovers interrupted publication, preserving unexpected edits. Publication is not
an atomic transaction across multiple files: a reader can briefly observe a mix
during a successful update. A process interruption is recoverable; filesystem and
power-loss guarantees still depend on the operating system and storage device.

`clean` removes only files whose bytes still match their ownership record. It
preserves the Pkl source, keeps records for modified outputs, reports them, and
returns a failure status. Move edited files aside or restore their recorded
content before retrying generation. A malformed ownership record or ambiguous
recovery journal is preserved for inspection rather than discarded.

## Git ignore management

`init --gitignore` enables this block:

```gitignore
# BEGIN CONFSET GENERATED FILES
.confset-stage-*
/.oxfmtrc.json
/.oxlintrc.json
/.confset/
# END CONFSET GENERATED FILES
```

Entries are escaped, sorted, and deduplicated. Existing manual content, comments,
other marked blocks, and line endings are preserved. Once enabled, `generate`
and watch mode keep entries synchronized with published outputs. Without the
block, generation leaves `.gitignore` alone. Files outside the project directory
are reported as outside its ignore scope. Already tracked generated files are
reported; Confset never changes the Git index. Cleanup leaves the block available
for the next generation.

## Building and contributing

Use Rust **1.98.1** (the CI toolchain), then run:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

Direct dependencies use the latest stable releases selected for this version;
`Cargo.lock` pins the complete graph. `insta` snapshots cover the command surface,
diagnostics, and rendered formats. Review snapshot changes before accepting them.

The shared prebuilt workflow builds and tests macOS, Linux, and Windows on amd64
and arm64, including GNU and musl Linux distributions. Native integration tests run the public tools and the actual Oxlint/Oxfmt
language servers through an LSP test client. See [verification](docs/verification.md)
for setup and the distinction between protocol checks and editor UI checks.

When developing on a separate drive, set `CARGO_HOME`, `CARGO_TARGET_DIR`, `TMPDIR`,
`CONFSET_PKL_CACHE_DIR`, and native-tool cache locations to directories on that
drive. Resolve symlinks before relying on a directory's apparent location.

`python scripts/package.py --output DIST` builds a release executable and creates
its platform archive, Pkl package metadata, package ZIP, checksums, and third-party
license notices. Pass `--target TRIPLE` for an installed Rust target. See
[the release workflow](docs/releasing.md) for artifact names and publication.
