# 第 5 章：逐行渲染（`print_line`）

这一章的目标：bat **最长最核心**的一章——ch4 产出的"每行 `Vec<(Style, &str)>`"
拿到手后，怎么**清洗、解码、画装饰、展开 tab、换行/截断、上色、写出去**。
整章围绕 `InteractivePrinter::print_line`（printer.rs:648，300+ 行）。
读完你应该能回答："一行带样式的片段，怎么一步步变成终端上一行带颜色的字？"

## 涉及文件

- `src/printer.rs` — `print_line` + `preprocess`（tab 展开）
- `src/terminal.rs` — `as_terminal_escaped` / `to_ansi_color`（syntect Style → 终端转义串）
- `src/preprocessor.rs` — `replace_nonprintable` / `strip_ansi` / `sanitize` / `expand_tabs`
- `src/vscreen.rs` — `EscapeSequenceIterator`（解析输入里可能藏的 ANSI）

## 全景图（先有这个概念，再看代码）

```
ch4 产出：Vec<(Style, &str)>   每行喂 highlight_regions_for_line 拆出的样式片段
   │
   ▼
print_line(out_of_range, …)    （printer.rs:648）
   │
   ├─ ① 解码：二进制? UTF-16? 剥 BOM? 剥 overstrike? strip/sanitize ANSI?
   ├─ ② 高亮：highlight_regions_for_line → Vec<(Style, &str)>
   │        （out_of_range=true 时喂引擎但"不打印"，见下）
   ├─ ③ 挤压空行（-s）
   ├─ ④ 画装饰：行号 / git 标记 / 网格竖线（cursor_max 减去装饰宽）
   ├─ ⑤ 逐片段上色 + 写（NoWrapping 路 A / 换行 路 B）
   └─ ⑥ 高亮行收尾（-H：关下划线 / 背景色填空格）
   │
   ▼
write!(handle, …)  →  终端 / less stdin / minus（ch6 决定落到哪）
```

**为什么 `out_of_range` 参数？** ch4 的 `controller.print_file_ranges` 读一行时，
可能这行**不在 `--line-range` 范围内**（被过滤掉了）。但 syntect 高亮器是
有状态的，**跳过的行也得喂给引擎**（不然上下文断），只是"不打印"。
所以 `print_line(true, ...)` = "喂引擎但别写出来"，`false` = "正常打印"。
  │
  ▼ ④ 画行左侧装饰（行号 / git 标记 / 网格竖线）
  │
  ▼ ⑤ 逐片段上色 + 写出去（按 wrapping_mode 分"不截断" / "换行"两条路）
  │
  ▼ ⑥ 高亮行（-H）收尾：关下划线
  │
