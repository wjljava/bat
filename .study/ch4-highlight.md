# 第 4 章：高亮核心（选 syntax + 拆样式区域）

这一章的目标：bat 真正**把字节变成颜色**的心脏——`InteractivePrinter` 怎么挑语法、
怎么调 syntect 引擎、一行怎么被拆成"带样式的片段"。读完你应该能回答：
"一个 `.rs` 文件进来，syntect 凭什么知道该按 Rust 语法高亮？有状态高亮器为什么每文件一个？"

## 涉及文件

- `src/printer.rs` — `InteractivePrinter::new`（选 theme/syntax）、`highlight_regions_for_line`（拆片段）
- `src/assets.rs` — `get_syntax` / `get_syntax_for_path`（自动检测语法）

## 全景图（先有这个概念，再看代码）

```
OpenedInput（ch3 打开的文件） + Config（ch2）
   │
   ▼
InteractivePrinter::new()              （printer.rs:218）
   ├─ assets.get_theme(&config.theme)  拿主题（Monokai / ansi / …）
   ├─ Colors::colored / plain         装饰区配色
   ├─ 攒 decorations（行号/git/网格）  ch7 详解
   ├─ 判"需不需要匹配语法"
   │     └─ assets.get_syntax(…)        （assets.rs:216）四级优先级
   │          ① -l 显式语言
   │          ② 路径（-m 映射 / 文件名 / 扩展名）
   │          ③ 首行内容（shebang / 文件头）
   │          ④ --fallback-syntax
   └─ 包成 HighlighterFromSet          ★ 有状态 syntect 高亮器（跨行复用）
   │
   ▼
每行：highlight_regions_for_line(line)  （printer.rs:445）
   └─ highlighter.highlight_line(line, syntax_set)
        → Vec<(Style, &str)>            样式 + 片段 交替
```

主线：`bat foo.rs` 到这一章，`Controller::print_input` 已经拿到
`OpenedInput`（ch3 打开的文件）+ `Config`，决定用 `InteractivePrinter`（非 loop_through 时）。
本章讲它 `new()` 里的两件事：**选 theme** 和 **选 syntax**，然后每行进
`highlight_regions_for_line` 拆成 `Vec<(Style, &str)>`。

syntect 是 bat 的**高亮引擎**（Rust 写的语法高亮库，类似 tree-sitter 但更轻）。
它的工作模型是：

```
一行字符串 + 语法定义 + 主题
        │
        ▼
syntect::HighlightLines::highlight_line(line, syntax_set)
        │
        ▼
Vec<(syntect::highlighting::Style, &str)>   // "样式 + 片段" 交替
   例: [(KEYWORD_STYLE, "fn"), (NORMAL, " "),
        (FUNC_STYLE, "main"), (NORMAL, "()"), ...]
```

**关键心智模型**：syntect 把一行**切碎了**，每段前面挂一个"该段该用什么颜色"。
bat 自己不写"这个词该什么色"——它**问引擎要**，引擎答 `[(风格, 片段)]`。
后面 ch5 的 `print_line` 再把这些片段逐个上色、写出去。

`Style` 是 syntect 的颜色定义（前景/背景/粗斜体），ch5 才真正画到终端。
本章只到"拿到 `Vec<(Style, &str)>`"为止。

---

## 阶段 1：`InteractivePrinter::new` 的第一半 —— 定 theme 和颜色

`src/printer.rs:218`。完整（截取到选 theme 那段，syntax 选完在阶段 3 讲）：

