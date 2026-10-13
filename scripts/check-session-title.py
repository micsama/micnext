"""隔离的标题整体验收：先 cargo build -p micnext，再 python3 scripts/check-session-title.py。只调用本机模拟模型。"""

import http.server
import json
import os
import signal
import socket
import sqlite3
import subprocess
import tempfile
import threading
import time
import urllib.request
import urllib.error
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
D = Path(tempfile.mkdtemp(prefix="micnext-title-acceptance-"))
requests = []
hold = threading.Event()
started = threading.Event()
mode = ["normal"]
checks = []
title_count = [0]


class Mock(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        b = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        requests.append(b)
        is_title = "concise title" in str(b["messages"][0])
        if is_title:
            title_count[0] += 1
            started.set()
            if mode[0] == "held":
                hold.wait(15)
            output = {
                "normal": "自动主题",
                "held": "新主题",
                "long": "字" * 41,
                "multi": "两行\n标题",
                "quoted": "标题：“保留格式”。",
                "blank": "  ",
            }.get(mode[0], "自动主题")
        else:
            output = "正文已完成"
        chunks = [
            {
                "id": "test",
                "object": "chat.completion.chunk",
                "created": 0,
                "model": "local",
                "choices": [
                    {"index": 0, "delta": {"content": output}, "finish_reason": None}
                ],
            },
            {
                "id": "test",
                "object": "chat.completion.chunk",
                "created": 0,
                "model": "local",
                "choices": [
                    {
                        "index": 0,
                        "delta": {},
                        "finish_reason": "length"
                        if is_title and mode[0] == "unfinished"
                        else "stop",
                    }
                ],
            },
        ]
        payload = (
            "".join(("data: " + json.dumps(c) + "\n\n" for c in chunks))
            + "data: [DONE]\n\n"
        ).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        try:
            self.wfile.write(payload)
        except BrokenPipeError:
            pass


mock = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Mock)
threading.Thread(target=mock.serve_forever, daemon=True).start()
with socket.socket() as s:
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
config = D / "config.toml"
config.write_text(
    f'[core]\ndata_dir="{D}/data"\nowner="test"\n[gateway]\nlisten="127.0.0.1:{port}"\ntoken="title-check"\n'
)
base = f"http://127.0.0.1:{port}/api"
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
log = (D / "server.log").open("w")
proc = None
db = D / "data/micnext.db"


def api(method, path, body=None, expected=200):
    req = urllib.request.Request(
        base + path,
        data=None if body is None else json.dumps(body).encode(),
        method=method,
        headers={
            "Authorization": "Bearer title-check",
            "Content-Type": "application/json",
        },
    )
    try:
        r = opener.open(req, timeout=5)
    except urllib.error.HTTPError as e:
        r = e
    raw = r.read()
    body = json.loads(raw) if raw else None
    assert r.status == expected, (method, path, r.status, expected, body)
    return body


def wait(p, name):
    end = time.monotonic() + 10
    while time.monotonic() < end:
        if p():
            return
        time.sleep(0.03)
    raise AssertionError(name)


def sql(q, args=()):
    with sqlite3.connect(db) as c:
        return c.execute(q, args).fetchall()


def stop():
    if proc and proc.poll() is None:
        proc.send_signal(signal.SIGTERM)
        proc.wait(timeout=10)


def boot():
    global proc
    env = {
        k: v
        for k, v in os.environ.items()
        if k not in ("OPENAI_API_KEY", "DEEPSEEK_API_KEY")
    }
    proc = subprocess.Popen(
        [str(ROOT / "target/debug/micnext"), "--config", str(config)],
        cwd=ROOT,
        env=env,
        stdout=log,
        stderr=log,
    )

    def ready():
        assert proc.poll() is None, (D / "server.log").read_text()
        try:
            return api("GET", "/models") is not None
        except OSError:
            return False

    wait(ready, "boot")


def info(s):
    return api("GET", f"/sessions/{s}")


def finished(s):
    return not info(s)["title_generating"]


def create(text):
    return api(
        "POST",
        "/sessions",
        {
            "parts": [{"kind": "text", "text": text}],
            "persona_id": 1,
            "model_id": model,
            "reasoning_effort": None,
        },
        201,
    )["session_id"]


def regenerate(s):
    before = title_count[0]
    api("POST", f"/sessions/{s}/title", expected=202)
    wait(lambda: title_count[0] > before and finished(s), "title finished")


