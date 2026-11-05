# bat 架构总览（学习视角）

> 配套：`plan.md`（数据流路线）+ ch1~ch8（逐章精读）+ 本文末尾的 7 个判断深挖。
> 本文回答的是"这个项目**长什么样**"：有几个模块、它们怎么分、每个模块干什么、
> 几个关键设计决策**为什么**这么做。读代码前先有这张图，看每一章会轻松很多。

## 一、仓库布局：一个 crate（库 + bin 双身份）

bat 不是 workspace（不像 ripgrep 拆 11 个 crate），而是**一个** `Cargo.toml` + `src/`：

```
bat/
├── Cargo.toml          ← 单 crate；edition 2021；feature 裁功能（git/paging/lessopen/…）
├── src/               ← 库部分（可被 `use bat::…` 直接调用）
│   ├── lib.rs          ← 库对外 API：重导出 PrettyPrinter/Config/theme/…
│   ├── config.rs       ← Config 结构体（28 字段，ch2）
│   ├── input.rs        ← 读文件 + 编码嗅探（ch3）
│   ├── printer.rs      ← 渲染：Printer trait 两实现（ch4/ch5）
│   ├── decorations.rs  ← 行号/git/网格 可插拔装饰（ch7）
│   ├── output.rs / less.rs / pager.rs / paging.rs ← 输出 + 分页（ch6）
│   ├── assets.rs       ← 语法/主题 双来源（判断 2）
│   ├── preprocessor.rs / vscreen.rs / terminal.rs / wrapping.rs / …  ← 解码/ANSI/宽度/折行
│   └── error.rs        ← thiserror 派生的库错误类型（判断 7）
├── src/bin/bat/        ← 可执行程序部分（只干"和命令行/终端/退出码打交道"）
│   ├── main.rs         ← run() 分派表（ch1）
│   ├── app.rs          ← App::new / matches / config / inputs（ch1/ch2）
│   ├── clap_app.rs     ← clap 参数定义（ch1）
│   ├── assets.rs       ← assets_from_cache_or_binary（判断 2 的决策点）
│   └── config.rs / directories.rs / completions.rs / input.rs
├── assets/ build/      ← 语法/主题 源定义 + 编译期 build_assets
└── tests/ examples/    ← 集成测试 + 库用法示例
```

> 小白的疑问：为什么 `src/`（库）和 `src/bin/bat/`（程序）要分开？
> 因为 bat **既是库又是 CLI**（判断 1）。`src/lib.rs` 的 doc 注释写了
> `PrettyPrinter::new().input_from_bytes(...).print()` 这种"库调用"示例——
> 库调用方**不该**被迫依赖 `clap`（参数解析库），所以"命令行解析"全放 `src/bin/bat/`，
> 库只暴露 `PrettyPrinter` 这种"喂输入、拿输出"的高层 API。

## 二、模块分工（谁干什么，对应哪章）

bat 单 crate，"模块"是 `src/` 下的文件。每个模块一句话 + 对应章节：

| 模块（src/） | 一句话 | 对应笔记 |
|---|---|---|
| `config.rs` | `Config` 28 字段：三级配置（文件/环境/CLI）的汇聚点 | ch2 |
| `input.rs` | 打开文件、编码嗅探（UTF-8/16/二进制）、逐行读 | ch3 |
| `assets.rs` | 语法/主题 双来源（内置二进制 / 用户缓存），按名取 syntax | ch4 |
| `printer.rs` | 渲染核心：`Printer` trait + `InteractivePrinter`/`SimplePrinter` + 逐行 `print_line` | ch4、ch5 |
| `decorations.rs` | `Decoration` 可插拔（行号/git/网格），`Vec<Box<dyn Decoration>>` | ch7 |
| `preprocessor.rs` | 解码 / strip_ansi / sanitize（读"已上色"的文件） | ch5 |
| `vscreen.rs` | 解析一行里藏的 ANSI 转义序列，切 Text 段 | ch5 |
| `terminal.rs` | 终端宽度、行号宽度计算 | ch5 |
| `wrapping.rs` | 换行/截断（Word/Character 模式） | ch5 |
| `output.rs`/`less.rs`/`pager.rs`/`paging.rs` | 输出去哪（stdout / less 子进程 / 内嵌 minus）+ 防 IO 圈 | ch6 |
| `diff.rs` | git 改动标记（`git` feature） | ch7 |
| `style.rs` | 颜色、`StyleComponent` 开关 | ch7 |
| `error.rs` | `thiserror` 派生的 `enum Error`（库对外） | 判断 7 |

