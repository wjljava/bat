# t09：收尾 —— 退出码 + 一条命令的一生（复盘）

> 主路⑦ 钻完（t05~t08）。本篇收口：`for input` 循环跑完所有文件后，
> `run()` 怎么把结果变退出码；`default_error_handler` 的完整叶子；
> 最后把整棵树走一遍"复盘"，检验你有没有真把这条线串起来。

## 你在执行树的哪个位置

```
run_with_error_handler
└─ for input in inputs: print_input …   （t06~t08 钻完）
   循环结束 → Ok(no_errors)             ← 你在这里
        │
        ▼  返回 run()
main() 里的 match result                ← 你在这里
   ├─ Ok(true)  → exit 0
   ├─ Ok(false) → exit 1
   └─ Err(e)    → default_error_handler + exit 1
```

## 1. 循环结束后：`no_errors` → `Result<bool>`（controller.rs:137）

回看 t05 §6 的 `for` 循环：每个 input 出错时 `no_errors = false` 但**继续跑下一个**
（不中断）。循环跑完：

```rust
Ok(no_errors)   // 全成功 → Ok(true)；有文件读不到 → Ok(false)
```

这个 `bool` 一路 `?` 上抛回 `main()`（t01 §1），成为退出码。

## 2. 叶子：`default_error_handler`（src/error.rs:67）★ 库侧

```rust
pub fn default_error_handler(error: &Error, output: &mut dyn Write) {
    match error {
        Error::Io(e) if e.kind() == BrokenPipe => {
            process::exit(0);                    // ★ 特判：下游管道断了不算错
        }
        Error::SerdeYamlError(_) => {
            writeln!(output, "{}: Error while parsing metadata.yaml: {error}",
                paint_prefix(Red, "[bat error]")).ok();
        }
        _ => {
            writeln!(&mut stderr().lock(), "{}: {error}",
                paint_prefix(Red, "[bat error]")).ok();   // 一般错误 → stderr
        }
    }
}
```

两个关键叶子：

1. **BrokenPipe → `exit(0)`**（error.rs:71-73）：`bat foo.rs | head` 时 `head` 读够就关管道，
   bat 再写会收到 `BrokenPipe`。这不是 bat 的错（下游主动断开），所以**直接 0 退出**，
   不打印错误。这是管道契约的一部分。
2. **一般错误 → stderr**（error.rs:82-88）：红色 `[bat error]` 前缀（`paint_prefix`，
   error.rs:59：设了 `NO_COLOR` 就不上色）。**结果走 stdout、错误走 stderr**——
   管道只接 stdout，报错不打进管道（否则 `bat | grep` 会把错误信息也 grep 了）。

**Rust 机制 · 库 vs CLI 的错误分界**（architecture.md 判断 7）：
`bat` 是**库**，库的 `default_error_handler` 只负责"把错误写进调用方给的 `output`"
（`&mut dyn Write`）——它**不自己 `exit`、不直接碰 stderr**（除了 BrokenPipe 特判）。
"真正打印到终端 stderr + 定退出码"是 **CLI 侧** `main()`（t01 §1）的活。
两个身份各管一段，互不污染。

## 3. `main()` 定退出码（main.rs:474）

```rust
match result {
    Err(error) => { default_error_handler(&error, &mut stderr.lock()); process::exit(1); }
    Ok(false)  => process::exit(1),     // 半成功：有文件没打成
    Ok(true)   => process::exit(0),     // 全成功
}
```

三条路全在 t01 讲过，这里是收口：**0=全好，1=有错（无论致命还是半成功）**。
脚本判断 `if bat f1 f2 f3; then 全成功 else 有文件没打 end`。

## 4. 复盘：一条命令的一生（`bat -n --theme Monokai src/main.rs`）

把 t01~t09 串成一条线，**不看前面的文档，自己能不能复述**：

