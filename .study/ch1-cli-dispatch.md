# 第 1 章：命令行解析与 run 分派

这一章的目标：你敲一条 `bat ...` 命令后，程序**从启动到决定"干什么活"为止**的完整过程。
读完你应该能回答："我敲的这串参数，程序从入口到分派，每一步走了哪行代码、做了什么判断。"

## 涉及文件

- `src/bin/bat/main.rs` — 程序入口（`main`、`run`、`run_controller`）
- `src/bin/bat/app.rs` — `App`（解析参数、配置三级合并、`inputs`/`config`）
- `src/bin/bat/clap_app.rs` — 每个 flag 的定义（clap builder API）
- `src/bin/bat/config.rs` — 配置文件/环境变量怎么读成参数
- `src/input.rs`、`src/output.rs`、`src/controller.rs`（前段）— 交棒后发生什么

## 全景图（先有这个概念，再看代码）

```
bat -n --theme Monokai foo.rs
        │
        ▼
main()                              ← 程序入口（main.rs:465）
   └─ run()                        ← 解析 + 分派（main.rs:383）
        │
        ├─ App::new()             ← 把参数/配置/输入三样备齐（app.rs:69）
        │     └─ matches()        ← 三级配置合并（config 文件/环境变量/CLI）
        │
        ├─ 遇到 -h/--help          → 用 bat 自己渲染帮助后退出（app.rs:84）
        ├─ 遇到 list-languages     → 打语言表就退（main.rs:433）
        ├─ 遇到 list-themes        → 逐个渲染主题就退（main.rs:440）
        ├─ 遇到 cache 子命令       → 改缓存就退（main.rs:410）
        └─ 默认                    → run_controller()（main.rs:458）★ 最常见
                │
                ▼
        run_controller()          ← 选 assets、设终端标题、交给 Controller（main.rs:289）
                │
                └─ Controller::run()   ← 真正的活从这里开始（ch3+）
```

下面按"一个真实命令从头走到尾"的顺序，把每个阶段**带注释的完整代码**贴出来讲。

---

## 小白先建立一个地图

bat 是**单 crate** 项目（不像 ripgrep 拆十几个 crate），代码分两块：

| 块 | 文件 | 角色 |
|----|------|------|
| 可执行文件（bin） | `src/bin/bat/*` | 处理命令行、配置、分派——"程序怎么被调用" |
| 库（lib） | `src/*.rs` | 真正干活——打开文件、高亮、渲染、输出 |

`src/bin/bat/main.rs` 是入口。它**几乎不干活**，只做三件事：解析参数、判断走哪条路、
把活丢给 `bat` 库里的 `Controller`。下面按数据流一节一节走。

---

## 阶段 0：`main` —— 程序的总开关

`src/bin/bat/main.rs`

```rust
fn main() {
    let result = run();                       // 真正干活的是 run()，返回 Result<bool>
    match result {
        Err(error) => {
            let stderr = std::io::stderr();
            default_error_handler(&error, &mut stderr.lock());  // 打印 [bat error] ...
            process::exit(1);                 // 出错 → 退出码 1
        }
        Ok(false) => {                        // "中间出过错但处理完了"（比如某文件读不到）
            process::exit(1);                 // 也按失败退，退出码 1
        }
        Ok(true) => {
            process::exit(0);                 // 全成功 → 退出码 0
        }
    }
}
```

**小白会问：为什么 `run` 返回的是 `Result<bool>`，不是普通 `Result<()>`？**

那个 `bool` 区分"完全成功"和"有部分文件读不到但程序跑完了"。比如
`bat a.rs b_missing.rs c.rs`，其中 `b_missing.rs` 不存在——程序没崩，错误信息打到了
stderr，但整体算"半成功"，返回 `Ok(false)`，最后退出码给 1。这对脚本友好：脚本能用
退出码判断"是不是所有文件都打出来了"。

> `run` 的函数签名（main.rs:383）也标了同样的约定：
> `/// Returns Ok(true) on full success and Ok(false) if any intermediate errors occurred.`

---

## 阶段 1：`App::new()` —— 把参数、配置、输入三样东西备齐

`main.rs` 里 `run()` 第一行（main.rs:384）：