bin 侧（`src/bin/bat/`）只做三件事：**解析参数**（`clap_app`/`app`）、
**分派到各命令**（`main.rs` 的 `run()`，ch1）、**把 config/资产喂给库**（`assets.rs`/`config.rs`）。

## 三、贯穿全项目的 7 个架构判断

（每个判断"为什么这么设计"的深挖在本文第五节。）

1. **库/bin 双身份**：`src/` 是库（`PrettyPrinter` API），`src/bin/bat/` 是 CLI，命令行解析放 bin 不污染库依赖。
2. **assets 双来源**：语法/主题 内置编译进 exe（开箱即用）+ 用户缓存（`cache --build` 加自定义不重编），版本号必须匹配。
3. **`Box<dyn Printer>` 渲染器二选一**：纯文本 vs 高亮是**运行时**（`loop_through`）才定，只能动态分发。
4. **`Vec<Box<dyn Decoration>>` 装饰可插拔**：行号/git/网格任意组合，加新装饰只加实现不改渲染循环。
5. **`HighlightLines` 有状态跨行**：syntect 高亮是状态机，上下文跨行保留，在 `InteractivePrinter` 持有。
6. **feature 门控裁功能**：`git`/`paging`/`lessopen` 编译期可裁，库调用方不被拖进 CLI 依赖。
7. **错误：库 `thiserror` 不用 `anyhow`**：库暴露**具体**错误类型给调用方 `match`，CLI 内部才打印兜底。

## 四、数据流总图（从 `bat -n --theme Monokai foo.rs` 到退出码）

```
bat -n --theme Monokai foo.rs
  │
  ▼  [bin] main() ──► run() 分派表（ch1）
  │     ├─ list-languages / list-themes / config-file / …  各成一条路
  │     └─ 默认 → run_controller()
  ▼
  [lib] Controller::run() → print_input()（src/controller.rs）
  │     ├─ Input::open()（ch3：开文件、编码嗅探、缓存首行）
  │     │   └─ InputReader::read_line  逐行读（UTF-8/16/二进制/unbuffered 各走各的）
  │     ▼
  │     InteractivePrinter::new()（ch4：选 syntax + 装配 decorations）
  │     ├─ assets.get_syntax()      -l → 路径 → 首行 → fallback（ch4）
  │     └─ HighlightLines（syntect，每文件一个，跨行复用）
  │     ▼  每行：
  │     print_line()（ch5：解码→高亮→挤压空行→画装饰→逐片段上色→写输出）
  │       └─ vscreen 解析 ANSI、terminal 算宽度、wrapping 折行
  │     ▼
  ▼  OutputType（ch6：Stdout / Pager(less 子进程) / BuiltinPager(minus 线程)）
  │     └─ 防 IO 圈：输入==输出 时退化成 stdout
  ▼
  退出码（Result<bool>：Ok(false) = 半成功/没匹配到，ch1）
```

## 五、7 个判断深挖（读真实项目该带走的"为什么"）

> 前 4 节是"图"，这节是每条判断**为什么**这么做的展开。
> 逐行代码已在 ch1~ch8 覆盖，这里只讲设计判断，引用时给文件名不逐行贴。

### 判断 1：库/bin 双身份（`src/lib.rs` vs `src/bin/bat/`）

**为什么**：把"命令行解析"（`clap_app.rs`）放 bin 不放 lib，库调用方不被迫依赖 `clap`。
库只暴露 `PrettyPrinter::input_from_bytes` / `input_from_file`（`pretty_printer.rs`）——
调用方不关心"命令行参数"，只关心"这段字节/文件要怎么高亮"。
bat 同时是库和 CLI，所以多了一层"lib API 怎么设计"的考量；ripgrep 是纯 CLI 没有这层。

### 判断 2：`HighlightingAssets` 双来源（`src/assets.rs` + `bin/bat/assets.rs`）

