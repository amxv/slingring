#!/usr/bin/env python3
"""Fixture-only smoke for verified native helper/guardian; never launches a browser."""
import base64
import hashlib
import json
import os
import pathlib
import socket
import struct
import sys

URL = "about:blank#fake-lease-fixture"


def take(sock, n):
    out = bytearray()
    while len(out) < n:
        data = sock.recv(n - len(out))
        if not data:
            raise EOFError
        out.extend(data)
    return bytes(out)


def fake():
    profile, = [s.split("=", 1)[1] for s in sys.argv[1:] if s.startswith("--user-data-dir=")]
    profile = pathlib.Path(profile)
    assert profile.name.startswith("agent-browser-profile-")
    assert profile.parent == pathlib.Path(os.environ["TMPDIR"]).resolve()
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        listener.listen(4)
        listener.settimeout(18)
        (profile / "fake.pid").write_text(str(os.getpid()))
        port = profile / ".port"
        port.write_text(f"{listener.getsockname()[1]}\n/devtools/browser/fixture\n")
        port.rename(profile / "DevToolsActivePort")
        while True:
            with listener.accept()[0] as peer:
                peer.settimeout(18)
                try:
                    serve(peer)
                except EOFError:
                    pass


def serve(peer):
    headers = bytearray()
    while not headers.endswith(b"\r\n\r\n"):
        headers.extend(take(peer, 1))
        assert len(headers) < 8192
    lines = headers.decode("ascii").split("\r\n")
    assert lines[0] == "GET /devtools/browser/fixture HTTP/1.1"
    key, = [line.split(":", 1)[1].strip() for line in lines if line.lower().startswith("sec-websocket-key:")]
    accept = base64.b64encode(hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest())
    peer.sendall(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: " + accept + b"\r\n\r\n")
    while True:
        first, second = take(peer, 2)
        if first & 15 == 8:
            return
        assert first == 0x81 and second & 0x80
        length = second & 127
        if length >= 126:
            length = int.from_bytes(take(peer, 2 if length == 126 else 8), "big")
        assert length <= 65536
        mask = take(peer, 4)
        msg = json.loads(bytes(x ^ mask[i % 4] for i, x in enumerate(take(peer, length))))
        method = msg["method"]
        params = msg.get("params", {})
        result = {
            "Browser.getVersion": {"protocolVersion": "1.3", "product": "fixture", "revision": "fixture", "userAgent": "fixture", "jsVersion": "fixture"},
            "Target.getTargets": {"targetInfos": []},
            "Target.createTarget": {"targetId": "fixture-page"},
            "Target.attachToTarget": {"sessionId": "fixture-page-session"},
            "Page.navigate": {"frameId": "fixture-frame"},
            "Runtime.evaluate": {"result": {"type": "string", "value": URL if "location.href" in params.get("expression", "") else "fixture-title"}},
        }.get(method, {})
        frame = json.dumps({"id": msg["id"], "result": result}).encode()
        assert len(frame) < 65536
        header = bytes([0x81, len(frame)]) if len(frame) < 126 else b"\x81\x7e" + struct.pack("!H", len(frame))
        peer.sendall(header + frame)



if __name__ == "__main__":
    assert pathlib.Path(sys.argv[0]).name == "fake-chrome"
    fake()