终端上的一行彩色文字
```

**为什么 `out_of_range` 参数？** ch4 的 `controller.print_file_ranges` 读一行时，
可能这行**不在 `--line-range` 范围内**（被过滤掉了）。但 syntect 高亮器是
有状态的，**跳过的行也得喂给引擎**（不然上下文断），只是"不打印"。
所以 `print_line(true, ...)` = "喂引擎但别写出来"，`false` = "正常打印"。

---

## ① 解码（printer.rs:656）

```rust
fn print_line(
    &mut self,
    out_of_range: bool,
    handle: &mut OutputHandle,
    line_number: usize,
    line_buffer: &[u8],
    max_buffered_line_number: MaxBufferedLineNumber,
) -> Result<()> {
    let line = if self.config.show_nonprintable {
        // -A：把 tab/换行/控制字符画成可读符号（ch2 的 nonprintable_notation）
        replace_nonprintable(line_buffer, self.config.tab_width, self.config.nonprintable_notation).into()
    } else {
        let mut line = match self.content_type {
            // 二进制 且 没强制 as-text → 这一行直接跳过（啥都不打）
            Some(ContentType::BINARY) | None if !matches!(self.config.binary, BinaryBehavior::AsText) =>
                return Ok(()),
            // UTF-16LE/BE → 解码成 UTF-8（剥 BOM）
            Some(ContentType::UTF_16LE) => UTF_16LE.decode_with_bom_removal(line_buffer).0,
            Some(ContentType::UTF_16BE) => UTF_16BE.decode_with_bom_removal(line_buffer).0,
            // 普通 UTF-8（含 lossy 处理非法字节）
            _ => {
                let line = String::from_utf8_lossy(line_buffer);
                // 首行可能带 BOM（\u{feff}）→ 剥掉
                if line_number == 1 {
                    match line.strip_prefix('\u{feff}') {
                        Some(stripped) => stripped.to_string().into(),
                        None => line,
                    }
                } else { line }
            }
        };

        // man page / help 的粗体：X\x08X（字符+退格+同字符）
        if self.strip_overstrike {
            if let Some(pos) = line.find('\x08') {
                line = strip_overstrike(&line, pos).into();
            }
        }

        // ch4 决定的清洗：sanitize（超集）优先，否则 strip_ansi
        if self.sanitize {
            line = sanitize(&line).into()
        } else if self.strip_ansi {
            line = strip_ansi(&line).into()
        }

        line
    };

    // ② 喂引擎拆片段（ch4 详解）
    let regions = self.highlight_regions_for_line(&line)?;
    if out_of_range {
        return Ok(());   // ★ 喂了引擎但"这行被 line-range 过滤"→ 不打印
    }
    // …③④⑤⑥ 在下面
}
```

**小白会问：`from_utf8_lossy` 是什么？直接 `from_utf8` 报错不更好？**

Lossy 把**非法字节**换成 U+FFFD（替换符）而不是报错。
bat 读的文件可能含截断的 UTF-8（二进制被当文本、损坏文件），
`lossy` 保证"能读多就读多"，不因一个坏字节整行崩掉。
这是"宽容读取"的常见套路。

**①里 `replace_nonprintable`（preprocessor.rs:59）**——`-A` 时的核心：

```rust
pub fn replace_nonprintable(input: &[u8], tab_width: usize, notation) -> String {
    // 逐字符走，按类型替换：
    //   空格 ' ' → '·'
    //   tab  '\t' → '↹'（1列宽）或 '├──┤'（按 tab_width 对齐）
    //   换行 '\x0A' → '^J\x0A'（caret 记法）或 '␊\n'（unicode 记法）
    //   C0 控制字符（\x00-\x1F）→ '^@'..' ^_'（caret）或 '␀'..'␟'（unicode）
    //   DEL（\x7F）→ '^?' 或 '␡'
    //   可打印 ASCII → 原样
    //   其它 → \u{XXXX} 转义
}
```
tab 的 `├──┤` 很讲究：它按 `tab_width` 计算"从当前位置到下一个 tab 停靠点要几列"，
画出来的横线长度正好对齐，视觉上和真 tab 一样。

**`strip_ansi`（preprocessor.rs:140）** 简单：用 vscreen 的迭代器
只保留 `Text` 段、丢掉所有转义序列。
**`sanitize`（preprocessor.rs:152）** 更强：除剥 ANSI 外，把"会操作终端"的字节
（C0 控制、C1、DEL、bidi 控制字符 U+202A-F、零宽字符 U+200B-D、BOM U+FEFF）
全换成 U+FFFD。这是给"文件管理器预览面板"看**不可信**文件用的——
防止文件里的转义序列劫持你的终端。

---

## ③ 挤压空行（`-s`，printer.rs:707）

```rust
    // Skip squeezed lines.
    if let Some(squeeze_limit) = self.config.squeeze_lines {
        if line.trim_end_matches(['\r', '\n']).is_empty() {   // 是空行？
            self.consecutive_empty_lines += 1;
            if self.consecutive_empty_lines > squeeze_limit {
                return Ok(());   // 超过限制 → 这一行不打印
            }
        } else {
            self.consecutive_empty_lines = 0;   // 非空行，计数清零
        }
    }
