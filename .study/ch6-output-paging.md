# 第 6 章：输出与分页（`OutputType` / `OutputHandle` / pager）

这一章的目标：讲清"一行彩色字节最终送到哪"——普通 stdout、起 `less` 子进程、
还是 bat 内嵌的 `minus` pager。读完你应该能回答：
"`bat foo.rs` 为什么有时进 less、有时不？`--pager builtin` 和起 less 差在哪？"

## 涉及文件

- `src/controller.rs` — 决定输出目标的逻辑（`run_with_error_handler`）
- `src/output.rs` — `OutputType` / `OutputHandle` / `BuiltinPager`
- `src/pager.rs` — 选哪个 pager（`get_pager`）
- `src/less.rs` — less 版本探测（`retrieve_less_version`）
- `src/paging.rs` — `PagingMode` 枚举（就 7 行）

## 全景图（先有这个概念，再看代码）

```
Controller::run() → run_with_error_handler()     （controller.rs:47）
   │
   ├─ 决定输出目标（输出目标早于逐行渲染，因为 ch5 的 write! 要知道往哪写）
   │     │
   │     ├─ PagingMode::Always         → try_pager（起 less，不滚动也进）
   │     ├─ PagingMode::QuitIfOneScreen → try_pager（less -F：不滚动就退）
   │     └─ PagingMode::Never          → stdout
   │
   ├─ 选 pager（pager.rs::get_pager）
   │     优先级：--pager > BAT_PAGER > PAGER > "less"
   │     特例：pager=bat 自己 → 拒（防递归）
   │     特例：pager=builtin → 内嵌 minus（不起子进程，起线程）
   │
   ▼
OutputType（三选一）
   ├─ Stdout(io::Stdout)        → 你的终端
   ├─ Pager(Child)             → less 子进程的 stdin 管道
   └─ BuiltinPager(minus)      → bat 进程内的 minus 线程
   │
   ▼
OutputHandle::IoWrite / FmtWrite    ch5 的 write! 落到这里
```

主线：`Controller::run`（ch1 末尾交接过来的）第一步就是"决定输出目标"。
读完这一章，你就懂 `bat foo.rs` 为什么**有时**会进 less、
`echo hi | bat` 为什么**不**进、`--pager builtin` 又是啥。

`PagingMode`（`src/paging.rs`，就 7 行）是 ch2 那个派生字段的三态：

```rust
pub enum PagingMode {
    Always,            // 永远进 pager（--paging=always）
    QuitIfOneScreen,   // 内容 < 一屏就进（less -F：不滚动直接退）
    Never,            // 不进 pager，直接 stdout（--paging=never / 管道）
}
```

`Controller::run` 拿它**决定输出目标**，三种可能：

| 目标 | 什么时候 | 字节写进哪 |
|------|---------|-----------|
| `OutputType::Stdout` | `Never`，或分页但内容不够一屏 | 你的终端 stdout |
| `OutputType::Pager(Child)` | 起 `less` 等**外部** pager | 子进程 stdin 管道 |
| `OutputType::BuiltinPager` | `--pager=builtin` | bat 进程内的 `minus` 线程 |

**关键**：决定起不起 pager 在 `run_with_error_handler` 里
（controller.rs:57），**早于**逐行渲染（ch5）。
因为"写到哪"必须先定好，ch5 的 `write!` 才知道往哪写。

---

## 阶段 1：`run_with_error_handler` —— 决定输出目标

`src/controller.rs:47`。截取"选目标"那一段：

