"""Compare active-context SQL access paths on a private real-backup copy."""
import argparse
import hashlib
import json
import pathlib
import shutil
import sqlite3
import tempfile
import time


def run(conn, sql, args):
    ticks = 0

    def tick():
        nonlocal ticks
        ticks += 1
        return False

    conn.set_progress_handler(tick, 100)
    started = time.perf_counter()
    rows = conn.execute(sql, args).fetchall()
    elapsed = (time.perf_counter() - started) * 1000
    conn.set_progress_handler(None, 0)
    return rows, {"ms": round(elapsed, 3), "rows": len(rows), "approx_vm_steps": ticks * 100}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("db", type=pathlib.Path)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    args = parser.parse_args()
    assert not pathlib.Path(str(args.db) + "-wal").exists()
    fingerprint = hashlib.sha256(args.db.read_bytes()).digest()
    with tempfile.TemporaryDirectory(prefix="micnext-context-probe-") as directory:
        path = pathlib.Path(directory) / "context.db"
        shutil.copyfile(args.db, path)
        path.chmod(0o600)
        conn = sqlite3.connect(path)
        sid = conn.execute("SELECT session_id FROM core_model_calls GROUP BY session_id ORDER BY count(*) DESC LIMIT 1").fetchone()[0]
        boundary, payload = conn.execute("SELECT id,payload FROM core_messages WHERE session_id=? AND kind='Boundary' AND compaction_revoked_at IS NULL ORDER BY id DESC LIMIT 1", (sid,)).fetchone()
        compaction = json.loads(payload)["boundary"]["Compaction"]
        keep = compaction["keep_from"]
        assert keep is not None, "This experiment covers a Compaction with keep_from"
        bound = (sid, boundary + 1, boundary, 9223372036854775807, keep)
        cols = "m.id,m.session_id,m.payload,m.created_at,m.delivered_at,c.model,c.finished_at-c.started_at,c.first_output_at-c.started_at,c.input_tokens,c.output_tokens,c.cache_read_tokens"
        tail = "core_messages m LEFT JOIN core_model_calls c ON c.id=m.model_call_id"
        unclaimed = "m.run_id IS NULL AND m.kind IN ('UserInput','Completion','Command','JobNotice','Trigger') AND NOT EXISTS(SELECT 1 FROM core_input_holds h WHERE h.message_id=m.id)"
        original = f"SELECT {cols},({unclaimed}) FROM {tail} WHERE m.session_id=?1 AND ((CASE WHEN ?5 IS NULL THEN m.id>=?2 ELSE (m.created_at,m.id)>=(SELECT created_at,id FROM core_messages WHERE id=?5) END) OR EXISTS(SELECT 1 FROM core_boundary_inputs b WHERE b.boundary_id=?3 AND b.message_id=m.id)) AND m.id<=?4 ORDER BY m.created_at,m.id"
        ranged = f"WITH chosen(id) AS (SELECT m.id FROM core_messages m WHERE m.session_id=?1 AND (m.created_at,m.id)>=(SELECT created_at,id FROM core_messages WHERE id=?5) AND m.id<=?4 UNION SELECT m.id FROM core_boundary_inputs b JOIN core_messages m ON m.id=b.message_id WHERE b.boundary_id=?3 AND m.session_id=?1 AND m.id<=?4) SELECT {cols},({unclaimed}) FROM chosen JOIN {tail} ON m.id=chosen.id ORDER BY m.created_at,m.id"
        # Make the two joins explicit; SQL join order is not used as an execution-order promise.
        ranged = ranged.replace(f"FROM chosen JOIN {tail} ON m.id=chosen.id", "FROM chosen JOIN core_messages m ON m.id=chosen.id LEFT JOIN core_model_calls c ON c.id=m.model_call_id")
        result = {"sqlite": sqlite3.sqlite_version, "session": sid, "active_boundary": boundary,
                  "keep_from": keep, "session_messages": conn.execute("SELECT count(*) FROM core_messages WHERE session_id=?", (sid,)).fetchone()[0]}
        expected = None
        for name, sql in (("original", original), ("range_union_without_index", ranged), ("range_union_with_time_index", ranged)):
            if name == "range_union_with_time_index":
                conn.execute("CREATE INDEX probe_context_position ON core_messages(session_id,created_at,id)")
            samples = []
            for _ in range(3):
                rows, sample = run(conn, sql, bound)
                if expected is None:
                    expected = rows
                assert rows == expected, "Context rows/order/flags changed"
                samples.append(sample)
            result[name] = {"samples": samples, "plan": [r[3] for r in conn.execute("EXPLAIN QUERY PLAN " + sql, bound)]}
        result["rows_order_and_flags_equal"] = True
        conn.close()
    assert hashlib.sha256(args.db.read_bytes()).digest() == fingerprint
    result["source_unchanged"] = True
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({k:v for k,v in result.items() if not isinstance(v,dict)}, ensure_ascii=False))
    for name in ("original", "range_union_without_index", "range_union_with_time_index"):
        print(name, result[name]["samples"])


if __name__ == "__main__":
    main()
