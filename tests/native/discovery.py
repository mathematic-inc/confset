"""Run real tools against generated native files, without configuration-path flags."""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import traceback


def pkl(value):
    if isinstance(value, dict):
        return (
            "new Dynamic {\n"
            + "\n".join(
                f"[{json.dumps(key)}] = {pkl(item)}" for key, item in value.items()
            )
            + "\n}"
        )
    if isinstance(value, list):
        return "List(" + ", ".join(pkl(item) for item in value) + ")"
    return json.dumps(value, ensure_ascii=False)


class Native:
    def __init__(self, confset, modules, workspace):
        self.confset = str(Path(confset).resolve())
        self.modules = Path(modules).resolve()
        self.workspace = Path(workspace).resolve()
        self.workspace.mkdir(parents=True, exist_ok=True)
        self.version = subprocess.check_output(
            [self.confset, "--version"], text=True
        ).split()[1]
        self.base = (
            "package://github.com/mathematic-inc/confset/releases/download/"
            f"v{self.version}/confset@{self.version}"
        )

    def command(self, tool):
        npm = {
            "oxlint": "oxlint/bin/oxlint",
            "oxfmt": "oxfmt/bin/oxfmt",
            "prettier": "prettier/bin/prettier.cjs",
            "knip": "knip/bin/knip.js",
            "typespec": "@typespec/compiler/cmd/tsp.js",
        }
        if tool in npm:
            node = os.environ.get("CONFSET_NODE") or shutil.which("node")
            if not node:
                raise RuntimeError("Node is required for native integration tests")
            return [node, str(self.modules / npm[tool])]
        executable = os.environ.get("CONFSET_NATIVE_" + tool.upper().replace("-", "_"))
        executable = executable or shutil.which(tool)
        if not executable:
            raise RuntimeError(f"Required native tool is missing: {tool}")
        return [executable]

    def generate(self, root, body):
        source = (
            f'amends "{self.base}#/Config.pkl"\n'
            f'import "{self.base}#/Builtins.pkl"\n' + body
        )
        (root / "confset.pkl").write_text(source, encoding="utf-8", newline="\n")
        environment = os.environ | {
            "CONFSET_PKL_OFFLINE": "true",
            "CONFSET_PKL_CACHE_DIR": str(root / ".cache"),
        }
        environment.pop("CONFSET_CONFIG", None)
        self.run([self.confset, "generate"], root, env=environment)

    @staticmethod
    def run(command, root, *, success=True, env=None):
        result = subprocess.run(
            command,
            cwd=root,
            env=env,
            text=True,
            encoding="utf-8",
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            timeout=90,
        )
        if (result.returncode == 0) != success:
            raise AssertionError(
                f"{command} exited {result.returncode} in {root}:\n{result.stdout}"
            )
        return result.stdout

    def check(self, tool, config, files, args, filename, contains=None, format=None):
        with tempfile.TemporaryDirectory(
            prefix=tool + "-", dir=self.workspace
        ) as temporary:
            root = Path(temporary)
            for name, content in files.items():
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(content, encoding="utf-8", newline="\n")
            self.run(["git", "init", "-q"], root)
            symbol = tool.replace("-", "_")
            selection = "\nformat = " + json.dumps(format) if format else ""
            self.generate(
                root,
                'tools { ["native"] = (Builtins.'
                + symbol
                + ") { config = "
                + pkl(config)
                + selection
                + " } }\n",
            )
            command = self.command(tool) + args
            self.run(command, root)
            if contains:
                name, expected = contains
                actual = (root / name).read_text(encoding="utf-8")
                if expected not in actual:
                    raise AssertionError(f"{tool}: expected {expected!r} in {actual!r}")
            # A deliberately malformed generated file must make the same invocation fail.
            # This proves the tool actually discovers the filename instead of using defaults.
            if tool != "shfmt":
                self.generate(
                    root,
                    'files { ["invalid"] { path = '
                    + json.dumps(filename)
                    + '\nformat = "text"\nconfig = "[this is not valid configuration\\n" } }',
                )
                invalid = subprocess.run(
                    command,
                    cwd=root,
                    text=True,
                    encoding="utf-8",
                    stdout=subprocess.PIPE,
                    stderr=subprocess.STDOUT,
                    timeout=90,
                )
                # Tombi reports an invalid discovered file and uses defaults instead
                # of failing. Its warning still identifies the actual config path.
                if invalid.returncode == 0 and not (
                    tool == "tombi"
                    and filename in invalid.stdout
                    and "failed to parse config" in invalid.stdout
                ):
                    raise AssertionError(
                        f"{tool} did not reject or report {filename}:\n{invalid.stdout}"
                    )
        print(f"PASS native discovery: {tool} ({filename})", flush=True)


