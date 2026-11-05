# 第 2 章：配置系统（`Config` 结构体 28 字段逐个讲）

这一章的目标：把 bat 的"参数总开关"`Config` 讲透——每个字段**对应哪个命令行 flag、
默认值是什么、后面哪一步（ch3-ch6）会读它**。读完你应该能回答：
"我加了 `--theme`/`--paging`/`-p` 这些 flag，它们各自落到 `Config` 哪个字段、值怎么算出来的？"

## 涉及文件

- `src/config.rs` — `Config` 结构体定义（28 字段，本章主角）
- `src/bin/bat/app.rs` — `App::config()` 怎么把 `matches` 翻译成 `Config`（赋值逻辑）
- `src/bin/bat/config.rs` — 配置三级合并（`config_file` / 环境变量 / `BAT_OPTS`）

## 全景图（先有这个概念，再看代码）

```
命令行 flag ──────────┐
配置文件 (bat.toml) ───┼─► App::config()  ──► Config { 28 字段 }  ──► ch3-6 只读不写
环境变量 (BAT_*) ──────┘

字段分 5 组（ch3-ch6 各取所需）：
  ① 语言      language / fallback_syntax              → ch4 选 syntax
  ② 二进制    show_nonprintable / binary / tab_width  → ch5 渲染
  ③ 派生      loop_through / true_color               → ch4 选 Printer、ch5 上色
  ④ 风格      style_components / wrapping / paging    → ch5 装饰/换行、ch6 分页
  ⑤ 主题映射  theme / syntax_mapping / pager          → ch4 主题、ch6 分页器
```

读这一章的方法：**逐字段问三句**——它对应哪个 flag？默认值是什么？谁会用到它（ch3-ch6 哪一步）？

`Config`（`src/config.rs:37`）是个**大结构体，28 个字段**，
它不自己"算"任何东西——所有值都在 `app.config()` 里被**一次性填好**，
然后整个 `Config` 被传给 `Controller`，后面只读不写（`&Config` 借用）。
2. 默认值是什么？
3. 谁会用到它（ch3-ch6 哪一步）？

---

## 字段清单（按 `src/config.rs` 源码顺序）

```rust
#[derive(Debug, Clone, Default)]
pub struct Config<'a> {
    pub language: Option<&'a str>,            // -l/--language
    pub fallback_syntax: Option<&'a str>,     // --fallback-syntax
    pub show_nonprintable: bool,             // -A/--show-all
    pub nonprintable_notation: NonprintableNotation,  // --nonprintable-notation
    pub binary: BinaryBehavior,              // --binary
    pub term_width: usize,                   // --terminal-width
    pub tab_width: usize,                    // --tabs
    pub loop_through: bool,                  // (派生，见下)
    pub colored_output: bool,                // --color / -f
    pub true_color: bool,                    // (派生，COLORTERM)
    pub style_components: StyleComponents,   // --style / -p / -n / -b
    pub wrapping_mode: WrappingMode,         // --wrap
    pub paging_mode: PagingMode,            // --paging / -P
    pub visible_lines: VisibleLines,         // -r / -d
    pub theme: String,                       // --theme
    pub syntax_mapping: SyntaxMapping<'a>,   // -m / --ignored-suffix
    pub pager: Option<&'a str>,              // --pager
    pub use_italic_text: bool,               // --italic-text
    pub highlighted_lines: HighlightedLineRanges,  // -H
    pub use_custom_assets: bool,             // --no-custom-assets
    #[cfg(feature = "lessopen")]
    pub use_lessopen: bool,                  // --lessopen
    pub set_terminal_title: bool,            // --set-terminal-title
    pub squeeze_lines: Option<usize>,        // -s / --squeeze-limit
    pub strip_ansi: StripAnsiMode,          // --strip-ansi
    pub sanitize: StripAnsiMode,            // --sanitize
    pub quiet_empty: bool,                  // -E
    pub unbuffered: bool,                   // -u
    pub number_nonblank: bool,              // -b
}
```

下面**逐个**讲。为省篇幅，"默认值"都标在字段后；只有逻辑非显然的才展开。

---

### 语言相关（ch4 选 syntax 用）

