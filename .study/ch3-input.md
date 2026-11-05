# 第 3 章：输入层（`Input` → `OpenedInput` → `InputReader`）

这一章的目标：讲清"原料怎么进 bat"——从 `App::inputs()` 产出的 `Vec<Input>`，
到 `Controller` 逐个**打开**、**逐行读**出来。读完你应该能回答：
"一个文件从'被指认'到'第一行字节被读到'，中间经过了哪三层类型、编码怎么嗅探的？"

## 涉及文件

- `src/bin/bat/input.rs` — 工厂（`new_file_input` / `new_stdin_input`，20 行）
- `src/input.rs` — 主体（`Input` / `OpenedInput` / `InputReader`，667 行）

## 全景图（先有这个概念，再看代码）

```
App::inputs()                ch1 阶段 4a 产出
   │
   ▼
Vec<Input>                  "要读什么"（描述，没真正打开）
   │
   ├─ InputKind::OrdinaryFile(path)
   ├─ InputKind::StdIn
   └─ InputKind::CustomReader(…)
   │
   ▼
Controller::print_input → input.open(stdin, …)     （controller.rs:159）
   │  根据 kind 分三路：打开文件 / 接 stdin / 包装 reader
   ▼
OpenedInput { reader: InputReader, … }   "打开了的文件"（带 reader）
   │
   ▼
InputReader::try_new            （input.rs:267）
   ├─ fill_buf 拿前 1024 字节    ← 嗅探编码用
   ├─ read_until(\n) 缓存首行    ← 提前吃掉，read_line 第一次直接吐
   └─ inspect_content_type      ← content_inspector 判 UTF-8/16/二进制
   │
   ▼
逐行 read_line(buf)           （input.rs:311）
   ├─ 第一次：吐缓存的 first_line
   ├─ UTF-16LE/BE：按 2 字节换行符读
   ├─ unbuffered：读到多少给多少（流式）
   └─ 普通：read_until(b'\n')
```

一条主线命令 `bat foo.rs` 到这一章：`foo.rs` 已经被 `App::inputs()` 变成
一个 `Input`。本章讲 `Input` 怎么被**打开**成 `OpenedInput`，再被**逐行读**出来。

**为什么分三层？** `Input` 可以被**克隆**（比如 ch1 `list-languages` 里
`plain_config` 的输入要复用它做不同风格打印）；`OpenedInput` 持有
`File`/reader，**不能**再克隆（文件句柄是独占的）。分两层正好。

---

## 阶段 1：工厂 `src/bin/bat/input.rs` —— 怎么造 `Input`

完整代码（`src/bin/bat/input.rs`，就 20 行）：

```rust
pub fn new_file_input<'a>(file: &'a Path, name: Option<&'a Path>) -> Input<'a> {
    // 普通文件：Input::ordinary_file 记下路径；
    // 再套上 name（ch1 的 -n/--file-name），没给就退回真实路径当显示名
    named(Input::ordinary_file(file), name.or(Some(file)))
}

pub fn new_stdin_input(name: Option<&Path>) -> Input<'_> {
    // stdin：Input::stdin()；name 可以是 -n 给的"假装的文件名"
    named(Input::stdin(), name)
}

fn named<'a>(input: Input<'a>, name: Option<&Path>) -> Input<'a> {
    if let Some(provided_name) = name {
        let mut input = input.with_name(Some(provided_name));
        input.description_mut().set_kind(Some("File".to_owned()));  // header 显示 "File: xxx"
        input
    } else {
        input
    }
}
```

**小白会问：`name` 是干嘛的？`bat foo.rs` 不就有路径了吗，为什么还要"假装文件名"？**

两个场景：
1. `echo hi | bat -n main.rs`：输入其实是 stdin，但你**骗** bat "当成 main.rs 读"，
   这样 ch4 选 syntax 时按 `.rs` 高亮，header 也显示 `File: main.rs`。
2. 多个文件想给显示名：`bat -n "a.c" real_a -n "b.c" real_b`。

`set_kind(Some("File"))` 让 header 前缀变成 `File: 'xxx'`（见 ch5 `print_header`）。
没指定 name 的 stdin 就是 `STDIN`（kind=None）。

---

## 阶段 2：`Input` 的三种"原料"（`src/input.rs:74`）

```rust
pub(crate) enum InputKind<'a> {
    OrdinaryFile(PathBuf),            // 普通文件路径
    StdIn,                           // stdin
    CustomReader(Box<dyn Read + 'a>), // 自定义 reader（库调用方喂字节）
}
```