```rust
pub fn run_with_error_handler(
    &self,
    inputs: Vec<Input>,
    output_handle: Option<&mut OutputHandle<'_>>,   // 库调用方可自己指定（ch1 list-languages 用的 FmtWrite）
    mut handle_error: impl FnMut(&Error, &mut dyn Write),
) -> Result<bool> {
    // 只有"没指定输出 handle"时才需要自己选
    let mut output_type_opt: Option<OutputType> = None;

    #[cfg(feature = "paging")]
    if output_handle.is_none() {
        use crate::input::InputKind;
        use std::path::Path;

        // ★ 关键：一个输入文件都不存在时，别起 pager（直接打错误到 stdout）
        let mut paging_mode = self.config.paging_mode;
        if self.config.paging_mode != PagingMode::Never {
            let call_pager = inputs.iter().any(|input| {
                if let InputKind::OrdinaryFile(ref path) = input.kind {
                    Path::new(path).exists()
                } else {
                    true   // stdin / 自定义 reader 永远算"有输入"
                }
            });
            if !call_pager {
                paging_mode = PagingMode::Never;   // 强制降级
            }
        }

        let wrapping_mode = self.config.wrapping_mode;
        output_type_opt = Some(OutputType::from_mode(
            paging_mode, wrapping_mode, self.config.pager)?);   // ★ 真正的三选一
    }

    // 没开 paging feature → 永远 stdout
    #[cfg(not(feature = "paging"))]
    if output_handle.is_none() {
        output_type_opt = Some(OutputType::stdout());
    }

    // 算"输出是否绑到 pager"（影响错误写到哪）
    let attached_to_pager = match (&output_handle, &output_type_opt) {
        (Some(_), _) => true,                       // 库调用方给的 handle → 当 attached
        (None, Some(ot)) => ot.is_pager(),           // 我们选了 pager
        (None, None) => false,
    };

    // clircle 标识符：防"输入==输出"的 IO 圈（ch3 讲过）
    let stdout_identifier = if cfg!(windows) || attached_to_pager {
        None    // Windows 或已绑 pager → 跳过圈检测
    } else {
        clircle::Identifier::stdout()
    };

    // 拿到最终 writer
    let mut writer = match (output_handle, &mut output_type_opt) {
        (Some(OutputHandle::FmtWrite(w)), _) => OutputHandle::FmtWrite(w),
        (Some(OutputHandle::IoWrite(w)), _) => OutputHandle::IoWrite(w),
        (None, Some(ot)) => ot.handle()?,           // ★ OutputType → OutputHandle
        (None, None) => unreachable!("No output handle and no output type available"),
    };
    // …后面 for 循环逐个 input 调 print_input（ch5 那串），
    // 出错时按 attached_to_pager 决定错误写 stderr 还是写进 pager（见下）
    let mut no_errors: bool = true;
    let stderr = io::stderr();

    for (index, input) in inputs.into_iter().enumerate() {
        let identifier = stdout_identifier.as_ref();
        let is_first = index == 0;
        let result = if input.is_stdin() {
            self.print_input(input, &mut writer, io::stdin().lock(), identifier, is_first)
        } else {
            // ★ 非 stdin 输入传 io::empty() 当"伪 stdin"（#1902：stdin 其实没用）
            self.print_input(input, &mut writer, io::empty(), identifier, is_first)
        };
        if let Err(error) = result {
            match writer {
                // 库调用方给的 buffer → 直接返回错误（不写 stderr）
                OutputHandle::FmtWrite(_) => return Err(error),
                OutputHandle::IoWrite(ref mut writer) => {
                    if attached_to_pager {
                        handle_error(&error, writer);          // 写进 pager stdin
                    } else {
                        handle_error(&error, &mut stderr.lock()); // 写终端 stderr
                    }
                }
            }
            no_errors = false;
        }
    }
    Ok(no_errors)   // 全成功 → true；有文件读不到 → false（ch1 main 的退出码）
}
```

**小白会问：为什么错误有时写进 pager、有时写 stderr？**
`attached_to_pager` 时，bat 的**整个输出都进了 less**，
错误也得进 pager 才能"看到"（否则 less 满屏内容，错误却打到 less 背后看不见了）。
非 pager 时错误就正常打 stderr。

**`io::empty()` 那行（#1902）的坑**：
非 stdin 输入（普通文件）根本没用到"stdin 参数"——
但函数签名要求传个 `BufRead`。以前传 `io::stdin().lock()` 会**意外**把
真实 stdin 占用掉（比如 bat 内部读 stdin 时用户还在敲东西）。
现在统一传 `io::empty()`（永远读不到字节），既满足类型又不副作用。

---

## 阶段 2：`OutputType::from_mode` —— 三选一的核心

`src/output.rs:76`：

