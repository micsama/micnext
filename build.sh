#!/bin/sh
set -eu
cd "$(dirname "$0")"

profile=release
if [ "${1-}" = debug ]; then
    profile=debug
    shift
fi

build() {
    command -v cargo >/dev/null 2>&1 || { echo "请先手动安装 Rust 工具链（cargo）" >&2; exit 1; }
    command -v bun >/dev/null 2>&1 || { echo "请先手动安装 Bun" >&2; exit 1; }

    bun install --cwd web --frozen-lockfile
    bun run --cwd web --bun build
    case "$profile" in
        release)
            # 确保本轮生成的全部页面资源重新嵌入二进制。
            cargo clean -p mic-gateway --release
            cargo build --release --locked
            ;;
        debug)
            cargo build --locked
            ;;
    esac
    printf '\n构建完成：target/%s/micnext\n' "$profile"
}

if [ "$#" -eq 0 ]; then
    build
    printf '运行：./target/%s/micnext\n' "$profile"
else
    case "$1" in
        run)
            shift
            build
            exec "./target/$profile/micnext" "$@"
            ;;
        *)
            printf '用法：%s [debug] [run [micnext 参数...]]\n' "$0" >&2
            exit 1
            ;;
    esac
fi
