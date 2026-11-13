# t06：主路⑦ 中段 —— print_input → Input::open → 编码嗅探全分支

> 输出目标定好了（t05），现在 `for input in inputs` 循环里对**每个输入**调
> `print_input`。本篇钻：`print_input` 怎么"打开"一个输入（stdin / 普通文件 / 自定义
> reader 三个子分支），以及编码怎么被嗅探出来（UTF-8/16/二进制）。
> 钻完返回循环，下一站是"选 Printer + 选 syntax"（t07）。

## 你在执行树的哪个位置

```
Controller::run_with_error_handler
└─ for input in inputs:
   └─ print_input            ← 你在这里（controller.rs:141）
      ├─【子分支】lessopen 预处理（feature 门控）
      ├─ Input::open         ← 打开输入
      │    ├─ StdIn          ★ 叶子（防 IO 圈）
      │    ├─ OrdinaryFile   ★ 叶子（File::open + 防 IO 圈 + 目录特判）
      │    └─ CustomReader   ★ 叶子（库调用方喂的字节）
      │         └─ InputReader::try_new  → 编码嗅探全分支
      ├─【子分支】git diff 获取（feature 门控）
      └─ 选 Printer → print_file → …（t07/t08）
```

## 1. `print_input` 的开头（controller.rs:141）

```rust
fn print_input(&self, input: Input, writer, stdin, stdout_identifier, is_first) -> Result<()> {
    let mut opened_input = {
        #[cfg(feature = "lessopen")]
        match self.preprocessor {
            Some(ref p) if self.config.use_lessopen => p.open(input, stdin, stdout_identifier)?,
            _ => input.open(stdin, stdout_identifier)?,
        }
        #[cfg(not(feature = "lessopen"))]
        input.open(stdin, stdout_identifier)?          // 大多数构建走这
    };
    opened_input.reader.unbuffered = self.config.unbuffered;   // 把 -u 开关灌进 reader
    …
```

两个子分支先钻：

### 子分支：lessopen 预处理（feature 门控）

`lessopen` 功能：某些系统上 less 通过 `$LESSOPEN` 钩子调 bat 来高亮 man 页。
若开了此 feature 且 `use_lessopen` 为真，`input.open` 前先过一遍
`LessOpenPreprocessor`（`src/lessopen.rs`）——它把"less 打开的文件路径"还原成
真实的文件内容再喂给 bat（因为 lessopen 场景下 stdin 里可能是占位符）。
**没开 feature 时整段消失**（`#[cfg(not(feature = "lessopen"))]`），只走
`input.open`。这是 t02 讲的"feature = 编译期裁剪"的又一体现。

`stdin` 参数从哪来：回看 t05 §6，循环里 `input.is_stdin()` 时传
`io::stdin().lock()`，否则传 `io::empty()`（controller.rs:115-119）——
非 stdin 输入根本不用 stdin，但函数签名要 `BufRead`，用 `io::empty()`
（永远读不到字节）满足类型又无副作用（#1902 的坑，t05 §6 讲过）。

## 2. `Input::open` 三子分支（src/input.rs:192）

`Input` 有**三种原料**（`InputKind`，input.rs:74）：

```rust
pub(crate) enum InputKind<'a> {
    OrdinaryFile(PathBuf),              // 普通文件
    StdIn,                              // 标准输入
    CustomReader(Box<dyn Read + 'a>),   // 库调用方喂的字节流
}
```

`open` 按 `kind` 三选一，每个都是一个叶子：

### 叶子：`StdIn`（input.rs:152）

- 先**防 IO 圈**：输入是 stdin、且 stdout 也绑到同一终端（`stdout_identifier`
  `surely_conflicts_with`）→ 报 "IO circle detected"。`cat - | bat > 同一处` 会死循环，拦掉。
- 造 `InputReader::try_new(stdin)`（编码嗅探在 §3）。

### 叶子：`OrdinaryFile`（input.rs:168）

- `File::open`，打不开 → 友好报错（路径先 `sanitize_for_terminal` 去掉控制字符）。
- **目录特判**（input.rs:176）：`metadata.is_dir()` → 报 "'path' is a directory"
  （`bat some_dir/` 不该静默失败）。
- **同样防 IO 圈**（input.rs:180-186）：文件 == stdout 重定向目标 → 报错。