```rust
pub(crate) fn new(
    config: &'a Config<'a>,
    assets: &'a HighlightingAssets,      // ch3 的 assets（语法+主题集合）
    input: &mut OpenedInput,            // ch3 打开的文件（content_type 在这里）
    line_changes: &'a Option<LineChanges>,  // git diff 结果（无 git feature 时没有）
) -> Result<Self> {
    // 1) 按 config.theme（ch2 解析出的主题名）从 assets 拿 Theme
    let theme = assets.get_theme(&config.theme);

    // 主题自带的"行高亮背景色"（-H 命中某行时用它）
    let background_color_highlight = theme.settings.line_highlight;

    // 2) 颜色配置：要上色就用主题色 + 终端支持 24bit？；不上色就全 plain
    let colors = if config.colored_output {
        Colors::colored(theme, config.true_color)   // 从主题抽 8 种装饰色
    } else {
        Colors::plain()
    };

    // 3) 建装饰组件（ch7 详解：行号 / git 标记 / 网格竖线）
    let mut decorations: Vec<Box<dyn Decoration>> = Vec::new();
    if config.style_components.numbers() {
        decorations.push(Box::new(LineNumberDecoration::new(&colors)));
    }
    #[cfg(feature = "git")]
    {
        if config.style_components.changes() && line_changes.as_ref().is_some_and(|c| !c.is_empty()) {
            decorations.push(Box::new(LineChangesDecoration::new(&colors)));
        }
    }
    // panel_width = 所有装饰的总宽度（行左侧留白）
    let mut panel_width: usize = decorations.len() + decorations.iter().fold(0, |a, x| a + x.width());
    // 网格竖线（grid）不算进 panel_width（因为 print_* 都假设 panel_width 不含它）
    if config.style_components.grid() && !decorations.is_empty() {
        decorations.push(Box::new(GridBorderDecoration::new(&colors)));
    }
    // 终端太窄（放不下 5 字符+装饰）就干脆不画装饰
    if config.term_width < (decorations.len() + decorations.iter().fold(0, |a, x| a + x.width())) + 5 {
        decorations.clear();
        panel_width = 0;
    }

    // 4) 判断"这文件是不是二进制"（ch3 的 content_type）
    let is_printing_binary = input.reader.content_type
        .is_some_and(|c| c.is_binary() && !config.show_nonprintable);

    // 5) 需不需要"匹配语法"：
    //    二进制且没强制 as-text → 不用；要上色 或 Auto 清洗 → 需要
    let needs_to_match_syntax = (!is_printing_binary || matches!(config.binary, BinaryBehavior::AsText))
        && (config.colored_output || config.strip_ansi == StripAnsiMode::Auto
            || config.sanitize == StripAnsiMode::Auto);
```

**小白会问：为什么"不上色"也要"匹配语法"（`needs_to_match_syntax` 里有 `strip_ansi==Auto`）？**

因为 `--strip-ansi=Auto` 的语义是"检测到**非**纯文本才去掉 ANSI"。
要判断"是不是纯文本"，就得先选出语法（纯文本对应 `Plain Text` 语法）。
所以上不上色，只要涉及"Auto"清洗，都得走一遍语法匹配。

`Colors`（printer.rs:986）就是"装饰区该用什么色"：
```rust
pub struct Colors {
    pub grid: Style,            // 网格竖线
    pub rule: Style,            // 文件间横线
    pub header_value: Style,    // header 里的文件名/大小
    pub git_added: Style,       // + 绿
    pub git_removed: Style,    // 红
    pub git_modified: Style,    // ~ 黄
    pub line_number: Style,     // 行号（默认终端灰）
}
```
`colored()` 从主题抽这些色；`plain()` 全默认（无色）。

---

## 阶段 2：`new` 的第二半 —— 选 syntax（`assets.get_syntax`）

紧接阶段 1（printer.rs:282）：

```rust
    let (is_plain_text, strip_overstrike, highlighter_from_set) = if needs_to_match_syntax {
        const PLAIN_TEXT_SYNTAX: &str = "Plain Text";
        const MANPAGE_SYNTAX: &str = "Manpage";
        const COMMAND_HELP_SYNTAX: &str = "Command Help";
        // ★ 核心：从 assets 选出一套"语法 + 语法集合"
        match assets.get_syntax(config.language, config.fallback_syntax, input, &config.syntax_mapping) {
            // 正常：拿到了具体语法
            Ok(syntax_in_set) => (
                syntax_in_set.syntax.name == PLAIN_TEXT_SYNTAX,   // 是不是纯文本
                syntax_in_set.syntax.name == MANPAGE_SYNTAX || syntax_in_set.syntax.name == COMMAND_HELP_SYNTAX, // 是不是 man page
                Some(HighlighterFromSet::new(syntax_in_set, theme)), // 包个"带高亮器的句柄"
            ),
            // 检测不到 → 退回 Plain Text（纯文本引擎也能高亮，就是没颜色区分）
            Err(Error::UndetectedSyntax(_)) => (
                true,
                false,
                Some(assets.find_syntax_by_name(PLAIN_TEXT_SYNTAX)?
                    .map(|s| HighlighterFromSet::new(s, theme))
                    .expect("A plain text syntax is available")),
            ),
            // 其它错误（比如指定了不存在的语法名）→ 直接上抛
            Err(e) => return Err(e),
        }
    } else {
        // 不需要匹配语法（纯二进制且没 as-text）：三者都关
        (false, false, None)
    };
```