构造方法（input.rs:131）：
```rust
pub fn ordinary_file(path: impl AsRef<Path>) -> Self {
    Self::_ordinary_file(path.as_ref())
}

fn _ordinary_file(path: &Path) -> Self {
    let kind = InputKind::OrdinaryFile(path.to_path_buf());
    let metadata = InputMetadata {
        size: fs::metadata(path).map(|m| m.len()).ok(),  // 文件大小（header 显示用）
        ..InputMetadata::default()
    };
    Input { description: kind.description(), metadata, kind }
}

pub fn stdin() -> Self { /* kind=StdIn, metadata 空 */ }
pub fn from_reader(reader: Box<dyn Read + 'a>) -> Self { /* kind=CustomReader */ }
```

`InputDescription`（input.rs:17）是"怎么称呼这个输入"：
```rust
pub struct InputDescription {
    pub(crate) name: String,        // 真实名（路径）
    title: Option<String>,         // 显示标题（-n 可覆盖 name）
    kind: Option<String>,          // "File" / None（header 前缀）
    summary: Option<String>,       // 一句话摘要
}
```
`title()` 返回 `title` 优先，没给就 `name`。header 显示的就是它。

**小白会问：`description.title()` 和 `metadata.user_provided_name` 有什么区别？**
`user_provided_name` 是 `Open` 时用来**重新探测 syntax 的路径**
（ch4 `input.path()` 读它）；`title` 是**显示**给 header 用的字符串。
`-n main.rs` 时两者都设，但语义不同：一个影响"按什么语言高亮"，一个影响"显示什么"。

---

## 阶段 3：`Input::open` —— 真的打开文件

`Controller::print_input`（controller.rs:159）调 `input.open(stdin, stdout_identifier)`。
完整代码（input.rs:192）：

```rust
pub(crate) fn open<R: BufRead + 'a>(
    self,
    stdin: R,
    stdout_identifier: Option<&Identifier>,
) -> Result<OpenedInput<'a>> {
    let description = self.description().clone();
    match self.kind {
        InputKind::StdIn => {
            // 防"IO 圈"：输入=stdin 且输出也绑到同一终端 → 死循环风险，直接报错
            if let Some(stdout) = stdout_identifier {
                let input_identifier = Identifier::try_from(clircle::Stdio::Stdin)
                    .map_err(|e| format!("Stdin: Error identifying file: {e}"))?;
                if stdout.surely_conflicts_with(&input_identifier) {
                    return Err("IO circle detected. ...".into());
                }
            }
            Ok(OpenedInput {
                kind: OpenedInputKind::StdIn,
                description,
                metadata: self.metadata,
                reader: InputReader::try_new(stdin)?,   // ★ 关键：reader 在这里造
            })
        }
        InputKind::OrdinaryFile(path) => Ok(OpenedInput {
            kind: OpenedInputKind::OrdinaryFile(path.clone()),
            description,
            metadata: self.metadata,
            reader: {
                let path_display = crate::preprocessor::sanitize_for_terminal(&path.to_string_lossy());
                let mut file = File::open(&path)
                    .map_err(|e| format!("'{path_display}': {e}"))?;  // 打不开 → 友好报错
                if file.metadata()?.is_dir() {
                    return Err(format!("'{path_display}' is a directory.").into());
                }
                // 同样防 IO 圈：文件 == stdout 绑定的文件也报错
                if let Some(stdout) = stdout_identifier {
                    let input_identifier = Identifier::try_from(file)
                        .map_err(|e| format!("{path_display}: Error identifying file: {e}"))?;
                    if stdout.surely_conflicts_with(&input_identifier) {
                        return Err(format!("IO circle detected. ...").into());
                    }
                    file = input_identifier.into_inner().expect("...");
                }
                InputReader::try_new(BufReader::new(file))?   // ★ 文件包成 BufReader
            },
        }),
        InputKind::CustomReader(reader) => Ok(OpenedInput {
            description,
            kind: OpenedInputKind::CustomReader,
            metadata: self.metadata,
            reader: InputReader::try_new(BufReader::new(reader))?,
        }),
    }
}
```