def run_suite(native, selected):
    cases = [
        (
            "oxlint",
            {"rules": {"eqeqeq": "deny"}},
            {"index.ts": "export const equal = (a: number, b: number) => a === b;\n"},
            ["index.ts"],
            ".oxlintrc.json",
            None,
        ),
        (
            "oxfmt",
            {"semi": False},
            {"index.ts": "export const value = 1;\n"},
            ["index.ts"],
            ".oxfmtrc.json",
            ("index.ts", "value = 1\n"),
        ),
        (
            "prettier",
            {"semi": False},
            {"index.js": "export const value = 1;\n"},
            ["--write", "index.js"],
            ".prettierrc.json",
            ("index.js", "value = 1\n"),
        ),
        (
            "prettier",
            {"semi": False},
            {"index.js": "export const value = 1;\n"},
            ["--write", "index.js"],
            ".prettierrc.yaml",
            ("index.js", "value = 1\n"),
            "yaml",
        ),
        (
            "prettier",
            {"semi": False},
            {"index.js": "export const value = 1;\n"},
            ["--write", "index.js"],
            ".prettierrc.toml",
            ("index.js", "value = 1\n"),
            "toml",
        ),
        (
            "prettier",
            {"plugins": ["prettier-plugin-svelte"], "semi": False},
            {"Component.svelte": "<script>const value=1;</script><p>{value}</p>\n"},
            ["--write", "Component.svelte"],
            ".prettierrc.json",
            ("Component.svelte", "const value = 1\n"),
        ),
        (
            "rustfmt",
            {"hard_tabs": True},
            {"main.rs": 'fn main(){println!("ok");}\n'},
            ["main.rs"],
            "rustfmt.toml",
            ("main.rs", "\tprintln!"),
        ),
        (
            "ruff",
            {"lint": {"ignore": ["F401"]}},
            {"main.py": "import os\n"},
            ["check", "main.py"],
            "ruff.toml",
            None,
        ),
        (
            "ty",
            {"rules": {"invalid-assignment": "ignore"}},
            {"main.py": 'value: int = "text"\n'},
            ["check", "main.py"],
            "ty.toml",
            None,
        ),
        (
            "rumdl",
            {"global": {"disable": ["MD013"]}},
            {"README.md": "# Title\n\n" + "a" * 150 + "\n"},
            ["check", "README.md"],
            ".rumdl.toml",
            None,
        ),
        (
            "ryl",
            {"rules": {"key-duplicates": "enable"}},
            {"input.yaml": "---\nname: example\n"},
            ["check", "input.yaml"],
            ".ryl.toml",
            None,
        ),
        (
            "ryl",
            {"extends": "default"},
            {"input.yaml": "---\nname: example\n"},
            ["check", "input.yaml"],
            ".yamllint.yaml",
            None,
            "yaml",
        ),
        (
            "tombi",
            {},
            {"input.toml": 'name = "example"\n'},
            ["lint", "input.toml"],
            "tombi.toml",
            None,
        ),
        (
            "typos",
            {"default": {"extend-words": {"teh": "teh"}}},
            {"input.txt": "teh\n"},
            ["input.txt"],
            "typos.toml",
            None,
        ),
        (
            "sqlfluff",
            {"sqlfluff": {"dialect": "sqlite"}},
            {"input.sql": "SELECT 1;\n"},
            ["lint", "input.sql"],
            ".sqlfluff",
            None,
        ),
        (
            "actionlint",
            {},
            {
                ".github/workflows/check.yml": "name: Check\non: push\njobs:\n  test:\n    runs-on: ubuntu-latest\n"
                "    steps:\n      - run: echo hello\n"
            },
            ["-shellcheck=", ".github/workflows/check.yml"],
            ".github/actionlint.yaml",
            None,
        ),
        (
            "gitleaks",
            {"extend": {"useDefault": True}},
            {"input.txt": "Hello, world.\n"},
            ["dir", ".", "--no-banner", "--redact"],
            ".gitleaks.toml",
            None,
        ),
        (
            "lychee",
            {"offline": True},
            {"links.md": "# Links\n\nNo links yet.\n"},
            ["--no-progress", "links.md"],
            "lychee.toml",
            None,
        ),
        (
            "cargo-deny",
            {"bans": {"multiple-versions": "deny"}},
            {
                "Cargo.toml": '[package]\nname="native-fixture"\nversion="0.1.0"\nedition="2024"\n',
                "src/lib.rs": "pub fn value() -> u32 { 1 }\n",
            },
            ["check", "bans"],
            "deny.toml",
            None,
        ),
        (
            "buf",
            {"version": "v2", "lint": {"use": ["MINIMAL"]}},
            {
                "example/v1/message.proto": 'syntax = "proto3";\npackage example.v1;\nmessage Item { string name = 1; }\n'
            },
            ["lint"],
            "buf.yaml",
            None,
        ),
        (
            "typespec",
            {},
            {"main.tsp": "model Item { name: string; }\n"},
            ["compile", "main.tsp", "--no-emit"],
            "tspconfig.yaml",
            None,
        ),
        (
            "knip",
            {"entry": ["main.ts"], "project": ["main.ts"]},
            {
                "package.json": '{"name":"native-fixture","private":true}\n',
                "main.ts": 'console.log("hello");\n',
            },
            ["--no-progress"],
            "knip.json",
            None,
        ),
        (
            "shfmt",
            {"root": True, "*.sh": {"indent_style": "space", "indent_size": 4}},
            {"main.sh": "#!/bin/sh\nif true; then\necho hello\nfi\n"},
            ["-w", "main.sh"],
            ".editorconfig",
            ("main.sh", "    echo hello\n"),
        ),
        (
            "yamlfmt",
            {},
            {"input.yaml": "name: example\n"},
            ["input.yaml"],
            ".yamlfmt",
            None,
        ),
    ]
    unknown = set(selected) - {case[0] for case in cases}
    if unknown:
        raise ValueError(f"Unknown native tools: {sorted(unknown)}")
    failures = []
    for case in cases:
        if not selected or case[0] in selected:
            try:
                native.check(*case)
            except Exception:
                failures.append(f"{case[0]} ({case[4]})")
                traceback.print_exc()
    if failures:
        raise AssertionError("Native discovery failed: " + ", ".join(failures))


def arguments():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--confset", required=True)
    parser.add_argument(
        "--modules", required=True, help="Installed public npm packages"
    )
    parser.add_argument(
        "--workspace", required=True, help="Directory for disposable native fixtures"
    )
    parser.add_argument("--tool", action="append", default=[])
    return parser.parse_args()


if __name__ == "__main__":
    args = arguments()
    run_suite(Native(args.confset, args.modules, args.workspace), args.tool)
