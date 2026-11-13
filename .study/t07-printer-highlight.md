# t07：主路⑦ 中段 —— Printer 二选一 + 选 syntax/theme（含装饰器装配）

> 上一篇 t06 钻完"打开输入 + 编码嗅探 + git diff"。现在回到 `print_input`
> （controller.rs:192）：**选 Printer**（整棵树的关键二选一），然后钻进
> `InteractivePrinter::new` 把 theme/syntax/装饰器全装配好。
> 逐行渲染在 t08，本篇停在"装配完成、准备吐第一行"。

## 你在执行树的哪个位置

```
print_input（t06 已钻完开文件）
└─【分支】Printer 二选一            ← 你在这里（controller.rs:192）
   ├─ loop_through → SimplePrinter（纯文本，一行带过）
   └─ 否则 → InteractivePrinter::new（钻到叶子）
        ├─ 叶子：get_theme + Colors
        ├─ 叶子：装饰器装配（旧 ch7 拆到此处）
        ├─ 叶子：get_syntax 四级优先级
        ├─ 叶子：HighlighterFromSet（有状态高亮器）
        └─ 叶子：strip_ansi/sanitize 落实
```

## 1. 二选一（controller.rs:192）

```rust
let mut printer: Box<dyn Printer> = if self.config.loop_through {
    Box::new(SimplePrinter::new(self.config))            // 纯文本透传
} else {
    Box::new(InteractivePrinter::new(
        self.config, self.assets, &mut opened_input,
        #[cfg(feature = "git")]
        &line_changes,
    )?)
};
self.print_file(&mut *printer, writer, &mut opened_input, !is_first,
    #[cfg(feature = "git")] &line_changes)
```

**Rust 机制 · 为什么必须 `Box<dyn Printer>` 而不是泛型**（architecture.md 判断 3）：
`loop_through` 是 **t01 §2e 那个"或非"在运行时算出来的值**（取决于交互性、-f、-n…
六个条件）。泛型 `fn f<T: Printer>` 要求**编译期**定 T，这里做不到——只能
`Box<dyn Printer>`（堆上装一个 vtable 指针，运行时按 vtable 分发调用）。
代价是每次 `print_line` 一次虚函数调用；但渲染不是热点（热点在 syntect 高亮），可接受。

`SimplePrinter`：`loop_through`（管道/批处理）时的"退化版"——只把行原样写出去，
不选语法不画装饰。一行带过即可，它的存在意义是让 `cat` 模式行为正确。

**选完 Printer，钻 `InteractivePrinter::new` 的四个叶子。**

## 2. 叶子：theme + Colors（printer.rs:227 起）

```rust
let theme = assets.get_theme(&config.theme);              // 按名字取主题（找不到 → 默认）
let background_color_highlight = theme.settings.line_highlight;   // -H 命中行的背景色

let colors = if config.colored_output {
    Colors::colored(theme, config.true_color)    // 从主题抽 8 种装饰色
} else {
    Colors::plain()                            // 不上色 → 全默认
};
```

`Colors`（printer.rs:994）8 个字段：`grid`/`rule`/`header_value`/`git_added`(绿)/
`git_removed`(红)/`git_modified`(黄)/`line_number`(默认终端灰)。
`true_color`（t01 §2e 派生字段，读 `COLORTERM`）决定 24bit 色还是降级 16 色——
**"要不要上色"（`colored_output`）和"上多深的色"（`true_color`）是两件事**（t01 讲过）。

## 3. 叶子：装饰器装配（旧 ch7 拆到此处，printer.rs:243-276）★ 可插拔设计

```rust
let mut decorations: Vec<Box<dyn Decoration>> = Vec::new();
if config.style_components.numbers() {
    decorations.push(Box::new(LineNumberDecoration::new(&colors)));   // 行号
}
#[cfg(feature = "git")]
if config.style_components.changes() && line_changes…is_some_and(|c| !c.is_empty()) {
    decorations.push(Box::new(LineChangesDecoration::new(&colors)));  // + / - / ~ 标记
}
let mut panel_width = decorations.len() + decorations.iter().fold(0, |a, x| a + x.width());
if config.style_components.grid() && !decorations.is_empty() {
    decorations.push(Box::new(GridBorderDecoration::new(&colors)));   // 网格竖线（不算进 panel_width）
}
// 终端太窄（宽度 < 装饰总宽 + 5）→ 全关（宁可没装饰不能挤爆）
if config.term_width < (decorations.len() + 装饰总宽) + 5 {
    decorations.clear();
    panel_width = 0;
}
```

**为什么 `Vec<Box<dyn Decoration>>` 而不是 enum**（architecture.md 判断 4）：
装饰是**自由组合**（只行号 / 行号+git / 全开…），enum 要为每种组合写 match，组合爆炸；
`Vec` + trait 对象任意拼。开闭原则：加新装饰（如"行尾注释标记"）只需**加一个实现
+ 这里多 push 一个**，不改 `print_file`/渲染循环。

每个装饰**自带 `width()`**（行号 4、grid 1、git 标记 1…），`panel_width` 累加后
决定内容区宽度（终端宽 − panel_width）——t08 的 tab 展开、换行截断全靠它。

`t01` 讲过 `-n`/`-b`/`-p` 会走 `forced_style_components` 把 `style_components`
"锁死"成只含行号（app.rs:592-603），这里 `style_components.numbers()` 读的就是它——
CLI 参数到这里真正"变成画面"。

## 4. 叶子：`get_syntax` 四级优先级（assets.rs:216）