**小白会问：`clircle::Identifier` 是干嘛的？**
`clircle` 是个小库，能判断"两个 fd 是不是同一个文件"。
bat 把输入和输出绑到同一个终端/文件时（比如 `bat > foo` 且读的就是 `foo`），
会形成**读自己刚写的内容**的死循环。`surely_conflicts_with` 提前发现就报错。
正常用法（`bat foo.rs`）不会触发，但 `bat -n x | cat x` 这种怪事会。

`OpenedInput`（input.rs:108）：
```rust
pub(crate) struct OpenedInput<'a> {
    pub(crate) kind: OpenedInputKind,
    pub(crate) metadata: InputMetadata,
    pub(crate) reader: InputReader<'a>,    // ★ 真正读字节的在这里
    pub(crate) description: InputDescription,
}
```

---

## 阶段 4：`InputReader::try_new` —— 探测编码 + 缓存首行

这是本章最绕的一段（input.rs:267）。它干两件事：**嗅探编码** + **把首行先读出来**。

```rust
pub(crate) fn try_new<R: BufRead + 'a>(mut reader: R) -> io::Result<InputReader<'a>> {
    // 1) 先 fill_buf 拿"已经在缓冲区里的字节"（最多 1024 字节）
    //    为什么用 fill_buf 而不是 read？因为 content_inspector 最多只看 1024 字节，
    //    fill_buf 不消费输入，能把"首行可能 >1024 字节"的情况也覆盖到
    let mut inspection_prefix = {
        let buffered = reader.fill_buf()?;
        buffered[..buffered.len().min(CONTENT_INSPECTION_LIMIT)].to_vec()
    };

    // 2) 读第一行（到 \n 为止）缓存起来
    let mut first_line = vec![];
    if !inspection_prefix.is_empty() {
        reader.read_until(b'\n', &mut first_line)?;
    }

    // 3) 如果首行比 1024 还长，用首行前 1024 字节重新当"嗅探前缀"
    //    （自定义 BufRead 可能一次只给 <1024 字节，得兜底）
    let first_line_prefix_len = first_line.len().min(CONTENT_INSPECTION_LIMIT);
    if first_line_prefix_len > inspection_prefix.len() {
        inspection_prefix.clear();
        inspection_prefix.extend_from_slice(&first_line[..first_line_prefix_len]);
    }

    // 4) 嗅探编码（content_inspector 库：看前几字节判 UTF-8/16/二进制）
    let content_type = inspect_content_type(&inspection_prefix);

    // 5) UTF-16 的话，首行要用 16 位的换行符重新读（\n 是 2 字节）
    if content_type == Some(ContentType::UTF_16LE) {
        read_utf16_line(&mut reader, &mut first_line, 0x00, 0x0A)?;
    } else if content_type == Some(ContentType::UTF_16BE)
        && !(first_line.len() % 2 == 0 && first_line.ends_with(&[0x00, 0x0A]))
    {
        read_utf16_line(&mut reader, &mut first_line, 0x0A, 0x00)?;
    }

    Ok(InputReader {
        inner: Box::new(reader),
        first_line,               // ★ 首行已经吃进来了，read_line 第一次直接返回它
        content_type,
        unbuffered: false,
    })
}
```

**为什么首行要提前读？** 两个原因：
1. `content_inspector` 看前 1024 字节判编码，需要"已经在内存里"的首行。
2. 首行往往决定 syntax（比如 shebang `#!/bin/bash`），ch4 要用。
   提前读完，后面 `read_line` 第一次调用**直接从缓存吐**，不重复读。

`inspect_content_type`（input.rs:349）：
```rust
fn inspect_content_type(first_line: &[u8]) -> Option<ContentType> {
    if first_line.is_empty() { return None; }
    let content_type = content_inspector::inspect(first_line);
    // 特例：UTF-8 但前 4 字节是 ZIP 魔数（PK..) → 当二进制
    if content_type == ContentType::UTF_8 && has_zip_signature(first_line) {
        Some(ContentType::BINARY)
    } else {
        Some(content_type)
    }
}
```
**小白会问：ZIP 文件明明有合法 UTF-8 字符，为什么要当二进制？**
因为 ZIP/PE 文件前几字节是 `PK\x03\x04` 这类魔数，光看字节"能当 UTF-8 解码"
不代表它是文本。bat 显式把 ZIP 魔数识别成 BINARY，避免把压缩包当文本高亮。

---

## 阶段 5：`InputReader::read_line` —— 逐行读

这是 ch4 `print_file_ranges` 反复调的那个（controller.rs:278）。
完整代码（input.rs:311）：

