# t01：main() 与 run() 分派表 —— 执行树的根

> DFS 文档系列第 1 篇。约定：从 `main()` 出发，每到一个分支就钻到叶子讲清楚，
> 讲完明确写"返回父分支，继续"。每篇开头有"你在树的哪里"定位图。

## 你在执行树的哪个位置

```
main()  ← 你在这里
└─ run()
   ├─ 【分支①】--diagnostic
   ├─ 【分支②】--completion
   ├─ 【分支③】cache 子命令
   ├─ 【分支④】--list-languages
   ├─ 【分支⑤】--list-themes
   ├─ 【分支⑥】五个小 flag
   └─ 【主路⑦】run_controller
```

本篇讲：树怎么长出来的——`main()`、`run()` 前置（App::new + 三级配置合并）、分派表本身。
各分支的内部钻取在 t02~t09，本篇只走到"站在分派表前"为止。

## 1. `main()` —— 入口，只定退出码

`src/bin/bat/main.rs:471`：

```rust
fn main() {
    let result = run();          // 所有活都在 run() 里干，返回 Result<bool>
    match result {
        Err(error) => {
            let stderr = std::io::stderr();
            default_error_handler(&error, &mut stderr.lock());  // 打印 [bat error]: ...
            process::exit(1);
        }
        Ok(false) => process::exit(1),   // 中间有错但跑完了（"半成功"）
        Ok(true)  => process::exit(0),   // 全成功
    }
}
```

三个退出态（main.rs:474-485）：

| `run()` 返回 | 含义 | 退出码 |
|---|---|---|
| `Ok(true)` | 每个输入都成功 | 0 |
| `Ok(false)` | 有输入出错但打印了警告继续跑完 | 1 |
| `Err(e)` | 致命错误，提前终止 | 打印后 1 |

为什么是 `Result<bool>` 不是 `Result<()>`：`bat a.rs b_missing.rs c.rs` 里 b 不存在——
程序没崩（错误打到 stderr），但"不是全成功"。`Ok(false)` 让 shell 脚本能用
`if bat x.rs; then` 精确判断"每个文件都打出来了吗"。

> `default_error_handler` 在**库**侧 `src/error.rs:67`（经 main.rs:38 `use bat::error::*` 引入）：
> BrokenPipe（下游管道方提前退出，比如 `bat | head`）直接 `exit(0)`——被 head 截断不算 bat 的错；
> 其余错误打印 `red "[bat error]"` 前缀 + 错误消息。

## 2. `run()` 前置：`App::new()` 备齐三样东西

`main.rs:389`：

```rust
fn run() -> Result<bool> {
    let app = App::new()?;                 // ① 解析出"这次要干什么"
    let config_dir = PROJECT_DIRS.config_dir();  // ② 用户配置目录（~/.config/bat）
    let cache_dir  = PROJECT_DIRS.cache_dir();   // ③ 语法/主题缓存目录
    // ↓ 下面是分派表（§4）
```

`App::new()`（`src/bin/bat/app.rs:69`）做四件事，按顺序：

### 2a. 探测交互性（app.rs:73）

```rust
let interactive_output = std::io::stdout().is_terminal();
```

stdout 接到终端 → `true`；被管道（`bat | grep`）→ `false`。
这一个值后面决定了**上色、分页、loop_through**一大串行为，是整个 CLI 的第一个分岔点。

### 2b. 双份解析：区分"命令行敲的" vs "配置给的"（app.rs:78-80）

```rust
let cli_matches = Self::cli_matches(interactive_output);   // 只解析原始命令行
let number_from_cli = cli_matches.get_flag("number");          // -n 是敲的还是配的？
let number_nonblank_from_cli = cli_matches.get_flag("number-nonblank");
```

为什么要单独再解析一遍"纯命令行"：管道场景下，配置文件里的 `-n` 不该生效
（`cat -n | 管道` 的语义是"透传"），只有**用户亲自在命令行敲的** `-n` 才强制交互渲染。
这两个派生字段后面进 `Config.loop_through`（t01 §2c 会看到）和 `forced_style_components`。

### 2c. 三级配置合并（app.rs:199）

`App::matches()` 把**命令行 > 环境变量 > 配置文件**拍平成**一份参数列表**再交给 clap：

```rust
// 正常路径（app.rs:220-244）
let mut cli_args = wild::args_os();        // ③ 命令行参数（最后放 → 优先级最高）

// ① 配置文件（BAT_OPTS 环境变量优先于配置文件，见 config.rs）
let config_args = match get_args_from_env_opts_var() {
    Some(result) => result,                 // BAT_OPTS 存在 → 用它（模拟一串假 CLI 参数）
    None => get_args_from_config_file(),    // 否则读系统+用户两级 bat.toml
};
let mut args = /* ... */;                  // args = 配置文件参数

args.extend(get_args_from_env_vars());      // ② BAT_THEME 等 8 个环境变量盖在配置上
args.insert(0, cli_args.next().unwrap());   // 程序名放最前
cli_args.for_each(|a| args.push(a));        // ③ 命令行参数放最后（clap 后出现的覆盖先出现的）
```

