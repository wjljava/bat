# 编译期支线（DFS）：`cargo build` 的执行树 —— build 脚本从启动到把素材焊进二进制

> 定位：`.study` 的**编译期支线**，与 `architecture.md` 并列（不进 t01~t09 编号序列）。
> 阅读时机：**读完 t09 之后**。t01~t09 讲的是"bat 跑起来后怎么把文件高亮出来"（运行期）；
> 本篇讲的是"**bat 被 `cargo build` 编译时**，build 脚本在幕后干了什么、把哪些素材焊进二进制"。
> 组织方式沿用 t 系列的 **DFS**：从 `cargo build` 出发，每到一个分支就钻到叶子讲清楚，
> 钻完写"返回"，再走下一条。

## 你在执行树的哪个位置（先有这张图）

```
cargo build
└─① 先编译并运行 build 脚本（build/main.rs:6 main()）   ← 你在这里
   │   这是个有 main() 的普通程序，跑完即弃，不进最终二进制
   │   用到的库 → [build-dependencies]（syn/quote/...，只活在编译期）
   ├─【分支A】syntax_mapping::build_static_mappings()（main.rs:11，**总是跑**）
   │    叶子：读 toml → syn/quote 生成 Rust 静态查找表 → 写 $OUT_DIR → include! 嵌入
   ├─【分支B】#[cfg(feature = "application")] application::gen_man_and_comp()（main.rs:14）
   │    ├─ 叶子：读 .in 模板 → render_template 替换占位符 → 写 $OUT_DIR（util.rs）
   │    └─ 叶子：cargo:rustc-env 把生成文件路径告诉 rustc → include_str! 嵌入
   │    （feature 关时整段不编译 → 返回时"分支B 不存在"）
   ▼
② rustc 编译主程序 src/（用 [dependencies]）
   代码里的 include_str!(env!("...")) / include! 在编译那一刻把 ① 的产物焊进二进制
   ▼
③ 产物 bat.exe 自包含：补全脚本 / 语法映射表已内嵌，运行时不读任何源文件
```

**和 t01~t09 的分界**：t01~t09 全在 ②③ 之后（bat 跑起来了）。本篇只讲 ① + ② 里的"嵌入"那一下。
build 脚本跑完，它生成的东西就冻结进二进制了。

## 0. 入口：`build/main.rs` —— 就 17 行（main.rs:6）

```rust
#[cfg(feature = "application")]   // main.rs:1
mod application;                  // main.rs:2  ← 没开 application 时，这行连同模块都不编译
mod syntax_mapping;               // main.rs:3
mod util;                         // main.rs:4

fn main() -> anyhow::Result<()> { // main.rs:6
    println!("cargo:rerun-if-changed=build/");        // main.rs:9  ③ 见 §0c
    syntax_mapping::build_static_mappings()?;          // main.rs:11 ★ 分支A（总是跑）
    #[cfg(feature = "application")]                   // main.rs:13
    application::gen_man_and_comp()?;                 // main.rs:14 ★ 分支B（feature 门控）
    Ok(())
}
```

三个前置概念先立住，再钻分支：

### 0a. build 脚本"先于主程序跑"，且和主程序是**两个独立编译单元**

`Cargo.toml:11` 的 `build = "build/main.rs"` 告诉 cargo：编译主程序**之前**，先把
`build/main.rs` 当一个独立小程序编译并运行一次。它跑完，cargo 才去编 `src/`。

| 编译单元 | 用的库表 | 产物 |
|---|---|---|
| build 脚本（`build/main.rs`） | `[build-dependencies]`（Cargo.toml:109） | 跑完即弃，**不进二进制** |
| 主程序（`src/`） | `[dependencies]`（Cargo.toml:43） | 最终 `bat.exe` |

> 心智：**build 脚本是"编译期的临时工"**。它的活（生成文件）做完自己就销毁，
> 只有它**生成的产物**通过 `include_*!` 活到运行时。所以 `[build-dependencies]` 里的
> `syn`/`quote`（cargo-toml 第六部分讲过）不会出现在用户手里的 bat.exe。

### 0b. 分支B 的 `#[cfg(feature)]` 裁剪（main.rs:1, 13）

`mod application;`（第 2 行）和 `application::gen_man_and_comp()`（第 14 行）都被
`#[cfg(feature = "application")]` 包着——**没开这个 feature 时，这两行连同整个
application 模块在编译期物理不存在**。效果呼应 cargo-toml 学的：库用户
`default-features = false` 关掉 application 时，build 脚本只跑分支A（第 11 行没门控，
总是跑）。这是"feature 决定编译期代码在不在"的又一体现（architecture.md 判断 6）。

### 0c. `cargo:rerun-if-changed` 控制"何时重跑 build 脚本"（main.rs:9）

build 脚本→cargo 的通信。默认规则是"build 目录任何文件变了就重跑"，这里显式声明
**只盯 `build/` 目录**——改 `src/` 不触发 build 脚本重跑，只有 `build/` 里的代码变了才重跑。
（分支B 的 `application.rs:7-13` 还有一串 `cargo:rerun-if-changed=assets/manual/`、
`cargo:rerun-if-env-changed=...`，同机制，把监视范围扩到"素材目录 + 特定环境变量"。）