```rust
pub language: Option<&'a str>,        // 默认 None（自动检测）
pub fallback_syntax: Option<&'a str>, // 默认 None
```

`language`：用户显式 `-l Rust` 时 Some("Rust")。ch4 里 `assets.get_syntax` 第一行就看它——
Some 就直接用，跳过自动检测。

`fallback_syntax`：自动检测**失败**时才用（`--fallback-syntax`）。
区别：`language` 是"我就用这个"，`fallback` 是"检测不到就退而用这个"。
两者都有时 `language` 优先。

**小白会问：为什么要有 `fallback_syntax`？** 因为有些文件按扩展名检测不出语言
（比如 `.conf`），但你想"检测不到就默认当 INI 看"，`-m '*.conf:INI'` 做不到
（那是强制映射），`--fallback-syntax INI` 才是"兜底"。

---

### 二进制 / 非打印字符（ch3 读文件、ch5 渲染用）

```rust
pub show_nonprintable: bool,             // 默认 false，-A 时 true
pub nonprintable_notation: NonprintableNotation, // 默认 Unicode
pub binary: BinaryBehavior,             // 默认 NoPrinting
```

`show_nonprintable`：`-A` 时把 tab 画成 `├──┤`、把换行画成 `␊`。
ch5 的 `print_line` 第一步就判断它——true 走 `replace_nonprintable`。

`nonprintable_notation`：`--nonprintable-notation=caret` 时用 `^G` 而非 `␇`。

`binary`：`--binary=as-text` 时**强制把二进制当文本**高亮；
`no-printing`（默认）遇到二进制就只打一行警告、不打印内容。
ch5 `print_line` 里 `Some(ContentType::BINARY) | None` 那个 match 分支就靠它决定。

---

### 终端尺寸 / tab

```rust
pub term_width: usize,   // 默认 = Term::stdout().size().1（实际终端宽度）
pub tab_width: usize,    // 默认 4（见 app.rs:449 的逻辑）
```

`term_width`：`--terminal-width N` 显式指定；支持 `+5`/`-5` 偏移。
这是**所有换行/截断**的基准（ch5 的 `cursor_max` 就是它）。

`tab_width` 的默认有点绕（app.rs:444-455）：
```rust
tab_width: ... .unwrap_or(
    if style_components.plain() && paging_mode == PagingMode::Never { 0 }
    else { 4 }
)
```
**plain + 不分页**（典型：`bat -p | 管道`）时 tab_width=0 = "原样透传 tab 不展开"；
否则 4。0 的语义在 `preprocess`（printer.rs:470）里：`tab_width==0` 直接 `*cursor += len` 不做展开。

---

### 两个"派生"字段（小白最容易懵）

```rust
pub loop_through: bool,   // 不是直接来自某个 flag，是一堆条件的"或非"
pub true_color: bool,     // 不是 flag，是读 COLORTERM 环境变量
```

`loop_through`（app.rs:434）：
```rust
loop_through: !(
    self.interactive_output
    || color == Some("always")
    || decorations == Some("always")
    || self.matches.get_flag("force-colorization")   // -f
    || self.number_from_cli                            // -n
    || self.number_nonblank_from_cli                   // -b
),
```
**任何一条成立 → loop_through=false（交互渲染）**；全不成立 → true（纯文本批处理）。
ch4 `Controller::print_input` 就靠它选 Printer：
```rust
let mut printer = if self.config.loop_through {
    Box::new(SimplePrinter::new(self.config))       // 纯文本
} else {
    Box::new(InteractivePrinter::new(...))          // 高亮
}
```

`true_color`（app.rs:369）：`is_truecolor_terminal()` 读 `COLORTERM`，
值为 `truecolor` 或 `24bit` 才 true。ch5 上色时用 true/false 决定
"24bit 色"还是"8/16 色降级"。

**小白会问：`true_color` 为什么不跟 `colored_output` 一个字段？**
因为"要不要上色"（`colored_output`）和"上多深的色"（`true_color`）是两件事：
可以 `colored_output=true` 但终端只支持 16 色，那 `true_color=false`，
syntect 的 24bit 色会被 `terminal::to_ansi_color` 降级成 16 色调色板。

