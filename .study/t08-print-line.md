# t08：主路⑦ 高潮 —— print_file → print_file_ranges → print_line 全叶子

> 前面把 Printer/syntax/装饰器都装配好了（t07）。现在是"一行字变彩色字符"的核心：
> `print_file` 怎么组织 header/行/footer，`print_file_ranges` 怎么按行范围筛选+缓冲，
> `print_line` 的六步 + NoWrapping/换行两路。钻完这一篇，主路⑦就通了。

## 你在执行树的哪个位置

```
print_file（controller.rs:214）
├─ printer.print_header         ★ 叶子（文件头 / 二进制警告 / 横线）
├─ line_ranges（Ranges vs DiffContext）→ print_file_ranges
│    └─ 逐行 print_line         ← 你在这（printer.rs:656）★ 剧情高潮
│         ① 解码 ② 高亮 ③ 挤空行 ④ 画装饰 ⑤ 上色写出去(两路) ⑥ 收尾
└─ printer.print_footer
```

## 1. `print_file`（controller.rs:214）

```rust
fn print_file(&self, printer: &mut dyn Printer, writer, input: &mut OpenedInput,
              add_header_padding: bool, #[cfg(feature = "git")] line_changes) -> Result<()> {
    // ① header：首行非空 或 开了 header 组件就打印
    if !input.reader.first_line.is_empty() || self.config.style_components.header() {
        printer.print_header(writer, input, add_header_padding)?;
    }
    // ② 正文：首行非空才走
    if !input.reader.first_line.is_empty() {
        let line_ranges = match self.config.visible_lines {
            VisibleLines::Ranges(r) => r.clone(),                    // 默认：全显
            #[cfg(feature = "git")]
            VisibleLines::DiffContext(ctx) => {                        // -d --diff-context N
                // 把 git 改动的每个行号展开成 [N-ctx, N+ctx] 的 LineRange 列表
                …LineRanges::from(line_ranges)
            }
        };
        self.print_file_ranges(printer, writer, &mut input.reader, &line_ranges)?;
    }
    // ③ footer
    printer.print_footer(writer, input)?;
    Ok(())
}
```

三个叶子按顺序：**header → 正文（行范围筛选）→ footer**。
`add_header_padding`（= `!is_first`，controller.rs:204 传入）：非第一个文件时
header 前多画一道横线分隔（文件间分隔线）。

`DiffContext` 叶子（git feature）：`-d --diff-context N` 时，把 git diff 的改动行
每个展开成"前后 N 行上下文"的 `LineRange`，正文只打这些行——
配合 t06 的"diff 模式跳过无改动文件"，实现 `bat -d` 只看被改的地方。

## 2. `print_header` 的叶子（printer.rs:489）

钻几个关键分支：

- **空输入 + `quiet_empty`（-E）**（printer.rs:496）：直接 `return`，啥都不打
  （连 header 都没有）。
- **二进制警告**（printer.rs:505-516）：`content_type == BINARY` 且没 `-A`、
  不是 `as-text` 时，打一行 `yellow "[bat warning]: … will not be printed …"`
  就返回（不打印二进制内容，但管道里仍保留）。
- **文件头组件**（printer.rs:536）：`HeaderFilename` / `HeaderFilesize` 两个组件
  按**固定顺序**（Vec，不用 HashSet 的随机序）打印。开了 `grid` 就先画 `┬` 角线。

## 3. `print_file_ranges`：行范围筛选 + 缓冲（controller.rs:252）

这是"按 `-r`/`-d` 只打某些行"的实现，**核心是一个环形缓冲 + 三态判断**：

```rust
let buffer_size = line_ranges.largest_offset_from_end() + 1;   // 需要"往前看"多少行
let mut buffered_lines: VecDeque<(Vec<u8>, usize)> = with_capacity(buffer_size);

loop {
    if reached_eof && buffered_lines.is_empty() { break; }

    // ① 读一行填缓冲（-b 时空行不推进行号计数器）
    if !reached_eof {
        if reader.read_line(&mut current_line_buffer)? {
            if self.config.number_nonblank && 行是纯换行 { /* 不 current_line_number+=1 */ }
            else { current_line_number += 1; }
            buffered_lines.push_back((mem::take(&mut current_line_buffer), current_line_number));
        } else { reached_eof = true; }
    }
    if buffered_lines.len() < buffer_size && !reached_eof { continue; }  // 缓冲没填满先不吐

    // ② 取出队首一行，判断它"在不在可见范围"
    let Some((line, line_nr)) = buffered_lines.pop_front() else { break };
    let max_buffered_line_number = …(区分"文件末尾"还是"临时行尾"，给高亮器看);

    match line_ranges.check(line_nr, max_buffered_line_number) {
        BeforeOrBetweenRanges => {
            // 范围外：仍调 print_line(out_of_range=true) —— 只为让 syntect 高亮器推进状态！
            // 但 print_line 内部见 out_of_range 会提前 return（不真正写输出）
            printer.print_line(true, writer, line_nr, &line, max_buffered_line_number)?;
        }
        InRange => {
            // 范围内：先按需画 snip 分隔线（"…"），再真正打印
            if style_snip { /* 进到新范围时 print_snip */ }
            printer.print_line(false, writer, line_nr, &line, max_buffered_line_number)?;
            if self.config.unbuffered { writer.flush()?; }   // -u 流式：每行立刻刷出
        }
        AfterLastRange => break,   // 过了最后一个范围，收工
    }
}
```

