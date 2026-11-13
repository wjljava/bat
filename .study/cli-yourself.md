# 从 bat 到你自己的 CLI（动手桥接篇）

> 定位：前面所有文档（t01~t09 / cli-domain / architecture / cargo-toml / build）都是
> "**读懂 bat**"的视角。这篇是最后一块拼图——**把读懂的东西变成"你能自己写一个 CLI"**。
> 前面你"看懂了猫"，这篇教你"**养一只自己的猫**"。
> 组织方式：先给一个**真的能 `cargo run` 的最小 CLI 骨架**，再讲"从骨架升级到像 bat"
> 的每一层，最后是"该抄什么 / 别抄什么"的判断表 + 三个递进练习。

## 0. 你现在在哪

```
读懂 bat？  ✅  t01~t09（执行流）+ cli-domain（领域知识）+ architecture（为什么）
写自己的 CLI？⚠️  缺"动手"这一环 —— 本篇补上
```

**本篇的检验标准**：读完，你能不看任何参考，从零 `cargo new` 一个带"子命令 + 配置 +
管道契约 + 正确退出码"的 CLI 雏形，并知道 bat 里哪些设计该照搬、哪些是"bat 太大才需要"。

---

## 1. 最小可运行骨架（真能 `cargo run`，对应 bat t01 的抽象版）

目标：做一个 `minicat`（简化版 cat）：`minicat a.txt b.txt` 打印两个文件、`-n` 加行号、
管道 `echo hi | minicat` 也能读。这是 bat 的"去翅膀"版——**先学会没翅膀的，再加翅膀**。

> **这个项目已经建好并实测通过，就在 `.study/minicat/`**（`Cargo.toml` + `src/main.rs`，
> 零依赖，`main.rs` 里每段都注释了对应 bat 哪篇）。你可以直接：
> ```
> cd .study/minicat && cargo run -- a.txt
> echo hi | cargo run -- -n          # → "1  hi"
> cargo run -- missing.txt           # 退出码 1，提示在 stderr
> ```
> 下面这段代码就是那份 `main.rs`，**读懂它比读懂 bat 快 10 倍**（bat 是它的复杂版）。

**步骤 1：起项目**（已做，见上面；从零建的话）
```
cargo new minicat && cd minicat
# 编辑 Cargo.toml，加依赖（就一个，先别贪多）：
#   [dependencies]
#   clap = { version = "4", features = ["derive"] }
```

**步骤 2：`src/main.rs`（~60 行，能跑）**

```rust
use std::env;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process;

fn main() {
    // —— 对应 bat t01：解析参数（先手写，理解机制；熟练后再换 clap）
    let args: Vec<String> = env::args().collect();
    let number = args.contains(&String::from("-n"));
    let files: Vec<PathBuf> = args.iter().skip(1)
        .filter(|a| !a.starts_with('-'))           // 位置参数 = 文件
        .map(PathBuf::from)
        .collect();

    let mut no_errors = true;                       // —— 对应 bat t09 的"半成功"

    if files.is_empty() {
        // 没给文件 → 读 stdin（对应 bat t06 的 StdIn 分支）
        no_errors = read_from(io::stdin().lock(), number);
    } else {
        for (i, f) in files.iter().enumerate() {
            if i > 0 { writeln!(io::stdout()).ok(); }  // 文件间空行（bat 的 rule）
            match File::open(f) {
                Ok(fh) => { if !read_from(fh, number) { no_errors = false; } }
                Err(_) => {
                    // —— 对应 bat t09：错误走 stderr，不中断（cli-domain ① 契约）
                    eprintln!("minicat: {}: not found", f.display());
                    no_errors = false;
                }
            }
        }
    }

    process::exit(if no_errors { 0 } else { 1 });   // —— 对应 bat t09：退出码
}

fn read_from(mut r: impl Read, number: bool) -> bool {
    let mut content = String::new();
    let ok = r.read_to_string(&mut content).is_ok();
    for (ln, line) in content.lines().enumerate(1) {
        if number {
            println!("{:>4}  {}", ln, line);        // —— 行号（bat 的行号装饰雏形）
        } else {
            println!("{}", line);
        }
    }
    ok
}
```