**机制关键**：三层参数在解析前就被拍平成一条 `Vec<OsString>`，clap 根本不知道
哪段来自配置文件哪段来自命令行——它只看"后出现的覆盖先出现的"（clap 的
`overrides_with` 语义）。这就是"三级覆盖"的全部实现，没有任何"优先级表"数据结构。

配置文件怎么变成参数（`src/bin/bat/config.rs`）：
- `get_args_from_config_file()`：读系统级 `/etc/bat/config` + 用户级 `~/.config/bat/config`
  两个文件，拼成一段文本，按行切、跳过 `#` 注释、`shell_words::split` 处理引号
  （所以 `theme = "Sublime Snazzy"` 带空格也能正确切）。
- `get_args_from_env_vars()`（config.rs:155）：8 个 `BAT_*` 环境变量逐个映射成
  `--flag=value` 形式（`BAT_THEME` → `--theme=值`），等号形式保证 shell 词法不会再切。
- `BAT_OPTS`：整个环境变量里塞"一长串假命令行参数"，`shell_words` 解析后直接当配置层用——
  是"不想改配置文件也不想 export 八个变量"的偷懒写法。

两个特判短路（app.rs:207-218）：
- `bat cache ...`：子命令**完全不读**配置和环境变量（`cache --build` 就是纯工具操作）
- `--no-config` / `--diagnostic`：跳过配置文件，只用环境变量+命令行

### 2d. 帮短路（app.rs:84-116）

用户敲 `-h/--help`：不直接用 clap 打印，而是**用 bat 自己**渲染帮助文本
（`display_help`，app.rs:126：把 help 文本包成 `Input::from_reader`，造一个 plain 样式的
`Config`，走 `Controller` 高亮+分页打印，然后 `exit(0)`）。
——这是全项目**第一处**提前看到 Controller 的用武之地，但它是独立闭环，不进分派表。

**Rust 机制 · trait 对象初见面**：`display_help` 里的 `Input::from_reader(Box::new(...))`，
`Box<dyn Read>`（`Box` 里装一个 `dyn Read`，即"实现了 Read trait 的、大小未知的某个类型"）。
为什么用 `Box<dyn Read>` 而不是泛型 `R: Read`：`Input` 要**存**这个 reader 当字段跨函数用，
`Box` 把"运行时才知道大小"的 trait 对象装进堆上，字段大小固定。这是后面 t05/t06
到处出现的 `Box<dyn X>` 的第一课：**"存"用 Box<dyn>，"传"用泛型，两者可互换**。

### 2e. 派生字段：`loop_through` 的"或非"（app.rs:428-437）

`config()` 里最容易懵的一个字段，先在这立住（它决定 t05 选哪个 Printer）：

```rust
loop_through: !(self.interactive_output
    || color == Some("always")
    || decorations == Some("always")
    || self.matches.get_flag("force-colorization")   // -f
    || self.number_from_cli                           // 命令行敲的 -n
    || self.number_nonblank_from_cli),                // 命令行敲的 -b
```

**六条全不成立才 true**（纯文本批处理模式），任何一条成立 → false（交互渲染）。
语义：`bat *.rs` 一次读 20 个文件、或被管道，逐字算 ANSI 太吵太慢，就切
`SimplePrinter` 纯文本；但用户明确要了效果（-n/-f/-b/color=always）就不切。

**Rust 机制 · `#[derive(Default)]`**：`Config` 派生 `Default`，所以 `display_help`
里能写 `Config { style_components: ..., ..Default::default() }`——只填几个字段，
其余 24 个自动取默认值。没有它就得手写 28 行赋值。

## 3. 分派表：`run()` 的 7 个分支

`main.rs:394-467`，按代码顺序（DFS 先序）：

```
run()
├─【分支①】--diagnostic        调 invoke_bugreport 打印诊断 markdown，return（main.rs:394）
├─【分支②】--completion <shell> 打印编译期嵌入的补全脚本，return（main.rs:403）
├─【分支③】subcommand cache
│    ├─ 有子参数（--build/--clear）→ run_cache_subcommand，return（main.rs:420）
│    └─ 无子参数 → 特判：把当前目录名为 "cache" 的文件当普通文件读，走主路（main.rs:428）
└─【_ 分支】先 inputs() + config()（main.rs:436-437），再 if-else 串：
     ├─【分支④】--list-languages   生成语言表 → 塞回 run_controller 打印（main.rs:439）
     ├─【分支⑤】--list-themes      逐主题渲染预览（main.rs:446）
     ├─【分支⑥】--config-file / --generate-config-file / --config-dir /
     │            --cache-dir / --acknowledgements  各一行输出（main.rs:449-463）
     └─【主路⑦】run_controller     ★ 最常见：真正读文件打印（main.rs:465）
```

