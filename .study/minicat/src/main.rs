// minicat —— "去翅膀的 cat"，用来练手写 CLI 的骨架。
// 每一段都注释了它对应 bat 学习文档的哪篇（.study/t*.md、cli-domain.md、cli-yourself.md）。
// 零外部依赖：参数是手写的，先把机制吃透；要上 clap 见 cli-yourself.md §2 层 1。
//
// 跑法（在 .study/minicat 目录）：
//   cargo run -- a.txt b.txt
//   cargo run -- -n a.txt
//   echo hi | cargo run
//   cargo run -- missing.txt        # 退出码应为 1，且提示打在 stderr
//   echo hi | cargo run -- -n       # 管道 + 行号 → "1  hi"

use std::env;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process;

fn main() {
    // —— 对应 bat t01 §2（App::new 解析参数）：这里手写版 ——
    let args: Vec<String> = env::args().collect();

    // -n / --number 开关
    let number = args.iter().any(|a| a == "-n" || a == "--number");

    // 位置参数 = 文件（跳过以 '-' 开头的 flag 和程序名 args[0]）
    // 对应 bat t01 的 'FILE' 位置参数；空 = 读 stdin
    let files: Vec<PathBuf> = args.iter().skip(1)
        .filter(|a| !a.starts_with('-'))
        .map(PathBuf::from)
        .collect();

    let mut stdout = io::stdout();

    // —— 对应 bat t09 / cli-domain ①："半成功"语义 ——
    let mut no_errors = true;

    if files.is_empty() {
        // —— 对应 bat t06 的 StdIn 分支：没给文件就读 stdin ——
        let mut stdin = io::stdin().lock();
        let mut content = String::new();
        if stdin.read_to_string(&mut content).is_err() {
            no_errors = false;
        }
        print_lines(&content, number, &mut stdout);
    } else {
        for (i, f) in files.iter().enumerate() {
            // 文件间一道空行（对应 bat 的 rule 分隔，cli-domain ⑥）
            if i > 0 {
                writeln!(stdout).ok();
            }
            match File::open(f) {
                Ok(mut fh) => {
                    let mut content = String::new();
                    // 文件读不出来（比如其实是个目录）→ 算"半成功"
                    if fh.read_to_string(&mut content).is_err() {
                        no_errors = false;
                    }
                    print_lines(&content, number, &mut stdout);
                }
                Err(_) => {
                    // —— 对应 bat t09 / cli-domain ①：错误走 stderr，不中断、不崩 ——
                    // 关键：别打 stdout（否则 `minicat x | grep` 会把这行也 grep 进去）
                    eprintln!("minicat: {}: not found", f.display());
                    no_errors = false;
                }
            }
        }
    }

    // —— 对应 bat t09：退出码（Ok(true)=0 / Ok(false)=1）——
    // 全好 = 0；有文件没打成 = 1。shell 脚本 `if minicat f1 f2; then` 判的就是这个。
    process::exit(if no_errors { 0 } else { 1 });
}

/// 把内容逐行打印；number 为真时每行加行号（对应 bat t08 的行号装饰雏形）。
/// 行号用 `{width}` 右对齐，和 bat 的行号列宽逻辑一致。
fn print_lines(content: &str, number: bool, mut out: impl std::io::Write) {
    let max = content.lines().count().max(1);
    let width = max.to_string().len();
    for (i, line) in content.lines().enumerate() {
        let ln = i + 1;   // enumerate 从 0 起，行号要 1 基
        if number {
            let _ = writeln!(out, "{:>width$}  {}", ln, line, width = width);
        } else {
            let _ = writeln!(out, "{}", line);
        }
    }
}