`HighlighterFromSet`（printer.rs:187）是个小包装：
```rust
struct HighlighterFromSet<'a> {
    highlighter: HighlightLines<'a>,   // ★ syntect 的高亮器（每文件一个，跨行复用）
    syntax_set: &'a SyntaxSet,
}

impl<'a> HighlighterFromSet<'a> {
    fn new(syntax_in_set: SyntaxReferenceInSet<'a>, theme: &'a Theme) -> Self {
        Self {
            highlighter: HighlightLines::new(syntax_in_set.syntax, theme),
            syntax_set: syntax_in_set.syntax_set,
        }
    }
}
```

**`HighlightLines` 为什么是"每文件一个"？**
syntect 的高亮是**有状态的**——它要记住"上一行结尾在哪种语法上下文"
（比如字符串没闭合、括号没匹配）。`HighlightLines` 持有这个上下文，
所以每个 `OpenedInput` 配一个 `HighlighterFromSet`，跨多行复用。
这就是为什么它在 `InteractivePrinter` 里是 `Option` 字段——
整个文件的生命周期里持续持有它。

**`is_plain_text` / `strip_overstrike` 这两个 bool 干嘛的？**
- `is_plain_text`：决定 `Auto` 模式下要不要去 ANSI（纯文本保留转义序列，
  因为那可能是内容本身）。阶段 4 会用到。
- `strip_overstrike`：man page 用"退格覆盖"实现粗体（`X\x08X`），
  检测到 Manpage/CommandHelp 语法就标记"要剥覆盖序列"，ch5 渲染前调 `strip_overstrike`。

---

## 阶段 3：`assets.get_syntax` —— 语法到底怎么挑出来

`src/assets.rs:216`。这是"自动检测"的核心，**优先级从高到低**：

```rust
pub(crate) fn get_syntax(
    &self,
    language: Option<&str>,          // ch2 的 config.language（-l）
    fallback_syntax: Option<&str>,   // ch2 的 config.fallback_syntax
    input: &mut OpenedInput,
    mapping: &SyntaxMapping,
) -> Result<SyntaxReferenceInSet<'_>> {
    // 优先级 1：用户显式 -l Rust → 直接用
    if let Some(language) = language {
        let syntax_set = self.get_syntax_set()?;
        return syntax_set.find_syntax_by_token(language)
            .map(|syntax| SyntaxReferenceInSet { syntax, syntax_set })
            .ok_or_else(|| Error::UnknownSyntax(language.to_owned()));
    }

    // 优先级 2：按路径（含 -m 映射 + 文件名 + 扩展名）
    let path = input.path();
    let absolute_path = path.and_then(|p| PathAbs::new(p).ok()
        .map(|abs| abs.as_path().to_path_buf()).or_else(|| Some(p.to_owned())));

    let path_syntax = if let Some(ref path) = absolute_path {
        self.get_syntax_for_path(path, mapping).or_else(|e| {
            // 路径探测失败 → 再试"解析符号链接后的路径"
            // 处理：Aliases/0install -> ../Formula/zero-install.rb 这种
            // 链接名没扩展名、目标有的情况（#1001）
            if matches!(e, Error::UndetectedSyntax(_)) {
                if let Ok(resolved) = fs::canonicalize(path) {
                    if resolved != *path {
                        return match self.get_syntax_for_path(&resolved, mapping) {
                            Ok(syntax) => Ok(syntax),
                            Err(Error::UndetectedSyntax(_)) => Err(e),
                            Err(err) => Err(err),
                        };
                    }
                }
            }
            Err(e)
        })
    } else {
        // stdin / 自定义 reader 没有真实路径 → 标记"检测不到"
        Err(Error::UndetectedSyntax("[unknown]".into()))
    };

    // 优先级 3：上面全失败 → 看首行内容猜语法（shebang / 文件头）
    match path_syntax {
        Err(Error::UndetectedSyntax(path)) => {
            if let Some(syntax_in_set) = self.get_first_line_syntax(&mut input.reader)? {
                Ok(syntax_in_set)
            } else if let Some(language) = fallback_syntax {
                // 优先级 4：--fallback-syntax 兜底
                self.find_syntax_by_token(language)?
                    .ok_or_else(|| Error::UnknownSyntax(language.to_owned()))
            } else {
                Err(Error::UndetectedSyntax(path))   // 真检测不到
            }
        }
        _ => path_syntax,
    }
}
```

