# t05：主路⑦ 前半 —— run_controller → Controller → OutputType 三选一

> 六个边路分支钻完，正式进**主路⑦**（最常见的那条：读文件打印）。
> 本篇钻：`run_controller` 怎么把活交给 `Controller`，以及 `Controller` 开工前
> **先定输出去哪**（OutputType 三选一的每个叶子）。逐行读文件那半在 t06~t08。

## 你在执行树的哪个位置

```
run() →【主路⑦】run_controller            ← 入口
   ├─ assets_from_cache_or_binary         ★ 叶子（缓存 / 内置）
   ├─ Controller::new(config, &assets)
   └─ Controller::run → run_with_error_handler
        ├─【子分支】OutputType::from_mode  ★ 本篇钻完
        │    ├─ stdout
        │    ├─ Pager（less 子进程）
        │    └─ BuiltinPager（内嵌 minus 线程）
        │    + 特判叶子：无文件退 Never / 防 IO 圈 / 找不到 less 退回 stdout
        └─ for input { print_input } …    （t06~t08）
```

## 1. 入口：`run_controller`（main.rs:289）

```rust
fn run_controller(inputs: Vec<Input>, config: &Config, config_dir: &Path, cache_dir: &Path) -> Result<bool> {
    let assets = assets_from_cache_or_binary(config.use_custom_assets, config_dir, cache_dir)?;
    let controller = Controller::new(config, &assets);
    if config.paging_mode != PagingMode::Never && config.set_terminal_title {
        set_terminal_title_to(get_new_terminal_title(&inputs))?;   // 先刷终端标题
    }
    controller.run(inputs, None)        // None = 让 controller 自己定输出目标
}
```

三个要点：

- **`assets` 创建**：语法+主题资产本体诞生于此，后面全是 `&` 借用（§4 Rust 机制）。
- **终端标题前置**（main.rs:297-299）：要分页且 `--set-terminal-title` 时，
  在**起 pager 前**把终端标题刷成 `bat: foo.rs, bar.rs`（`get_new_terminal_title`，
  main.rs:278：把所有输入的描述拼一串）。这样 less 顶栏/终端标签页能显示文件名。
  标题字符先经 `sanitize_for_terminal`（main.rs:269）剥掉 OSC 控制字节，
  防止文件名里的转义序列把终端标题"截断或嵌套"。
- **`run(inputs, None)`**：`None` 表示"输出目标你自己定"——这就是 OutputType
  子分支的触发条件。t04 的 `list_themes` 传的是 `Some(handle)`（自己定），对比记住。

## 2. assets 叶子：`assets_from_cache_or_binary`（bin/bat/assets.rs:19）

```rust
pub fn assets_from_cache_or_binary(use_custom_assets, config_dir, cache_dir) -> Result<HighlightingAssets> {
    if use_custom_assets {
        ensure_cache_built(config_dir, cache_dir);        // 首次跑：自动建一次缓存（失败只警告）
    }
    if let Some(metadata) = AssetsMetadata::load_from_folder(cache_dir)? {
        if !metadata.is_compatible_with(crate_version!()) {
            return Err(format!("The binary caches ... are not compatible ... rebuild or clear ..."));
        }
    }
    let custom_assets = if use_custom_assets {
        HighlightingAssets::from_cache(cache_dir).ok()    // 读缓存
    } else {
        None
    };
    Ok(custom_assets.unwrap_or_else(HighlightingAssets::from_binary))  // 兜底内置
}
```

两来源 + 一个硬规则（t03 的 build 就是为这里服务的）：

1. **`from_cache`**：`bat cache --build` 产出的 `syntaxes.bin`/`themes.bin`（预编译，秒加载）。
2. **`from_binary`**：编译期 `include` 进 exe 的内置资产（t02 分支②同款管线），永远可用 → 兜底。
3. **版本不匹配 → 显式 `Err`**（assets.rs:29-41）：缓存里存的 `crate_version` 和当前
   二进制不一致就**报错并提示 rebuild/clear**，不静默降级到内置——
   "我 build 了自定义主题 bat 却偷偷用内置"会让人困惑，显式失败可预期（architecture.md 判断 2）。

> `use_custom_assets` 来自 `Config`（t01 讲过：`--no-custom-assets` 时 false）。
> 这就是 t01 那个字段真正落地的地方。

## 3. `Controller::new` 与 `run`（controller.rs:30, 39）