```rust
pub fn from_mode(
    paging_mode: PagingMode,
    wrapping_mode: WrappingMode,
    pager: Option<&str>,   // ch2 的 config.pager（--pager 指定）
) -> Result<Self> {
    use self::PagingMode::*;
    Ok(match paging_mode {
        // 永远分页 → 起 pager（一屏也起）
        Always => OutputType::try_pager(SingleScreenAction::Nothing, wrapping_mode, pager)?,
        // 一屏就退 → 起 pager（但 less 加 -F：不滚动就退）
        QuitIfOneScreen => OutputType::try_pager(SingleScreenAction::Quit, wrapping_mode, pager)?,
        // 不分页 → stdout
        _ => OutputType::stdout(),
    })
}
```

`try_pager`（output.rs:93）是真正"起子进程"的地方，**最绕**，分段讲：

```rust
fn try_pager(single_screen_action, wrapping_mode, pager_from_config) -> Result<Self> {
    use crate::pager::{self, PagerKind, PagerSource};
    use std::process::{Command, Stdio};

    // 1) 选 pager（pager.rs，见阶段 3）：config > BAT_PAGER > PAGER > "less"
    let pager_opt = pager::get_pager(pager_from_config)
        .map_err(|_| "Could not parse pager command.")?;
    let pager = match pager_opt {
        Some(pager) => pager,
        None => return Ok(OutputType::stdout()),   // 没指定 → 直接 stdout
    };

    // 2) 特例：pager 是 bat 自己 → 禁止（防递归：bat 套 bat 套 bat ...）
    if pager.kind == PagerKind::Bat {
        return Err(Error::InvalidPagerValueBat);
    }

    // 3) 特例：pager 是 "builtin" → 用内嵌 minus（不起子进程，起线程）
    if pager.kind == PagerKind::Builtin {
        return Ok(OutputType::BuiltinPager(BuiltinPager::new()));
    }

    // 4) 解析出可执行文件路径
    let resolved_path = match grep_cli::resolve_binary(&pager.bin) {
        Ok(path) => path,
        Err(_) => {
            crate::bat_warning!("Pager '{}' not found, outputting to stdout instead", pager.bin);
            return Ok(OutputType::stdout());   // 找不到 less → 退回 stdout（不崩）
        }
    };

    let mut p = Command::new(resolved_path);
    let args = pager.args;

    // 5) 如果是 less → 给它加一堆"让 bat 的输出好看"的参数
    if pager.kind == PagerKind::Less {
        // 只有从"通用 PAGER 环境变量"来的才允许覆盖参数
        //（BAT_PAGER / --pager 是用户明确指定的，别动）
        let replace_arguments_to_less = pager.source == PagerSource::EnvVarPager;

        if args.is_empty() || replace_arguments_to_less {
            p.arg("-R");   // --RAW-CONTROL-CHARS：让 less 认 ANSI 颜色
            if single_screen_action == SingleScreenAction::Quit {
                p.arg("-F");  // --quit-if-one-screen：不滚动直接退
            }
            if wrapping_mode == WrappingMode::NoWrapping(true) {
                p.arg("-S");  // --chop-long-lines：长行不折
            }
            // less 版本探测（less.rs）：老版本加 --no-init，BusyBox 版不加 -K
            let less_version = retrieve_less_version(&pager.bin);
            if less_version != Some(LessVersion::BusyBox) {
                p.arg("-K");  // --quit-on-intr：Ctrl-C 时 less 和 bat 一起退
            }
            if single_screen_action == SingleScreenAction::Quit {
                match less_version {
                    None => p.arg("--no-init"),
                    Some(LessVersion::Less(v)) if (v < 530 || (cfg!(windows) && v < 558)) => p.arg("--no-init"),
                    _ => {}
                }
            }
        } else {
            p.args(args);   // 用户自带 less 参数 → 原样用
        }
        p.env("LESSCHARSET", "UTF-8");
        #[cfg(feature = "lessopen")]
        p.arg("--no-lessopen");   // 防止 less 又调 $LESSOPEN 预处理一遍
    } else {
        p.args(args);   // 非 less 的 pager：参数原样
    }

    // 6) 起子进程，stdin 管道化（bat 输出通过管道喂给 pager）
    Ok(p.stdin(Stdio::piped())
        .spawn()
        .map(OutputType::Pager)
        .unwrap_or_else(|_| {
            crate::bat_warning!("Pager '{}' not found, outputting to stdout instead", &pager.bin);
            OutputType::stdout()   // spawn 失败也退回 stdout（鲁棒）
        }))
}
```