`get_syntax_for_path`（assets.rs:151）内部再分四层（doc 注释写得很清楚）：
1. **映射 `MapTo`**（`-m '*.build:Python'` 这种强制映射）
2. **完整文件名**（`Dockerfile`、`Makefile` 这种没扩展名的特例）
3. **映射 `MapExtensionToUnknown`**（`-m '*.conf:unknown'` 显式标"不要检测"）
4. **文件扩展名**（`.rs` → Rust）

**小白会问：映射里 `MapTo` 和 `MapToUnknown` 什么区别？**
- `MapTo("Python")`：`*.build` **强制**当 Python 高亮（即使引擎觉得不对）。
- `MapToUnknown`：`*.conf` **强制不检测**（当纯文本），但你还能用 `--language` 覆盖。
`--ignored-suffix`（ch1 的 flag）就是批量加 `MapExtensionToUnknown` 规则。

`get_first_line_syntax`（ch3 首行嗅探的延续）：对**没有扩展名**的文件，
看首行内容猜（比如 `#!/usr/bin/env python` → Python，
`<html>` → HTML）。这就是为什么 bat 能高亮 shebang 脚本。

---

## 阶段 4：`new` 收尾 —— 决定 strip_ansi / sanitize 的具体值

（printer.rs:318）：

```rust
    // 把阶段 1-3 的 bool 落实成最终行为
    let strip_ansi = match config.strip_ansi {
        _ if config.show_nonprintable => false,                  // -A 时啥都不剥
        StripAnsiMode::Always => true,
        StripAnsiMode::Auto if is_plain_text => false,          // 纯文本保留转义
        StripAnsiMode::Auto => true,                            // 非纯文本 + Auto → 剥
        _ => false,
    };

    let sanitize = match config.sanitize {
        _ if config.show_nonprintable => false,
        StripAnsiMode::Always => true,
        StripAnsiMode::Auto if is_plain_text => false,
        StripAnsiMode::Auto => true,
        _ => false,
    };

    Ok(InteractivePrinter {
        panel_width, colors, config, decorations,
        content_type: input.reader.content_type,
        ansi_style: AnsiStyle::new(),                 // ch5 解析输入里的 ANSI 用
        line_changes,                                  // git diff
        highlighter_from_set,                          // ★ 阶段 2 选好的"带状态高亮器"
        background_color_highlight,                     // -H 命中行的背景色
        consecutive_empty_lines: 0,
        strip_ansi, sanitize, strip_overstrike,
    })
}
```

**注意 `ansi_style: AnsiStyle::new()`**——这是 ch5 解析输入里可能藏的 ANSI 序列用的。
`InteractivePrinter` 不只"输出"ANSI，还会**读**输入里已有的转义序列
（比如 `bat` 读一个本身就带颜色的 `.txt`），`AnsiStyle` 跟踪"当前处于什么样式状态"。

---

## 阶段 5：`highlight_regions_for_line` —— 一行拆成样式片段

这是 ch5 `print_line` 每行调一次的（printer.rs:445）：