```
① main() 启动（t01）→ run()
② App::new()（t01）：探测交互、双份解析、三级配置合并 → ArgMatches
③ 分派表（t01）：没 --list/--cache/--diagnostic… → 走主路⑦
④ app.inputs() → [Input("src/main.rs")]；app.config() → Config（28 字段，
   -n → style_components 含行号；--theme → theme；交互终端 → loop_through=false）
⑤ run_controller（t05）：
     assets_from_cache_or_binary（缓存或内置）
     Controller::new(config, &assets)   ← 'a 绑住两者
     Controller::run → run_with_error_handler
⑥ OutputType 三选一（t05）：paging_mode=QuitIfOneScreen → 起 less(-R -F -S -K)
⑦ for input: print_input（t06）：
     Input::open → OrdinaryFile → File::open + 编码嗅探(UTF-8)
     git diff 获取（feature 开）
⑧ Printer 二选一（t07）：loop_through=false → InteractivePrinter
     get_theme(Monokai)、get_syntax(路径 .rs → Rust)、装配装饰(行号)
     HighlightLines 有状态高亮器
⑨ print_file → print_file_ranges → 逐行 print_line（t08）：
     解码 → 高亮 → 挤空行 → 画行号装饰 → 两路上色写出 → footer
⑩ 循环结束（t09）：Ok(true)
⑪ main() 定退出码：exit 0
```

**复盘自测**：把 ①~⑪ 每步的"代码在哪行"补上（文件:行号），补不出来的那步，
回对应的 tNN 重读。这是检验整条线是否真懂的唯一标准。

## 5. 把"树"压回"线"

DFS 文档是**树**（每个分支钻到底再返回），但程序**实际跑的是线**（这次命令
只走了树的主路⑦那一枝）。两个视角的关系：

- **树**（t01~t09）：回答"bat 能干什么、每段代码在整体里的位置"。
- **线**（§4 复盘）：回答"这条具体命令实际走了哪一条"。

读代码时：卡在某段 → 回树找"我在哪个 tNN、哪个子分支"；
想验证整体 → 用线"把一条命令从 main 走到 exit"。

## 本区段 Rust 机制

1. **`Result<bool>` 的"半成功"语义**：`bool` 编码"是否有中间错误"，
   比 `Result<()>` 多一档——脚本能区分"全好"和"跑完了但有文件失败"。
2. **`dyn Write` 做错误输出的抽象**：`default_error_handler(error, output: &mut dyn Write)`
   ——库只负责"写进你给的 handle"，CLI 传 `stderr.lock()`，库调用方可传自己的 buffer。
   又一个 `Box/dyn` 的"存"用法（t06 讲过：存用 trait 对象、传用泛型）。

## 检验问题

1. `bat a.rs b_missing.rs c.rs`（b 不存在）最终退出码几？`no_errors` 怎么变的？
2. `bat foo.rs | head -1`（head 读完提前退出）退出码是 0 还是 1？哪行特判保证？
3. 为什么"错误信息"不会混进 `bat | grep` 的管道里（被 grep 到）？它写到哪了？
4. `default_error_handler` 为什么是库里的函数、却"不直接碰终端 stderr"（除 BrokenPipe）？这体现 bat 的什么设计（库/bin 分界）？
5. 不看 t01~t08，把"一条命令的一生"①~⑪ 每步的文件:行号补上。哪步补不出？

## 系列结束

t01~t09 把 bat 的**完整执行树**钻到底了。配套文档：
- `architecture.md`：7 个设计判断的"为什么"（每个 `#[cfg]`/`Box<dyn>`/缓存决策背后的考量）
- `build.md`：编译期支线（build 脚本 + `include_str!` 把补全脚本/语法焊进二进制）
- `cargo-toml.md`：feature 门控的配置（git/paging/lessopen… 各自裁什么）

学完带走的（architecture.md §八 五个模式）：**库/bin 分界、运行期可组合用 trait 对象、
有状态对象提层持有、feature 门控裁功能、错误类型跟身份走**。