逐分支一句话（钻取在后续 t 文件）：

| 分支 | 干什么 | 钻到叶子在哪 |
|---|---|---|
| ① diagnostic | 收集系统/环境/配置/less 版本成 markdown 报告 | t02 |
| ② completion | `--completion bash\|zsh\|fish\|ps1`，打印补全脚本 | t02 |
| ③ cache | `cache --build` 重建语法/主题缓存；`--clear` 清除 | t03 |
| ④ list-languages | 列语言表（自己塞回主路打印，plain 模式） | t04 |
| ⑤ list-themes | 每个主题用 Controller 渲一段 Rust 预览 | t04 |
| ⑥ 五个小 flag | 打印路径/生成配置/致谢 | t02 |
| ⑦ 主路 | run_controller → Controller → 读/高亮/渲染/输出 | t05~t09 |

三个值得当场立住的**设计判断**（细节钻取留 t03/t04）：

1. **③ 无参特判**（main.rs:417-433）：`bat cache` 没带 `--build/--clear` 时，clap 会把
   当前目录恰好叫 `cache` 的文件当"要读的文件"处理——这是"子命令 vs 位置参数"的歧义消解。
2. **④ 塞回主路**（main.rs:439-445）：语言表本质是"一段文本"，`clone` 一份 config 改成
   plain 样式，包成 `Input::from_reader` 走 `run_controller`——**不维护第二套打印逻辑**。
   注意 `config.clone()`：`Config` 派生 `Clone`，28 字段全可克隆（`String`/`usize`/`Copy` 的
   `&str`），所以深拷贝便宜、语义干净。
3. **⑤ 递归点**（main.rs:446-448）：`list_themes` 内部**又造 Controller 又走 t05 的主路**
   （每个主题渲一次预览文件）——执行树在这里"自己套了一层自己"，t04 细讲。

## 3a. 公共前置：`app.config()` 构造 Config（28 字段，`_` 分支第一步）

分派表的 `_` 分支（main.rs:436-437）在走任何 flag 之前，先做两件事：
`app.inputs()`（文件参数 → `Vec<Input>`）+ `app.config(&inputs)`（matches → `Config`）。
**Config 是所有后续步骤的"参数总开关"**：t05~t08 每一步都从它取自己那几个字段。
`config()` 在 `app.rs:263`，赋值体 362-531，把 28 个字段**一次填好**，之后全程
`&Config` 只读（§4 Rust 机制 2 讲它的生命周期）。

`Config<'a>`（`src/config.rs:37`）28 字段，按用途分组（**加粗**的是"非显然"的，下面展开）：

| 组 | 字段（→ flag，默认） | 谁用到 |
|---|---|---|
| 语言 | `language`（-l，None=自动）、`fallback_syntax`（--fallback-syntax，None） | t07 选 syntax |
| 二进制/非打印 | `show_nonprintable`（-A）、`nonprintable_notation`、`binary`（--binary） | t06/t08 |
| 终端/tab | `term_width`、**`tab_width`（默认 4，特殊情况 0）** | t08 换行/tab |
| 派生 | **`loop_through`（"或非"六条，§2e）**、**`true_color`（读 COLORTERM）** | t05 选 Printer、t08 上色 |
| 风格 | `style_components`（--style/-n/-b/-p，§2e 的 forced 逻辑算出） | t07 装饰 |
| 换行/分页/可见行 | `wrapping_mode`（--wrap，auto 展开）、**`paging_mode`（--paging，auto 展开）**、**`visible_lines`（-r/-d，Ranges vs DiffContext）** | t05 输出、t08 渲染 |
| 主题/映射/pager | **`theme`（--theme，auto→按终端深浅）**、`syntax_mapping`（-m）、`pager`（--pager） | t07 主题、t05 输出 |
| 行级特性 | `use_italic_text`、`highlighted_lines`（-H）、`squeeze_lines`（-s） | t08 |
| 资产/标题/清洗 | `use_custom_assets`（--no-custom-assets 时 false）、`use_lessopen`(cfg)、`set_terminal_title`、`strip_ansi`/`sanitize`（--strip-ansi/--sanitize） | t05、t06、t08 |
| 开关 | `quiet_empty`（-E）、`unbuffered`（-u）、`number_nonblank`（-b） | t08 |

