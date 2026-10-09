#!/bin/sh
set -eu
cd "$(dirname "$0")"

command -v cargo >/dev/null 2>&1 || { echo "请先手动安装 Rust 工具链（cargo）" >&2; exit 1; }
command -v bun >/dev/null 2>&1 || { echo "请先手动安装 Bun" >&2; exit 1; }

bun install --cwd web --frozen-lockfile
bun run --cwd web --bun build
# 确保本轮生成的全部页面资源重新嵌入二进制。
cargo clean -p mic-gateway --release
cargo build --release --locked

printf '\n构建完成：target/release/micnext\n运行：./target/release/micnext\n'