先算"需不需要匹配语法"（printer.rs:282 前的 `needs_to_match_syntax`）：
二进制且没 `--binary=as-text` → **不匹配**（整行套中性灰）；要上色或 `Auto` 清洗 → 匹配。

匹配时 `assets.get_syntax(config.language, config.fallback_syntax, input, &config.syntax_mapping)`
按**四级优先级**（从高到低）：

```
① -l/--language 显式指定          → 直接 find_syntax_by_token（查不到 → Err）
② 按文件路径（get_syntax_for_path）→ 内部又四层：
     a. -m 映射 MapTo（'*.build:Python' 强制）
     b. 完整文件名（Dockerfile/Makefile 无扩展名特例）
     c. -m 映射 MapExtensionToUnknown（*.conf:unknown 强制不检测）
     d. 扩展名（.rs → Rust）
     （路径探测失败还会 canonicalize 解析符号链接重试——链接名无扩展名、目标有的情况 #1001）
③ 首行内容嗅探（get_first_line_syntax）→ shebang '#!/bin/env python' → Python
④ --fallback-syntax 兜底           → 都没有就用它
全失败 → Err(UndetectedSyntax) → 调用方退 Plain Text（纯文本引擎，无颜色区分）
```

**②和③的顺序值得记**：路径先于内容——`foo.sh` 就算首行不是 shebang 也当 Shell。
**stdin 没有路径**（②直接跳过，报 "unknown"），全靠③首行嗅探 + ④兜底；
这就是 `echo hi | bat` 只能靠 `-l` 或 `--fallback-syntax` 指定语言的原因。

## 5. 叶子：`HighlighterFromSet`（有状态高亮器，printer.rs:195）

```rust
struct HighlighterFromSet<'a> {
    highlighter: HighlightLines<'a>,    // ★ syntect 状态机
    syntax_set: &'a SyntaxSet,
}
```

syntect 高亮是**有状态的**（architecture.md 判断 5）：一行开了字符串没闭合、
括号没匹配，状态要带进下一行。所以它必须是 `InteractivePrinter` 的**实例字段**
（整个文件期间持有），不能每行新建——否则每行都"从头当行首解析"，
跨行字符串/注释的高亮就全乱。

配套的 `is_plain_text` / `strip_overstrike` 两个 bool 和 `HighlighterFromSet` 一起在
`get_syntax` 调用处产生（printer.rs:290-323，§4 讲过的那段三元组）：
- `is_plain_text`：选出的语法是不是 "Plain Text"（`Auto` 清洗时纯文本要**保留**
  转义序列，因为那可能是内容本身）；
- `strip_overstrike`：Manpage/CommandHelp 语法用 `X\x08X` 退格覆盖实现粗体，
  命中就标记"渲染前剥覆盖序列"。

## 6. 叶子：收尾 `new`（printer.rs:326 起）

把前五个叶子落实进结构体：

```rust
let strip_ansi = match config.strip_ansi {         // printer.rs:326
    _ if config.show_nonprintable => false,      // -A 时啥都不剥（要原样显示）
    StripAnsiMode::Always => true,
    StripAnsiMode::Auto if is_plain_text => false,  // ★ 纯文本保留
    StripAnsiMode::Auto => true,
    _ => false,
};
let sanitize = match config.sanitize { … }         // printer.rs:334 同款（严格超集：光标/bidi/零宽→U+FFFD）

Ok(InteractivePrinter {
    panel_width, colors, config, decorations,
    content_type: input.reader.content_type,
    ansi_style: AnsiStyle::new(),        // 读输入里藏的 ANSI 用（vscreen 解析）
    line_changes,
    highlighter_from_set,               // ★ 有状态高亮器，跨行复用
    background_color_highlight,
    consecutive_empty_lines: 0,
    strip_ansi, sanitize, strip_overstrike,
})
```

**`ansi_style` 值得单独记一笔**：`InteractivePrinter` 不只**输出** ANSI，
还会**读**输入里已有的转义序列（`bat` 可以高亮一个本身就带颜色的 .txt）——
`AnsiStyle` 跟踪"读到行中间时处于什么样式状态"，t08 的渲染循环会用到。

## 本区段 Rust 机制

1. **`Box<dyn Printer>` vs 泛型的判断标准**（判断 3）：
   **选哪个实现是运行期才知道的 → trait 对象；编译期就定的 → 泛型（单态化更快）**。
2. **`#[cfg(feature = "git")]` 混在表达式里**：`&line_changes` 参数整块被 cfg 包着
   （controller.rs:198-200）——feature 关时这行**不存在**，函数调用自然少一个参数。
   读代码时看到"参数数量对不上"，先查有没有 cfg。
3. **有状态对象提层持有**（判断 5）：`HighlightLines` 跨行带状态 → 放结构体字段，
   不藏函数里。t06 的 `InputReader.first_line` 缓存是同款模式。

## 检验问题

1. 为什么 `printer` 必须是 `Box<dyn Printer>`？换成泛型 `fn f<T: Printer>` 会在哪一步编译不过？
2. `echo 'print(1)' | bat`（无 `-l`）能高亮成 Python 吗？靠四级优先级的哪一级？
3. 终端宽度 50、装饰总宽 8，`term_width < panel_width + len + 5` 成立吗？装饰还画吗？
4. `HighlighterFromSet` 若改成每行 `print_line` 里新建，哪类文件的高亮会最先出错（举一个例子）？

## 下一站

装配完毕，`print_file` 开始吐行：**header → 逐行 print_line → footer** 的全叶子 → **t08**。
