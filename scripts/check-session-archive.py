"""归档整体功能验收：cargo build -p micnext 后运行本脚本；只使用隔离数据库和本机模拟模型。"""

import http.server
import json
import os
from pathlib import Path
import signal
import socket
import sqlite3
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
directory = Path(tempfile.mkdtemp(prefix="micnext-archive-acceptance-"))
release = threading.Event()
requests = []
checks = []


class Mock(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        requests.append(body)
        release.wait(15)
        title = "concise title" in str(body["messages"][0])
        chunks = [
            {
                "id": "mock",
                "object": "chat.completion.chunk",
                "created": 0,
                "model": "local",
                "choices": [
                    {
                        "index": 0,
                        "delta": {"content": "归档主题" if title else "正文完成"},
                        "finish_reason": None,
                    }
                ],
            },
            {
                "id": "mock",
                "object": "chat.completion.chunk",
                "created": 0,
                "model": "local",
                "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
            },
        ]
        payload = (
            "".join("data: " + json.dumps(chunk) + "\n\n" for chunk in chunks)
            + "data: [DONE]\n\n"
        ).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


mock = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Mock)
threading.Thread(target=mock.serve_forever, daemon=True).start()
with socket.socket() as sock:
    sock.bind(("127.0.0.1", 0))
    port = sock.getsockname()[1]
config = directory / "config.toml"
config.write_text(
    f'[core]\ndata_dir="{directory}/data"\nowner="test"\n[gateway]\nlisten="127.0.0.1:{port}"\ntoken="archive-check"\n'
)
base = f"http://127.0.0.1:{port}/api"
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
db = directory / "data/micnext.db"
log = (directory / "server.log").open("w")
process = None


def api(method, path, body=None, expected=200):
    req = urllib.request.Request(
        base + path,
        data=None if body is None else json.dumps(body).encode(),
        method=method,
        headers={
            "Authorization": "Bearer archive-check",
            "Content-Type": "application/json",
        },
    )
    try:
        response = opener.open(req, timeout=5)
    except urllib.error.HTTPError as error:
        response = error
    raw = response.read()
    parsed = json.loads(raw) if raw else None
    assert response.status == expected, (
        method,
        path,
        response.status,
        expected,
        parsed,
    )
    return parsed


def wait(predicate, name):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.03)
    raise AssertionError(name)


def query(sql, args=()):
    with sqlite3.connect(db) as connection:
        return connection.execute(sql, args).fetchall()


def boot():
    global process
    env = {
        k: v
        for k, v in os.environ.items()
        if k not in ("OPENAI_API_KEY", "DEEPSEEK_API_KEY")
    }
    process = subprocess.Popen(
        [str(ROOT / "target/debug/micnext"), "--config", str(config)],
        cwd=ROOT,
        env=env,
        stdout=log,
        stderr=log,
    )

    def ready():
        assert process.poll() is None, (directory / "server.log").read_text()
        try:
            return api("GET", "/models") is not None
        except OSError:
            return False

    wait(ready, "startup")


def stop():
    if process and process.poll() is None:
        process.send_signal(signal.SIGTERM)
        process.wait(timeout=10)


def info(sid):
    return api("GET", f"/sessions/{sid}")


def listed(channel):
    ids = []
    path = f"/sessions?channel={channel}&limit=2"
    while path:
        page = api("GET", path)
        ids.extend(item["id"] for item in page["items"])
        cursor = page["next"]
        path = (
            f"/sessions?channel={channel}&limit=2&before_at={cursor['before_at']}&before_id={cursor['before_id']}"
            if cursor
            else None
        )
    return ids


