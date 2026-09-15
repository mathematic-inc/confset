"""Exercise config reloads through the real Oxlint and Oxfmt editor LSP servers."""

import json
import os
from pathlib import Path
import queue
import subprocess
import tempfile
import threading
import time

from discovery import Native, arguments


class Editor:
    def __init__(self, command, root):
        self.root = root
        self.messages = queue.Queue()
        self.sequence = 0
        self.log = tempfile.TemporaryFile()
        self.process = subprocess.Popen(
            command + ["--lsp"],
            cwd=root,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=self.log,
        )
        threading.Thread(target=self.read, daemon=True).start()

    def read(self):
        try:
            while True:
                headers = {}
                while True:
                    line = self.process.stdout.readline()
                    if not line:
                        raise EOFError("language server closed stdout")
                    if line == b"\r\n":
                        break
                    key, value = line.decode().split(":", 1)
                    headers[key.lower()] = value.strip()
                body = self.process.stdout.read(int(headers["content-length"]))
                self.messages.put(json.loads(body))
        except Exception as error:
            self.messages.put(error)

    def send(self, message):
        body = json.dumps({"jsonrpc": "2.0", **message}).encode()
        self.process.stdin.write(f"Content-Length: {len(body)}\r\n\r\n".encode() + body)
        self.process.stdin.flush()

    def notify(self, method, params):
        self.send({"method": method, "params": params})

    def wait(self, predicate, timeout=30):
        deadline = time.monotonic() + timeout
        seen = []
        while time.monotonic() < deadline:
            try:
                message = self.messages.get(timeout=deadline - time.monotonic())
            except queue.Empty:
                break
            if isinstance(message, Exception):
                raise message
            seen.append(message)
            if "method" in message and "id" in message:
                method = message["method"]
                if method == "workspace/configuration":
                    result = [
                        {"enable": True, "run": "onType"}
                        for _ in message["params"]["items"]
                    ]
                elif method == "workspace/workspaceFolders":
                    result = [{"uri": self.root.as_uri(), "name": "fixture"}]
                else:
                    result = None
                self.send({"id": message["id"], "result": result})
            elif predicate(message):
                return message
        self.log.seek(0)
        raise AssertionError(
            f"LSP timed out. Messages: {seen}\n{self.log.read().decode(errors='replace')}"
        )

    def request(self, method, params):
        self.sequence += 1
        request_id = self.sequence
        self.send({"id": request_id, "method": method, "params": params})
        response = self.wait(lambda message: message.get("id") == request_id)
        if "error" in response:
            raise AssertionError(response)
        return response.get("result")

    def initialize(self, file, text):
        self.request(
            "initialize",
            {
                "processId": os.getpid(),
                "rootUri": self.root.as_uri(),
                "workspaceFolders": [{"uri": self.root.as_uri(), "name": "fixture"}],
                "initializationOptions": [
                    {
                        "workspaceUri": self.root.as_uri(),
                        "options": {"run": "onType"},
                    }
                ],
                "capabilities": {
                    "workspace": {
                        "didChangeWatchedFiles": {"dynamicRegistration": True}
                    },
                    "textDocument": {"publishDiagnostics": {}, "formatting": {}},
                },
            },
        )
        self.notify("initialized", {})
        self.notify(
            "textDocument/didOpen",
            {
                "textDocument": {
                    "uri": file.as_uri(),
                    "languageId": "typescript",
                    "version": 1,
                    "text": text,
                }
            },
        )

    def changed(self, file):
        self.notify(
            "workspace/didChangeWatchedFiles",
            {"changes": [{"uri": file.as_uri(), "type": 2}]},
        )

    def close(self):
        try:
            if self.process.poll() is None:
                self.request("shutdown", None)
                self.notify("exit", None)
                self.process.wait(timeout=5)
        finally:
            if self.process.poll() is None:
                self.process.kill()
            self.process.wait()
            self.process.stdin.close()
            self.process.stdout.close()
            self.log.close()


def oxlint(native, root):
    file = root / "index.ts"
    source = "export const compare = (a: number, b: number) => a == b;\n"
    file.write_text(source, encoding="utf-8", newline="\n")

    def generate(severity):
        native.generate(
            root,
            'tools { ["lint"] = (Builtins.oxlint) { '
            'config { rules { eqeqeq = "' + severity + '" } } } }',
        )

    generate("deny")
    editor = Editor(native.command("oxlint"), root)
    try:
        editor.initialize(file, source)

        def diagnostics(message):
            return (
                message.get("method") == "textDocument/publishDiagnostics"
                and message["params"]["uri"] == file.as_uri()
            )

        editor.wait(
            lambda message: (
                diagnostics(message)
                and any(
                    "eqeqeq" in str(item) for item in message["params"]["diagnostics"]
                )
            )
        )
        generate("off")
        editor.changed(root / ".oxlintrc.json")
        editor.wait(
            lambda message: (
                diagnostics(message) and not message["params"]["diagnostics"]
            )
        )
    finally:
        editor.close()
    print(
        "PASS editor reload: oxlint diagnostics update without reopening the document",
        flush=True,
    )


def oxfmt(native, root):
    file = root / "index.ts"
    source = "export const value=1\n"
    file.write_text(source, encoding="utf-8", newline="\n")

    def generate(semi):
        native.generate(
            root,
            'tools { ["format"] = (Builtins.oxfmt) { '
            "config { semi = " + json.dumps(semi) + " } } }",
        )

    generate(True)
    editor = Editor(native.command("oxfmt"), root)
    try:
        editor.initialize(file, source)

        def formatted():
            edits = editor.request(
                "textDocument/formatting",
                {
                    "textDocument": {"uri": file.as_uri()},
                    "options": {"tabSize": 2, "insertSpaces": True},
                },
            )
            # This fixture is ASCII, so its UTF-16 columns are also Python offsets.
            assert source.isascii()
            lines = source.splitlines(keepends=True)

            def offset(position):
                return sum(map(len, lines[: position["line"]])) + position["character"]

            result = source
            for edit in sorted(
                edits or [],
                key=lambda item: offset(item["range"]["start"]),
                reverse=True,
            ):
                start = offset(edit["range"]["start"])
                end = offset(edit["range"]["end"])
                result = result[:start] + edit["newText"] + result[end:]
            return result

        assert "value = 1;" in formatted()
        generate(False)
        editor.changed(root / ".oxfmtrc.json")
        assert "value = 1\n" in formatted()
    finally:
        editor.close()
    print(
        "PASS editor reload: oxfmt formatting changes in the existing session",
        flush=True,
    )


if __name__ == "__main__":
    args = arguments()
    native = Native(args.confset, args.modules, args.workspace)
    for tool, check in [("oxlint", oxlint), ("oxfmt", oxfmt)]:
        if not args.tool or tool in args.tool:
            with tempfile.TemporaryDirectory(
                prefix=tool + "-editor-", dir=native.workspace
            ) as directory:
                check(native, Path(directory))