```rust
fn highlight_regions_for_line<'b>(
    &mut self,
    line: &'b str,
) -> Result<Vec<(syntect::highlighting::Style, &'b str)>> {
    // 没选到语法（纯二进制没 as-text）→ 整行套一个"中性灰"
    let highlighter_from_set = match self.highlighter_from_set {
        Some(ref mut highlighter_from_set) => highlighter_from_set,
        _ => return Ok(vec![(EMPTY_SYNTECT_STYLE, line)]),
    };

    // 性能保护：超长的行（>16KB）不做语法高亮（太慢），只当一个整体
    let too_long = line.len() > 1024 * 16;
    let for_highlighting: &str = if too_long { "\n" } else { line };

    // ★★ 核心一行：喂给 syntect，拿回"样式+片段"列表
    let mut highlighted_line = highlighter_from_set
        .highlighter
        .highlight_line(for_highlighting, highlighter_from_set.syntax_set)?;

    if too_long {
        highlighted_line[0].1 = line;   // 超长行：把整行塞回第一个片段（放弃细分）
    }

    Ok(highlighted_line)
}
```

**小白会问：`highlight_line` 为什么是"每行调一次"而不是一把全喂？**

syntect 的语法定义是**逐行状态机**（一行可能开括号、开字符串，
状态传下行）。`highlight_line` 接收"单行"，内部用 `HighlightLines`
持有的上下文决定"这行开头接上行的什么状态"。
所以 ch5 是 for 循环逐行调它，正好匹配这种有状态模型。

`EMPTY_SYNTECT_STYLE`（printer.rs:67）是个固定"中性灰"（#7f7f7f），
给"没语法可用"的行兜底——至少还有颜色，不是纯黑。

---

## 这一章产出了什么（交给 ch5）

经过 ch4，`InteractivePrinter` 手里有了：
- `highlighter_from_set`：有状态的 syntect 高亮器（每行喂 `highlight_line` 拆片段）
- `panel_width` / `decorations`：行左侧装饰（ch7 详解怎么画）
- `colors`：装饰区用色
- `strip_ansi` / `sanitize` / `strip_overstrike`：渲染前对输入做的清洗开关
- `background_color_highlight`：`-H` 命中行的背景色

**每行**进 ch5 的 `print_line` 时：
1. 先按这几个开关清洗/解码（UTF-16、剥 ANSI、sanitize）
2. 调 `highlight_regions_for_line` 拿到 `Vec<(Style, &str)>`
3. 逐个片段上色、处理换行/截断、写出去

---

## 本章涉及的 Rust 机制（小白需要补的知识点）

1. **`Option<HighlighterFromSet>`**：`InteractivePrinter.highlighter_from_set`
   是 `Option`——"不需要匹配语法时（纯二进制没 as-text）是 `None`，
   此时 `highlight_regions_for_line` 直接返回整行套中性灰"。
2. **syntect 的有状态高亮器**：`HighlightLines` 内部记住"上一行结尾在什么
   语法上下文"，所以必须跨行复用（ch4 阶段 2 的 `HighlighterFromSet`）。
   这是"有状态迭代器/状态机"的典型——状态藏在调用者（`InteractivePrinter`）里。
3. **`match` 枚举 `Error::UndetectedSyntax`**：`get_syntax` 返回三种可能
   （拿到 / 检测不到 / 其它错），用 `match` 分别处理——
   "检测不到"退回 Plain Text，"其它错"（指定了不存在的语法名）直接上抛。
4. **`?` 和 `.expect()` 的混用**：`find_syntax_by_name(…)?` 是"可能失败就上抛"，
   `.expect("A plain text syntax is available")` 是"保证成功，否则 panic"——
   Plain Text 一定内置，所以 `expect` 安全。

## 检验问题

1. `HighlightLines` 为什么每个文件一个、跨行复用？如果每行新建会怎样？
2. 自动检测语法的四级优先级（`-l` → 路径 → 首行 → `--fallback-syntax`），
   哪一级最优先？`-m '*.conf:unknown'` 和 `--fallback-syntax INI` 哪个先触发？
3. 为什么"不上色"（`colored_output=false`）也可能要"匹配语法"？
4. 超长行（>16KB）为什么不细做语法高亮？`too_long` 分支怎么处理它？
5. `Manpage` / `CommandHelp` 语法为什么单独标记 `strip_overstrike`？man page 里 `\x08` 是干嘛的？

## 下一步

**ch5 — 逐行渲染（`print_line`）**：清洗、解码、装饰、tab 展开、
换行/截断、上色、写输出。syntect 给的 `Vec<(Style, &str)>` 怎么变成终端上
一行带颜色的字。这一章是最长也最核心的一章。