```rust
fn run() -> Result<bool> {
    let app = App::new()?;                    // 一次性解析出"这次到底要干什么"
    let config_dir = PROJECT_DIRS.config_dir();  // 用户配置目录（如 ~/.config/bat）
    let cache_dir  = PROJECT_DIRS.cache_dir();   // 语法/主题缓存目录

    // …后面全是 if 分支，决定走 list-languages / list-themes / 普通读取（见阶段 4）
}
```

`App` 是本章的主角（`src/bin/bat/app.rs:55`）。它的 `new()` 做三件大事。完整代码：

```rust
impl App {
    pub fn new() -> Result<Self> {
        #[cfg(windows)]
        let _ = nu_ansi_term::enable_ansi_support();  // Windows 上开 ANSI 颜色支持

        // 关键判定 1：stdout 是不是"交互式终端"。
        // 你在终端里敲 bat → true；管道 `bat | less` → false。
        // 这个值后面决定"要不要上色、要不要分页"。
        let interactive_output = std::io::stdout().is_terminal();

        // 关键判定 2：单独把"命令行上真的敲了的参数"再解析一遍。
        // 目的：区分 -n / -b 是用户在命令行敲的，还是配置文件/环境变量给的。
        // （后面管道输出时，只有"命令行明确要的"才会生效，见 App::config 的 loop_through）
        let cli_matches = Self::cli_matches(interactive_output);
        let number_from_cli = cli_matches.get_flag("number");
        let number_nonblank_from_cli = cli_matches.get_flag("number-nonblank");

        // 真正要用的 matches：合并了 配置文件 + 环境变量 + 命令行（详见阶段 2）
        let matches = Self::matches(interactive_output)?;

        // 用户敲了 -h / --help？那就先"用 bat 自己渲染帮助"再退出（不是直接打印）
        if matches.get_flag("help") {
            let help_type = if wild::args_os().any(|arg| arg == "--help") {
                HelpType::Long
            } else {
                HelpType::Short
            };
            // … 计算要不要分页、要不要上色（逻辑略，见 app.rs:91-104）
            Self::display_help(
                interactive_output, help_type, use_pager, use_color,
                pager, theme_options, use_custom_assets,
            )?;
            std::process::exit(0);            // 显示完帮助直接退出，不进后续流程
        }

        Ok(App {
            matches,
            interactive_output,
            number_from_cli,
            number_nonblank_from_cli,
        })
    }
}
```

**小白会问：bat 为什么连 `--help` 都要"渲染一遍"而不是直接打印文本？**

因为 bat 的本事就是"把文本变成带颜色的好看输出"。帮助文本也是文本，
让它**用自己的高亮 + 主题 + 分页**来显示（app.rs:146-175），
比 clap 自带的灰色纯文本一致得多。这就是为什么 bat 的帮助也是带颜色的。

---

## 阶段 2：`App::matches()` —— 配置三级合并

这是 bat 参数最绕的一段。**优先级从高到低**：命令行参数 > 环境变量 > 配置文件。
完整代码（app.rs:198）：

