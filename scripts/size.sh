#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
command -v python3 >/dev/null 2>&1 || { echo '需要 python3' >&2; exit 1; }

python3 - "$@" <<'PY'
from pathlib import Path
from datetime import datetime
import json
import re
import shutil
import subprocess
import sys
import tempfile

USAGE = """用法：
  ./scripts/size.sh               分析 target/release/micnext 的磁盘组成（通常是 ./build.sh 的去符号产物）
  ./scripts/size.sh --bloat       重新构建带符号的 release，列出 crate 与大函数排行
  ./scripts/size.sh --diff <rev>  与 <rev> 对比各 crate 机器码和去符号体积（当前工作区 vs <rev>）"""
BLOAT = ["cargo", "bloat", "--release", "--locked", "-p", "micnext", "--bin", "micnext"]
BASE = Path("target/size-base")


def run(*args, **kwargs):
    return subprocess.check_output(args, text=True, **kwargs)


def mib(value):
    return f"{value / 1024 / 1024:7.3f} MiB"


def amount(value):
    return f"{mib(value)}  {value:>10,} B"


def signed(value):
    return f"{'+' if value > 0 else '-' if value < 0 else ' '}{abs(value) / 1024:9.1f} KiB"


def need(*tools):
    for tool in tools:
        if shutil.which(tool) is None:
            hint = "cargo install cargo-bloat --locked" if tool == "cargo-bloat" else "Xcode Command Line Tools"
            sys.exit(f"需要 {tool}（{hint}）")


def stripped_size(binary):
    with tempfile.TemporaryDirectory(prefix="micnext-size-") as directory:
        copy = Path(directory) / "micnext"
        shutil.copyfile(binary, copy)
        subprocess.run(["strip", str(copy)], check=True)
        return copy.stat().st_size


def crates(cwd, target_dir):
    """构建并返回 (cargo-bloat crate 统计, 二进制路径)；编译进度直接输出到终端。"""
    out = run(*BLOAT, "--crates", "-n", "0", "--message-format", "json", "--target-dir", str(target_dir), cwd=cwd)
    return json.loads(out), Path(target_dir) / "release/micnext"


def segments_report():
    binary = Path("target/release/micnext")
    if not binary.is_file():
        sys.exit("缺少 target/release/micnext；请先运行 ./build.sh")
    need("file", "otool", "strip")
    description = run("file", str(binary)).strip()
    if "Mach-O 64-bit executable" not in description or "universal binary" in description:
        sys.exit("仅支持单架构的 64 位 Mach-O release 二进制")

    total = binary.stat().st_size
    print(description)
    print(f"文件总大小：{amount(total)}")
    print(f"文件修改时间：{datetime.fromtimestamp(binary.stat().st_mtime).isoformat(timespec='seconds')}")

    # NOTE: 使用 segment 的 filesize，排除 __PAGEZERO 和 BSS 等虚拟内存。
    segments, sections = [], []
    for block in re.split(r"Load command \d+\n", run("otool", "-l", str(binary))):
        if not re.search(r"^\s*cmd LC_SEGMENT_64$", block, re.M):
            continue
        name = re.search(r"^\s*segname (\S+)$", block, re.M).group(1)
        length = int(re.search(r"^\s*filesize (\d+)$", block, re.M).group(1))
        if length:
            segments.append((name, length))
        for section in re.split(r"^Section\n", block, flags=re.M)[1:]:
            section_name = re.search(r"^\s*sectname (\S+)$", section, re.M).group(1)
            size = int(re.search(r"^\s*size (0x[0-9a-fA-F]+)$", section, re.M).group(1), 16)
            flags = int(re.search(r"^\s*flags (0x[0-9a-fA-F]+)$", section, re.M).group(1), 16)
            if flags & 0xff not in (1, 12, 18) and size:
                sections.append((f"{name},{section_name}", size))

    print("\n磁盘组成（segment；不含虚拟保留区）：")
    for name, size in sorted(segments, key=lambda item: item[1], reverse=True):
        print(f"  {name:30} {amount(size)}  {size / total:6.2%}")
    remaining = total - sum(size for _, size in segments)
    if remaining:
        print(f"  {'segment 外数据':30} {amount(remaining)}  {remaining / total:6.2%}")
    print("\nsection 明细（包含于上面的 segment，不能重复相加）：")
    for name, size in sorted(sections, key=lambda item: item[1], reverse=True):
        print(f"  {name:30} {amount(size)}  {size / total:6.2%}")
    print("  __text=机器码；__const/__cstring=常量/字符串/嵌入资源；")
    print("  __eh_frame/__unwind_info/__gcc_except_tab=栈展开/异常信息；")
    print("  __LINKEDIT=符号表、动态链接及签名等元数据。")

    stripped = stripped_size(binary)
    if stripped < total:
        print(f"\nstrip 副本大小：{amount(stripped)}（可减少 {(total - stripped) / total:.2%}，未做运行验证）")
    else:
        print("\n已是去符号产物。")

    assets = Path("web/dist")
    if assets.is_dir():
        files = [(path, path.stat().st_size) for path in assets.rglob("*") if path.is_file()]
        print(f"\nweb/dist 原始文件合计：{amount(sum(size for _, size in files))}（参考，不等于二进制中的占用）")
        for path, size in sorted(files, key=lambda item: item[1], reverse=True)[:5]:
            print(f"  {amount(size)}  {path}")
    print("\ncrate 排行：--bloat；与历史版本对比：--diff <rev>")


