#!/usr/bin/env python3
"""Smoke test for the lp-lsp binary over stdio (python3 stdlib only).

Usage: smoke.py [path/to/lp-lsp] [--allow-null-formatting]

Opens a small LP file with a duplicate constraint name and messy spacing,
checks that diagnostics are pushed and that formatting answers with a list of
edits, exercises the custom `lp/*` requests on a clean model (advertised
capabilities, model, explain, graph, matrix, diff, solverInfo, an error path),
then shuts the server down. Exits non-zero on any failure.
"""

import json
import queue
import subprocess
import sys
import threading
import time

TIMEOUT_S = 10.0
URI = "file:///smoke/model.lp"
TEXT = """Minimize
 obj:   3 x  +2 y
Subject To
 c1:  x + y >= 1
 c1: x   - y <= 4
Bounds
   x <= 10
End
"""


CLEAN_URI = "file:///smoke/clean.lp"
CLEAN_TEXT = """Minimize
 obj: 3 x + 2 y
Subject To
 c1: x + y >= 2
 c2: x - y <= 8
Bounds
 x <= 10
End
"""

LP_REQUESTS = ["lp/model", "lp/explain", "lp/graph", "lp/matrix", "lp/diff", "lp/solve", "lp/solverInfo"]


def fail(message: str) -> None:
    print(f"FAIL: {message}", file=sys.stderr)
    sys.exit(1)


def send(proc: subprocess.Popen, message: dict) -> None:
    body = json.dumps({"jsonrpc": "2.0", **message}).encode("utf-8")
    proc.stdin.write(f"Content-Length: {len(body)}\r\n\r\n".encode("ascii") + body)
    proc.stdin.flush()


def read_message(stream) -> dict | None:
    """Read one framed message; None at end of stream."""
    length = None
    while True:
        line = stream.readline()
        if not line:
            return None
        line = line.rstrip(b"\r\n")
        if not line:
            break
        name, _, value = line.decode("ascii").partition(":")
        if name.strip().lower() == "content-length":
            length = int(value.strip())
    if length is None:
        raise ValueError("message without Content-Length")
    body = stream.read(length)
    if len(body) != length:
        return None
    return json.loads(body.decode("utf-8"))


def reader(proc: subprocess.Popen, inbox: queue.Queue) -> None:
    try:
        while (message := read_message(proc.stdout)) is not None:
            inbox.put(message)
    except (ValueError, json.JSONDecodeError) as e:
        inbox.put({"smoke_error": str(e)})
    inbox.put(None)


def wait_for(proc: subprocess.Popen, inbox: queue.Queue, predicate, what: str) -> dict:
    """Return the first message matching `predicate`, answering server requests."""
    deadline = time.monotonic() + TIMEOUT_S
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            fail(f"timed out waiting for {what}")
        try:
            message = inbox.get(timeout=remaining)
        except queue.Empty:
            fail(f"timed out waiting for {what}")
        if message is None:
            fail(f"server closed stdout while waiting for {what}")
        if "smoke_error" in message:
            fail(f"bad framing from server: {message['smoke_error']}")
        if predicate(message):
            return message
        if "id" in message and "method" in message:
            # Server-to-client request (progress, registration, ...): accept it.
            send(proc, {"id": message["id"], "result": None})
        elif message.get("method") == "window/logMessage":
            print(f"server log: {message['params'].get('message')}")


def response_to(request_id: int):
    return lambda m: m.get("id") == request_id and "method" not in m