**先跑通，验证三件事（对应 cli-domain ① 契约）：**
- `minicat a.txt` → 打印内容，退出码 0
- `minicat missing.txt` → 错误打到 **stderr**（`minicat: missing.txt: not found`），退出码 1
- `echo hi | minicat -n` → 读到 `1  hi`（管道也能跑）

> 关键：你刚手写"解析参数 + 读文件 + 错误走 stderr + 退出码"这套——**这就是所有 CLI 的骨架**，
> bat 的 t01/t06/t09 全是它的"复杂版"。你现在从骨架看 bat，会瞬间明白 bat 每步在干嘛。

---

## 2. 从骨架到"像 bat"：4 层升级（每层抄 bat 哪个模式）

按这个顺序加，每加一层都跑一遍确认没坏：

### 层 1：参数解析交给 clap（抄 bat t01 的 `clap_app.rs`）

把手写 `env::args` 换成 clap（`clap = { features=["derive"] }`）：

```rust
use clap::Parser;

#[derive(Parser)]
struct Cli {
    /// 文件参数（0 个 = 读 stdin，对应 bat 的 'FILE' 位置参数）
    files: Vec<PathBuf>,
    /// 显示行号（对应 bat 的 -n）
    #[arg(short, long)]
    number: bool,
    /// 主题（对应 bat 的 --theme，先只存不用）
    #[arg(long)]
    theme: Option<String>,
}
```

**抄的点**：clap 的"位置参数"（`files: Vec<PathBuf>`）自动处理"0 个=读 stdin"。
bat 在 t01 用 `clap_app.rs`（830 行）定义 30+ 个 flag，你只需 3 个起步。**别一上来抄 30 个**。

### 层 2：配置三级合并（抄 bat t01 §2c + cli-domain ②）

加一个 `Config` 结构体，按"命令行 > 环境变量 > 配置文件"填默认值。
**最小版**：读 `MINICAT_THEME` 环境变量兜底主题；熟练后再上"配置文件拍平成参数"那招
（bat 的 `shell_words` 套路，cli-domain ② 讲过"为什么"）。

**别贪**：bat 有 28 个 Config 字段，你的第一个 CLI 3~5 个就够。

### 层 3：输出契约（抄 bat cli-domain ① + ⑥）

- **管道时别发颜色码**：`is_terminal()`（对应 bat t01 §2a）判断 stdout 是不是终端，
  不是就不上色（否则 `minicat | grep` 会看到 `\x1b[31m` 乱码）。
- **超一屏可起 pager**（可选）：bat 用 `less`（t05），你的可以暂时不做，但**知道该在
  "输出是 TTY 且内容超一屏"时做**。

### 层 4：feature 门控（抄 bat cargo-toml §2，**你的项目可跳过**）

bat 把功能切 `git`/`paging`/`lessopen` 等 feature 是因为它**又是库又是程序**。
**你的第一个独立 CLI 不需要 feature 门控**——那是"库用户裁剪依赖"才有的需求
（cargo-toml §2c 讲过）。先全开，等有人当库用你的东西再说。

---

## 3. "该抄 / 别抄"判断表（最关键的一张）

读完 bat 最大的坑是**把"bat 的做法"当成"所有 CLI 的做法"**。这张表帮你分：

