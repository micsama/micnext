#!/bin/sh
set -eu
cd "$(dirname "$0")/.."

[ "$#" -le 1 ] || { echo '用法：./scripts/size.sh [--bloat]' >&2; exit 1; }

case "${1-}" in
    '') ;;
    --bloat)
        command -v cargo-bloat >/dev/null 2>&1 || {
            echo '请先安装：cargo install cargo-bloat --locked' >&2
            exit 1
        }
        printf 'cargo-bloat 将按默认 features 重新构建 release；随后分析构建结果。\n\n'
        cargo bloat --release --locked -p micnext --bin micnext --crates -n 20
        cargo bloat --release --locked -p micnext --bin micnext -n 20
        ;;
    *) echo '用法：./scripts/size.sh [--bloat]' >&2; exit 1 ;;
esac

command -v python3 >/dev/null 2>&1 || { echo '需要 python3' >&2; exit 1; }
python3 - <<'PY'
from pathlib import Path
from datetime import datetime
import re
import shutil
import subprocess
import sys
import tempfile


def run(*args):
    return subprocess.check_output(args, text=True)


def amount(value):
    return f"{value / 1024 / 1024:8.3f} MiB  {value:>10,} B"


binary = Path("target/release/micnext")
if not binary.is_file():
    sys.exit("缺少 target/release/micnext；请先运行 ./build.sh")
for tool in ("file", "otool", "strip"):
    if shutil.which(tool) is None:
        sys.exit(f"需要 {tool}（本脚本分析 macOS Mach-O；请安装 Xcode Command Line Tools）")
description = run("file", str(binary)).strip()
if "Mach-O 64-bit executable" not in description or "universal binary" in description:
    sys.exit("仅支持单架构的 64 位 Mach-O release 二进制")

total = binary.stat().st_size
print(description)
print(f"文件总大小：{amount(total)}")
print(f"文件修改时间：{datetime.fromtimestamp(binary.stat().st_mtime).isoformat(timespec='seconds')}")

# NOTE: 使用 segment 的 filesize，排除 __PAGEZERO 和 BSS 等虚拟内存。
segments = []
sections = []
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

with tempfile.TemporaryDirectory(prefix="micnext-size-") as directory:
    copy = Path(directory) / "micnext"
    shutil.copyfile(binary, copy)
    subprocess.run(["strip", str(copy)], check=True)
    stripped = copy.stat().st_size
print(f"\nstrip 副本大小：{amount(stripped)}")
print(f"可减少：        {amount(total - stripped)}  {(total - stripped) / total:.2%}")
print("这是移除符号后的体积估算；副本未做运行/重新签名验证。")

assets = Path("web/dist")
if assets.is_dir():
    files = [(path, path.stat().st_size) for path in assets.rglob("*") if path.is_file()]
    print(f"\n当前 web/dist 原始文件合计：{amount(sum(size for _, size in files))}")
    print("仅作资源瘦身参考；可能与构建时资源不同，不等于二进制中的占用。")
    for path, size in sorted(files, key=lambda item: item[1], reverse=True)[:5]:
        print(f"  {amount(size)}  {path}")
print("\ncrate / 大函数排行：./scripts/size.sh --bloat（会重新构建；归因仅覆盖机器码，是近似值）")
PY