```rust
pub(crate) fn read_line(&mut self, buf: &mut Vec<u8>) -> io::Result<bool> {
    // 第一次调用：直接吐缓存的首行
    if !self.first_line.is_empty() {
        buf.append(&mut self.first_line);   // 用 append（消费语义）把首行搬进 buf
        return Ok(true);
    }

    // 后面按编码走不同的换行符
    if self.content_type == Some(ContentType::UTF_16LE) {
        return read_utf16_line(&mut self.inner, buf, 0x00, 0x0A);   // LE: \x00\x0A
    }
    if self.content_type == Some(ContentType::UTF_16BE) {
        return read_utf16_line(&mut self.inner, buf, 0x0A, 0x00);   // BE: \x0A\x00
    }

    // -u 流式模式：不等完整行，读到多少给多少
    if self.unbuffered {
        return self.read_line_unbuffered(buf);
    }

    // 普通模式：读到 \n（含 \n 本身）
    let res = self.inner.read_until(b'\n', buf).map(|size| size > 0)?;
    Ok(res)
}
```

**返回值语义**（小白常搞混）：
- `Ok(true)`：读到了数据（可能是完整行，也可能是不带 `\n` 的最后一段）
- `Ok(false)`：EOF，**buf 此时是空的**（没有新数据）

`read_utf16_line`（input.rs:368）：UTF-16 换行是 2 字节，普通 `read_until(b'\n')`
不够，得循环读直到"最后两字节正好是 `\x00\x0A`（LE）或 `\x0A\x00`（BE）"。

`read_line_unbuffered`（input.rs:332）：`tail -f log | bat -u` 场景。
不等 `\n`，把**当前缓冲区里所有字节**一次性给出去：
```rust
fn read_line_unbuffered(&mut self, buf: &mut Vec<u8>) -> io::Result<bool> {
    let available = self.inner.fill_buf()?;
    if available.is_empty() { return Ok(!buf.is_empty()); }  // 没新数据
    if let Some(pos) = available.iter().position(|&b| b == b'\n') {
        buf.extend_from_slice(&available[..=pos]);   // 读到 \n 为止（含）
        self.inner.consume(pos + 1);
    } else {
        let len = available.len();
        buf.extend_from_slice(available);            // 没有 \n，全给（半行也吐）
        self.inner.consume(len);
    }
    Ok(true)
}
```

**小白会问：unbuffered 为什么能"半行"就吐？**
流式场景（`tail -f`）用户要的是"日志一写一行立刻看到"，
等完整 `\n` 会卡住。`-u` 下 bat 自动**关掉行号**（ch1 讲过），
因为半行没编号会乱——这是 ch2 `unbuffered` 字段的设计后果。

---

## 本章涉及的 Rust 机制（小白需要补的知识点）

1. **`Box<dyn Read + 'a>`**：`InputKind::CustomReader` 装的是"任何实现了 `Read`
   的东西"（trait 对象），`+ 'a` 是生命周期约束——它借的内存得活过 `Input`。
2. **`?` 在 `io::Result` 上的用法**：`File::open(&path).map_err(|e| format!(...))?`
   把 `io::Error` 转成 bat 的 `Error`（带友好提示）再 `?` 上抛。
3. **`clircle::Identifier`**：能判断"两个 fd 是不是同一个文件"。
   bat 防"输入 == 输出"的死循环（IO 圈）就靠它（ch3 阶段 3 的 `surely_conflicts_with`）。
4. **`ContentInspector`（第三方库）**：看前 1024 字节判编码（UTF-8/16/BINARY）。
   bat 不自己写编码检测，复用这个库（和 fd 的 `content_inspector` 同源思路）。

## 检验问题

1. `Input` 和 `OpenedInput` 为什么必须分两个类型？哪个能 `clone`？
2. `try_new` 里为什么用 `fill_buf` 而不是 `read` 来拿嗅探字节？
3. 首行被"提前读进 `first_line`"，那第一次 `read_line` 会再读一遍吗？
4. UTF-16LE 的换行符是哪些字节？`read_utf16_line` 循环到什么时候停？
5. `tail -f log | bat -u` 为什么行号会消失？这是哪个字段决定的？

## 下一步

**ch4 — 高亮核心**：`InteractivePrinter::new` 怎么选 syntax、
`highlight_regions_for_line` 怎么调 syntect 引擎把一行拆成"带样式的片段"。
这是 bat 真正"把文件变成颜色"的心脏。