---

### 风格组件（ch7 画装饰用）

```rust
pub style_components: StyleComponents,  // 默认 = StyleComponent::Default 展开
```
`StyleComponents` 是个 `HashSet<StyleComponent>`（style.rs:89），
里面每个 `StyleComponent` 是 ch7 要画的"装饰开关"：
`Grid`（网格竖线）、`LineNumbers`（行号）、`HeaderFilename`（文件头）、
`Changes`（git 改动标记）、`Snip`（snip 分隔线）、`Rule`（文件间横线）。

ch1 讲过 `forced_style_components` 怎么算它。ch5 的 `print_line` 一开头
`if self.panel_width > 0` 就是在遍历这些组件算行左侧留白。

---

### 换行 / 分页 / 可见行

```rust
pub wrapping_mode: WrappingMode,        // 默认 auto
pub paging_mode: PagingMode,            // 默认 auto（见 app.rs 算 PagingMode 那段）
pub visible_lines: VisibleLines,        // 默认 Ranges(全显)
```

`wrapping_mode`：`--wrap` 的值。`auto` 在 `config()` 里展开成
`NoWrapping(false)`（非交互/管道）或 `Character`（交互且宽）。
ch5 的 `print_line` 末尾靠它分两条路：`NoWrapping(_)` 直接写满一行不截断，
否则做"按字符/按词换行"。

`paging_mode`：ch1 阶段 4 讲过那个大 if/else。ch6 的 `OutputType::from_mode` 读它
决定"要不要真的起一个 pager 子进程"。

`visible_lines`（config.rs:11）是个枚举：
```rust
pub enum VisibleLines {
    Ranges(LineRanges),        // -r 30:40 这种"按行范围过滤"
    DiffContext(usize),       // -d --diff-context N，"按 git 改动行过滤"
}
```
ch4 `print_file_ranges` 读它：Ranges 直接 clone；DiffContext 把 git diff 的行
展开成 `LineRange` 列表。默认 `Ranges(LineRanges::all())` = 全显。

---

### 主题 / 语法映射（ch4 选 syntax 用）

```rust
pub theme: String,                  // 默认由 theme() 解析（auto→按终端深浅选）
pub syntax_mapping: SyntaxMapping<'a>,  // -m 收集到这
pub pager: Option<&'a str>,         // --pager（pager 本身，不是 paging_mode）
```

`theme`（app.rs:486）：`theme(self.theme_options()).to_string()`。
`theme_options` 把 `--theme/--theme-dark/--theme-light` 三个 flag 收进
`ThemeOptions` 结构体，再喂给 `theme.rs` 的 `theme()` 函数解析。
解析细节（auto 怎么探测终端深浅色）在 ch7 不展开，这里只知"默认是
按终端当前深浅色自动挑"。

`syntax_mapping`：`-m '*.build:Python'` 的映射规则。ch4
`assets.get_syntax` 用它做"路径 → 语法"的第一优先匹配。

`pager`：`--pager` 指定 pager 命令。注意它和 `paging_mode` 分开——
`paging_mode` 决定"要不要用 pager"，`pager` 决定"用哪个 pager"。
ch6 的 `OutputType::from_mode` 两个都读。

---

### 行级特性

```rust
pub use_italic_text: bool,                 // --italic-text=always
pub highlighted_lines: HighlightedLineRanges, // -H 40 / -H 30:40
pub squeeze_lines: Option<usize>,           // -s（默认 None=不挤压）
```

`use_italic_text`：终端要不要用斜体。ch5 上色时传给 `as_terminal_escaped`。
默认 false，且很多终端（tmux）不支持斜体，所以默认关。

`highlighted_lines`（config.rs:94）：`-H` 指定的"要高亮的行"。
ch5 `print_line` 里 `highlight_this_line = config.highlighted_lines.check(...)`，
命中就给它套 `background_color_highlight` 背景色。

`squeeze_lines`：`-s` 时把连续空行压成 1 行。ch5 `print_line` 里
`if let Some(squeeze_limit) = self.config.squeeze_lines` 那段就是数空行。

---

### 资产 / 标题 / 预处理开关