```
`-s`（squeeze-blank）默认 `squeeze_limit=1`：连续空行只保留 1 行。
ch2 讲过 `squeeze_lines` 字段；`consecutive_empty_lines` 是 `InteractivePrinter`
的实例字段（每文件重置为 0），跨行累计。

---

## ④ 行左侧装饰（printer.rs:739）

```rust
    let mut cursor: usize = 0;            // 当前"已打印"的列位置
    let mut cursor_max: usize = self.config.term_width;   // 一行最多多少列（换行基准）
    let mut cursor_total: usize = 0;      // tab 展开用（跨片段累计列）
    let mut panel_wrap: Option<String> = None;   // 续行用的"装饰前缀"（懒生成）

    // -H 命中本行？（看 config.highlighted_lines）
    let highlight_this_line = self.config.highlighted_lines.0
        .check(line_number, max_buffered_line_number) == RangeCheckResult::InRange;
    if highlight_this_line && self.config.theme == "ansi" {
        self.ansi_style.update(ANSI_UNDERLINE_ENABLE);   // ansi 主题用下划线标记
    }
    // 其它主题用背景色标记
    let background_color = self.background_color_highlight.filter(|_| highlight_this_line);

    // 画装饰（行号/git 标记/网格竖线）
    if self.panel_width > 0 {
        let display_line_number =
            if self.config.number_nonblank && line.trim_end_matches(['\r','\n']).is_empty() {
                0                                    // -b 模式空行不显示行号
            } else { line_number };
        // 每个装饰组件算它自己的文本+宽度（ch7 详解）
        let decorations = self.decorations.iter()
            .map(|d| d.generate(display_line_number, false, self));
        for deco in decorations {
            write!(handle, "{} ", deco.text)?;     // 写出 "  42 │ " 这类前缀
            cursor_max -= deco.width + 1;           // 内容区宽度 = 终端宽 - 装饰宽
        }
    }