**为什么**：内置（`from_binary`）开箱即用；自定义（`from_cache`，`bat cache --build`）
让用户加语法/主题**不用重新编译 bat**。决策点 `assets_from_cache_or_binary`：
有缓存且版本匹配用缓存，否则回退内置。版本不匹配**显式报错**（提示重建/清除）
而非静默降级——"我 build 了主题 bat 却用内置"会让人困惑，显式失败行为可预期。

### 判断 3：`Printer` trait 两个实现（`src/printer.rs`）

`SimplePrinter`（纯文本，`loop_through` 时）和 `InteractivePrinter`（高亮+装饰）
都实现 `Printer`，controller 用 `Box<dyn Printer>` 运行时二选一（`controller.rs:192`）。
**为什么用 dyn 不用泛型**：`loop_through` 是**运行时**才知道的值（取决于 6 个条件，ch2），
泛型要编译期定，没法用。代价是 vtable 调用，但渲染不是热点（热点在 ch4 syntect 高亮），可接受。

### 判断 4：`Decoration` trait 对象（`src/decorations.rs`）

装饰装进 `Vec<Box<dyn Decoration>>`，按 `style_components`（ch7）**运行时**决定挂哪些。
**为什么不用 enum**：装饰可**自由组合**（只有行号 / 行号+git / 全开），enum 要为每种
组合写 match（组合爆炸）；trait 对象用 `Vec` 任意拼。开闭原则：加新装饰（如"行尾注释
标记"）只需加一个实现 + 在 `new` 里多 push，**不改** controller / ch5 渲染循环。
每个装饰**自带 `width()`**，`panel_width` 累加所有装饰宽度，内容区 = 终端宽 − panel_width。

### 判断 5：`HighlightLines` 有状态，跨行复用（`src/printer.rs`）

syntect 语法定义是**状态机**：一行开了字符串没闭合、或开了括号，下一行要在该上下文继续
高亮。`HighlightLines` 内部记住"上一行结尾在什么语法上下文"，所以**必须跨行保留**——
每行 `print_line` 都新建它会"重新当行首解析"，高亮就乱。
**设计**：`HighlighterFromSet`（包 `HighlightLines`）是 `InteractivePrinter` 的**实例字段**，
不藏在函数里反复建。这是"有状态对象提层持有"的模式，和 ch3 `InputReader` 持 `first_line` 缓存同款。

### 判断 6：feature 门控（`#[cfg(feature = …)]` 贯穿全仓库）

`Cargo.toml` 的 feature（`git`/`paging`/`lessopen`/`application`/`minimal-application`/`build-assets`…）
把功能切成可选，同一份代码编出"完整版"和"精简版"。
**为什么**：库调用方（判断 1）不该被拖进 `clap`/`gix`/`minus` 这些 CLI 专用依赖——
关掉 `git`/`paging` 二进制小、编译快。代价：`#[cfg]` 代码"看着在但其实没编译"（feature 没开时），
读代码要留意。全仓库到处是，前 8 章凡标 `git feature` 的地方都意味着"对应 feature 开才存在"。

### 判断 7：错误——库用 `thiserror`，不引入 `anyhow`（`src/error.rs`）

`Error` 是 `thiserror` 派生的 `enum`（`Io`/`SyntectError`/`UndetectedSyntax`/`Msg`…），
标 `#[non_exhaustive]`（库保留未来加变体的自由，调用方 `match` 必须留 `..` 兜底）。
**为什么不用 anyhow**：bat **是库**（判断 1），库调用方拿到 `bat::Error` 时想
`match` 具体变体（`UndetectedSyntax` vs `Io`）做不同处理，`anyhow::Error`（动态类型）
会抹掉这种精确性。这是 ripgrep 会话"anyhow 该用在哪"结论的**反向印证**：
ripgrep（纯 CLI）用 anyhow；bat（库+CLI）**库部分 thiserror、CLI 部分打印兜底**，两个身份互不污染。

## 六、跨项目对比（bat vs ripgrep vs fd vs tokei）

读真实项目最该带走的是"**认出模式**"。四个项目共享 Rust CLI 生态的几个套路，
差异全在"核心算法是什么"：