**`-R -F -S -K --no-init` 这串参数的小白解释**（最常问）：
- `-R`：less 默认把 ANSI 颜色码**原样显示**成乱码（`\x1b[31m` 那种）。
  `-R`（= `--RAW-CONTROL-CHARS`）告诉 less "这些是真颜色，渲染出来"。
  这是 bat 配色能进 less 的关键。
- `-F`（`QuitIfOneScreen` 才加）：内容 < 一屏时**直接退出**，不等你按 q。
  这就是 `echo short | bat` 不会卡 less 的原因。
- `-S`：长行**截断**而非折行（`--chop-long-lines`）。
- `-K`：按 Ctrl-C 时 less **跟着 bat 一起退**（不然 less 卡住 bat 退了 less 还挂着）。
- `--no-init`：老版 less 的 `--quit-if-one-screen` 有 bug，要加它绕；
  新版（530+）不需要，且加了会**破坏鼠标滚轮**，所以版本判断才决定加不加。

`LessVersion`（less.rs）就是干这个探测的：
```rust
pub enum LessVersion { Less(usize), BusyBox }

pub fn retrieve_less_version(less_path: &dyn AsRef<OsStr>) -> Option<LessVersion> {
    let resolved_path = grep_cli::resolve_binary(less_path.as_ref()).ok()?;
    let cmd = Command::new(resolved_path).arg("--version").output().ok()?;
    if cmd.status.success() {
        parse_less_version(&cmd.stdout)         // 正常 less：解析 "less 551 ..."
    } else {
        parse_less_version_busybox(&cmd.stderr) // BusyBox：解析 stderr 里的 "BusyBox v..."
    }
}
```
**小白会问：为什么 BusyBox 的 `--version` 是失败？**
BusyBox 的 less 是个多命令复用程序，`--version` 它不认（报 unrecognized option 到 stderr），
但报错文本里有 `BusyBox v1.35.0`。bat 据此识别"这是 BusyBox"，
就不加 `-K`（BusyBox 版不支持）。这是为了在**嵌入式/精简系统**上也能工作。

---

## 阶段 3：`pager.rs` —— 到底用哪个 pager

`src/pager.rs:100`：

```rust
pub(crate) fn get_pager(config_pager: Option<&str>) -> Result<Option<Pager>, ParseError> {
    let bat_pager = env::var("BAT_PAGER");
    let pager = env::var("PAGER");

    // 优先级：--pager(config) > BAT_PAGER > PAGER > 默认 "less"
    let (cmd, source) = match (config_pager, &bat_pager, &pager) {
        (Some(config_pager), _, _) => (config_pager, PagerSource::Config),
        (_, Ok(bat_pager), _) => (bat_pager.as_str(), PagerSource::EnvVarBatPager),
        (_, _, Ok(pager)) => (pager.as_str(), PagerSource::EnvVarPager),
        _ => ("less", PagerSource::Default),
    };

    // "less -R -F" 这种带参数的字符串 → 按 shell 词法拆开
    let parts = shell_words::split(cmd)?;
    match parts.split_first() {
        Some((bin, args)) => {
            let kind = PagerKind::from_bin(bin);   // less / more / most / bat / builtin / unknown
            // ★ 特例：从"通用 PAGER"来的 more/most/bat → 偷偷换成 less
            let use_less_instead = if source == PagerSource::EnvVarPager {
                matches!(kind, PagerKind::More | PagerKind::Most | PagerKind::Bat)
            } else { false };

            Ok(Some(if use_less_instead {
                Pager::new("less", &[], PagerKind::Less, PagerSource::EnvVarPager)
            } else {
                Pager::new(bin, args, kind, source)
            }))
        }
        None => Ok(None),
    }
}
```