```rust
fn matches(interactive_output: bool) -> Result<ArgMatches> {
    // 1) 快速判断：用户敲的是 -V/--version/--diagnostic 吗？这些不需要配置。
    let should_skip_config = wild::args_os().any(|arg|
        matches!(arg.to_str(), Some("-V" | "--version" | "--diagnostic" | "--diagnostics")));

    // 2) 用户敲 -h/--help 吗？help 要读配置（好用上你配置的 theme），但容错要放宽
    let help_requested = wild::args_os().any(|arg|
        matches!(arg.to_str(), Some("-h" | "--help")));

    // 3) 子命令 `bat cache`：完全跳过配置和环境变量，只认命令行
    if wild::args_os().nth(1) == Some("cache".into()) {
        let args = wild::args_os().collect::<Vec<_>>();
        return Ok(clap_app::build_app(interactive_output).get_matches_from(args));
    }

    // 4) 显式 --no-config，或者上面 1) 命中（version/diagnostic）：只用 环境变量+命令行
    if wild::args_os().any(|arg| arg == "--no-config") || should_skip_config {
        let args = Self::build_args_without_config();   // 环境变量 + 命令行
        return Ok(clap_app::build_app(interactive_output).get_matches_from(args));
    }

    // 5) 正常路径：把三层参数拼成一个 Vec，再交给 clap 解析
    let mut cli_args = wild::args_os();                  // 命令行参数（iter）

    // 先读配置文件里的参数（BAT_OPTS 环境变量优先于配置文件的 shell_words 解析）
    let config_args = match get_args_from_env_opts_var() {
        Some(result) => result,                            // BAT_OPTS 存在 → 用它
        None => get_args_from_config_file(),               // 否则读 config 文件
    };

    // help 时配置文件解析失败也容忍（用空配置），非 help 则直接报错
    let mut args = if help_requested {
        config_args.unwrap_or_default()
    } else {
        config_args.map_err(|_| "Could not parse configuration file")?
    };

    // 环境变量（BAT_THEME、BAT_STYLE…）盖在配置之上
    args.extend(get_args_from_env_vars());

    // 程序名放最前，剩余命令行参数放最后（盖在一切之上）
    args.insert(0, cli_args.next().unwrap());
    cli_args.for_each(|a| args.push(a));

    // help 时用 try_ 解析，失败就退回"不带配置"再解析一次（容错）
    if help_requested {
        let app = clap_app::build_app(interactive_output);
        match app.try_get_matches_from(args) {
            Ok(matches) => Ok(matches),
            Err(_) => {
                let fallback_args = Self::build_args_without_config();
                Ok(clap_app::build_app(interactive_output).get_matches_from(fallback_args))
            }
        }
    } else {
        Ok(clap_app::build_app(interactive_output).get_matches_from(args))
    }
}
```

**小白会问：`config_args` 为什么能直接 `extend` 进 `args`？**

因为 `get_args_from_config_file` / `get_args_from_env_vars` 返回的也是
`Vec<std::ffi::OsString>`——和命令行参数**同一种类型**。三层参数在"解析前"
就被拍平成一条 `OsString` 列表，clap 不知道它们来自哪一层，只看到一长串参数，
后面出现的会覆盖（`overrides_with`）。这就是"命令行盖环境变量盖配置文件"的实现方式。

配置文件怎么被读成参数？看 `src/bin/bat/config.rs:106`：

```rust
pub fn get_args_from_config_file() -> Result<Vec<OsString>, shell_words::ParseError> {
    let mut config = String::new();
    let system_config = system_config_file();   // /etc/bat/config（系统级）
    let user_config = config_file();            // 用户级（BAT_CONFIG_PATH 或默认）
    if let Ok(c) = fs::read_to_string(&system_config) { config.push_str(&c); config.push('\n'); }
    if !same_file(&system_config, &user_config) {
        if let Ok(c) = fs::read_to_string(&user_config) { config.push_str(&c); }
    }
    get_args_from_str(&config)                  // 把整段文本按 shell 词法切成参数
}
```

`get_args_from_str`（config.rs:139）按行切、跳过空行和 `#` 注释、
再用 `shell_words::split` 处理引号（所以 `--theme "Sublime Snazzy"` 能带空格）。

环境变量映射（config.rs:155）——每个环境变量对应一个 flag：

```rust
pub fn get_args_from_env_vars() -> Vec<OsString> {
    [
        ("--tabs", "BAT_TABS"),
        ("--terminal-width", "BAT_WIDTH"),
        ("--theme", bat::theme::env::BAT_THEME),
        ("--theme-dark", bat::theme::env::BAT_THEME_DARK),
        ("--theme-light", bat::theme::env::BAT_THEME_LIGHT),
        ("--pager", "BAT_PAGER"),
        ("--paging", "BAT_PAGING"),
        ("--style", "BAT_STYLE"),
    ]
    .iter()
    .filter_map(|(flag, key)| env::var(key).ok().map(|var| [flag.to_string(), var].join("=")))
    .map(|a| a.into())
    .collect()
}
```

**小白会问：为什么环境变量是拼成 `--theme=TwoDark` 这种"等号形式"？**

因为后面要 `insert(0, 程序名)` 后整包丢给 clap，clap 接受 `--key=value` 形式，
拼成字符串最省事，也保证 `=` 不会被 shell 词法再切一刀。