**三个设计点值得记**：

1. **范围外的行也喂给 `print_line`（`out_of_range=true`）**（controller.rs:326）：
   syntect 高亮器是**有状态的**（t07 §5 讲过），必须把跳过的行也过一遍高亮器
   才能"追上"上下文，否则跳回范围内的行高亮会错。`print_line` 见 `out_of_range`
   就高亮完直接 `return`（不写输出）——**"算状态"和"打不打"分离**。
2. **`-b` 空行不推进行号**（controller.rs:282-287）：`number_nonblank` 时，
   纯空行的行号计数器不 +1，行号才"只编非空行"。
3. **`-u` 每行 `flush`**（controller.rs:342-344）：流式模式立刻刷出，
   `tail -f log | bat -u` 才能实时看到。

## 4. `print_line` 六步（printer.rs:656）★ 高潮

一行进来，按顺序：

### ① 解码（printer.rs:664-707）

```rust
let line = if self.config.show_nonprintable {
    replace_nonprintable(line_buffer, tab_width, notation).into()   // -A：tab→├┤、\n→␊
} else {
    match self.content_type {
        Some(BINARY) | None if !AsText => return Ok(()),            // 二进制不打（除非 as-text）
        Some(UTF_16LE) => UTF_16LE.decode_with_bom_removal(buf).0,
        Some(UTF_16BE) => UTF_16BE.decode_with_bom_removal(buf).0,
        _ => { /* UTF-8：from_utf8_lossy，首行去 BOM */ }
    };
    if self.strip_overstrike { … }        // man page 的退格粗体
    if self.sanitize { line = sanitize(&line).into() }              // 严格超集：光标/bidi/零宽→U+FFFD
    else if self.strip_ansi { line = strip_ansi(&line).into() }     // 只去颜色码
};
```

二进制文件在这一步就被拦掉（`return Ok(())`，除非 `--binary=as-text`）——
和 t06 的编码嗅探、t07 的 `is_printing_binary` 串起来。

### ② 高亮（printer.rs:709）

```rust
let regions = self.highlight_regions_for_line(&line)?;   // syntect：一行 → Vec<(Style, &str)>
if out_of_range { return Ok(()); }                        // 范围外：高亮完不打印（§3 设计点1）
```

`highlight_regions_for_line`（printer.rs:453）调 t07 §5 的 `HighlightLines`（有状态），
把一行切成"带 syntect 样式的片段"列表。没选中语法时整行套中性灰。

### ③ 挤压空行（printer.rs:715-724）

```rust
if let Some(limit) = self.config.squeeze_lines {         // -s N
    if 行是空 { self.consecutive_empty_lines += 1;
        if self.consecutive_empty_lines > limit { return Ok(()); } }   // 超过 N 个空行就跳过
    else { self.consecutive_empty_lines = 0; }
}
```

`consecutive_empty_lines` 是 printer 的**实例字段**（跨行计数，t07 讲过"有状态提层持有"）。

### ④ 画左侧装饰（printer.rs:748-765）

```rust
if self.panel_width > 0 {
    let display_line_number = if self.config.number_nonblank && 行是空 { 0 } else { line_number };
    for deco in self.decorations.iter().map(|d| d.generate(display_line_number, false, self)) {
        write!(handle, "{} ", deco.text)?;        // 行号 / git 标记 逐个写到行左
        cursor_max -= deco.width + 1;              // 内容区宽度相应缩短
    }
}
```

`decorations` 就是 t07 §3 装配的 `Vec<Box<dyn Decoration>>`，逐个 `generate` 出
"这一行的行号文本"，写到行首；`cursor_max`（内容最大宽度）随装饰宽度递减。

### ⑤ 逐片段上色 + 写出去 —— 两路（printer.rs:768 起）★ 最绕

按 `wrapping_mode` 分两条路：

