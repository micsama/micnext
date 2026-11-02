#!/bin/sh
set -eu
cd "$(dirname "$0")"
repo_dir=$PWD

profile=release
if [ "${1-}" = debug ]; then
    profile=debug
    shift
fi

build() {
    command -v cargo >/dev/null 2>&1 || { echo "请先手动安装 Rust 工具链（cargo）" >&2; exit 1; }
    command -v bun >/dev/null 2>&1 || { echo "请先手动安装 Bun" >&2; exit 1; }
    command -v python3 >/dev/null 2>&1 || { echo "请先手动安装 Python 3" >&2; exit 1; }

    bun install --cwd web --frozen-lockfile
    (cd web && bun ./node_modules/vite/bin/vite.js build)
    case "$profile" in
        release)
            # 确保本轮生成的全部页面资源重新嵌入二进制。
            cargo clean -p mic-gateway --release
            cargo build --release --locked --config 'profile.release.strip="symbols"'
            ;;
        debug)
            cargo build --locked
            ;;
    esac
    printf '\n构建完成：target/%s/micnext\n' "$profile"
}

# prepare 只检查：网页构建时旧服务仍持有数据库，停止后再 start。
mode=${1-build}
if [ "$#" -gt 0 ]; then shift; fi
case "$mode" in
    build|run|prepare)
        build
        db=$(bun scripts/db-path.ts "$@")
        case "$mode" in
            prepare) python3 scripts/db/convert.py --db "$db" --check ;;
            build) python3 scripts/db/convert.py --db "$db" --if-idle ;;
            run)
                python3 scripts/db/convert.py --db "$db" --apply
                exec "./target/$profile/micnext" "$@"
                ;;
        esac
        printf '运行：./build.sh %srun [原 micnext 参数...]\n' "$(if [ "$profile" = debug ]; then printf 'debug '; fi)"
        ;;
    start)
        if [ "${1-}" != --db ] || [ "$#" -lt 3 ]; then
            printf '用法：%s [debug] start --db <数据库路径> -- [micnext 参数...]\n' "$0" >&2
            exit 1
        fi
        db=$2
        shift 2
        [ "$1" = -- ] || { echo '数据库路径后需要 --' >&2; exit 1; }
        shift
        python3 scripts/db/convert.py --db "$db" --apply
        # 服务进程的 PATH 常缺用户目录（如 uv 装在 ~/.local/bin）：重启时取登录 shell 的 PATH，只带标记行防 profile 输出混入。
        login_path=$(sh -lc 'printf "\n__MICNEXT_PATH__=%s\n" "$PATH"' </dev/null 2>/dev/null | sed -n 's/^__MICNEXT_PATH__=//p') || true
        if [ -n "$login_path" ]; then PATH=$login_path; export PATH; fi
        cd "${MICNEXT_RUN_CWD-$repo_dir}"
        unset MICNEXT_RUN_CWD
        exec "$repo_dir/target/$profile/micnext" "$@"
        ;;
    *)
        printf '用法：%s [debug] [build|prepare|run [micnext 参数...]]\n' "$0" >&2
        exit 1
        ;;
esac
