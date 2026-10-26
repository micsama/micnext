import fcntl
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from convert import convert

SCRIPT = Path(__file__).with_name("convert.py")


class ConversionTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="micnext-convert-")
        self.addCleanup(self.directory.cleanup)
        self.db = Path(self.directory.name) / "micnext.db"
        with sqlite3.connect(self.db) as conn:
            conn.executescript("""
                CREATE TABLE schema_migrations(module TEXT PRIMARY KEY, version INTEGER NOT NULL);
                INSERT INTO schema_migrations VALUES ('core', 25), ('wechat', 5);
                CREATE TABLE core_sessions(id INTEGER PRIMARY KEY);
                INSERT INTO core_sessions VALUES (1);
                CREATE TABLE core_messages(id INTEGER PRIMARY KEY, session_id INTEGER NOT NULL, payload TEXT NOT NULL,
                  delivered_at INTEGER, delivery_failure TEXT,
                  kind TEXT GENERATED ALWAYS AS (json_extract(payload, '$.kind')) VIRTUAL);
                INSERT INTO core_messages(id, session_id, payload) VALUES
                  (10, 1, '{"kind":"Notification","source":"micnext","text":"t"}'), (11, 1, '{"kind":"HarnessNote","text":"n"}');
                CREATE INDEX idx_messages_undelivered ON core_messages(session_id) WHERE kind IN ('Reply', 'Notification');
                CREATE TABLE core_pushes(id INTEGER PRIMARY KEY, text TEXT NOT NULL);
                CREATE TABLE wechat_delivery(message_id INTEGER PRIMARY KEY, user_id TEXT NOT NULL, plan TEXT NOT NULL);
                INSERT INTO wechat_delivery VALUES (10, 'u', '{"chunks":["a"]}');
            """)

    def run_script(self, mode):
        return subprocess.run([sys.executable, str(SCRIPT), "--db", str(self.db), mode], capture_output=True, text=True)

    def version(self):
        with sqlite3.connect(self.db) as conn:
            return conn.execute("SELECT version FROM schema_migrations WHERE module='core'").fetchone()[0]

    def test_check_then_convert_and_repeat(self):
        self.assertEqual(self.run_script("--check").returncode, 0)
        self.assertEqual(self.version(), 25)
        result = self.run_script("--apply")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.version(), 28)
        with sqlite3.connect(self.db) as conn:
            self.assertEqual(conn.execute("SELECT seen_message_id FROM core_sessions").fetchone(), (11,))
            payloads = conn.execute("SELECT json_extract(payload, '$.files') FROM core_messages ORDER BY id").fetchall()
            self.assertEqual(payloads, [("[]",), (None,)])
            self.assertEqual(conn.execute("SELECT plan FROM wechat_delivery").fetchone(), ('{"chunks":["a"],"files":[]}',))
        self.assertEqual(self.run_script("--apply").returncode, 0)

    def test_live_service_is_not_modified(self):
        with self.db.with_name("micnext.lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.assertEqual(self.run_script("--if-idle").returncode, 0)
            self.assertNotEqual(self.run_script("--apply").returncode, 0)
            self.assertEqual(self.version(), 25)

    def test_conversion_failure_rolls_back_the_whole_plan(self):
        first = self.db.with_name("first.sql")
        first.write_text("CREATE TABLE converted(id INTEGER PRIMARY KEY);\n")
        second = self.db.with_name("second.sql")
        second.write_text("INSERT INTO missing_table VALUES (1);\n")
        with sqlite3.connect(self.db) as conn:
            with self.assertRaises(sqlite3.Error):
                convert(conn, [("core", 26, first), ("core", 27, second)])
            self.assertIsNone(conn.execute("SELECT name FROM sqlite_master WHERE name='converted'").fetchone())
        self.assertEqual(self.version(), 25)

    def test_missing_step_and_newer_database_fail_without_writes(self):
        for version in [9, 999]:
            with sqlite3.connect(self.db) as conn:
                conn.execute("UPDATE schema_migrations SET version=?", (version,))
            self.assertNotEqual(self.run_script("--apply").returncode, 0)
            self.assertEqual(self.version(), version)


if __name__ == "__main__":
    unittest.main()
