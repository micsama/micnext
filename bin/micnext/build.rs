//! 编译时从 git 读出版本：主次版本取自 workspace Cargo.toml，修订号为改动该版本行以来、改到程序内容的提交数。
//! 注入 `MICNEXT_VERSION`、`MICNEXT_COMMIT`、`MICNEXT_COMMIT_TIME`（unix 秒）；不在 git 仓库里则都不注入。

use std::path::Path;
use std::process::Command;

/// workspace 根清单，相对本包目录（build script 的工作目录）。
const WORKSPACE_MANIFEST: &str = "../../Cargo.toml";
/// 进入程序的文件（相对仓库根）；只改文档、脚本、测试的提交不计入修订号。
const PROGRAM_PATHS: &[&str] = &[
    ":(top)crates",
    ":(top)bin",
    ":(top)web",
    ":(top)Cargo.toml",
    ":(top)Cargo.lock",
    ":(top,exclude)web/tests",
    ":(top,exclude,glob)**/tests.rs",
    ":(top,exclude,glob)**/tests/**",
];

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status.success().then_some(())?;
    Some(String::from_utf8(out.stdout).ok()?.trim().to_owned())
}

fn main() {
    let Some(head) = git(&["log", "-1", "--format=%H %ct"]) else {
        println!("cargo:warning=不在 git 仓库中构建，修订号与提交未知");
        return;
    };
    let (commit, time) = head.split_once(' ').expect("git log 输出「哈希 时间」");
    // 版本行从未提交过（刚改、尚未提交）时修订号记 0。
    let base = git(&[
        "log",
        "-1",
        "--format=%H",
        "-G",
        "^version = \"",
        "--",
        WORKSPACE_MANIFEST,
    ])
    .filter(|base| !base.is_empty());
    let patch = match base {
        Some(base) => {
            let range = format!("{base}..HEAD");
            let mut args = vec!["rev-list", "--count", &range, "--"];
            args.extend(PROGRAM_PATHS);
            git(&args).expect("git rev-list 可用")
        }
        None => "0".to_owned(),
    };
    let major = std::env::var("CARGO_PKG_VERSION_MAJOR").expect("cargo 提供");
    let minor = std::env::var("CARGO_PKG_VERSION_MINOR").expect("cargo 提供");
    println!("cargo:rustc-env=MICNEXT_VERSION={major}.{minor}.{patch}");
    println!("cargo:rustc-env=MICNEXT_COMMIT={commit}");
    println!("cargo:rustc-env=MICNEXT_COMMIT_TIME={time}");
    // HEAD 换分支、当前分支前进（commit、pull、reset）时重读。
    let mut watched = vec!["HEAD".to_owned(), "packed-refs".to_owned()];
    watched.extend(git(&["symbolic-ref", "-q", "HEAD"]));
    for name in watched {
        if let Some(path) = git(&["rev-parse", "--git-path", &name]) {
            if Path::new(&path).exists() {
                println!("cargo:rerun-if-changed={path}");
            }
        }
    }
}