## 分支A（钻到叶子）：`syntax_mapping` —— 扩展名→语法的静态映射表

**返回父分支前先把这条钻完。** `build/main.rs:11` 调它。

`build/syntax_mapping.rs`（369 行）用 `syn`/`quote`/`prettyplease` 做**程序化生成 Rust 源码**。
**你现在不必逐行看懂**（这是 codegen 炫技），记住"最小理解"三句：

1. **数据 → 代码**：读 `src/syntax_mapping/builtins/**/*.toml`（人工写的"文件名/扩展名 → 语法"
   映射，如 `.rs` → Rust、`Dockerfile` → Docker），把每条映射**生成成一段 Rust 静态查找表源码**。
   核心在 `build_static_mappings()`（syntax_mapping.rs:352）：把映射构造成 `MappingTarget`
   枚举（syntax_mapping.rs:24，`MapTo`/`MapToUnknown`/`MapExtensionToUnknown` 三态，
   带 `ToTokens` 实现能把每个枚举值吐成 Rust token）。
2. **`include!` 进主程序**：生成的 `.rs` 写到 `$OUT_DIR`，主程序 `src/syntax_mapping.rs` 用
   `include!` 嵌进去，变成一张**编译进二进制的查找表**。这张表就是 **t07 讲"选 syntax"
   （`assets.get_syntax_for_path`）时查的那张表**——t07 只消费它，不关心它怎么生成。
3. **按目标平台 `cfg` 过滤**（syntax_mapping.rs:281-299 的 `get_def_paths()`）：
   `#[cfg(target_os = "linux")]`/`"windows"`——在 Windows 编译时只收 `common`+`windows`
   子目录的 toml，Linux 才收 `unix-family`/`linux`。同一份源码，不同目标平台生成不同映射集。

**为什么能跳过**：它讲的是"怎么把配置生成代码"，不影响你理解"bat 运行时怎么高亮"
（那是 t07 的事）。以后真想学 codegen，再翻 `build_static_mappings()`（352 行）和
`ToTokens`（39 行）——那是"让数据会说话、直接生成 Rust 代码"的干净范式。

**分支A 结束，返回 build 脚本 main()，继续走分支B。**

## 分支B（钻到叶子）：`application` —— 补全脚本 + man 页

**先回到 main()：`#[cfg(feature = "application")] application::gen_man_and_comp()?`（main.rs:14）。
feature 关时整条分支不存在（§0b），直接跳"返回"。feature 开时钻进去：**

`build/application.rs:6` 的 `gen_man_and_comp()`，干两件事：**生成 4 个 shell 补全 + 1 个 man 页，
再把路径告诉 rustc**。三步链路：

### B1. 读模板（`assets/completions/*.in`，application.rs:40-64）

对 4 个 shell 模板 + man 模板各调一次 `render_template`：

```rust
render_template(&variables,
    "assets/completions/_bat.ps1.in",          // 模板（带 {{PROJECT_EXECUTABLE}} 占位符）
    out_dir.join("assets/completions/_bat.ps1"))?;   // 渲染好的输出
```

模板文件里写死 `{{PROJECT_EXECUTABLE}}` 这种**双花括号占位符**（不是 PowerShell 语法，
是 bat 自己定的渲染标记）。为什么要占位符：补全脚本里
`Register-ArgumentCompleter -Native -CommandName '{{PROJECT_EXECUTABLE}}'` 要填的是
**可执行文件名**——万一有人 fork 改名，脚本里的命令名也得跟着变，不能写死。

### B2. 替换占位符（`render_template`，util.rs:6）

```rust
let mut content = fs::read_to_string(in_file)?;        // util.rs:11 读模板
for (variable_name, value) in variables {
    let pattern = format!("{{{{{variable_name}}}}}");   // util.rs:15 → 拼出 "{{NAME}}"
    content = content.replace(&pattern, value);          // util.rs:16 全局替换
}
fs::write(out_file, content)?                            // util.rs:19 写到 OUT_DIR
```

`variables` 是 application.rs:21-28 攒的数组（`PROJECT_NAME`/`PROJECT_EXECUTABLE`/
`PROJECT_VERSION` 等，值来自环境变量，`PROJECT_NAME` 默认 `"bat"`）。替换后
`{{PROJECT_EXECUTABLE}}` 变成 `bat`。

### B3. 把路径告诉 rustc（`cargo:rustc-env`，application.rs:66-81）

渲染好的文件写在 `$OUT_DIR/assets/completions/`。光生成没用，得让主程序知道去哪 `include`。
build 脚本用这个 println 干这件事：

```rust
println!("cargo:rustc-env=BAT_GENERATED_COMPLETION_PS1={}",
    out_dir.join("assets/completions/_bat.ps1").display());   // application.rs:74-77
```

`cargo:rustc-env=KEY=VALUE` 会在**编译主程序时**给 rustc 设一个环境变量 `KEY`。
主程序里的 `env!("BAT_GENERATED_COMPLETION_PS1")` 就能取到这个路径。