```rust
pub fn new<'a>(config: &'a Config, assets: &'a HighlightingAssets) -> Controller<'a> {
    Controller { config, assets,
        #[cfg(feature = "lessopen")] preprocessor: LessOpenPreprocessor::new().ok() }
}

pub fn run(&self, inputs: Vec<Input>, output_handle: Option<&mut OutputHandle<'_>>) -> Result<bool> {
    self.run_with_error_handler(inputs, output_handle, default_error_handler)
}
```

`Controller` 只存**两个引用**（`&'a Config`、`&'a HighlightingAssets`）——它不拥有
任何输入/资产，纯"调度器"。`run` 只是给 `run_with_error_handler` 填上
`default_error_handler`（t01 §1 见过它）再转调。

**Rust 机制 · 双引用绑同一 `'a`**：`Controller<'a>` 的 `'a` 同时约束
`config`（里面还有 `&'a str` 借用自 ArgMatches）和 `assets`。
读法："**Controller 活着的这段时间，它借用的两样东西都必须活着**"。
main.rs 里 `config`/`assets`/`controller` 同作用域、assets 活到 `run` 结束，全程合法。
你之前问的"生命周期到底在绑什么"——绑的就是**两个借用的最小公共寿命**。

## 4. 输出目标子分支：`run_with_error_handler` 的前半（controller.rs:47）

开工前**先定输出写去哪**。截取关键段：

```rust
let mut output_type_opt: Option<OutputType> = None;

#[cfg(feature = "paging")]
if output_handle.is_none() {                       // 只有"没被指定 handle"才自己定
    let mut paging_mode = self.config.paging_mode;
    if self.config.paging_mode != PagingMode::Never {
        let call_pager = inputs.iter().any(|i|
            if let InputKind::OrdinaryFile(p) = i.kind { Path::new(p).exists() } else { true });
        if !call_pager { paging_mode = PagingMode::Never; }   // ★ 特判叶子①
    }
    output_type_opt = Some(OutputType::from_mode(paging_mode, self.config.wrapping_mode, self.config.pager)?);
}

#[cfg(not(feature = "paging"))]                    // ★ 特判叶子②：没编译 paging
if output_handle.is_none() {
    output_type_opt = Some(OutputType::stdout());
}
```

两个特判叶子：
- **叶子①：一个输入文件都不存在 → 强制 `Never`**（controller.rs:63-75）：
  `bat not_exist.rs` 时不起 less（否则 less 起来却没内容可读，卡住）；错误直接打 stdout。
- **叶子②：feature 关 → 永远 stdout**（controller.rs:86-89）：编译期裁剪，
  没有 paging 代码就 `OutputType::stdout()`。

拿到最终 writer（controller.rs:103-108，`ot.handle()?`）后进入 §5 的逐文件循环。

## 5. OutputType 三选一：`from_mode`（src/output.rs:76）

```rust
pub fn from_mode(paging_mode, wrapping_mode, pager) -> Result<Self> {
    Ok(match paging_mode {
        Always          => try_pager(SingleScreenAction::Nothing, wrapping_mode, pager)?,
        QuitIfOneScreen => try_pager(SingleScreenAction::Quit,    wrapping_mode, pager)?,
        _              => OutputType::stdout(),        // Never → stdout
    })
}
```

`paging_mode` 三个值（t01 §2e 的 config() 算出来的）决定**要不要**起 pager：
`Never`=纯 stdout；`Always`=一屏也起；`QuitIfOneScreen`=起了但一屏就退（最常见）。
"起哪个"进 `try_pager`。

### 5a. 叶子：选 pager（`pager::get_pager`，src/pager.rs:100）

```rust
pub(crate) fn get_pager(config_pager: Option<&str>) -> Result<Option<Pager>, ParseError> {
    // 优先级：--pager(config) > BAT_PAGER > PAGER > 默认 "less"
}
```

### 5b. 叶子：pager 的特殊种类（output.rs:109-115）

拿到 `Pager` 后连闯三关：

1. **`PagerKind::Bat`**（pager 设成 bat 自己）→ `Err(InvalidPagerValueBat)`。
   防递归：`bat | bat | bat ...` 无限套娃。
2. **`PagerKind::Builtin`**（`--pager=builtin`）→ `OutputType::BuiltinPager(BuiltinPager::new())`。
   **不起子进程**，起的是内嵌的 `minus` 库线程（叶子见 §5d）。
3. **普通 pager（less 等）** → 解析可执行文件路径，找不到 → 警告并**退回 stdout**（不崩）。

### 5c. 叶子：less 的参数（output.rs:131-188）

