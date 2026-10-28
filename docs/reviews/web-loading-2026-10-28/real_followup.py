"""Real-backup follow-up: isolate calendar/projection costs and finish one baseline query."""
import argparse
import hashlib
import json
import pathlib
import platform
import shutil
import sqlite3
import tempfile
import time

from real_probe import CALL, measure


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("db", type=pathlib.Path)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    args = parser.parse_args()
    assert not pathlib.Path(str(args.db) + "-wal").exists(), "Use a standalone backup"
    fingerprint = hashlib.sha256(args.db.read_bytes()).digest()
    report = {"sqlite": sqlite3.sqlite_version, "python": platform.python_version(),
              "os": platform.system(), "architecture": platform.machine(), "repetitions": 3}
    with tempfile.TemporaryDirectory(prefix="micnext-real-followup-") as directory:
        path = pathlib.Path(directory) / "indexed.db"
        shutil.copyfile(args.db, path)
        path.chmod(0o600)
        conn = sqlite3.connect(path, isolation_level=None)
        conn.execute("CREATE INDEX probe_call_messages ON core_messages(model_call_id,kind)")
        hot = conn.execute("SELECT session_id FROM core_model_calls GROUP BY session_id ORDER BY count(*) DESC LIMIT 1").fetchone()[0]
        day = "strftime('%Y-%m-%d', c.started_at / 1000, 'unixepoch', 'localtime')"
        no_day = CALL.replace(day, "NULL")
        assert no_day != CALL
        projections = {
            "indexed_original_call_rows": CALL + " WHERE c.session_id=? ORDER BY c.id",
            "indexed_without_unused_day": no_day + " WHERE c.session_id=? ORDER BY c.id",
            "usage_fields_only": "SELECT c.input_tokens,c.output_tokens,c.cache_read_tokens,c.cache_write_tokens,c.reasoning_tokens,m.price_input,m.price_cache_read,m.price_cache_write,m.price_output FROM core_model_calls c LEFT JOIN core_models m ON m.id=c.model_id WHERE c.session_id=?",
            "calendar_localtime_only": "SELECT " + day + " FROM core_model_calls c WHERE c.session_id=?",
            "calendar_utc_only": "SELECT strftime('%Y-%m-%d',c.started_at/1000,'unixepoch') FROM core_model_calls c WHERE c.session_id=?",
        }
        report["projection_costs"] = {}
        for name, sql in projections.items():
            samples = [measure(conn, [(sql, (hot,))])[0] for _ in range(3)]
            report["projection_costs"][name] = samples
            print(name, samples, flush=True)

        # Check that narrow rows preserve the existing call_cost and usage-total semantics.
        def totals(rows, full):
            result = [0] * 7
            cost = 0.0
            for row in rows:
                vals = row[10:19] if full else row
                inp, out, read, write, reason, pi, pr, pw, po = vals
                result[0] += 1
                if inp is None:
                    result[5] += 1
                    result[6] += 1
                    continue
                for i, value in enumerate((inp, out or 0, read or 0, write or 0), 1):
                    result[i] += value
                terms = [(max(0,inp-(read or 0)-(write or 0)),pi), (read or 0,pr),
                         (write or 0,pw if pw is not None else pi), (out or 0,po)]
                if read is None or any(n and price is None for n,price in terms):
                    result[6] += 1
                else:
                    cost += sum(n*(price or 0) for n,price in terms)/1e6
            return result, cost
        assert totals(conn.execute(projections["indexed_original_call_rows"], (hot,)), True) == totals(conn.execute(projections["usage_fields_only"], (hot,)), False)
        report["narrow_usage_counts_and_cost_equal"] = True
        conn.close()
        original = pathlib.Path(directory) / "baseline.db"
        shutil.copyfile(args.db, original)
        original.chmod(0o600)
        conn = sqlite3.connect(original)
        print("running unindexed full session query, deadline=120s", flush=True)
        report["full_unindexed_session_query"], _ = measure(conn, [(CALL + " WHERE c.session_id=? ORDER BY c.id", (hot,))], seconds=120)
        conn.close()
        print(report["full_unindexed_session_query"], flush=True)
    assert hashlib.sha256(args.db.read_bytes()).digest() == fingerprint
    report["source_unchanged"] = True
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")


if __name__ == "__main__":
    main()
