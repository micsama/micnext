"""Measure extracted SQL on private copies of a downloaded SQLite backup.

Outputs timings, counts and plans only. No credentials or message bodies are printed.
This is a SQL benchmark, not an HTTP or Rust Store benchmark.
"""
import argparse
import contextlib
import hashlib
import json
import pathlib
import re
import shutil
import sqlite3
import tempfile
import threading
import time

ROOT = pathlib.Path(__file__).resolve().parents[3]
TRACE = (ROOT / "crates/mic-store/src/trace/mod.rs").read_text()
CALL = re.search(r'const CALL_SELECT: &str = "(.*?)";', TRACE, re.S).group(1)


def measure(conn, operations, seconds=5):
    start = time.perf_counter()
    rows, statements = 0, 0
    digest = hashlib.sha256()
    conn.set_progress_handler(lambda: time.perf_counter() - start > seconds, 1000)
    try:
        for sql, args in operations:
            for row in conn.execute(sql, args):
                rows += 1
                digest.update(repr(row).encode())
            statements += 1
        outcome = "completed"
    except sqlite3.OperationalError as error:
        if str(error) != "interrupted":
            raise
        outcome = "deadline_exceeded"
    finally:
        conn.set_progress_handler(None, 0)
    return {"status": outcome, "ms": round((time.perf_counter() - start) * 1000, 3),
            "rows": rows, "statements_completed": statements}, digest.digest()


def extracted_queries(conn):
    hot = conn.execute("SELECT session_id FROM core_model_calls GROUP BY session_id ORDER BY count(*) DESC LIMIT 1").fetchone()[0]
    page = conn.execute("SELECT id FROM core_runs WHERE session_id=? AND state<>'executing' ORDER BY id DESC LIMIT 20", (hot,)).fetchall()
    tree = re.search(r'\.prepare\(\s*"(.*?)",', TRACE[TRACE.index("pub async fn trace_sessions"):], re.S).group(1)
    peers = re.findall(r'conn.prepare\(\s*"(.*?)",', TRACE[TRACE.index("pub async fn call_peers"):TRACE.index("pub async fn dashboard")], re.S)
    sessions = (ROOT / "crates/mic-store/src/sessions.rs").read_text()
    summary = re.search(r'format!\(\s*"(.*?)"\s*\)', sessions[sessions.index("pub(crate) fn summaries"):], re.S).group(1)
    summary = summary.replace("{SESSION_COLS}", "id").replace("{PREVIEW_CHARS}", "80").replace(
        "{filter}", "s.kind='root' AND s.channel='web' AND s.archived_at IS NULL AND s.pinned_at IS NULL "
        "AND EXISTS (SELECT 1 FROM core_messages m WHERE m.session_id=s.id AND m.kind IN ('UserInput','Command'))")
    summary += " ORDER BY act DESC,id DESC LIMIT 31"
    page_calls = [(CALL + " WHERE c.run_id=? ORDER BY c.id", (r[0],)) for r in reversed(page)]
    message_sql = "SELECT m.id,m.session_id,m.payload,m.created_at,m.delivered_at,c.model,c.finished_at-c.started_at,c.first_output_at-c.started_at,c.input_tokens,c.output_tokens,c.cache_read_tokens FROM core_messages m LEFT JOIN core_model_calls c ON c.id=m.model_call_id WHERE m.run_id=? ORDER BY m.id"
    # CALL without reply/boundary/side correlations; retains all other columns for comparison.
    lean = re.sub(r"\(SELECT id FROM core_messages WHERE model_call_id = c.id AND kind = '(?:Reply|Boundary|SideOutput)'\)", "NULL", CALL)
    return hot, {
        "settings": [("SELECT default_persona_id,default_workdir,max_turns FROM core_settings", ())],
        "trace_tree": [(tree, ())],
        "trace_page_call_queries_20_runs": page_calls,
        "trace_page_message_queries_20_runs": [(message_sql, (r[0],)) for r in reversed(page)],
        "session_usage_call_rows": [(CALL + " WHERE c.session_id=? ORDER BY c.id", (hot,))],
        "session_usage_without_message_correlations": [(lean + " WHERE c.session_id=? ORDER BY c.id", (hot,))],
        "dashboard_7_days_call_rows": [(CALL + " WHERE strftime('%Y-%m-%d',c.started_at/1000,'unixepoch','localtime') >= date('now','localtime','-6 days') ORDER BY c.id", ())],
        "peers_sql_only": [(sql, ()) for sql in peers],
        "web_list_summary_sql": [(summary, ())],
        "subscription_full_replay_sql": [(message_sql.replace("m.run_id=?", "m.session_id=? AND m.kind<>'SideOutput'"), (hot,))],
        "subscription_last_id_sql": [("SELECT id FROM core_messages WHERE session_id=? AND kind<>'SideOutput' ORDER BY id DESC LIMIT 1", (hot,))],
    }