pager 是 less 时，bat 按规则给它加参数（**只有来源是"通用 PAGER 环境变量"时才允许覆盖**，
`BAT_PAGER`/`--pager` 是用户明确指定的就别动）：

| 参数 | 作用 | 何时加 |
|---|---|---|
| `-R` | 让 less 渲染 ANSI 颜色（不加则颜色码变乱码） | 总是 |
| `-F` | 内容 < 一屏直接退（`echo short \| bat` 不卡 less） | `QuitIfOneScreen` |
| `-S` | 长行截断不折行 | `NoWrapping(true)` |
| `-K` | Ctrl-C 时 less 跟 bat 一起退 | 非 BusyBox 版 |
| `--no-init` | 老版 less 的 `-F` 有 bug，靠它绕 | `QuitIfOneScreen` 且 less 版本 < 530 |

less 版本探测在 `src/less.rs`：`retrieve_less_version` 跑一次 `less --version`，
正常 less 从 stdout 解析版本号，**BusyBox 版**的 `--version` 是失败的（报 unrecognized，
但 stderr 里有 `BusyBox v1.35.0`）→ 识别为 BusyBox，就不加 `-K`（它不支持）。
这是为了在嵌入式/精简系统上也能跑。

### 5d. 叶子：起子进程（output.rs:190-199）

```rust
Ok(p.stdin(Stdio::piped()).spawn()
    .map(OutputType::Pager)
    .unwrap_or_else(|_| {
        bat_warning!("Pager '{}' not found, outputting to stdout instead");
        OutputType::stdout()      // spawn 失败也退回 stdout（鲁棒，不崩）
    }))
```

bat 的输出走**管道**喂给 pager 的 stdin。另有一个隐藏叶子——**防 IO 圈**
（controller.rs:97-101 + input.rs 读文件时）：如果"输入文件 == stdout 重定向到的那个文件"
（比如 `bat foo.rs > foo.rs`），直接写自己会边读边写死循环。用 `clircle` 抓 stdout 的
`Identifier` 传给输入层，发现读的就是 stdout 指向的文件就报错。
Windows 或已绑 pager 时跳过此检测（`cfg!(windows) || attached_to_pager`，controller.rs:97-101）。

## 6. 逐文件循环 + 错误去向（controller.rs:112-136）

输出目标定了，进入 `for input in inputs`，逐个 `print_input`（t06 钻）。
**每个 input 出错时的错误去向**是个独立叶子：

```rust
if let Err(error) = result {
    match writer {
        OutputHandle::FmtWrite(_) => return Err(error),          // 库调用方给的 buffer → 直接上抛
        OutputHandle::IoWrite(w) => {
            if attached_to_pager { handle_error(&error, w); }    // 输出在 pager 里 → 错误也写进 pager
            else { handle_error(&error, &mut stderr.lock()); }  // 否则写终端 stderr
        }
    }
    no_errors = false;
}
```

**为什么错误有时进 pager**（最常问）：`attached_to_pager` 时 bat 的**全部输出都在 less 里**，
错误若打到 less 背后的 stderr 就看不见了（less 占满终端），所以把错误也写进 pager 的
stdin 让用户"在 less 里看到"。`no_errors` 累积成 `Ok(false)`/`Ok(true)`，
最终决定 t01 的退出码。

**Rust 机制 · 泛型参数 `impl FnMut`**（controller.rs:51）：
`handle_error: impl FnMut(&Error, &mut dyn Write)`——把"错误怎么处理"当成
**参数**传进来（库调用方可自定义），`FnMut` 允许处理函数带可变状态
（比如记"第几次出错"）。这是 Rust 的"**依赖倒置**"惯用法：Controller 不自己决定
怎么打印错误，而是接受一个回调。`run()`（§3）传的是 `default_error_handler`。

## 检验问题

1. `bat not_exist.rs` 会起 less 吗？哪一行特判拦住了它（`paging_mode` 被改成什么）？
2. `--pager=builtin` 和 `--pager=less` 在 `try_pager` 里各走哪个 `PagerKind` 分支？
   一个起子进程、一个起线程，分别在哪个叶子？
3. 为什么"错误有时写进 pager 的 stdin、有时写终端 stderr"？由哪个变量（`attached_to_pager`）决定？
4. `Controller<'a>` 的两个字段都是引用。`'a` 约束的是什么？main.rs 里为什么全程合法？

## 下一站

输出目标定好，现在**读第一个文件**：`print_input` 怎么打开输入（编码嗅探全分支）→ **t06**。