---

## 阶段 3：`clap_app.rs` —— 每个 flag 在这里定义

上面所有"参数名"（`--number`、`--style`、`--theme`…）的**定义**都集中在
`src/bin/bat/clap_app.rs` 的 `build_app()`（830 行，本章不全贴，只讲关键几个）。
小白重点理解三类：

```rust
// 位置参数：文件名。num_args(1..) 表示至少要 1 个文件……但配合 stdin，0 个也行（从 - 读）
.arg(Arg::new("FILE").help("File(s) to print / concatenate. Use '-' for standard input.")
     .num_args(1..).value_parser(value_parser!(PathBuf)))

// 带计数的开关：-p 可以出现多次。Count 记录出现次数（1 次 / 2 次 = -pp）
.arg(Arg::new("plain").short('p').long("plain").action(ArgAction::Count))

// 带值的枚举：--color 只允许 auto/never/always，值非法直接报错
.arg(Arg::new("color").long("color").value_parser(["auto","never","always"])
     .default_value("auto"))
```

**小白会问：`ArgAction::Count` 和普通 `SetTrue` 差在哪？**

`-h`、`--diff` 这种"有/没有"的开关用 `SetTrue`（bool）。
但 `-p` 要区分"用了一次"还是"用了两次（-pp）"，所以用 `Count`——
`app.config()` 里会 `get_count("plain")` 拿到次数来决定要不要顺带关掉分页。

---

## 阶段 4：`App::config()` 和 `App::inputs()` —— 把参数翻译成"配置"和"输入"

`run()`（main.rs:430）里两个关键调用：

```rust
let inputs = app.inputs()?;        // 把文件名参数变成 Input 列表
let config = app.config(&inputs)?; // 把 matches 变成 Config 结构体（后面每章都用到）
```

### 4a. `inputs()` —— 文件参数 → Input 列表

完整代码（app.rs:540）：

```rust
pub fn inputs(&self) -> Result<Vec<Input<'_>>> {
    // -n 选项指定"显示名"（可多次，和 FILE 一一对应）
    let filenames: Option<Vec<&Path>> = self.matches
        .get_many::<PathBuf>("file-name").map(|vs| vs.map(|p| p.as_path()).collect());
    // 位置参数 FILE（真正的文件路径，'-' 表示 stdin）
    let files: Option<Vec<&Path>> = self.matches
        .get_many::<PathBuf>("FILE").map(|vs| vs.map(|p| p.as_path()).collect());

    // 两个列表都得要么都没有、要么等长（一个显示名配一个文件）
    if filenames.is_some() && files.is_some()
        && filenames.as_ref().map(|v| v.len()) != files.as_ref().map(|v| v.len()) {
        return Err("Must be one file name per input type.".into());
    }

    let mut filenames_or_none: Box<dyn Iterator<Item = Option<&Path>>> = match filenames {
        Some(filenames) => Box::new(filenames.into_iter().map(Some)),
        None => Box::new(std::iter::repeat(None)),      // 没指定显示名 → 一直 None
    };
    // 连一个文件都没给（也没 -n）：默认读 stdin
    if files.is_none() {
        return Ok(vec![new_stdin_input(filenames_or_none.next().unwrap_or(None))]);
    }
    let files_or_none: Box<dyn Iterator<Item = _>> = match files {
        Some(ref files) => Box::new(files.iter().map(|name| Some(*name))),
        None => Box::new(std::iter::repeat(None)),
    };

    // 把 文件 和 显示名 逐对 zip，生成 Input
    let mut file_input = Vec::new();
    for (filepath, provided_name) in files_or_none.zip(filenames_or_none) {
        if let Some(filepath) = filepath {
            if filepath.to_str().unwrap_or_default() == "-" {
                file_input.push(new_stdin_input(provided_name));   // 路径是 "-" → 当 stdin
            } else {
                file_input.push(new_file_input(filepath, provided_name));
            }
        }
    }
    Ok(file_input)
}
```

**小白会问：`'-'` 为什么代表 stdin？**

这是 Unix 传统（`cat -` 读 stdin）。bat 沿用：`echo hi | bat -` 就是把管道当文件读，
`provided_name`（用 `-n` 或 `--file-name` 指定）告诉 bat "把这个输入当成叫 X 的文件"，
这样高亮也能按 X 的扩展名选语言。