def mixed(path, hot, separate):
    conn = sqlite3.connect(path, isolation_level=None, check_same_thread=False)
    conn.execute("PRAGMA journal_mode=WAL")
    reader = sqlite3.connect(f"file:{path}?mode=ro", uri=True, isolation_level=None, check_same_thread=False) if separate else conn
    gate, ready = threading.Lock(), threading.Event()
    heavy_result, errors = [], []

    def heavy():
        try:
            with gate:
                ready.set()
                heavy_result.append(measure(reader, [(CALL + " WHERE c.session_id=? ORDER BY c.id", (hot,))])[0])
        except BaseException as exc:
            errors.append(type(exc).__name__)
            ready.set()

    worker = threading.Thread(target=heavy)
    worker.start()
    assert ready.wait(10)
    start = time.perf_counter()
    with contextlib.nullcontext() if separate else gate:
        settings = measure(conn, [("SELECT max_turns FROM core_settings", ())])[0]
    end = time.perf_counter()
    worker.join(15)
    assert not worker.is_alive() and not errors, errors
    if separate:
        reader.close()
    conn.close()
    return {"separate_reader": separate, "heavy": heavy_result[0],
            "settings_total_ms": round((end - start) * 1000, 3), "settings_execution_ms": settings["ms"]}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("db", type=pathlib.Path)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    args = parser.parse_args()
    assert not any(pathlib.Path(str(args.db) + suffix).exists() for suffix in ("-wal", "-journal")), "Use a consistent standalone backup"
    fingerprint = hashlib.sha256(args.db.read_bytes()).digest()
    report = {"sqlite": sqlite3.sqlite_version, "deadline_ms": 5000, "variants": {}}
    signatures = {}
    with tempfile.TemporaryDirectory(prefix="micnext-real-sql-") as directory:
        for variant in ("baseline", "call_index", "three_indexes"):
            path = pathlib.Path(directory) / (variant + ".db")
            shutil.copyfile(args.db, path)
            path.chmod(0o600)
            conn = sqlite3.connect(path, isolation_level=None)
            conn.execute("PRAGMA journal_mode=WAL")
            if variant != "baseline":
                conn.execute("CREATE INDEX probe_call_messages ON core_messages(model_call_id,kind)")
            if variant == "three_indexes":
                conn.execute("CREATE INDEX probe_runs_session ON core_runs(session_id,id)")
                conn.execute("CREATE INDEX probe_sessions_parent ON core_sessions(parent_session_id)")
            hot, queries = extracted_queries(conn)
            results = {}
            for name, ops in queries.items():
                result, signature = measure(conn, ops)
                if result["status"] == "completed":
                    if name in signatures:
                        assert signature == signatures[name], name
                    signatures[name] = signature
                results[name] = result
                print(variant, name, result, flush=True)
            report["variants"][variant] = results
            report.setdefault("plans", {})[variant] = [r[3] for r in conn.execute(
                "EXPLAIN QUERY PLAN " + CALL + " WHERE c.session_id=? ORDER BY c.id", (hot,))]
            conn.close()
            if variant in ("baseline", "call_index"):
                report.setdefault("mixed", {})[variant] = [mixed(path, hot, separate) for separate in (False, True)]
        report["completed_query_results_equal_across_variants"] = True
        assert hashlib.sha256(args.db.read_bytes()).digest() == fingerprint
        report["source_unchanged"] = True
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")


if __name__ == "__main__":
    main()