```rust
pub use_custom_assets: bool,        // 默认 true（--no-custom-assets 时 false）
pub set_terminal_title: bool,       // 默认 false（--set-terminal-title）
pub quiet_empty: bool,              // 默认 false（-E）
pub unbuffered: bool,               // 默认 false（-u）
pub number_nonblank: bool,          // 默认 false（-b）
#[cfg(feature = "lessopen")]
pub use_lessopen: bool,
pub strip_ansi: StripAnsiMode,      // 默认 Never（--strip-ansi）
pub sanitize: StripAnsiMode,        // 默认 Never（--sanitize）
```

`use_custom_assets`：`bat cache --build` 建过自定义语法/主题缓存后，
`assets_from_cache_or_binary`（main.rs:43 调它）会优先读缓存；
false 就只用编译进二进制的内置资产。ch4 加载 assets 时用到。

`set_terminal_title`：分页时把终端标题刷成文件名（main.rs:265 `set_terminal_title_to`）。
ch6 起 pager 前会先调它。

`quiet_empty`：`-E`，空输入时**啥都不打**（连 header 都不打）。
ch5 `print_header` 第一行 `if self.content_type.is_none() && self.config.quiet_empty` 就靠它。

`unbuffered`：`-u` 流式读取（`tail -f log | bat -u`）。ch3 `InputReader` 读行时
`if self.unbuffered { return self.read_line_unbuffered(buf); }`。
ch1 讲过它还会自动关掉行号（`style_components` 里 remove LineNumbers）。

`number_nonblank`：`-b`，只给**非空行**编号。ch5 `print_line` 里
`display_line_number = if number_nonblank && 行是空 { 0 } else { line_number }`。

`strip_ansi` / `sanitize`：ch5 渲染前对输入做的"清洗"。
`strip_ansi` 只去掉 ANSI 颜色序列；`sanitize` 是它的超集，
额外把"会操作终端"的控制字节（光标移动、bidi、零宽字符）换成 U+FFFD。
ch5 `print_line` 里：
```rust
if self.sanitize { line = sanitize(&line).into() }
else if self.strip_ansi { line = strip_ansi(&line).into() }
```
两者都是 `StripAnsiMode::{Never,Always,Auto}`，`Auto` 在"检测到纯文本"时不清洗
（因为纯文本里的转义序列可能是内容本身）。

---

## 本章涉及的 Rust 机制（小白需要补的知识点）

1. **`#[derive(Debug, Clone, Default)]`**：`Config` 三行宏给它派生了
   "能打印 / 能克隆 / 有默认值"。`Default` 让 `Config { ..Default::default() }`
   这种"填几个字段、其余取默认"的写法成为可能（ch1 `display_help` 用过）。
2. **`Option<T>` 作为"没指定"的表达**：`language: Option<&str>` 的 `None`
   就是"没给 `-l`，走自动检测"。整章大量 `Option` 默认值就是这种语义。
3. **生命周期 `'a`**：`Config<'a>` 的 `&'a str` 字段指向 `ArgMatches` 里的字符串，
   不拥有它们（借用）。`'a` 保证"Config 活多久，这些 str 就得活多久"。
4. **派生字段的"或非"逻辑**：`loop_through` 不是直接来自某 flag，
   是一串 `!(cond1 || cond2 || ...)` 算出来的——这是"配置派生"的常见模式。

## 检验问题

1. `language` 和 `fallback_syntax` 都填了，ch4 按什么顺序选？
2. `tab_width` 默认 4，什么情况下会变 0？那个 0 在 `preprocess` 里是什么意思？
3. `loop_through` 是"或非"出来的。`bat -n`（带行号）会让它变什么？ch4 选哪个 Printer？
4. `strip_ansi` 设 `Auto`，输入是一段**纯文本**时它会清洗吗？为什么？
5. `paging_mode` 和 `pager` 各管什么？`bat --paging=never` 后 `pager` 还有用吗？

## 下一步

**ch3 — 输入（`Input` / `OpenedInput` / `InputReader`）**：
怎么打开文件、探测编码（UTF-16/二进制）、逐行读。
这一章读 `src/input.rs`（667 行）的主干。
