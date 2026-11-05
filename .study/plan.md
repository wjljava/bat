# bat 完整学习路线

> 目标：把 bat 学通透，覆盖从"命令行"到"彩色字符落到屏幕"的所有核心模块。
> 每一步都结合具体代码行拆解，遇到高级 Rust 特性（trait 对象、状态机、feature 门控、闭包借线程等）停下来讲透。

## 数据流总览

### 5 步直线（先记这个，再看下面的细节展开）

1. **解析参数**：把 `bat foo.rs` 拆成"要读什么文件、什么风格、什么主题、要不要分页"。
2. **读文件**：打开文件、探测编码（UTF-8/16/二进制）、逐行读出来。
3. **高亮**：选语法（`.rs` → Rust），syntect 引擎把一行拆成"带样式的片段"。
4. **渲染**：每行画行号/git 标记、展开 tab、换行截断、上色，写进输出。
5. **输出**：决定字符送到 stdout 还是 less 分页器。

### 细节展开（对应上面 5 步）

```
第 1 步：解析参数
   CLI + 配置文件 + 环境变量
   │
   ▼
App::new()                        （src/bin/bat/app.rs）
   ├─ matches()                   三级配置合并（config 文件 / 环境变量 / CLI 盖住）
   ├─ config()                    把 matches 翻译成 Config 结构体（28 字段）
   └─ inputs()                    把文件参数翻译成 Vec<Input>
   ▼
run() 分派（src/bin/bat/main.rs:409）
   ├─ list-languages / list-themes / config-file / ... 各成一条路
   └─ 默认 → run_controller()      ★ 最常见：普通读文件
   ▼
第 2 步：读文件
run_controller() → Controller::run() → print_input()
   └─ Input::open()               （src/input.rs）打开文件、嗅探编码、缓存首行
       └─ InputReader::read_line  逐行读（UTF-8/16/二进制/unbuffered 各走各的）
   ▼
第 3 步：高亮
   InteractivePrinter::new()      （src/printer.rs:218）
   ├─ assets.get_theme()          按 config.theme 拿主题
   ├─ assets.get_syntax()         （src/assets.rs:216）-l → 路径 → 首行 → fallback
   └─ HighlightLines（syntect）   有状态高亮器（每文件一个，跨行复用）
   每行：highlight_regions_for_line → Vec<(Style, &str)>
   ▼
第 4 步：渲染
   InteractivePrinter::print_line（src/printer.rs:648）
   ① 解码 ② 高亮 ③ 挤压空行 ④ 画装饰（行号/git/网格）
   ⑤ 逐片段上色+写出去（不截断/换行 两条路） ⑥ 高亮行收尾
   ▼
第 5 步：输出
   OutputType（src/output.rs）
   ├─ Stdout                     直接终端
   ├─ Pager(Child)               起 less 子进程（-R -F -S -K）
   └─ BuiltinPager               内嵌 minus 线程（--pager=builtin）
```

## 步骤 → 章节 对应（按数据流顺序）

| 数据流步骤 | 干什么 | 对应章节 |
|-----------|--------|---------|
| 1. 解析参数 | `App::new` / `matches` / `config` / `inputs` + `run` 分派表 | ch1（解析分派）、ch2（Config 28 字段） |
| 2. 读文件 | `Input` / `OpenedInput` / `InputReader`（编码嗅探、UTF-16、unbuffered） | ch3 |
| 3. 高亮 | `InteractivePrinter::new` 选 syntax + `highlight_regions_for_line` 调 syntect | ch4 |
| 4. 渲染 | `print_line`：解码/装饰/tab/换行/上色/写输出（vscreen 解析 ANSI） | ch5、ch7（装饰细节） |
| 5. 输出 | `OutputType` / `OutputHandle` / `BuiltinPager`（pager vs stdout） | ch6 |
| 6. 端到端 | 把 `bat -n --theme Monokai foo.rs` 整条链路串起来 | ch8 |
| 7. 架构 | 为什么这么设计（7 个设计判断） | `architecture.md`（独立总览，非编号章节） |

> 学习节奏：ch1 → ch2 → ch3 是"读文件之前"的三步；ch4 → ch5 → ch6 → ch7 是"一行字怎么变彩色"；
> ch8 是复盘，前面没看懂的可以在这里对照重新走一遍；
> `architecture.md`（独立文件，和 ch 并列）是"为什么这么设计"的总览 + 深挖。

## 章节索引

| 章 | 主题 | 核心文件 | 状态 |
|---|------|---------|------|
| 1 | 命令行解析 + `run` 分派表（输入什么 → 走哪条路） | `bin/bat/{main,app,clap_app}.rs` | [x] |
| 2 | `Config` 28 字段逐个讲（谁赋值、何时变） | `config.rs`、`app.rs` | [x] |
| 3 | 输入层：`Input` → `OpenedInput` → `InputReader`（编码嗅探、逐行读） | `input.rs` | [x] |
| 4 | 高亮核心：选 syntax + `highlight_line` 调 syntect（有状态高亮器） | `printer.rs`、`assets.rs` | [x] |
| 5 | 逐行渲染：`print_line`（解码/装饰/tab/换行/上色/写输出） | `printer.rs`、`terminal.rs`、`preprocessor.rs`、`vscreen.rs` | [x] |
| 6 | 输出与分页：`OutputType` / `BuiltinPager`（less 参数、防递归、IO 圈） | `output.rs`、`pager.rs`、`less.rs` | [x] |
| 7 | 装饰与样式：`Decoration` / `Colors` / `StyleComponent`（行号、git、网格怎么画） | `decorations.rs`、`style.rs`、`diff.rs` | [x] |
| 8 | 端到端：`bat -n --theme Monokai foo.rs` 完整执行路径（复盘） | 综合 | [x] |

> 架构总览另见独立文件 `architecture.md`（仓库布局、模块分工、7 个判断深挖、跨项目对比），
> 和 ch1~ch8 并列，不进编号序列。

## 学习节奏约定

1. 每一章先读代码，再逐行拆解。
2. 每遇到一个 Rust 机制（trait 对象、`Arc`、feature 门控、有状态迭代器……），停下来用**具体代码行**讲透。
3. 每章末尾给 2-3 个检验理解的小问题。
4. 卡壳就回问，别硬往下走。
