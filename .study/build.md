# 编译期支线：build 目录学习路线（`build/` 四个文件 + 嵌入机制）

> 这是 `.study` 的**编译期支线**，与 `architecture.md` 并列（不进 ch1~ch8 编号序列）。
> 阅读时机：**读完 ch8 之后再学这份**。ch1~ch8 讲的是"bat 运行起来后怎么把文件高亮出来"，
> 这份讲的是"bat 被 `cargo build` 编译时，build 脚本在幕后干了什么、把哪些素材焊进二进制"。

这一支线的目标：讲清 **build 目录（编译期逻辑）**——`cargo build` 时 build 脚本先于主程序跑，
把语法映射表、shell 补全脚本、man 页**在编译期生成并嵌入**二进制。读完你应该能回答：
"build 脚本和主程序是什么先后关系？`cargo:rustc-env` 和 `include_str!` 怎么把生成的文件焊进 bat.exe？
哪些文件必须懂、哪些可以跳过？"

## 涉及文件

- `build/main.rs` — build 脚本入口（17 行，全链路骨架）
- `build/application.rs` — 生成 man 页 + shell 补全（85 行，`#[cfg(feature = "application")]` 门控）
- `build/util.rs` — `render_template` 模板渲染工具（21 行）
- `build/syntax_mapping.rs` — 生成"扩展名→语法"静态映射表（369 行，**可跳过的 codegen**）
- `Cargo.toml` 的 `[build-dependencies]`（109-128 行）+ `[package].build = "build/main.rs"`（11 行）
- `src/bin/bat/completions.rs` — 消费生成产物的 `include_str!`（对照看"嵌入"落点）

## 全景图：cargo build 的时间线（先有这个概念，再看代码）

```
cargo build 被调用
   │
   ▼
① 先编译并运行 build/main.rs   （build/main.rs:6）
   │  这是一个有 main() 的普通 Rust 程序，跑完就没了，不产生可执行文件
   │  用到的库 → 来自 [build-dependencies]（不进最终二进制）
   │
   ├─ syntax_mapping::build_static_mappings()          （main.rs:11，总是跑）
   │     读 src/syntax_mapping/builtins/*.toml → 生成 Rust 静态查找表 → 写 $OUT_DIR
   │
   ├─ #[cfg(feature = "application")]                  （main.rs:13，feature 门控）
   │  application::gen_man_and_comp()                   （main.rs:14）
   │     渲染 4 个 shell 补全模板 + 1 个 man 模板 → 写 $OUT_DIR
   │     用 cargo:rustc-env 把生成文件路径告诉 rustc
   │
   ▼
② rustc 编译主程序 src/
   │  用到的库 → 来自 [dependencies]
   │  代码里的 include_str!(env!("BAT_GENERATED_COMPLETION_PS1"))
   │       → 编译器在编译那一刻把 ① 生成的文件内容"焊死"进二进制
   ▼
③ 产物 bat.exe 是"自包含"的：补全脚本、语法映射表已内嵌，运行时不读任何源文件
```

**和 ch1~ch8 的分界**：ch1~ch8 全在 ② ③ 之后（bat 已经跑起来了）。这份只讲 ① 这一步
以及 ② 里的"嵌入"那一下。build 脚本跑完，它生成的东西就冻结在二进制里了。

---

## 第 1 节：`build/main.rs` —— build 脚本的入口

完整代码（`build/main.rs`，就 17 行）：

```rust
#[cfg(feature = "application")]
mod application;
mod syntax_mapping;
mod util;

fn main() -> anyhow::Result<()> {
    println!("cargo:rerun-if-changed=build/");        // build/main.rs:9
    syntax_mapping::build_static_mappings()?;          // build/main.rs:11
    #[cfg(feature = "application")]
    application::gen_man_and_comp()?;                 // build/main.rs:14
    Ok(())
}
```

### 1.1 build 脚本"先于主程序跑"

`Cargo.toml` 第 11 行 `build = "build/main.rs"` 告诉 cargo："编译主程序之前，先把
`build/main.rs` 当一个独立的小程序编译并运行一次"。它跑完，cargo 才去编 `src/`。
所以它俩是**两个独立编译单元**，各用各的依赖表：

| 编译单元 | 用的库表 | 产物 |
|---|---|---|
| build 脚本（`build/main.rs`） | `[build-dependencies]` | 跑完即弃，不进二进制 |
| 主程序（`src/`） | `[dependencies]` | 最终 `bat.exe` |

> 关键心智：**build 脚本是"编译期的临时工"**。它的活（生成文件）做完，自己就销毁了；
> 只有它生成的**产物**通过 `include_*!` 活到运行时。

### 1.2 `#[cfg(feature)]` 怎么裁剪

注意第 1、13 行两处 `#[cfg(feature = "application")]`：

- `mod application;`（第 2 行）本身被 `#[cfg]` 包着——**没开 `application` feature 时，
  这一行连同它引入的模块直接不编译**