### 叶子：`CustomReader`（input.rs:191）

库调用方 `PrettyPrinter::input_from_bytes`（architecture.md 判断 1 的"库身份"）喂进来的
字节流，直接 `BufReader::new(reader)` 包一层读。

**Rust 机制 · `Box<dyn Read + 'a>`**：`CustomReader` 装的是"任何实现了 `Read` 的类型"
（trait 对象）。`+ 'a` 是生命周期约束——它借的内存得活过 `Input`。
"存进字段"用 `Box<dyn>`，"传参"可用泛型 `R: Read`（`open` 的 `stdin: R` 就是泛型）
——**存与传的两种写法**（t01 §2d 初见面，这里落地）。

## 3. 叶子：编码嗅探 `InputReader::try_new`（input.rs:267）

**这是本篇最该吃透的一段**——它干两件事：嗅探编码 + 缓存首行。

```rust
pub(crate) fn try_new<R: BufRead + 'a>(mut reader: R) -> io::Result<InputReader<'a>> {
    // 1) fill_buf 拿"已在缓冲区的字节"（最多 1024）——不消费输入
    let mut inspection_prefix = {
        let buffered = reader.fill_buf()?;
        buffered[..buffered.len().min(CONTENT_INSPECTION_LIMIT)].to_vec()
    };
    // 2) 读第一行（到 \n）缓存进 first_line
    let mut first_line = vec![];
    if !inspection_prefix.is_empty() { reader.read_until(b'\n', &mut first_line)?; }
    // 3) 首行 > 1024 时，用首行前 1024 重新当嗅探前缀（兜底）
    // 4) 嗅探编码
    let content_type = inspect_content_type(&inspection_prefix);
    // 5) UTF-16 的话首行要用 2 字节换行符重读
    if content_type == Some(ContentType::UTF_16LE) { read_utf16_line(&mut reader, &mut first_line, 0x00, 0x0A)?; }
    else if content_type == Some(ContentType::UTF_16BE) && … { read_utf16_line(…, 0x0A, 0x00)?; }

    Ok(InputReader { inner: Box::new(reader), first_line, content_type, unbuffered: false })
}
```

### 嗅探的分支（`inspect_content_type`，input.rs:349）

```rust
fn inspect_content_type(first_line: &[u8]) -> Option<ContentType> {
    if first_line.is_empty() { return None; }          // 空输入 → 不晓得编码，当普通
    let content_type = content_inspector::inspect(first_line);  // 第三方库：看前几字节
    if content_type == ContentType::UTF_8 && has_zip_signature(first_line) {
        Some(ContentType::BINARY)                      // ★ 特例：ZIP/PE 魔数 PK.. 当二进制
    } else {
        Some(content_type)
    }
}
```

四路结果（`content_inspector` 库判定）：

| 编码 | 判定依据 | 后续影响（t08 渲染） |
|---|---|---|
| `UTF_8` | BOM / 合法字节流 | 直接解码 |
| `UTF_16LE` / `UTF_16BE` | 交替 \0 字节 | 换行符是 2 字节，`read_utf16_line` 循环 |
| `BINARY` | 含 NUL 字节 / ZIP 魔数 | 默认 `NoPrinting` 只打警告（t01 讲的 `binary` 字段） |
| `None` | 空输入 | 当普通文本 |

**Rust 机制 · `fill_buf` vs `read`**：为什么用 `fill_buf`（不消费）而不是 `read`（消费）
拿嗅探字节？因为编码检测最多看 1024 字节，但"首行可能 >1024 字节"。`fill_buf`
只"peek"缓冲区已有字节、不推进游标，后续 `read_until` 读首行时**不会漏字节**。
用 `read` 会消费掉，嗅探和读首行就错位了。

## 4. 叶子：git diff 获取（controller.rs:162）★ feature 门控

`open` 完回到 `print_input`，有个 `#[cfg(feature = "git")]` 的整块：