try:
    boot()
    assert api("GET", "/settings/title-model") == {
        "model_id": None,
        "reasoning_effort": None,
    }
    api(
        "PUT",
        "/settings/title-model",
        {"model_id": None, "reasoning_effort": "none"},
        400,
    )
    api("PUT", "/settings/title-model", {"model_id": None}, 400)
    api(
        "PUT",
        "/settings/title-model",
        {"model_id": 99999, "reasoning_effort": None},
        404,
    )
    settings = api("GET", "/settings")
    settings.pop("system_prompt")
    settings["default_workdir"] = str(D)
    api("PUT", "/settings", settings, 204)
    endpoint = api(
        "POST",
        "/endpoints",
        {
            "name": "Local",
            "kind": "openai",
            "config": {
                "preset": "generic",
                "base_url": f"http://127.0.0.1:{mock.server_port}/v1",
            },
            "credential": {"op": "clear"},
        },
        201,
    )["id"]
    model = api(
        "POST",
        "/models",
        {
            "endpoint_id": endpoint,
            "name": "local",
            "context_window": None,
            "config": {},
        },
        201,
    )["id"]
    api(
        "PUT",
        "/settings/title-model",
        {"model_id": model, "reasoning_effort": "high"},
        400,
    )
    checks.append("title settings required nullable fields and invalid combinations")
    mode[0] = "held"
    sid = create("首问自动标题")
    wait(lambda: started.is_set(), "auto starts")
    assert info(sid)["title_generating"]
    api("POST", f"/sessions/{sid}/title", expected=409)
    wait(
        lambda: (
            sql("SELECT state FROM core_runs WHERE session_id=?", (sid,))
            == [("completed",)]
        ),
        "body finishes independently",
    )
    assert info(sid)["title_generating"]
    hold.set()
    wait(lambda: finished(sid), "auto finish")
    assert info(sid)["title"] == "新主题"
    checks.append("first input auto, duplicate 409, body finishes while title held")
    initial = info(sid)["last_activity_at"]
    messages = sql("SELECT COUNT(*) FROM core_messages WHERE session_id=?", (sid,))[0][
        0
    ]
    assert sql(
        "SELECT purpose,run_id FROM core_model_calls WHERE session_id=? ORDER BY id",
        (sid,),
    ) == [("conversation", 1), ("title", None)]
    for b in [b for b in requests if "concise title" in str(b["messages"][0])]:
        assert not b.get("tools")
        assert b.get("tool_choice") in (None, "none")
        assert len(b["messages"]) == 2
        assert "首问自动标题" in str(b["messages"][1])
        assert "reasoning_effort" not in b
    mode[0] = "long"
    regenerate(sid)
    assert info(sid)["title"] == "新主题" and info(sid)["title_error"]
    assert info(sid)["last_activity_at"] == initial
    assert (
        sql("SELECT COUNT(*) FROM core_messages WHERE session_id=?", (sid,))[0][0]
        == messages
    )
    for invalid in ["multi", "blank", "unfinished"]:
        mode[0] = invalid
        regenerate(sid)
        assert info(sid)["title_error"]
    mode[0] = "quoted"
    regenerate(sid)
    assert (
        info(sid)["title"] == "标题：“保留格式”。" and info(sid)["title_error"] is None
    )
    checks.append(
        "audit purpose, no chat/context insertion, failure preserves title, validation trim only"
    )
    before = title_count[0]
    api(
        "POST",
        f"/sessions/{sid}/messages",
        {"parts": [{"kind": "text", "text": "后续不自动"}]},
        202,
    )
    wait(
        lambda: (
            sql(
                "SELECT COUNT(*) FROM core_runs WHERE session_id=? AND state='completed'",
                (sid,),
            )[0][0]
            == 2
        ),
        "second body",
    )
    assert title_count[0] == before
    checks.append("subsequent user input never auto regenerates")
    mode[0] = "normal"
    png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4//8/AAX+Av4N70a4AAAAAElFTkSuQmCC"
    before = title_count[0]
    img = api(
        "POST",
        "/sessions",
        {
            "parts": [{"kind": "image", "base64": png}],
            "persona_id": 1,
            "model_id": model,
        },
        201,
    )["session_id"]
    api("POST", f"/sessions/{img}/title", expected=400)
    assert title_count[0] == before
    api(
        "POST",
        f"/sessions/{img}/messages",
        {"parts": [{"kind": "text", "text": "图后首条文字"}]},
        202,
    )
    wait(lambda: info(img)["title"] == "自动主题", "image then text")
    checks.append("image-only skips auto; first text after image triggers")
    with sqlite3.connect(db) as c:
        c.execute(
            "INSERT INTO core_sessions(kind,channel,chat,pwd,tool_scope,created_at) VALUES(?,?,?,?,?,?)",
            ("root", "web", "old", str(D), '"All"', 0),
        )
        old = c.execute("SELECT last_insert_rowid()").fetchone()[0]
        c.execute(
            "INSERT INTO core_messages(session_id,payload,created_at) VALUES(?,?,?)",
            (
                old,
                json.dumps(
                    {
                        "kind": "UserInput",
                        "person": 1,
                        "parts": [{"Text": {"text": "旧输入"}}],
                    }
                ),
                1,
            ),
        )
        c.execute(
            "INSERT INTO core_sessions(kind,trigger_module,trigger_ref,pwd,tool_scope,created_at) VALUES(?,?,?,?,?,?)",
            ("triggered", "test", "test", str(D), '"All"', 0),
        )
        nonroot = c.execute("SELECT last_insert_rowid()").fetchone()[0]
    api("POST", f"/sessions/{nonroot}/title", expected=403)
    api("POST", "/sessions/99999/title", expected=404)
    before = title_count[0]
    api(
        "POST",
        f"/sessions/{old}/messages",
        {"parts": [{"kind": "text", "text": "旧会话新问题"}]},
        202,
    )
    wait(
        lambda: (
            sql("SELECT state FROM core_runs WHERE session_id=?", (old,))
            == [("completed",)]
        ),
        "old body",
    )
    assert title_count[0] == before and info(old)["title"] is None
    checks.append("old text not eligible, Root boundary and missing ID errors")
    with sqlite3.connect(db) as c:
        for n in range(10):
            c.execute(
                "INSERT INTO core_messages(session_id,payload,created_at) VALUES(?,?,?)",
                (
                    old,
                    json.dumps(
                        {
                            "kind": "UserInput",
                            "person": 1,
                            "parts": [{"Text": {"text": f"问题{n}" + "字" * 400}}],
                        }
                    ),
                    n + 2,
                ),
            )
    regenerate(old)
    captured = [b for b in requests if "concise title" in str(b["messages"][0])][-1][
        "messages"
    ][1]["content"]
    if isinstance(captured, list):
        assert all(part["type"] == "text" for part in captured)
        captured = "".join(part["text"] for part in captured)
    assert "问题9" in captured and "问题0" not in captured
    # Provider 的 HarnessNote 呈现包含固定包裹；其中的用户文本保持有界。
    assert 2000 <= len(captured) < 2100
    checks.append("manual uses latest bounded text, not full history")
    req = urllib.request.Request(
        base + f"/sessions/{sid}/stream",
        headers={"Authorization": "Bearer title-check"},
    )
    events = []
    ready = threading.Event()
    done = threading.Event()

    def read():
        with opener.open(req, timeout=5) as r:
            while not done.is_set():
                line = r.readline().decode().strip()
                if line.startswith("event:"):
                    event = line[6:].strip()
                    events.append(event)
                    if event == "ready":
                        ready.set()
                    if events.count("session_title_changed") >= 2:
                        done.set()

    reader = threading.Thread(target=read, daemon=True)
    reader.start()
    assert ready.wait(5)
    regenerate(sid)
    assert done.wait(5)
    assert events.count("session_title_changed") >= 2
    checks.append(
        "current session SSE has start and finish invalidation; ready supports snapshot re-read"
    )
    dedicated = api(
        "POST",
        "/models",
        {
            "endpoint_id": endpoint,
            "name": "dedicated",
            "context_window": None,
            "config": {},
        },
        201,
    )["id"]
    api(
        "PUT",
        "/settings/title-model",
        {"model_id": dedicated, "reasoning_effort": None},
        204,
    )
    api("DELETE", f"/models/{dedicated}", expected=204)
    before = title_count[0]
    api("POST", f"/sessions/{sid}/title", expected=202)
    wait(lambda: finished(sid), "deleted failure")
    assert "已删除" in info(sid)["title_error"] and title_count[0] == before
    checks.append(
        "deleted dedicated model stays selected, fails visibly without fallback"
    )
    api(
        "PUT",
        "/settings/title-model",
        {"model_id": None, "reasoning_effort": None},
        204,
    )
    mode[0] = "held"
    hold.clear()
    started.clear()
    api("POST", f"/sessions/{sid}/title", expected=202)
    assert started.wait(5)
    stop()
    mode[0] = "normal"
    hold.set()
    before = title_count[0]
    boot()
    assert not info(sid)["title_generating"]
    time.sleep(0.1)
    assert title_count[0] == before and info(old)["title"] == "自动主题"
    checks.append(
        "restart clears runtime-only state without retry or old session backfill"
    )
    stop()
    before = title_count[0]
    env = {
        k: v
        for k, v in os.environ.items()
        if k not in ("OPENAI_API_KEY", "DEEPSEEK_API_KEY")
    }
    result = subprocess.run(
        [
            str(ROOT / "target/debug/micnext"),
            "--config",
            str(config),
            "-p",
            "一次性不取标题",
        ],
        cwd=D,
        env=env,
        capture_output=True,
        timeout=10,
    )
    assert result.returncode == 0, result.stderr.decode()
    assert title_count[0] == before
    checks.append("-p executes body without title call or waiting")
    print(
        json.dumps(
            {"passed": len(checks), "checks": checks, "directory": str(D)},
            ensure_ascii=False,
        ),
        flush=True,
    )
    (D / "result.json").write_text(json.dumps(checks, ensure_ascii=False, indent=2))
finally:
    hold.set()
    stop()
    mock.shutdown()
    log.close()