```

**`cursor_max` 的含义**（小白重点）：
一开始 `cursor_max = term_width`（整行宽）。
画完行号（4 列）+ 空间（1 列）后，`cursor_max` 变成 `term_width - 5`。
这就是"内容区还能放多少列"——**换行/截断都看它**，而非原始终端宽。
有装饰时内容区变窄，没装饰（panel_width=0）就整个终端宽。

`Decoration::generate(line_number, continuation, printer)` 三参：
- `line_number`：画哪行
- `continuation`：是不是"续行"（一行被换行折成第二行时，左边装饰区要补空格/竖线）
- `printer`：要读 `colors`/`line_changes`/`panel_width` 等
ch7 把 `LineNumberDecoration` / `GridBorderDecoration` / `LineChangesDecoration`
三个实现逐个拆。

---

## ⑤ 逐片段上色 + 写出去（最绕的部分，printer.rs:760）

`regions` 是 `Vec<(Style, &str)>`。先**剥 ANSI 转义序列**迭代（输入可能带颜色）：
```rust
for &(style, region) in &regions {
    let ansi_iterator = EscapeSequenceIterator::new(region);  // vscreen 解析
    for chunk in ansi_iterator {
        match chunk {
            // 普通文本片段
            EscapeSequence::Text(text) => { /* 上色+写 */ }
            // 输入里自带的转义序列（颜色）→ 原样透传，更新 ansi_style 状态
            _ => {
                write!(handle, "{}", chunk.raw())?;
                self.ansi_style.update(chunk);
            }
        }
    }
}
```

`Text` 片段怎么上色？这里**分两条路**，取决于 `wrapping_mode`：

### 路 A：不截断（`NoWrapping`，printer.rs:760-818）

```rust
if matches!(self.config.wrapping_mode, WrappingMode::NoWrapping(_)) {
    for &(style, region) in &regions {
        let ansi_iterator = EscapeSequenceIterator::new(region);
        for chunk in ansi_iterator {
            match chunk {
                EscapeSequence::Text(text) => {
                    let text = self.preprocess(text, &mut cursor_total);  // tab 展开
                    let text_trimmed = text.trim_end_matches(['\r','\n']);
                    // ★ 上色 + 写
                    write!(handle, "{}{}",
                        as_terminal_escaped(
                            style,                                  // syntect 给的 Style
                            &format!("{}{text_trimmed}", self.ansi_style),  // 拼"输入自带样式 + 文本"
                            true_color, colored_output, italics, background_color),
                        self.ansi_style.to_reset_sequence(),        // 收尾复位
                    )?;
                    // 行尾（被 trim 掉的 \r\n）若有背景高亮，补空格填满
                    if text.len() != text_trimmed.len() {
                        if let Some(background_color) = background_color {
                            let ansi_style = Style { background: to_ansi_color(background_color, true_color), ..Default::default() };
                            let width = if cursor_total <= cursor_max { cursor_max - cursor_total + 1 } else { 0 };
                            write!(handle, "{}", ansi_style.paint(" ".repeat(width)))?;
                        }
                        write!(handle, "{}", &text[text_trimmed.len()..])?;
    }
                _ => { /* 透传转义序列 */ }
            }
        }
    }
    // 非 plain 且行尾没 \n → 补一个换行（最后一段没换行的情况）
    if !self.config.style_components.plain() && line.bytes().next_back() != Some(b'\n') {
        writeln!(handle)?;
    }
}
```

**`preprocess`（printer.rs:470）就是 tab 展开**：
```rust
fn preprocess(&self, text: &str, cursor: &mut usize) -> String {
    if self.config.tab_width > 0 {
        return expand_tabs(text, self.config.tab_width, cursor);  // 展开
    }
    *cursor += text.len();   // tab_width==0：不展开，cursor 直接跳
    text.to_string()
}
```
`expand_tabs`（preprocessor.rs:9）把 `\t` 换成"到下一个 tab 停靠点"的空格数，
`cursor` 跨片段累计列位置，保证多片段共享同一对齐基准。

### 路 B：换行（`Character` / `Word`，printer.rs:819-972）

交互终端里一行可能**超过终端宽度**，得**自动折行**。这条比路 A 长很多：

```rust
} else {
    for &(style, region) in &regions {
        let ansi_iterator = EscapeSequenceIterator::new(region);
        for chunk in ansi_iterator {
            match chunk {
                EscapeSequence::Text(text) => {
                    let text = self.preprocess(text.trim_end_matches(['\r','\n']), &mut cursor_total);
                    let mut max_width = cursor_max - cursor;     // 当前行还能放多少列

                    // 逐字符写进临时 line_buf，攒到放不下再刷出去
                    let mut line_buf = String::with_capacity(max_width * 4);
                    let mut current_width = 0;
                    let word_wrap = matches!(self.config.wrapping_mode, WrappingMode::Word);
                    let mut last_ws_idx: Option<usize> = None;  // Word 模式记住最后空白位置

                    for c in text.chars() {
                        let cw = char_width(c);                 // 字符显示宽度（汉字=2，控制符=2）
                        current_width += cw;
                        if word_wrap && c.is_whitespace() {
                            last_ws_idx = Some(line_buf.len()); // Word 模式：在空白处折
                        }
                        // 加这个字符就超宽 → 在这里折行
                        if current_width > max_width {
                            if panel_wrap.is_none() {            // 懒生成"续行装饰前缀"
                                panel_wrap = if self.panel_width > 0 {
                                    Some(format!("{} ", self.decorations.iter()
                                        .map(|d| d.generate(line_number, true, self).text)
                                        .collect::<Vec<String>>().join(" ")))
                                } else { Some(String::new()) };
                            }
                            // 决定折点：Word 模式在"最后空白"折，Character 模式直接折
                            let (emit_end, rest_start) = if word_wrap {
                                if let Some(ws_idx) = last_ws_idx {
                                    // 跳过空白本身，剩余字符带到下一行
                                    let rs = ws_idx + line_buf[ws_idx..].chars().next().map(|ch| ch.len_utf8()).unwrap_or(0);
                                    (ws_idx, Some(rs))
                                } else { (line_buf.len(), None) }
                            } else { (line_buf.len(), None) };
                            // 刷出已折部分 + 换行 + 续行前缀
                            write!(handle, "{}{}\n{}",
                                as_terminal_escaped(style, &format!("{}{}", self.ansi_style, &line_buf[..emit_end]),
                                    self.config.true_color, self.config.colored_output, self.config.use_italic_text, background_color),
                                self.ansi_style.to_reset_sequence(),
                                panel_wrap.clone().unwrap())?;
                            cursor = 0;
                            max_width = cursor_max;             // 新一行重新算可放宽度
                            if let Some(rs) = rest_start {      // Word 模式：剩余字符带回
                                let remainder = line_buf[rs..].to_string();
                                let rem_width: usize = remainder.chars().map(char_width).sum();
                                line_buf.clear();
                                line_buf.push_str(&remainder);
                                current_width = rem_width + cw;
                            } else {
                                line_buf.clear();
                                current_width = cw;
                            }
                            last_ws_idx = None;
                        }
                        line_buf.push(c);
                    }
                    // 刷出这一段剩余部分
                    cursor += current_width;
                    write!(handle, "{}", as_terminal_escaped(
                        style, &format!("{}{line_buf}", self.ansi_style),
                        self.config.true_color, self.config.colored_output,
                        self.config.use_italic_text, background_color))?;
                }
                _ => { write!(handle, "{}", chunk.raw())?; self.ansi_style.update(chunk); }
            }
        }
    }
    // 背景高亮：把行尾（最后一段之后）也涂满背景色
    if let Some(background_color) = background_color {
        let ansi_style = Style { background: to_ansi_color(background_color, self.config.true_color), ..Default::default() };
        write!(handle, "{}", ansi_style.paint(" ".repeat(cursor_max - cursor)))?;
    }
    writeln!(handle)?;
}
```

**`char_width`（printer.rs:49）的小细节**：
```rust
fn char_width(c: char) -> usize {
    c.width().unwrap_or(if c.is_control() { 2 } else { 0 })
}
```
普通字符 `width()` 给 1（或汉字 2）。**控制字符**（`^@`..`^_`）
`width()` 返回 `None`，但它们在终端上按"caret 记法"（`^@`）占 **2 列**，
所以兜底成 2。这个细节影响"截断时算得准不准"。

**小白会问：`panel_wrap` 为什么要"懒生成"（`is_none()` 才建）？**
因为**大多数行不会换行**（正常文本一行放得下）。只有真正折行时
才需要"续行装饰前缀"（第二行左边补空格对齐行号区）。
懒生成 = 不折行的行**零成本**，避免每行都算一遍装饰前缀字符串。

---

## ⑥ 收尾：高亮行关下划线（printer.rs:974）

```rust
    if highlight_this_line && self.config.theme == "ansi" {
        write!(handle, "{}", ANSI_UNDERLINE_DISABLE.raw())?;
        self.ansi_style.update(ANSI_UNDERLINE_DISABLE);
    }

    Ok(())
}
```
`-H` 在 ansi 主题里用**下划线**标记高亮行（`@4m` 开、`@24m` 关）。
其它主题用背景色（⑤ 末尾的 `background_color` 填空格那块）。

---

## `as_terminal_escaped` —— "syntect Style → 终端字符串"的最后一跳

`src/terminal.rs:49`。ch4 的 `highlight_line` 给的是 syntect 的 `Style`
（前景/背景/粗斜体，纯 RGB），**不是终端能直接打的字符串**。
`as_terminal_escaped` 把它转成带 `\x1b[...m` 转义的可打印字符串：

```rust
pub fn as_terminal_escaped(
    style: highlighting::Style,   // syntect 的样式
    text: &str,                   // 要上色的文本（已拼好输入自带样式）
    true_color: bool,             // 终端支持 24bit？
    colored: bool,               // 要不要上色
    italics: bool,               // 斜体
    background_color: Option<highlighting::Color>,  // -H 命中行的背景
) -> String {
    if text.is_empty() { return text.to_string(); }

    let mut style = if !colored {
        Style::default()          // 不上色 → 默认样式
    } else {
        let mut color = Style {
            foreground: to_ansi_color(style.foreground, true_color),  // ★ 核心转换
            ..Style::default()
        };
        if style.font_style.contains(FontStyle::BOLD) { color = color.bold(); }
        if style.font_style.contains(FontStyle::UNDERLINE) { color = color.underline(); }
        if italics && style.font_style.contains(FontStyle::ITALIC) { color = color.italic(); }
        color
    };
    style.background = background_color.and_then(|c| to_ansi_color(c, true_color));
    style.paint(text).to_string()   // nu_ansi_term 的 paint → 带转义的字符串
}
```

`to_ansi_color`（terminal.rs:6）是**颜色降级**的核心：
```rust
pub fn to_ansi_color(color: highlighting::Color, true_color: bool) -> Option<nu_ansi_term::Color> {
    if color.a == 0 {
        // 主题用 #RRGGBBAA 编码"终端 8/16 色"（a=00, r=调色板号）
        // 前 8 色用基本 ANSI（\x1b[31m 红），8+ 用 256 色（\x1b[38;5;N）
        Some(match color.r { 0x00 => Black, 0x01 => Red, /* ... */ 0x07 => White, n => Fixed(n) })
    } else if color.a == 1 {
        // a=01 编码"用终端默认前景/背景"（ ansi 主题这么做）→ 返回 None（不加转义）
        None
    } else if true_color {
        Some(Rgb(color.r, color.g, color.b))   // 真 24bit
    } else {
        Some(Fixed(ansi256_from_rgb((color.r, color.g, color.b))))  // 降级到 256 色
    }
}
```

**小白会问：为什么主题能用 `#RRGGBBAA` 这种"RGBA 编码 RGB"？**

syntect 的 `Color` 是固定 4 字节 RGBA。bat 借用"透明通道 alpha"的取值**额外编码含义**：
- `a=00`：这是"终端 8/16 色调色板"，`r` 字节是调色板号（不是红值）
- `a=01`：这是"终端默认色"（不加转义）
- `a=其它`：正常 24bit RGB
这样**同一个 `Color` 结构体能表达"真 24bit"和"调色板编号"两种语义**，
`to_ansi_color` 据此决定输出哪种转义序列。主题作者就能"用默认色"或"指定 256 色"
而不用 24bit。

---

## 本章涉及的 Rust 机制（小白需要补的知识点）

1. **`write!` 的"输出目标无关"**：`write!(handle, …)` 对 `handle`
   （`OutputHandle`，ch6 讲它接到哪）写，**不关心**目标是终端还是 less。
   这是"面向 `Write` trait 写"的好处——渲染逻辑和输出目标解耦。
2. **`vscreen::EscapeSequenceIterator`（第三方库）**：把一行可能**藏着
   ANSI 转义序列**的字符串拆成 `Text` 段 + 转义序列段。bat 用它解析
   "输入本身带颜色"（比如 `bat` 读一个已上色的 `.txt`），
   再决定透传还是剥掉（`strip_ansi`/`sanitize`，ch2 字段）。
3. **`char_width` 对控制字符算 2**：`UnicodeWidthChar::width()` 对 `^@` 等
   控制字符返回 `None`，但终端上它们按"caret 记法"（`^@`）占 **2 列**，
   所以 `char_width` 兜底成 2——影响"截断时算得准不准"。
4. **`lazy` 的 `panel_wrap`**（`Option<String>`）：续行装饰前缀只在**真正
   折行时**才生成（`if panel_wrap.is_none()`），不折行的行零成本——
   "昂贵计算惰性化"的又一个例子（和 ch3 `InputReader.metadata` 的 `OnceCell` 同款）。

## 检验问题

1. `out_of_range=true` 时为什么还要喂 `highlight_regions_for_line`？不喂会怎样？
2. `cursor_max` 为什么一开始是 `term_width`，画完装饰要减？`panel_width=0` 时它等于什么？
3. `Word` 模式和 `Character` 模式折行点怎么选？`last_ws_idx` 记的是哪个位置？
4. `panel_wrap` 为什么懒生成？哪行代码触发它？
5. `to_ansi_color` 里 `a==1` 返回 `None` 意味着什么？ansi 主题为什么爱用它？
6. 控制字符（`^@`）显示宽度为什么算 2 不算 0/1？

## 下一步

**ch6 — 输出与分页**：`OutputHandle` 接到哪、`OutputType::from_mode`
怎么决定"起 less / 起内嵌 minus / 直接 stdout"，以及"防 IO 圈""less 版本探测"。