try:
    boot()
    settings = api("GET", "/settings")
    settings.pop("system_prompt")
    settings["default_workdir"] = str(directory)
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
    sid = api(
        "POST",
        "/sessions",
        {
            "parts": [{"kind": "text", "text": "执行期间归档"}],
            "persona_id": 1,
            "model_id": model,
        },
        201,
    )["session_id"]
    wait(lambda: len(requests) == 2, "body and title in flight")
    before = info(sid)
    assert before["title_generating"]
    api("POST", f"/sessions/{sid}/archive", expected=204)
    assert sid not in listed("web")
    assert info(sid) == before
    assert "archived_at" not in info(sid)
    at = query("SELECT archived_at FROM core_sessions WHERE id=?", (sid,))[0][0]
    assert isinstance(at, int)
    api("POST", f"/sessions/{sid}/archive", expected=204)
    assert query("SELECT archived_at FROM core_sessions WHERE id=?", (sid,)) == [(at,)]
    assert query("SELECT state FROM core_runs WHERE session_id=?", (sid,)) == [
        ("executing",)
    ]
    release.set()
    wait(
        lambda: (
            query("SELECT state FROM core_runs WHERE session_id=?", (sid,))
            == [("completed",)]
            and not info(sid)["title_generating"]
        ),
        "archived body and title complete",
    )
    assert info(sid)["title"] == "归档主题"
    assert sorted(
        row[0]
        for row in query(
            "SELECT purpose FROM core_model_calls WHERE session_id=?", (sid,)
        )
    ) == ["conversation", "title"]
    checks.append(
        "archive while body/title run; tasks finish, data survives and DTO unchanged"
    )
    api(
        "POST",
        f"/sessions/{sid}/messages",
        {"parts": [{"kind": "text", "text": "归档后的新消息"}]},
        202,
    )
    wait(
        lambda: (
            query(
                "SELECT COUNT(*) FROM core_runs WHERE session_id=? AND state='completed'",
                (sid,),
            )[0][0]
            == 2
        ),
        "next input finishes",
    )
    assert sid not in listed("web") and query(
        "SELECT archived_at FROM core_sessions WHERE id=?", (sid,)
    ) == [(at,)]
    checks.append(
        "new input works and never clears archive marker; repeated archive preserves first time"
    )
    fixtures = []
    with sqlite3.connect(db) as connection:
        for n, channel in enumerate(["web"] * 7 + ["wechat", "cli"]):
            connection.execute(
                "INSERT INTO core_sessions(kind,channel,chat,pwd,tool_scope,created_at) VALUES(?,?,?,?,?,?)",
                ("root", channel, f"fixture-{n}", str(directory), '"All"', n + 1),
            )
            fixture = connection.execute("SELECT last_insert_rowid()").fetchone()[0]
            connection.execute(
                "INSERT INTO core_messages(session_id,payload,created_at) VALUES(?,?,?)",
                (
                    fixture,
                    json.dumps(
                        {
                            "kind": "UserInput",
                            "person": 1,
                            "parts": [{"Text": {"text": f"question {n}"}}],
                        }
                    ),
                    n + 1,
                ),
            )
            fixtures.append((fixture, channel))
        connection.execute(
            "INSERT INTO core_sessions(kind,trigger_module,trigger_ref,pwd,tool_scope,created_at) VALUES(?,?,?,?,?,?)",
            ("triggered", "test", "test", str(directory), '"All"', 0),
        )
        nonroot = connection.execute("SELECT last_insert_rowid()").fetchone()[0]
    api("POST", "/sessions/999999/archive", expected=404)
    api("POST", "/sessions/bad/archive", expected=404)
    api("POST", f"/sessions/{nonroot}/archive", expected=403)
    archived = {fixtures[0][0], fixtures[3][0]}
    for fixture, channel in fixtures:
        if fixture in archived or channel != "web":
            api("POST", f"/sessions/{fixture}/archive", expected=204)
            assert fixture not in listed(channel)
            assert info(fixture)["channel"] == channel
    assert set(listed("web")) == {
        fixture
        for fixture, channel in fixtures
        if channel == "web" and fixture not in archived
    }
    checks.append(
        "missing/non-Root rejected; Web/WeChat/CLI archive; multi-page list skips without losing other items"
    )
    messages = query("SELECT COUNT(*) FROM core_messages WHERE session_id=?", (sid,))[
        0
    ][0]
    calls = query("SELECT COUNT(*) FROM core_model_calls WHERE session_id=?", (sid,))[
        0
    ][0]
    stop()
    boot()
    assert sid not in listed("web")
    assert (
        query("SELECT COUNT(*) FROM core_messages WHERE session_id=?", (sid,))[0][0]
        == messages
    )
    assert (
        query("SELECT COUNT(*) FROM core_model_calls WHERE session_id=?", (sid,))[0][0]
        == calls
    )
    with sqlite3.connect(db) as connection:
        connection.execute(
            "UPDATE core_sessions SET archived_at=NULL WHERE id=?", (sid,)
        )
    assert sid in listed("web") and info(sid)["title"] == "归档主题"
    checks.append(
        "archive survives restart; manual marker removal restores list and retains history"
    )
    result = {"passed": len(checks), "checks": checks, "directory": str(directory)}
    (directory / "result.json").write_text(
        json.dumps(result, ensure_ascii=False, indent=2)
    )
    print(json.dumps(result, ensure_ascii=False), flush=True)
finally:
    release.set()
    stop()
    mock.shutdown()
    log.close()