**`more`/`most` 为什么换成 less？**
（pager.rs:117 注释）：`more` 和 `most` **不支持 ANSI 颜色**，
你的 `PAGER=more` 会让 bat 的颜色全废。所以"从通用 PAGER 来的"
（用户没明确指定 pager）时，bat 偷偷换成 `less` 保住颜色。
但 `BAT_PAGER=more`（用户明确要）就不许动。

`PagerKind::from_bin`（pager.rs:43）：
```rust
fn from_bin(bin: &str) -> PagerKind {
    if bin == "builtin" { return PagerKind::Builtin; }
    let pager_bin = Path::new(bin).file_stem();   // 取可执行文件名（剥路径/后缀）
    let current_bin = env::args_os().next();        // 当前跑的 bat 自己
    let is_current_bin_pager = current_bin
        .map(|s| Path::new(&s).file_stem() == pager_bin).unwrap_or(false);
    match pager_bin.map(|s| s.to_string_lossy()).as_deref() {
        Some("less") => PagerKind::Less,
        Some("more") => PagerKind::More,
        Some("most") => PagerKind::Most,
        _ if is_current_bin_pager => PagerKind::Bat,   // pager 就是 bat 自己 → 递归！
        _ => PagerKind::Unknown,
    }
}
```
`PagerKind::Bat` 就是"pager 设成了 bat 本身"——bat 套 bat 套 bat 永远递归，
`try_pager` 阶段 2 的 `InvalidPagerValueBat` 直接拒绝。

---

## 阶段 4：`BuiltinPager` —— 内嵌 minus 是怎么跑的

`src/output.rs:23`。`--pager=builtin` 时**不起子进程**，在 bat 进程内
起一个 `minus`（Rust 写的 pager 库）线程：

```rust
#[cfg(feature = "paging")]
pub struct BuiltinPager {
    pager: minus::Pager,
    handle: Option<JoinHandle<Result<()>>>,
}

#[cfg(feature = "paging")]
impl BuiltinPager {
    fn new() -> Self {
        let pager = minus::Pager::new();

        // 给 minus 注册"home"键=跳顶、"end"键=跳底
        let mut input_register = minus::input::HashedEventRegister::default();
        input_register.add_key_events(&["home"], |_, _| {
            minus::input::InputEvent::UpdateUpperMark(0)
        });
        input_register.add_key_events(&["end"], |_, _| {
            minus::input::InputEvent::UpdateUpperMark(usize::MAX)
        });
        pager.set_input_classifier(Box::new(input_register))
            .expect("failed to set input classifier on newly created pager");

        // 起线程跑 minus 的"动态分页"（边收 bat 喂的字节边渲染）
        let handle = {
            let pager = pager.clone();
            Some(spawn(move || {
                minus::dynamic_paging(pager).map_err(Error::from)
            }))
        };
        Self { pager, handle }
    }
}
```

**小白会问：minus 是什么，为什么 bat 要内嵌一个 pager？**
`minus` 是 Bat 作者（sharkdp）自己写的 Rust pager（less 的替代品，支持颜色/搜索/动态输出）。
`--pager=builtin` 让 bat **不依赖系统 less**，在自己进程内分页。
好处：纯 Rust、跨平台一致、能直接"动态分页"（文件边生成边翻页，
适合 `tail -f | bat -u --pager=builtin` 这种流式场景）。
代价：占两个线程（bat 主线程喂字节 + minus 线程渲染）。

`Drop`（output.rs:233）确保 less 子进程 / minus 线程**随 OutputType 一起清理**：
```rust
impl Drop for OutputType {
    fn drop(&mut self) {
        match *self {
            OutputType::Pager(ref mut command) => { let _ = command.wait(); }  // 等 less 退
            OutputType::BuiltinPager(ref mut pager) => {
                if let Some(handle) = pager.handle.take() { let _ = handle.join(); }  // 等 minus 线程
            }
            OutputType::Stdout(_) => (),
        }
    }
}
```

---

## 阶段 5：`OutputHandle` —— ch5 写的是"谁"

`src/output.rs:249`。ch5 的 `write!(handle, ...)` 写进这个枚举：