def main() -> None:
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    allow_null_formatting = "--allow-null-formatting" in sys.argv[1:]
    binary = args[0] if args else "target/release/lp-lsp"

    try:
        proc = subprocess.Popen([binary], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
    except OSError as e:
        fail(f"cannot start {binary}: {e}")
    inbox: queue.Queue = queue.Queue()
    threading.Thread(target=reader, args=(proc, inbox), daemon=True).start()

    try:
        send(proc, {
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": None,
                "rootUri": None,
                "capabilities": {"textDocument": {"publishDiagnostics": {"relatedInformation": True}}},
            },
        })
        init = wait_for(proc, inbox, response_to(1), "initialize response")
        if "error" in init:
            fail(f"initialize failed: {init['error']}")
        capabilities = init["result"]["capabilities"]
        print(f"initialize: server {init['result'].get('serverInfo')}")
        if "diagnosticProvider" in capabilities:
            fail("server advertised pull diagnostics to a push-only client")
        send(proc, {"method": "initialized", "params": {}})

        send(proc, {
            "method": "textDocument/didOpen",
            "params": {"textDocument": {"uri": URI, "languageId": "lp", "version": 1, "text": TEXT}},
        })
        diagnostics = wait_for(
            proc,
            inbox,
            lambda m: m.get("method") == "textDocument/publishDiagnostics" and m["params"]["uri"] == URI,
            "textDocument/publishDiagnostics",
        )
        print(f"publishDiagnostics: {json.dumps(diagnostics['params']['diagnostics'])}")

        send(proc, {
            "id": 2,
            "method": "textDocument/formatting",
            "params": {"textDocument": {"uri": URI}, "options": {"tabSize": 4, "insertSpaces": True}},
        })
        formatting = wait_for(proc, inbox, response_to(2), "formatting response")
        if "error" in formatting:
            fail(f"formatting failed: {formatting['error']}")
        edits = formatting.get("result")
        print(f"formatting: {json.dumps(edits)}")
        if not isinstance(edits, list) and not (edits is None and allow_null_formatting):
            fail(f"formatting result is not a list: {edits!r}")

        lp = capabilities.get("experimental", {}).get("lp")
        if not lp or lp.get("protocol") != 1 or lp.get("requests") != LP_REQUESTS:
            fail(f"custom requests not advertised: {lp!r}")
        if not isinstance(lp.get("solver"), bool):
            fail(f"experimental.lp.solver is not a boolean: {lp!r}")
        send(proc, {
            "method": "textDocument/didOpen",
            "params": {"textDocument": {"uri": CLEAN_URI, "languageId": "lp", "version": 1, "text": CLEAN_TEXT}},
        })

        def call(request_id: int, method: str, params: dict) -> dict:
            send(proc, {"id": request_id, "method": method, "params": params})
            return wait_for(proc, inbox, response_to(request_id), f"{method} response")

        def result(request_id: int, method: str, params: dict) -> dict:
            response = call(request_id, method, params)
            if "error" in response:
                fail(f"{method} failed: {response['error']}")
            return response["result"]

        doc = {"textDocument": {"uri": CLEAN_URI}}
        model = result(10, "lp/model", doc)
        if (model["uri"], model["version"], model["summary"]["constraints"], model["summary"]["variables"]) != (CLEAN_URI, 1, 2, 2):
            fail(f"unexpected lp/model result: {model!r}")
        explained = result(11, "lp/explain", {**doc, "target": {"kind": "constraint", "name": "c1"}})
        if explained["name"] != "c1" or len(explained["terms"]) != 2:
            fail(f"unexpected lp/explain result: {explained!r}")
        graph = result(12, "lp/graph", doc)
        if len(graph["nodes"]) != 5 or graph["components"]["count"] != 1:
            fail(f"unexpected lp/graph result: {graph!r}")
        matrix = result(13, "lp/matrix", {**doc, "mode": "entries"})
        if matrix["shape"]["nonzeros"] != 6:
            fail(f"unexpected lp/matrix result: {matrix!r}")
        diff = result(14, "lp/diff", {**doc, "against": {"text": CLEAN_TEXT.replace("<= 8", "<= 9"), "label": "old"}})
        if diff["identical"] or diff["constraints"]["modified"][0]["name"] != "c2":
            fail(f"unexpected lp/diff result: {diff!r}")
        solver = result(15, "lp/solverInfo", {})
        if not isinstance(solver.get("available"), bool):
            fail(f"unexpected lp/solverInfo result: {solver!r}")
        print(f"custom requests ok (solver available: {solver['available']})")
        rejected = call(16, "lp/explain", {**doc, "target": {"kind": "variable", "name": "nope"}})
        if rejected.get("error", {}).get("code") != -32602:
            fail(f"unknown explain target should be -32602, got {rejected!r}")

        send(proc, {"id": 3, "method": "shutdown"})
        shutdown = wait_for(proc, inbox, response_to(3), "shutdown response")
        if "error" in shutdown:
            fail(f"shutdown failed: {shutdown['error']}")
        send(proc, {"method": "exit"})
        try:
            code = proc.wait(timeout=TIMEOUT_S)
        except subprocess.TimeoutExpired:
            fail("server did not exit after `exit`")
        if code != 0:
            fail(f"server exited with status {code}")
    finally:
        if proc.poll() is None:
            proc.kill()
    print("smoke test passed")


if __name__ == "__main__":
    main()