### 4b. `config()` —— matches → Config

这是 bat 最大的一坨 if/else（app.rs:269-537，268 行）。不逐行贴全部（太绕），
讲**小白最该记住的几个字段的来龙去脉**：

```rust
pub fn config(&self, inputs: &[Input]) -> Result<Config<'_>> {
    let style_components = self.style_components()?;   // 见下方专节

    // 分页模式 PagingMode：是本章最绕的逻辑，单独拎出来讲
    let paging_mode = match self.matches.get_one::<String>("paging").map(|s| s.as_str()) {
        Some("always") => { /* -pp 或第二个 -p 在后面则关掉，否则 Always */ }
        Some("never")  => PagingMode::Never,
        Some("auto") | None => {
            if extra_plain || self.matches.get_flag("no-paging") {
                PagingMode::Never
            } else if inputs.iter().any(Input::is_stdin) && !self.matches.get_flag("list-themes") {
                // 读 stdin：只有"输出是交互终端 且 输入不是交互终端"才分页
                if self.interactive_output && !std::io::stdin().is_terminal() {
                    PagingMode::QuitIfOneScreen
                } else { PagingMode::Never }
            } else if self.interactive_output {
                PagingMode::QuitIfOneScreen          // 交互终端、读普通文件 → 分页
            } else {
                PagingMode::Never                      // 输出被管道 → 不分页
            }
        }
        _ => unreachable!(),
    };

    Ok(Config {
        true_color: is_truecolor_terminal(),           // 终端支不支持 24bit 色（COLORTERM）
        language: /* -l 指定，或 -A 时变成 "show-nonprintable" */ ...,
        binary: /* --binary：as-text / no-printing */ ...,
        wrapping_mode: /* --wrap：character/word/never/auto */ ...,
        colored_output: /* -f 或 --color：要不要上色 */ ...,
        paging_mode,                                   // 上面算出来的
        term_width: maybe_term_width.unwrap_or(Term::stdout().size().1 as usize),
        loop_through: !(self.interactive_output
            || /* color=always / decorations=always / -f / -n / -b 任意一个成立就 false */),
        tab_width: /* --tabs，没给就 plain+不分页时 0，否则 4 */ ...,
        theme: theme(self.theme_options()).to_string(), // 解析出的主题名
        style_components,
        syntax_mapping,                                  // -m '*.build:Python' 收集到这
        pager: self.matches.get_one::<String>("pager").map(|s| s.as_str()),
        /* …还有二十来个字段，见 ch2 逐个讲 */
    })
}
```

**小白会问：`loop_through` 是啥，为什么这么多条件？**

它的含义是"批量处理模式"。正常你交互终端里看一个文件，bat 会用**交互式渲染**
（高亮、边框、行号）。但如果你是 `bat *.rs` 一次读 20 个文件、或输出被管道，
逐字重算 ANSI 颜色会很吵/很慢——`loop_through=true` 时 ch5 会换用
`SimplePrinter`（纯文本、不画装饰）。上面那一长串"否定条件"就是在问：
"用户有没有明确要交互效果（-f/-n/-b/color=always…）？有就别 loop_through。"

### 4c. `style_components()` —— `--style` 怎么变成一组开关

完整代码（app.rs:586-666）：