- `application::gen_man_and_comp()`（第 14 行）同样被包着

效果呼应你 `cargo-toml.md` 里学的：库用户 `default-features = false` 关掉 `application` 时，
**生成 man/补全这段逻辑在 build 脚本里物理不存在**，build 脚本只跑 `syntax_mapping` 那一行
（第 11 行没门控，总是跑）。这是"feature 决定编译期代码在不在"的又一处实例。

### 1.3 `cargo:rerun-if-changed` 控制"何时重跑 build 脚本"

第 9 行 `println!("cargo:rerun-if-changed=build/")` 是 build 脚本→cargo 的通信。默认规则是
"build 脚本目录下任何文件变了就重跑"，但这里显式声明"只盯 `build/` 目录"——意思是：
改 `src/` 不会触发 build 脚本重跑，只有 `build/` 里的代码变了才重跑。

（`application.rs:7-13` 里还有一串 `cargo:rerun-if-changed=assets/manual/`、
`cargo:rerun-if-env-changed=...`，同一机制，把监视范围扩到"素材目录 + 特定环境变量"。）

---

## 第 2 节：`build/application.rs` + `build/util.rs` —— 模板渲染 → 传路径

这一节是"补全脚本怎么生成"的完整链路，三步：**读模板 → 替换占位符 → 把结果路径告诉 rustc**。

### 2.1 读模板（`assets/completions/*.in`）

`application.rs` 开头（`gen_man_and_comp()`，第 6 行）对 4 个 shell 模板各调一次 `render_template`：

```rust
render_template(
    &variables,
    "assets/completions/_bat.ps1.in",            // 模板（带 {{PROJECT_EXECUTABLE}} 占位符）
    out_dir.join("assets/completions/_bat.ps1"), // 渲染好的输出
)?;
```

模板文件（`_bat.ps1.in`）里写死了 `{{PROJECT_EXECUTABLE}}` 这种**双花括号占位符**
（不是 PowerShell 语法，是 bat 自己定的渲染标记）。为什么要占位符？因为补全脚本里
`Register-ArgumentCompleter -Native -CommandName '{{PROJECT_EXECUTABLE}}'` 要填的是**可执行文件名**，
万一有人 fork 项目改名，脚本里的命令名也得跟着变，不能写死。

### 2.2 替换占位符（`render_template`，`util.rs`）

`render_template`（`build/util.rs:6`）就三行核心：

```rust
let mut content = fs::read_to_string(in_file)?;            // 读模板
for (variable_name, value) in variables {
    let pattern = format!("{{{{{variable_name}}}}}");      // util.rs:15 → 拼出 "{{NAME}}"
    content = content.replace(&pattern, value);            // 全局替换
}
fs::write(out_file, content)?                               // 写到 OUT_DIR
```

`variables` 是 `application.rs:21-28` 攒的一个 `HashMap`：`PROJECT_NAME`/`PROJECT_EXECUTABLE`/
`PROJECT_VERSION` 等。替换后 `{{PROJECT_EXECUTABLE}}` 变成 `bat`。

### 2.3 把路径告诉 rustc（`cargo:rustc-env`，`application.rs:66` 起）

渲染好的文件写在 `$OUT_DIR/assets/completions/`。光生成没用，得让主程序知道去哪 `include`。
build 脚本用这个 println 干这件事：

```rust
println!("cargo:rustc-env=BAT_GENERATED_COMPLETION_PS1={}",
    out_dir.join("assets/completions/_bat.ps1").display());   // application.rs:74-77
```

`cargo:rustc-env=KEY=VALUE` 会在**编译主程序时**给 rustc 设一个环境变量 `KEY`。
主程序里的 `env!("BAT_GENERATED_COMPLETION_PS1")` 就能取到这个路径。

**三步收口**：`模板（.in）→ 渲染（util.rs 替换，写 OUT_DIR）→ 传路径（cargo:rustc-env）`，
每一步都能对上上面这些行号。

---

## 第 3 节：嵌入概念 —— `include_str!` / `include!` / `env!`（"焊死"不是"运行时读盘"）

这一节消解一个最常见的误解：**"bat 怎么读那些生成文件的？"** 答案是——**它不读，是编译器在编译那一刻把内容嵌进去了**。

对照 `src/bin/bat/completions.rs`（就 6 行）：

```rust
pub const PS1_COMPLETION: &str = include_str!(env!("BAT_GENERATED_COMPLETION_PS1"));
//                                            └──────────┬──────────┘
//                             env! 取路径 → include_str! 把该文件内容原样嵌成 &str 常量
```

三个宏分工：

| 宏 | 干什么 | 编译期 / 运行时 |
|---|---|---|
| `env!("X")` | 取编译期环境变量 `X` 的值（这里是 build 脚本 `cargo:rustc-env` 设的路径） | 编译期 |
| `include_str!(路径)` | 把该文件**整个内容**读出来，作为 `&'static str` 常量塞进二进制 | 编译期 |
| `include!`(语法映射表) | 把一个 `.rs` 源码文件当成当前文件的一部分展开 | 编译期 |