**分支B 钻完（模板 → 渲染 → 传路径，三步都对上上面行号）。现在跳到 §1 看"传过去之后"
在 ② rustc 编译时怎么被焊死。**

## 1. 嵌入落点：`include_str!` / `include!` / `env!` —— "焊死"不是"运行时读盘"

**回到 ②（rustc 编译主程序）这一步。** 上面分支B 把路径传给了 rustc，现在主程序
`src/bin/bat/completions.rs`（就 6 行）消费它：

```rust
pub const PS1_COMPLETION: &str = include_str!(env!("BAT_GENERATED_COMPLETION_PS1"));
//                                  └──────────┬──────────┘
//                       env! 取路径 → include_str! 把该文件内容原样嵌成 &str 常量
```

三个宏分工（**编译期**发生，不是运行时读盘）：

| 宏 | 干什么 | 何时 |
|---|---|---|
| `env!("X")` | 取编译期环境变量 `X` 的值（这里是 build 脚本 `cargo:rustc-env` 设的路径） | 编译期 |
| `include_str!(路径)` | 把该文件**整个内容**读出来，作为 `&'static str` 常量塞进二进制 | 编译期 |
| `include!`（分支A 的语法映射表） | 把一个 `.rs` 源码文件当成当前文件的一部分展开 | 编译期 |

**所以** `PS1_COMPLETION` 这个常量，值就是那段 PowerShell 补全脚本全文，**已经躺在
bat.exe 里**。运行 `bat --completion ps1` 时（**t02 讲过**，main.rs:407
`writeln!(stdout, "{completion}")`）直接吐常量，**不需要** `assets/` 目录还在、
不需要任何运行时 IO。

> 这就是"bat.exe 单文件就能打印补全脚本"的原因：脚本在编译期被 `include_str!` 焊死了。
> 也是 Cargo.toml:10 `exclude = ["assets/syntaxes/*", "assets/themes/*"]` 敢把素材从
> crates.io 包裁掉的原因——素材只是"编译期的原料"，成品里已经有了。

**对照 t 系列的落点**（t 那边消费这些常量）：
- `--completion`（**t02**，main.rs:403）取的就是这份焊死的 `BASH/FISH/PS1/ZSH_COMPLETION`；
- `list_themes`（**t04**，main.rs:198）里的 `THEME_PREVIEW_DATA`（main.rs:45
  `include_bytes!("../../../assets/theme_preview.rs")`）是同一招——把预览代码也焊死进二进制。

**分支（嵌入落点）钻完，整棵树收口。**

## 2. 收口：把 build 脚本压成"一句话 + 一张时间线"

- **一句话**：`cargo build` 先跑 build/main.rs（编译期临时工），它读素材、生成文件、
  用 `cargo:rustc-env` 把路径传给 rustc；rustc 再编译主程序时，`include_str!`/`include!`
  把这些文件**焊进二进制**，运行期零 IO。
- **时间线**：`① build 脚本跑（分支A 映射表 + 分支B 补全/man）` →
  `② rustc 编译主程序（include 嵌入）` → `③ bat.exe 自包含`。

## 最小学习集（约 20 分钟，按"能离开 bat 还能用"排优先级）

| 优先级 | 学什么 | 对应上面 | 离开 bat 还通用吗 |
|---|---|---|---|
| ★★★ 必学 | build 脚本先于主程序跑、两单元各用各依赖表 | §0a | 通用（openssl/protobuf 系都用） |
| ★★★ 必学 | `cargo:rerun-if-changed` 何时重跑 | §0c | 通用 |
| ★★★ 必学 | `cargo:rustc-env` + `include_str!` 的"编译期嵌入" | 分支B + §1 | 通用（消解"bat 怎么读生成文件"） |
| ★★ 建议 | 模板渲染 `render_template`（读→替换→写） | 分支B B2 | 通用套路 |
| ★ 可跳 | 分支A 的 codegen（syn/quote/prettyplease） | 分支A | 进阶技巧，以后再说 |

## 检验问题（读完对照正文回答）

1. **build 脚本和主程序谁先跑？** 为什么 build 脚本用到的 `syn`/`quote` 不会出现在最终
   `bat.exe` 里？（答：build 脚本先跑；它的库来自 `[build-dependencies]`，只活在编译期，
   对应 §0a 那张表）
2. **`bat --completion ps1` 打印的脚本文本，运行时从磁盘哪读来的？**
   （答：不读磁盘——分支B 生成、`cargo:rustc-env` 传路径、`completions.rs` 的
   `include_str!` 焊死进二进制，见 §1）
3. **库用户 `default-features = false` 关掉 `application` 后，build 脚本还会生成 man/补全吗？**
   （答：不会——main.rs:13 的 `#[cfg(feature = "application")]` 把分支B 整段裁掉，
   只跑分支A，见 §0b）
4. **分支A 生成的"扩展名→语法"查找表，运行时哪一步被查？**（答：t07 的
   `assets.get_syntax_for_path` 选语法时查它）
