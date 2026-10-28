"""Synthetic SQLite mechanism probes; no application or user database is opened.

Run from any directory: python3 docs/reviews/web-loading-2026-10-28/probe.py
These are SQL/connection experiments, not Rust Store or HTTP benchmarks.
"""

import contextlib
import json
import pathlib
import re
import sqlite3
import tempfile
import threading
import time

ROOT = pathlib.Path(__file__).resolve().parents[3]


def timed(fn):
    start = time.perf_counter()
    value = fn()
    return round((time.perf_counter() - start) * 1000, 3), value


def build(path):
    conn = sqlite3.connect(path, isolation_level=None, check_same_thread=False)
    conn.execute("PRAGMA journal_mode=WAL")
    conn.execute("PRAGMA foreign_keys=ON")
    conn.executescript((ROOT / "crates/mic-store/src/schema/core.sql").read_text())
    conn.execute("BEGIN")
    for sid in range(1, 11):
        conn.execute(
            "INSERT INTO core_sessions(id,kind,channel,chat,pwd,tool_scope,created_at,persona_id) "
            "VALUES (?,'root','web',?,'/tmp','{}',0,1)", (sid, str(sid)),
        )
        conn.execute(
            "INSERT INTO core_runs(id,session_id,state,created_at,finished_at) "
            "VALUES (?,?,'completed',0,100)", (sid, sid),
        )
    for cid in range(1, 1001):
        sid = (cid - 1) // 100 + 1
        conn.execute(
            "INSERT INTO core_model_calls(id,session_id,run_id,purpose,model,started_at,finished_at) "
            "VALUES (?,?,?,'conversation','synthetic',0,100)", (cid, sid, sid),
        )
    payload = json.dumps({"kind": "HarnessNote", "text": "x" * 2048})
    reply = json.dumps({"kind": "Reply", "blocks": [{"Text": {"text": "x" * 2048}}]})
    conn.executemany(
        "INSERT INTO core_messages(id,session_id,run_id,model_call_id,payload,created_at) "
        "VALUES (?,?,?,?,?,?)",
        ((mid, (mid - 1) // 2000 + 1, (mid - 1) // 2000 + 1,
          mid // 20 if mid % 20 == 0 else None,
          reply if mid % 20 == 0 else payload, mid) for mid in range(1, 20001)),
    )
    conn.execute("COMMIT")
    assert not conn.execute("PRAGMA foreign_key_check").fetchall()
    return conn


def contention(conn, path, separate):
    ready = threading.Event()
    gate = threading.Lock()
    errors = []
    heavy_ms = []
    reader = sqlite3.connect(f"file:{path}?mode=ro", uri=True, isolation_level=None,
                             check_same_thread=False) if separate else conn

    def heavy():
        try:
            with gate:
                reader.execute("BEGIN")
                reader.execute("SELECT max(id) FROM core_messages").fetchone()
                ready.set()
                elapsed, _ = timed(lambda: reader.execute(
                    "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<1000000) "
                    "SELECT sum(x) FROM n"
                ).fetchone())
                heavy_ms.append(elapsed)
                reader.execute("COMMIT")
        except BaseException as exc:
            errors.append(str(exc))
            ready.set()

    worker = threading.Thread(target=heavy)
    worker.start()
    assert ready.wait(10), "worker did not start"

    def interactive():
        with contextlib.nullcontext() if separate else gate:
            conn.execute("SELECT general_prompt FROM core_settings").fetchone()
            conn.execute("UPDATE core_settings SET general_prompt=general_prompt || 'x' WHERE id=1")

    elapsed, _ = timed(interactive)
    worker.join(10)
    assert not worker.is_alive() and not errors, errors
    if separate:
        reader.close()
    return {"separate_reader": separate, "heavy_read_ms": heavy_ms[0],
            "settings_read_and_write_ms": elapsed}


def main():
    with tempfile.TemporaryDirectory(prefix="micnext-read-probe-") as tmp:
        path = pathlib.Path(tmp) / "probe.db"
        conn = build(path)
        source = (ROOT / "crates/mic-store/src/trace/mod.rs").read_text()
        select = re.search(r'const CALL_SELECT: &str = "(.*?)";', source, re.S).group(1)
        sql = select + " WHERE c.session_id = ? ORDER BY c.id"
        result = {"sqlite": sqlite3.sqlite_version, "messages": 20000, "calls": 1000,
                  "selected_calls": 100, "payload_text_bytes": 2048}
        result["before_plan"] = [r[3] for r in conn.execute("EXPLAIN QUERY PLAN " + sql, (1,))]
        elapsed, rows = timed(lambda: conn.execute(sql, (1,)).fetchall())
        result["before_ms"] = elapsed
        conn.execute("CREATE INDEX probe_call_message ON core_messages(model_call_id,kind)")
        result["after_plan"] = [r[3] for r in conn.execute("EXPLAIN QUERY PLAN " + sql, (1,))]
        elapsed, indexed = timed(lambda: conn.execute(sql, (1,)).fetchall())
        assert rows == indexed
        result["after_ms"] = elapsed
        result["contention_with_index"] = [contention(conn, path, separate) for separate in (False, True)]
        print(json.dumps(result, ensure_ascii=False, indent=2))
        conn.close()


if __name__ == "__main__":
    main()