**非显然字段逐个讲**（读 t05~t08 前先把这几个立住）：

- **`tab_width` 何时变 0**（app.rs:438-449）：`--tabs` 没给时，
  `if style_components.plain() && paging_mode == Never { 0 } else { 4 }`。
  "plain + 不分页"（典型 `bat -p | 管道`）时 0 = **tab 原样透传不展开**；
  那个 0 在 t08 的 `preprocess`（printer.rs 底）里语义是"不展开直接加宽度"。
- **`paging_mode` 的 auto 展开**（app.rs:278-311，最绕）：`--paging=auto`（默认）时按
  "输入是不是 stdin、stdout 是不是交互终端"算出 `QuitIfOneScreen` 或 `Never`；
  `-pp`（plain 两次）或 `--no-paging` 强制 `Never`。t05 靠它决定"要不要起 less"。
- **`theme` 的 auto 解析**（app.rs:480）：`--theme` 没给时走 `theme(theme_options)`，
  按**终端当前深浅色**自动挑一个主题（深浅探测在 `src/theme.rs`，不展开）。
  `--theme-dark/--theme-light` 可分别指定。
- **`visible_lines` 两种模式**（app.rs:481-500，config.rs:11）：
  `Ranges`（`-r 30:40` 按行范围过滤，默认全显）vs `DiffContext`（`-d --diff-context N`，
  按 git 改动行展开成范围，git feature）。t06/t08 靠它决定"打哪些行"。
- **`strip_ansi` / `sanitize` 的 `Auto` 语义**（app.rs:450-475）：都是 `StripAnsiMode::{Never,Always,Auto}`。
  `Auto` = "**检测到纯文本就不清洗**"（纯文本里的转义序列可能是内容本身），
  "是不是纯文本"要在 t07 选出语法后才知道，所以这两个值在 `InteractivePrinter::new`
  才最终落实（t07 §6）。`sanitize` 是 `strip_ansi` 的超集（额外把光标移动/bidi/零宽字符换 U+FFFD）。
- **`quiet_empty`（-E）**（app.rs:476）：空输入时**啥都不打**（连 header 都没有），
  t08 的 `print_header` 第一行就靠它（`content_type.is_none() && quiet_empty` → return）。
- **`unbuffered`（-u）**（app.rs:477）：流式读（`tail -f log | bat -u`），
  t06 的 `read_line` 走 `read_line_unbuffered`；且 §2e 讲过 `-u` 会自动关行号。

**Rust 机制 · 哪些字段是借用**：`language`/`fallback_syntax`/`pager`/`syntax_mapping`
是 `&'a str`（借用自 `App.matches` 的字符串），其余是值类型（`String`/`usize`/`bool`/枚举）。
所以 `Config` 要带生命周期 `'a`（§4 展开）——它"活多久"受限于借用的那些 `&str`。

## 4. 返回分派表之前：Rust 机制小结

本区段反复出现的机制（后面 t 篇不再重复）：

1. **`?` 错误传播**：`App::new()?`、`app.inputs()?`、`app.config(&inputs)?` 一路 `?` 上抛，
   `main()` 统一接住定退出码。中间任何一个致命错（配置文件解析失败、参数非法）全在这里收口。
2. **生命周期 `'a` 的源头**：`app.config(&inputs)` 返回 `Config<'_>`——里面 `language`、
   `pager` 等字段是 `&'a str`，**借用自 `App.matches`（`ArgMatches`）里的字符串**，不拥有它们。
   链条：`ArgMatches` 活多久 → `Config` 里的引用就合法多久。后面 t05 的
   `Controller::new(config: &'a Config, assets: &'a HighlightingAssets)` 把两个引用绑到
   同一个 `'a`：Controller 活着的期间，Config 借用的字符串和 assets 本体都必须活着。
   main 里三者都在同一作用域，全程合法。
3. **`wild::args_os()`**：Windows 上 `std::env::args_os` 会把含引号的参数切错，
   `wild` 按 C runtime 同款规则切。所以 App 里反复调它（2b 双解析、2c 合并）而不是 `std` 的。

## 检验问题

1. `bat a.rs b_missing.rs c.rs`（b 不存在）退出码是几？为什么不是 0？
2. 配置文件写了 `theme = "Monokai"`，命令行又敲了 `--theme Dracula`，最终用哪个？
   靠什么机制（不是"优先级表"）实现的？
3. `loop_through` 六条全不成立才 true。`bat -n file.rs`（交互终端）它是几？主路选哪个 Printer？
4. 为什么 `--help` 不直接 `eprintln` 帮助文本，而要造 Config + Controller 跑一遍？

## 下一站

分派表前 6 个分支（①②③⑥）的叶子钻取 → **t02**；分支③ cache 的 build/clear 细节 → **t03**。