**路 A：`NoWrapping`（不截断，printer.rs:768-826）**
```rust
for &(style, region) in &regions {
    let ansi_iterator = EscapeSequenceIterator::region);   // vscreen 解析：把 region 切成 Text/ANSI 段
    for chunk in ansi_iterator {
        match chunk {
            Text(text) => {
                let text = self.preprocess(text, &mut cursor_total);   // tab 展开（preprocessor.rs）
                write!(handle, "{}{}",
                    as_terminal_escaped(style, &format!("{}{text}", self.ansi_style),
                        true_color, colored_output, italics, background_color),
                    self.ansi_style.to_reset_sequence());
            }
            _ => { write!(handle, "{}", chunk.raw());   // ANSI 段原样透传
                   self.ansi_style.update(chunk); }     // 并更新"当前样式状态"
        }
    }
}
```
整行一行打出去，不截断不换行（`--wrap=never` / `chop-long-lines`）。

**路 B：换行（`Character`/`Word`，printer.rs:827-972）**
```rust
for grapheme in text.graphemes(true) {          // 按"字形"（Unicode 组合字符）遍历
    let cw = grapheme_width(grapheme);          // 全角/半角宽度
    current_width += cw;
    if word_wrap && 这是空白 { last_ws_idx = Some(当前 buffer 位置); }   // 记词边界
    if current_width > max_width {              // 超宽 → 换行
        // 懒生成 panel_wrap：换行后新行的左侧装饰（续行缩进）
        if panel_wrap.is_none() { panel_wrap = Some(把 decorations 再 generate 一次拼起来); }
        // word 模式：在最近的空白处断行，剩余带到下一行；character 模式：硬切
        write!(handle, "{}{}\n{}", 上色(断点前), reset, panel_wrap);   // 断点前 + 换行 + 续行装饰
        // 续行重设 cursor/current_width，把剩余字符带回
    }
    line_buf.push_str(grapheme);
}
```

换行的精髓：**`panel_wrap` 是懒生成的**——只有真发生换行才去 `generate` 一次续行
左侧装饰（行号会重复显示在续行首），避免每行都算。`word` 模式用 `last_ws_idx`
找"最近的空格"断行（不留半词），`character` 模式按字形硬切。

### ⑥ 收尾（printer.rs:974 起）

`-H` 高亮行的下划线关闭、背景色填充到行尾等收尾。

**Rust 机制 · `as_terminal_escaped`（最后一跳，printer.rs 底 + terminal.rs）**：
把 syntect 的 `Style`（RGB 色值）翻译成终端能懂的 ANSI 序列。`true_color` 决定
24bit 原色还是 `to_ansi_color` 降级 16 色调色板；`colored_output` 为 false 时
干脆不包任何颜色码。这是"syntect 的色 → 你终端屏幕上看到的色"的最终翻译层。

**Rust 机制 · `graphemes(true)`（Unicode 感知）**：为什么按 grapheme 不按 char？
一个"字"可能是多个 codepoint 组合（如 `é` = e + 结合符）。`char` 会把它拆成两截
导致宽度/断行错乱。`unicode-segmentation` 的 `graphemes` 按"人类看到的一个字"切，
这是**终端渲染正确性的关键**（尤其 CJK、表情、组合字符）。

## 本区段 Rust 机制

1. **`out_of_range` 分离"算状态"与"打不打"**：有状态高亮器必须吃全部行，
   但范围外的行不输出——用一个 bool 参数把两件事解耦（§3 设计点 1）。
2. **惰性计算**（`panel_wrap`）：`Option` 存"还没算"，真用到才算一次（换行时）。
3. **`Vec` 而非 `HashSet` 保序**（`print_header` 的 `header_components`，printer.rs:536）：
   组件打印要**确定顺序**（文件名在大小前），HashSet 序随机，故 `filter_map` 收进 Vec。

## 检验问题

1. `-r 30:40` 只打 30-40 行，29 行及以下的高亮器状态对吗？靠 `print_file_ranges` 的哪个设计保证（`out_of_range` 那步）？
2. `--wrap=word` 一行超宽，断点怎么选（和 `--wrap=character` 差在哪）？续行左侧的行号哪来的（`panel_wrap` 何时生成）？
3. 为什么换行/宽度计算按 `graphemes(true)` 而不按 `chars`？举一个按 char 会错的例子。
4. `tail -f log | bat -u` 为什么"看到"了实时内容？哪一行 `flush`？为什么同时自动关了行号？

## 返回与下一站

**`print_line` 六步钻完，回到 `print_file` 收尾（footer），再回到
`run_with_error_handler` 的 `for input` 循环处理下一个文件 / 循环结束。**
循环跑完，所有文件吐完 → 最后定退出码。→ **t09**。