```rust
fn forced_style_components(&self) -> Option<StyleComponents> {
    // --decorations=never → 一个组件都不留（全关）
    if self.matches.get_one::<String>("decorations").map(|s| s.as_str()) == Some("never") {
        return Some(StyleComponents(HashSet::new()));
    }
    // -n → 只留行号
    if self.matches.get_flag("number") {
        return Some(StyleComponents(HashSet::from([StyleComponent::LineNumbers])));
    }
    // -b → 只留行号（但跳过空行）
    if self.matches.get_flag("number-nonblank") || self.number_nonblank_from_cli {
        return Some(StyleComponents(HashSet::from([StyleComponent::LineNumbers])));
    }
    // -p（至少一次）→ Plain；若同时开 --diff，保留 git 标记和 snip 线
    if self.matches.get_count("plain") > 0 {
        let mut components = HashSet::from([StyleComponent::Plain]);
        if self.matches.try_contains_id("diff").unwrap_or_default() && self.matches.get_flag("diff") {
            #[cfg(feature = "git")]
            components.insert(StyleComponent::Changes);
            components.insert(StyleComponent::Snip);
        }
        return Some(StyleComponents(components));
    }
    None   // 没强制 → 走下方 style_components() 的默认/--style 解析
}

fn style_components(&self) -> Result<StyleComponents> {
    let matches = &self.matches;
    let mut styled_components = match self.forced_style_components() {
        Some(forced_components) => forced_components,
        // 用户给了 --style → 按逗号切、支持 + / - 增减
        None if matches.contains_id("style") => {
            let lists = matches.get_many::<String>("style").expect("styles present")
                .map(|v| StyleComponentList::from_str(v)).collect::<Result<Vec<_>>>()?;
            StyleComponentList::to_components(lists, self.interactive_output, true)
        }
        // 啥都没给 → 用默认组件集合
        None => StyleComponents(HashSet::from_iter(
            StyleComponent::Default.components(self.interactive_output).iter().cloned())),
    };

    // grid 和 rule 冲突（rule 是 grid 的子集）→ 去掉 rule 并提示
    if styled_components.grid() && styled_components.0.remove(&StyleComponent::Rule) {
        bat_warning!("Style 'rule' is a subset of style 'grid', 'rule' will not be visible.");
    }
    // 非缓冲模式（-u 流式）下自动关掉行号（半行时行号会乱）
    if self.matches.get_flag("unbuffered") {
        styled_components.0.remove(&StyleComponent::LineNumbers);
    }
    Ok(styled_components)
}
```

**小白会问：`--style` 里 `+` / `-` 前缀是干嘛的？**

`StyleComponentList::from_str`（style.rs:222）会把每个组件按前缀记成
`Add` / `Remove` / `Override`。`--style="numbers,-grid,+header"` 就是
"加行号、去网格、加文件头"。多个 `--style` 按**从后往前**叠加，
后写的盖前面的（除非只写了 +/- 前缀，那样是"修改"而非"替换"）。

---

## 阶段 5：`run()` 的分派表 —— 输入什么 → 走哪条路

回到 `main.rs:409`。这是 bat 的"模式分派"核心，对照表（**小白拿这条当地图**）：

```rust
match app.matches.subcommand() {
    // 子命令 bat cache：构建/清除语法和主题缓存（见 main.rs:410）
    Some(("cache", cache_matches)) => {
        if cache_matches.args_present() {
            run_cache_subcommand(...)?;        // 有子参数 → 真的去改缓存
            Ok(true)
        } else {
            // 没参数（比如当前目录恰好有个叫 cache 的文件）→ 当普通文件读
            let inputs = vec![Input::ordinary_file("cache")];
            let config = app.config(&inputs)?;
            run_controller(inputs, &config, cache_dir)
        }
    }
    // 其余所有情况（普通读取）
    _ => {
        let inputs = app.inputs()?;
        let config = app.config(&inputs)?;

        // 下面是一长串 if-else，每个 --list-* / --config-* 自成一条路：
        if app.matches.get_flag("list-languages") {
            // 列出所有支持的语言 → 生成文本 → 当"一个输入"喂给 controller 打印
            let languages: String = get_languages(&config, cache_dir)?;
            let inputs: Vec<Input> = vec![Input::from_reader(Box::new(languages.as_bytes()))];
            let mut plain_config = config.clone();
            plain_config.style_components = StyleComponents::new(StyleComponent::Plain.components(false));
            run_controller(inputs, &plain_config, cache_dir)   // 用纯文本打印语言表
        }
        else if app.matches.get_flag("list-themes") {
            list_themes(&config, config_dir, cache_dir, app.theme_options())?;
            Ok(true)
        }
        else if app.matches.get_flag("config-file")  { /* 打印配置路径 */ }
        else if app.matches.get_flag("generate-config-file") { /* 生成默认配置 */ }
        else if app.matches.get_flag("config-dir")   { /* 打印配置目录 */ }
        else if app.matches.get_flag("cache-dir")    { /* 打印缓存目录 */ }
        else if app.matches.get_flag("acknowledgements") { /* 打印致谢 */ }
        else {
            run_controller(inputs, &config, cache_dir)   // ★ 最常见：普通读文件
        }
    }
}
```