```rust
pub enum OutputHandle<'a> {
    IoWrite(&'a mut dyn io::Write),    // 真 IO（stdout / less 子进程 stdin）
    FmtWrite(&'a mut dyn fmt::Write),  // 内存 buffer（库调用方，如 ch1 list-languages）
}

impl OutputHandle<'_> {
    pub fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> Result<()> {
        match self {
            Self::IoWrite(handle) => handle.write_fmt(args).map_err(Into::into),
            Self::FmtWrite(handle) => handle.write_fmt(args).map_err(Into::into),
        }
    }
    pub fn flush(&mut self) -> Result<()> {
        match self {
            Self::IoWrite(handle) => handle.flush().map_err(Into::into),
            Self::FmtWrite(_) => Ok(()),   // buffer 不用 flush
        }
    }
}
```

`OutputType::handle()`（output.rs:216）把"目标"变成 `OutputHandle`：
```rust
pub fn handle<'a>(&'a mut self) -> Result<OutputHandle<'a>> {
    Ok(match *self {
        OutputType::Pager(ref mut command) =>
            OutputHandle::IoWrite(command.stdin.as_mut()
                .ok_or("Could not open stdin for pager")?),   // less 子进程 stdin
        OutputType::BuiltinPager(ref mut pager) =>
            OutputHandle::FmtWrite(&mut pager.pager),          // minus 的 Write 实现
        OutputType::Stdout(ref mut handle) =>
            OutputHandle::IoWrite(handle),                       // 普通 stdout
    })
}
```

**`FmtWrite` vs `IoWrite` 的关键差别**（小白易混）：
- `IoWrite`：底层是**文件/管道**（less 的 stdin、你的终端）。
  `flush` 是真 flush。ch5 的 `-u` 模式每行 `writer.flush()`（controller.rs:343）
  对它**立刻可见**（流式）。
- `FmtWrite`：底层是**内存 buffer**（`String` / 库调用方给的 `dyn Write`）。
  不会真 flush（`flush` 返回 Ok），数据攒着等 `OutputType` 被消费。

---

## 本章涉及的 Rust 机制（小白需要补的知识点）

1. **`#[cfg(feature = "paging")]` 门控**：整章的 pager 逻辑都包在 feature 里
   （Cargo.toml 的 `paging` feature）。库调用方可以关掉 paging，
   bat 退化成"只能输出到 stdout"——和 ch1 的 feature 判断同款。
2. **`Drop` 实现 `OutputType`**（output.rs:233）：`less` 子进程 / `minus`
   线程必须**随 OutputType 一起清理**（`command.wait()` / `handle.join()`），
   否则 bat 退了子进程还挂着。`Drop` 是"离开作用域自动清理"的标准机制。
3. **`std::process::Child` + `Stdio::piped()`**：`less` 是子进程，
   `bat` 把结果写进它的 stdin 管道（`Child.stdin`）。
   `Spawn` 失败（less 不存在）时 `unwrap_or_else` 退回 stdout（鲁棒，不崩）。
4. **`thread::spawn` + `minus`**：`BuiltinPager` 在 bat 进程内起一个线程
   跑 minus（Rust 写的 pager 库）。`pager.clone()` 因为 minus 要在
   新线程里持有，而 `minus::Pager` 内部是 `Clone`（Arc 共享）。

## 检验问题

1. `PagingMode::QuitIfOneScreen` 比 `Always` 多了哪个 less 参数？那个参数干嘛的？
2. 为什么从"通用 PAGER 环境变量"来的 `more` 会被偷偷换成 `less`，
   而 `BAT_PAGER=more` 不会？
3. `--pager=builtin` 时起的是"子进程"还是"线程"？`minus::dynamic_paging` 为啥要 `clone` pager？
4. less 版本探测为什么要看 **stderr**？什么时候会去看 stderr？
5. `OutputHandle::IoWrite` 和 `FmtWrite` 的 `flush()` 行为差在哪？`-u` 流式模式依赖哪个？
6. `pager.kind == PagerKind::Bat` 为什么直接 `Err`？不 Err 会怎样？

## 下一步

**ch7 — 装饰与样式**：`Decoration` trait、`LineNumberDecoration` /
`GridBorderDecoration` / `LineChangesDecoration` 三个实现怎么算宽度、
`Colors` 怎么从主题抽色、`StyleComponent` 怎么被"展开"成一组开关。
最后一章 ch8 把 8 章串成一条线。