**所以** `PS1_COMPLETION` 这个常量，值就是那段 PowerShell 补全脚本的全文，**已经躺在 bat.exe 里**。
运行 `bat --completion ps1` 时（`main.rs:407` `writeln!(stdout, "{completion}")`）直接吐常量，
**不需要** `assets/` 目录还在、不需要任何运行时 IO。

> 这就是"bat.exe 单文件就能打印补全脚本"的原因：脚本在编译期被 `include_str!` 焊死了。
> 也是 `Cargo.toml` 第 10 行 `exclude = ["assets/..."]` 敢把素材从 crates.io 包裁掉的原因——
> 素材只是"编译期的原料"，成品里已经有了。

**对照 ch1~ch8 的落点**：`main.rs:396-407` 那段 `--completion` 分派，取的就是这份焊死的常量；
`list_themes`（`main.rs:198`）里 `THEME_PREVIEW_DATA`（`main.rs:45` `include_bytes!`）也是同一招。

---

## 第 4 节：`build/syntax_mapping.rs` —— 最小理解（可跳过的 codegen）

> **跳过标注**：这文件 369 行，用了 `syn`/`quote`/`prettyplease` 做**程序化生成 Rust 源码**，
> 是 Rust 工程化的进阶技巧。**你现在不必逐行看懂**。记住"最小理解"三句就够：

1. **数据 → 代码**：读 `src/syntax_mapping/builtins/**/*.toml`（人工写的"文件名/扩展名 → 语法"
   映射，如 `.rs` → Rust、`Dockerfile` → Docker），把每条映射**生成成一段 Rust 静态查找表源码**。
2. **`include!` 进主程序**：生成的 `.rs` 写到 `$OUT_DIR`，主程序用 `include!` 嵌进去，变成一张
   编译进二进制的查找表（对应你 ch4 学的"选语法"那一步会查的表）。
3. **按目标平台 `cfg` 过滤**：`build/syntax_mapping.rs:281-299` 的 `get_def_paths()` 里有一串
   `#[cfg(target_os = "linux")]`、`#[cfg(target_os = "windows")]`——**在 Windows 上编译时只收
   `common` + `windows` 子目录的 toml，Linux 才收 `unix-family`/`linux`**。同一份源码，不同目标平台
   生成不同的映射集。

**为什么能跳过**：它讲的是"怎么把配置生成代码"（codegen 炫技），不影响你理解"bat 运行时怎么高亮"
（那是 ch4 的事，ch4 只用这张表，不关心它怎么生成）。**以后真想学 codegen，再翻这个文件的
`build_static_mappings()`（第 352 行）和 `ToTokens` 实现**——那是"让数据会说话、直接生成 Rust 代码"
的干净范式，值得专门研究。

---

## 最小学习集（约 20 分钟）

按"能离开 bat 还能用"的通用机制排优先级：

| 优先级 | 学什么 | 对应本节 | 离开 bat 还通用吗 |
|---|---|---|---|
| ★★★ 必学 | build 脚本先于主程序跑、两单元各用各依赖表 | 第 1 节 | 通用（openssl/protobuf 系都用） |
| ★★★ 必学 | `cargo:rerun-if-changed` 何时重跑 | 第 1 节 | 通用 |
| ★★★ 必学 | `cargo:rustc-env` + `include_str!` 的"编译期嵌入" | 第 2、3 节 | 通用（消解"bat 怎么读生成文件"） |
| ★★ 建议 | 模板渲染 `render_template`（读→替换→写） | 第 2 节 | 通用套路 |
| ★ 可跳 | `syntax_mapping.rs` 的 codegen（syn/quote/prettyplease） | 第 4 节 | 进阶技巧，以后再说 |

**一句话**：第 1-3 节是"编译期生成 + 嵌入"这条通用主线，20 分钟走完就通；第 4 节是可选进阶。

---

## 检验问题（读完对照正文回答）

1. **build 脚本和主程序谁先跑？** 为什么 build 脚本用到的 `syn`/`quote` 不会出现在最终
   `bat.exe` 里？（答：build 脚本先跑；它的库来自 `[build-dependencies]`，只活在编译期）
2. **`bat --completion ps1` 打印的脚本文本，运行时从磁盘哪读来的？**
   （答：不读磁盘——编译期由 `application.rs` 生成、`cargo:rustc-env` 传路径、
   `completions.rs` 的 `include_str!` 焊死进二进制）
3. **库用户 `default-features = false` 关掉 `application` 后，build 脚本还会生成 man/补全吗？**
   （答：不会——`build/main.rs:13` 的 `#[cfg(feature = "application")]` 把
   `gen_man_and_comp()` 整段裁掉，只跑 `syntax_mapping` 那条）
