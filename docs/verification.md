# Verification

The standard suite runs with Rust 1.98.1:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

Integration tests invoke the CLI in disposable directories, with isolated Pkl
caches and offline evaluation. They cover discovery, idempotent initialization,
representability, native filenames, ownership, stale outputs, symlinks, writer
coordination, crash recovery, managed ignore entries, and watch mode. Internal
publication tests construct durable journals at interruption boundaries and then
run real recovery against filesystem contents.

Snapshots use insta. To intentionally update them, run `INSTA_UPDATE=always cargo
test --test compiler_snapshots` and review each changed `.snap` file. CI never
accepts snapshots automatically.

## Public native tools

Install the pinned public tools in `tests/native/mise.toml` with mise, install
SQLFluff 4.3.0 with `uv tool install sqlfluff==4.3.0`, and install the npm fixtures:

```sh
npm ci --prefix tests/native
python tests/native/discovery.py --confset target/release/confset --modules tests/native/node_modules --workspace tests/native/fixtures
python tests/native/editor_reload.py --confset target/release/confset --modules tests/native/node_modules --workspace tests/native/fixtures
```

On Windows the executable is `confset.exe`. Native tools must be on `PATH`, or
set `CONFSET_NATIVE_TOOL` to an executable, with the tool's uppercase name and
hyphens replaced by underscores. For example, `CONFSET_NATIVE_CARGO_DENY` points
to `cargo-deny`. Set `CONFSET_NODE` to use a particular Node executable. The
`--tool NAME` option selects individual native checks and can be repeated.

Native fixtures should be beside the installed `node_modules` directory so Node
can resolve public plugins and packages. On an external development drive, use
an external `--workspace`, npm cache, mise data/cache directories, uv cache/tool
directories, and temporary directory. No test reads another project's settings.

After packaging, `python tests/native/pkl_package.py --package-directory dist
--workspace tests/native/fixtures` checks the library with the official Pkl
0.32.1 evaluator, including default amendments and duplicate-key detection.
Confset itself continues to use embedded pklr.

The discovery suite generates valid configurations, invokes the native commands
without configuration-path flags, checks selected output settings, and then
checks each tool's response to an invalid discovered file. Tombi warns and uses
defaults instead of failing; the test checks that warning. shfmt's EditorConfig
integration is checked by its actual indentation output.

The editor suite starts the real `oxlint --lsp` and `oxfmt --lsp` servers. A small
LSP client opens a document, regenerates the configuration, sends the standard
watched-file notification, and verifies changed diagnostics or formatting in the
same session. These are headless protocol integration checks, not a claim that
a particular editor's UI was exercised. For a manual UI check, open a project
with the ordinary Oxc extension, start `confset generate --watch`, edit a rule or
format option in Pkl, and verify diagnostics or format-on-save change after the
generated JSON updates.

The Intel macOS job builds ryl from its published Cargo package because its
upstream release provides only an arm64 macOS binary. Linux amd64 checks use
Lychee’s upstream musl archive so the current tool runs on the distribution’s
Ubuntu 22.04 compatibility baseline.

The shared prebuilt workflow runs the Rust suite and native integrations on all
eight distribution targets: macOS and Windows amd64/arm64, and GNU/musl Linux
amd64/arm64. It retains executable archives and Pkl package assets.
A configured matrix is not evidence of a passing run; inspect the CI results for
the revision being released.