| bat 的设计 | 该抄？ | 为什么 |
|---|---|---|
| 退出码（0/1）+ 半成功语义 | ✅ 抄 | 所有 CLI 的契约，脚本友好（cli-domain ①） |
| 正常走 stdout、错误走 stderr | ✅ 抄 | 管道铁律，错了下游全乱 |
| 管道时纯文本、终端时才高亮 | ✅ 抄 | "像 cat"的可组合性核心 |
| 三级配置合并 | ⭕ 抄（简化） | 你的工具值得有默认值，但 3 层可能过重，先 2 层 |
| clap 解析参数 | ✅ 抄 | 别手写，clap 是事实标准 |
| **库/bin 双身份**（`PrettyPrinter` API） | ❌ 别抄 | 你的第一个 CLI 不是库，别搞 `src/lib.rs` 抽象（architecture 判断 1） |
| **`Box<dyn Printer>` 二选一** | ❌ 别抄（先） | 只有"运行时按配置选实现"才需要 trait 对象（architecture 判断 3）；你的 CLI 没有这个复杂度时，直接普通函数 |
| **`Vec<Box<dyn Decoration>>` 可插拔装饰** | ❌ 别抄（先） | 加"行号/主题"两个固定功能用普通代码即可；等要"用户自由组合 N 种装饰"才上 trait 对象 |
| **有状态高亮器跨行持有** | ⭕ 视需求 | 只有做"语法高亮"才需要（architecture 判断 5）；你的工具若不高亮就不用 |
| **feature 门控** | ❌ 别抄（先） | 库才需要（见上） |
| **编译期把语法预编译成 .bin** | ❌ 别抄 | 那是"启动要秒开 226 个语法"才有的重活（cli-domain ⑤）；你的工具没有这需求 |

> **总原则**：bat 的复杂度来自"它要同时是**库**和**全功能高亮工具**"。
> **你写第一个 CLI：先做"纯 CLI 的单功能工具"**，抄上表 ✅ 的行，其余全砍。
> 等你的工具真长到那个复杂度，再回来抄对应的 bat 模式。

---

## 4. 三个递进练习（从 0 到"像 bat"）

按顺序做，每个做完再下一个：

1. **`minigrep`（~1h）**：`minigrep <pattern> <file>`，找匹配行、`-n` 行号、
   没匹配退出码 1、有匹配 0。**练**：参数解析、退出码语义、stderr 提示。
2. **`minitheme`（~半天）**：在你的 minigrep 里加 `--color`（终端上色、管道纯文本）+
   一个 `MINIGREP_THEME` 环境变量默认值。**练**：`is_terminal()`、stdout/stderr 分流、两级配置。
3. **`mybat-lite`（~1-2 天）**：读文件、按扩展名选"高亮规则"（先用 2 种语言手写正则，
   别上 syntect）、`-n` 行号、`--theme`。**练**：语法检测顺序（cli-domain ③ 的四级）、
   主题=scope→颜色（④）。做到这里你已经理解 bat 80% 的核心了。

每个练习的"检验"：跑一遍 cli-domain ① 的契约（管道、退出码、stderr）确认没破。

---

## 5. 检验（合上文档答）

1. 你的 CLI 报错时该打 stdout 还是 stderr？为什么（会破坏什么）？（cli-domain ①）
2. `minigrep foo bar.txt | grep foo` 里，minigrep 的"没找到"提示会不会进右边的 grep？
   怎么避免？
3. 为什么你的第一个 CLI **不该**有 `src/lib.rs` 库抽象、也不该用 `Box<dyn X>`？
   什么情况下才该上？（architecture 判断 1/3/4）
4. 你要给工具加"默认主题"，先做哪一级配置（环境变量还是配置文件）？为什么这个顺序？（②）
5. 做完练习 3（mybat-lite），扩展名检测不出来时你打算怎么"猜"语言？（cli-domain ③ 的检测顺序）

---

## 配套指回

- 想看某层升级在 bat 里的完整实现 → 对应 t 篇（第 2 节每层都标了）
- 想知道"为什么这么设计" → architecture.md 的 7 个判断
- 想知道"配置项产生什么效果" → cargo-toml.md
- 领域概念不懂 → cli-domain.md 对应小节