```rust
#[cfg(feature = "git")]
let line_changes = if self.config.visible_lines.diff_mode()
    || (!self.config.loop_through && self.config.style_components.changes())
{
    match opened_input.kind {
        OpenedInputKind::OrdinaryFile(ref path) => {
            let diff = get_git_diff(path);      // 读 git 工作区对该文件的改动
            // diff 模式（--diff-context）下，无改动的文件整个跳过
            if self.config.visible_lines.diff_mode()
                && diff.as_ref().map(|c| c.is_empty()).unwrap_or(false) {
                return Ok(());                  // ★ 叶子：无 git 改动 → 直接返回，不打印
            }
            diff
        }
        _ if self.config.visible_lines.diff_mode() => return Ok(()),  // 非文件输入在 diff 模式跳过
        _ => None,
    }
} else { None }
```

钻两个叶子：
- **`get_git_diff`**（`src/diff.rs`，`git` feature）：调 `gix` 读工作区改动，
  算出"哪些行被增/删/改"，供 t07 的 `LineChangesDecoration` 画 `+`/`-` 标记。
- **diff 模式跳过**：`--diff-context N` 时，没有 git 改动的文件**一行都不打**
  （直接 `return Ok(())`）——这就是"只看被改过的行"的语义。

feature `git` 关着时整块不编译，`line_changes` 恒为 `None`，git 标记自然不画。

## 5. `read_line` 逐行读（input.rs:311）—— 循环吐字节的底层

（t08 的 `print_file_ranges` 会反复调它，这里立住语义。）

```rust
pub(crate) fn read_line(&mut self, buf: &mut Vec<u8>) -> io::Result<bool> {
    if !self.first_line.is_empty() {             // 第一次：吐 try_new 缓存的首行
        buf.append(&mut self.first_line);
        return Ok(true);
    }
    if self.content_type == Some(ContentType::UTF_16LE) { return read_utf16_line(&mut self.inner, buf, 0x00, 0x0A); }
    if self.content_type == Some(ContentType::UTF_16BE) { return read_utf16_line(&mut self.inner, buf, 0x0A, 0x00); }
    if self.unbuffered { return self.read_line_unbuffered(buf); }   // -u 流式：半行也吐
    let res = self.inner.read_until(b'\n', buf).map(|size| size > 0)?;  // 普通：读到 \n
    Ok(res)
}
```

- **返回值**：`Ok(true)`=读到数据（可能是无 `\n` 的最后一段）；`Ok(false)`=EOF 且 buf 空。
- **UTF-16 换行是 2 字节**（LE=`\x00\x0A` / BE=`\x0A\x00`），普通 `read_until(b'\n')` 不够，
  `read_utf16_line` 循环读到"末尾两字节正好是换行符"才停。
- **unbuffered**（`-u`，`read_line_unbuffered` input.rs:332）：`tail -f log | bat -u` 场景，
  不等 `\n`，缓冲区里有多少给多少（半行也吐）→ 所以 t01 讲过 `-u` 会自动关行号。

## 本区段 Rust 机制

1. **三值枚举做"输入类型"**（`InputKind`）：`OrdinaryFile`/`StdIn`/`CustomReader`
   穷举"输入从哪来"，`match` 全覆盖编译器帮你查漏。
2. **`?` 配 `map_err` 改错误类型**：`File::open(…).map_err(|e| format!("'{path}': {e}"))?`
   把 `io::Error` 包成 bat 的友好 `Error` 再上抛（error.rs 的 `thiserror` 派生，architecture.md 判断 7）。
3. **feature 门控三处**：lessopen（§1）、git diff（§4）、还有 t05 的 paging——
   同一份代码按 feature 编出不同能力的二进制。

## 检验问题

1. `bat 一个目录/` 会发生什么？哪一行叶子拦住了它？
2. `try_new` 为什么用 `fill_buf`（不消费）而不是 `read` 拿嗅探字节？用 `read` 会漏什么？
3. 一个 ZIP 文件字节上能当合法 UTF-8 解，为什么 bat 还把它判成 BINARY（哪行特判）？
4. `--diff-context 3` 且某文件**没有** git 改动，bat 会打印它吗？哪行 `return` 决定的？
5. `read_line` 的 `Ok(false)` 返回时 `buf` 是什么状态？为什么 EOF 判断要"buf 空"而不是"读到 0"？

## 返回与下一站

**`print_input` 的"打开输入 + 编码嗅探 + git diff"钻完，返回 `print_input` 继续：
选 Printer（Simple vs Interactive）+ 选 syntax/theme → t07。**