**输入 → 路径 对照表（小白重点）**：

| 你敲的命令 | 走到哪 | 干嘛 |
|-----------|--------|------|
| `bat foo.rs` | `_` 分支最后一个 else → `run_controller` | 高亮打印 foo.rs |
| `echo hi \| bat` | `inputs` 里 FILE 为空 → stdin → `run_controller` | 高亮打印 stdin |
| `bat -L` | `list-languages` 分支 | 列出语言表（纯文本） |
| `bat --list-themes` | `list-themes` 分支 | 逐个渲染主题预览 |
| `bat cache --build` | `Some(("cache",..))` 且 `args_present` | 重建语法缓存 |
| `bat -h` | `App::new` 里 help 短路（阶段 1） | 用 bat 自己渲染帮助后 exit |

**小白会问：`list-languages` 为什么要把语言表"塞回 controller"打印？**

因为 bat 不想维护两套"打印到终端"的逻辑。语言表本质就是"一段文本"，
转成 `Input::from_reader` 后复用 `run_controller` 那条链（还会带行号、分页），
少写一套代码，且行为和普通读取完全一致。

---

## 阶段 6：`run_controller()` —— 把活正式交给 Controller

`main.rs:289`：

```rust
fn run_controller(inputs: Vec<Input>, config: &Config, cache_dir: &Path) -> Result<bool> {
    // 加载语法/主题资产（优先自定义缓存，否则用编译进二进制的）
    let assets = assets_from_cache_or_binary(config.use_custom_assets, cache_dir)?;
    let controller = Controller::new(config, &assets);
    // 要分页且设置了终端标题 → 先把标题刷到终端（让 less 顶栏显示文件名）
    if config.paging_mode != PagingMode::Never && config.set_terminal_title {
        set_terminal_title_to(get_new_terminal_title(&inputs))?;
    }
    controller.run(inputs, None)        // None = 让 controller 自己决定输出目标
}
```

`Controller` 是"把每个 Input 打印成成品"的调度器，ch4/ch5 才进它内部。
本章到此为止——**命令行已经变成"一组 Input + 一份 Config"，交给 Controller 了**。

---

## 本章涉及的 Rust 机制（小白需要补的知识点）

1. **`Result<bool>` 作为返回值**：`main` 的 `Result<ExitCode>`/`Result<bool>` 是
   "把结果往上传、最后定退出码"的 Rust 惯用法。`Ok(false)` 这种"半成功"
   是 CLI 特有的设计（bat/fd 都有）。
2. **`?` 运算符**：`App::new()?` 把 `Result` 一路往上抛，`main` 统一接住。
   这是"错误传播不爆炸"的标准写法。
3. **`#[cfg(windows)]` / `#[cfg(feature = ...)]`**：同一行代码在 Windows 上跑
   `enable_ansi_support`（开 ANSI 颜色），其它平台跳过。feature 门控裁可选功能
   （ch9 详讲）。
4. **`wild::args_os()` 反复迭代**：`wild` 库能正确切分 Windows 上的引号/转义
   （`std::env::args_os` 在 Windows 上对含引号的参数会切错）。ch1 多次调用它
   是为了"先快速判断 help/version，再完整解析"。

## 检验问题

1. 为什么 `run()` 返回 `Result<bool>` 而不是 `Result<()>`？`Ok(false)` 什么时候出现？
2. 三级配置"命令行 > 环境变量 > 配置文件"具体是怎么实现的？为什么能"盖"住？
3. 为什么 `bat -h` 的帮助是带颜色的、可能还会分页？
4. `bat -p` 和 `bat -pp` 行为差在哪？`Count` 这个 ArgAction 起了什么作用？
5. `loop_through` 为 true 时，后面 ch5 会换成哪个 Printer？为什么批量/管道时要换？

## 下一步

**ch2 — 配置系统**：`Config` 结构体 28 个字段逐个讲"谁给它赋值、什么时候变"，
把本章略过的 `config()` 那 268 行补全。