| | bat | ripgrep | fd | tokei |
|---|-----|---------|----|-------|
| 身份 | 库 + CLI | 纯 CLI | 纯 CLI | 库 + CLI |
| 核心 | syntect 高亮渲染 | 正则搜索 | 过滤 + 命令执行 | 逐行统计 code/comment/blank |
| 模块组织 | 单 crate（`src/`+`src/bin/`） | 11 crate workspace | 单 crate | 单 crate + `build.rs` 代码生成 |
| 目录遍历 | 无（读单文件） | `ignore::WalkParallel` | `ignore::WalkParallel` | `ignore::WalkParallel` |
| 并行 | 基本单线程 | 工作窃取 + 串行刷 | N 遍历 + N 执行（crossbeam） | 三层（ignore + rayon 计数） |
| 线程间 | 无 | crossbeam（结果） | crossbeam（命中+命令） | crossbeam + rayon |
| 错误库 | 库 `thiserror` | `anyhow` | `anyhow` | 库 `Box<dyn Error>`（部分失败可容忍） |
| 语言/语法定义 | 运行时读二进制/缓存 | （无语言概念） | （无） | 编译期代码生成进二进制 |

> 心智模型：看到 `ignore`/`crossbeam` 就用"ripgrep 生态的并行 CLI 工具"那套；
> 看到 `syntect` 就用"bat 的高亮渲染"那套；看到 `build.rs`+tera 生成 400 种语言就用
> "tokei 的代码生成驱动统计器"那套。

## 七、读代码的建议顺序

1. 先读 **本文**（架构全貌）+ `plan.md`（数据流 5 步）。
2. 按 ch1 → ch8 顺序读（已是数据流顺序）；第五节"7 个判断深挖"作为"为什么"的补充，
   卡在"是代码细节还是设计意图"时回查。
3. 每章读之前，先只看第二节"模块分工"表里对应的那一行，知道"这一章在哪个模块里"。
4. 卡住时回到第四节"数据流总图"，确认自己在哪一步。

## 八、学完带走的 bat 架构心法（5 个可复用模式）

1. **库/bin 分界**：CLI 行为放 bin，可复用 API 放 lib，依赖边界清晰（判断 1）。
2. **运行期可组合 → trait 对象**：`Box<dyn X>` 做"按配置插拔"的部分（判断 3、4），
   代价是可忽略的动态分发。
3. **有状态对象提层持有**：跨调用的状态（语法上下文、首行缓存、装饰前缀）
   放在"拥有它"的对象里（`InteractivePrinter`/`InputReader`），不藏在函数里反复建（判断 5）。
4. **feature 门控裁功能**：`#[cfg(feature)]` 编译期开关，库不被 CLI 依赖拖大（判断 6）。
5. **错误类型跟身份走**：库用 `thiserror`（具体），CLI 才"打印兜底"（判断 7）。
   这是 ripgrep 会话"anyhow 该用在哪"的完整答案。

读真实代码的核心能力是**认出模式**，不是背 API：
"这是运行期可组合所以用了 trait 对象""这是有状态所以提层持有""这是编译期可裁所以用了 cfg"。

---

## 小白检验问题

1. 为什么"命令行解析"（`clap_app.rs`）放 bin 不放 lib？放 lib 会让库调用方多依赖什么？
2. `assets_from_cache_or_binary` 版本不匹配时为什么**报错**而不是静默降级到内置？
3. `Printer` 用 `Box<dyn>` 而不是泛型，根源是 `loop_through` 是**运行时**还是**编译期**的值？
4. 加一个新装饰（如"行尾注释标记"），要改几处？要改 `controller.rs` 或 ch5 渲染循环吗？
5. 为什么 `HighlighterFromSet` 是 `InteractivePrinter` 的实例字段？每行 `print_line` 新建会出什么 bug？
6. 关掉 `git` feature 编译 bat，ch7 的 `LineChangesDecoration` 还存在吗？`#[cfg]` 那段会编译进去吗？
7. bat 作为库，调用方 `match` 一个 `bat::Error` 必须写 `..`，是为什么（`#[non_exhaustive]`）？
8. bat（库+CLI）和 ripgrep（纯 CLI）的错误处理风格差在哪？哪个用了 anyhow？
