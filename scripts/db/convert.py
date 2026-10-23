#!/usr/bin/env python3
"""构建/更新阶段的独立数据转换工具；不由业务库启动路径加载。"""
import argparse
import fcntl
import sqlite3
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
VERSIONS = {
    "core": ROOT / "crates/mic-store/src/schema/version",
    "wechat": ROOT / "crates/mic-channel-wechat/src/schema/version",
}


def plan(conn):
    steps = []
    for module, current in VERSIONS.items():
        row = conn.execute("SELECT version FROM schema_migrations WHERE module = ?", (module,)).fetchone()
        if row is None:
            continue
        actual, target = row[0], int(current.read_text())
        if actual > target:
            raise RuntimeError(f"{module}: 库 v{actual} 高于当前 v{target}，不支持反向转换")
        paths = []
        for version in range(actual + 1, target + 1):
            path = Path(__file__).parent / f"{module}-{version}.sql"
            if not path.is_file():
                raise RuntimeError(f"{module}: 缺少 v{version - 1} → v{version} 转换脚本；请手动转换或删除旧库重建")
            paths.append((module, version, path))
        print(f"数据库 {module}: v{actual} → v{target}，待转换 {len(paths)} 步", flush=True)
        steps.extend(paths)
    return steps


def statements(sql):
    pending = ""
    for line in sql.splitlines(keepends=True):
        pending += line
        if sqlite3.complete_statement(pending):
            yield pending
            pending = ""
    if pending.strip():
        raise RuntimeError("转换脚本包含未结束的 SQL")


def convert(conn, steps):
    # 重建带外键的表需要关闭约束；提交前检查整个结果，任一步失败全部回滚。
    conn.execute("PRAGMA foreign_keys = OFF")
    conn.execute("BEGIN IMMEDIATE")
    try:
        for module, version, path in steps:
            print(f"转换 {module}: v{version - 1} → v{version} ({path.name})", flush=True)
            for statement in statements(path.read_text()):
                conn.execute(statement)
            conn.execute("UPDATE schema_migrations SET version = ? WHERE module = ?", (version, module))
        violations = conn.execute("PRAGMA foreign_key_check").fetchall()
        if violations:
            raise RuntimeError(f"转换后外键检查失败：{violations}")
        conn.commit()
    except BaseException:
        conn.rollback()
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--db", type=Path, required=True)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--apply", action="store_true")
    mode.add_argument("--if-idle", action="store_true")
    args = parser.parse_args()
    if not args.db.exists():
        print("数据库不存在，新程序将直接初始化当前结构", flush=True)
        return
    with sqlite3.connect(args.db.resolve().as_uri() + "?mode=ro", uri=True) as conn:
        steps = plan(conn)
    if args.check or not steps:
        return
    with args.db.with_name("micnext.lock").open("a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            if args.if_idle:
                print("服务仍在运行，仅检查；转换将在停止服务后的 start/run 阶段执行", flush=True)
                return
            raise RuntimeError("服务仍在运行，不能转换数据库，请先停止服务")
        with sqlite3.connect(args.db.resolve().as_uri() + "?mode=rw", uri=True) as conn:
            # 锁内重读版本，避免重复转换。
            steps = plan(conn)
            convert(conn, steps)
        print("数据库转换完成", flush=True)


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, sqlite3.Error, OSError) as error:
        print(f"数据库转换失败：{error}", file=sys.stderr)
        sys.exit(1)