def bloat_report():
    need("cargo-bloat")
    print("按默认 features 重新构建带符号的 release（会覆盖 target/release/micnext）。\n", flush=True)
    subprocess.run([*BLOAT, "--crates", "-n", "30"], check=True)
    subprocess.run([*BLOAT, "-n", "30"], check=True)
    print("归因只覆盖机器码（.text），是近似值；[Unknown] 主要是 C 代码（SQLite 等）。")


def diff_report(rev):
    need("cargo-bloat", "strip")
    sha = run("git", "rev-parse", "--verify", f"{rev}^{{commit}}").strip()
    label = run("git", "log", "-1", "--format=%h %cs %s", sha).strip()
    if not Path("web/dist").is_dir():
        sys.exit("缺少 web/dist；请先构建前端（双方共用同一份资源，只比较 Rust 部分）")

    source = BASE / "src"
    if source.exists():
        subprocess.run(["git", "worktree", "remove", "--force", str(source)], check=False)
        shutil.rmtree(source, ignore_errors=True)
    subprocess.run(["git", "worktree", "add", "--detach", "--quiet", str(source), sha], check=True)
    try:
        shutil.copytree("web/dist", source / "web/dist", dirs_exist_ok=True)
        print(f"构建基线 {label}（target 缓存在 {BASE}/target）…", flush=True)
        base, base_bin = crates(source, (BASE / "target").resolve())
    finally:
        subprocess.run(["git", "worktree", "remove", "--force", str(source)], check=True)
    print("构建当前工作区…", flush=True)
    head, head_bin = crates(".", Path("target").resolve())

    before = {item["name"]: item["size"] for item in base["crates"]}
    after = {item["name"]: item["size"] for item in head["crates"]}
    rows = sorted(
        ((name, before.get(name, 0), after.get(name, 0)) for name in before.keys() | after.keys()),
        key=lambda row: abs(row[2] - row[1]),
        reverse=True,
    )
    print(f"\n基线：{label}\n当前：工作区（HEAD {run('git', 'rev-parse', '--short', 'HEAD').strip()}）")
    print(f"\n{'crate':28} {'基线':>11} {'当前':>11} {'变化':>14}")
    shown = [row for row in rows if abs(row[2] - row[1]) >= 8 * 1024][:30]
    for name, old, new in shown:
        mark = "（新增）" if not old else "（移除）" if not new else ""
        print(f"  {name:26} {mib(old)} {mib(new)} {signed(new - old)} {mark}")
    rest = sum(new - old for _, old, new in rows) - sum(new - old for _, old, new in shown)
    print(f"  {'其余':26} {'':11} {'':11} {signed(rest)}")

    totals = [
        (".text 机器码", base["text-section-size"], head["text-section-size"]),
        ("文件（带符号）", base["file-size"], head["file-size"]),
        ("去符号后", stripped_size(base_bin), stripped_size(head_bin)),
    ]
    print()
    for name, old, new in totals:
        print(f"  {name:24} {mib(old)} → {mib(new)}  {signed(new - old)}")
    print("\n双方嵌入同一份 web/dist，差值只反映 Rust 代码与依赖；crate 归因是近似值。")


args = sys.argv[1:]
if not args:
    segments_report()
elif args == ["--bloat"]:
    bloat_report()
elif len(args) == 2 and args[0] == "--diff":
    diff_report(args[1])
else:
    sys.exit(USAGE)
PY
