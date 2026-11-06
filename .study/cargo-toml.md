# Cargo.toml 逐项拆解（写给 Rust 小白的完整说明）

这一章的目标：把 bat 的 `Cargo.toml` **每个配置项**讲透——它是什么意思、cargo 什么时候读它、
最终影响编译出的二进制长什么样。读完你应该能回答：
"这行 `optional = true` 到底让哪个库没被编进去？"`#[cfg(feature = "git")]` 和 `git = ["gix"]` 是什么关系？

> 这不是编号章节（ch1~ch8 讲运行时数据流），而是**编译期配置**的独立补充，
> 和 `architecture.md` 并列。建议放在读完 ch8 之后看，因为里面会反复提到 feature 门控。

## 一句话总览：Cargo.toml 是什么

它是 Rust 包（crate）的**说明书 + 配方**，cargo 靠它做三件事：
1. **识别这个包**（`[package]`：叫什么、什么版本、谁写的）
2. **该拉哪些依赖**（`[dependencies]` / `[build-dependencies]` / `[dev-dependencies]`：三张表，各管一段代码）
3. **怎么编译**（`[features]`、`[profile]`、平台条件 `target.`：决定哪些代码进、优化到什么程度）

bat 这张表特别有教学价值，因为它**既是库又是程序**，feature 设计能完整演示"一个 crate 如何按场景裁剪自己"。

---

## 第一部分：`[package]`（第 1-14 行）—— 包的身份卡

这一整块都是**给人和 crates.io 看的元数据**，cargo 主要用它决定"编译什么、叫什么"。

| 行 | 配置 | 含义 / 效果 |
|---|---|---|
| 7 | `name = "bat"` | 包名。决定 `use bat::...`、二进制文件名、crates.io 上的名字 |
| 9 | `version = "0.26.1"` | 语义化版本（主.次.修订）。别人依赖 `bat = "0.26"` 时按这个匹配 |
| 2 | `authors` | 作者信息，纯展示 |
| 3 | `categories` | crates.io 的分类标签，决定它在网站哪个页面出现 |
| 4 | `description` | 一句话介绍 |
| 5 / 8 | `homepage` / `repository` | 项目主页 / 源码地址 |
| 6 | `license = "MIT OR Apache-2.0"` | **双许可**：用户可在 MIT 和 Apache-2.0 里**任选其一**使用 |

### 关键的两条：`exclude` 和 `build`

- **`exclude = ["assets/syntaxes/*", "assets/themes/*"]`（第 10 行）**
  发布到 crates.io 时**不把**语法定义和主题文件打包进去（它们体积巨大，且是 git 子模块）。
  注意：这只影响 crates.io 的**发布包**，不影响你本地 git 仓库里的文件。
  呼应之前学的：这些素材在**编译期**被 build 脚本"焊"进二进制，发布时就不需要了。

- **`build = "build/main.rs"`（第 11 行）**
  指定 **build 脚本**入口。cargo 编译主程序**之前**会先跑 `build/main.rs`。
  它做的事：生成"文件扩展名 → 语法"的静态映射表、生成 man 页和 shell 补全脚本。
  这是前面"为什么 assets 要先编译"问题的落点。

- **`edition = '2021'`（第 12 行）**
  使用 Rust **2021 语言版本**（`dyn` 语法、空 struct 更新、`impl Trait` 捕获规则等）。
  不同 edition 的语法/行为不同，这里固定用 2021。

- **`rust-version = "1.88"`（第 14 行，MSRV）**
  **最低支持 Rust 版本**。cargo 用它解析依赖锁文件（`Cargo.lock`），保证在 1.88 上能编译。
  注释写着"有合理理由才升"。

---

## 第二部分：`[features]`（第 16-41 行）—— 特性开关（本项目的设计核心）

feature 是**编译期的开关**，决定哪些**可选依赖**和哪些 `#[cfg(feature = "...")]` 代码块参与编译。
bat 的灵魂设计：**当库用时砍掉应用专属功能，当程序用时全开。**

### 先看 feature 的两种写法

```toml
git = ["gix"]                    # 激活 git 时，把可选依赖 gix 拉进来
build-assets = ["syntect/yaml-load", ...]  # 激活时，打开 syntect 内部的 yaml-load 特性
```
等号右边的数组是"开了这个 feature 时，要连带激活的东西"——可以是自己的依赖、也可以是**别的 crate 的 feature**（`syntect/yaml-load` 就是打开 syntect 这个依赖的 yaml-load 特性）。

### 逐个 feature

| feature | 定义 | 干什么 | 什么时候开 |
|---|---|---|---|
| `default` | `["application", "git"]` | **默认全开**：完整程序 + git 标记 | `cargo build` 不带参数时 |
| `application` | `["bugreport","build-assets","minimal-application"]` | 应用所需全部。注释明说：**当库用时该关它** | 做完整 bat 程序 |
| `minimal-application` | `["clap","etcetera","paging","regex-onig","wild"]` | 最小可运行的应用骨架，开发者快速迭代用 | 介于"库"和"完整程序"之间 |
| `git` | `["gix"]` | 显示 git 修改标记（行旁 +/−） | 需要 git 集成 |
| `paging` | `["shell-words","grep-cli","minus"]` | 分页输出（起 less / 内置 minus） | 需要翻页 |
| `lessopen` | `["execute"]` | 支持 `$LESSOPEN` 预处理器 | 需要调用外部预处理 |
| `build-assets` | `["syntect/yaml-load","syntect/plist-load","regex","walkdir"]` | 打开 syntect 的 yaml/plist 加载 + 让 build 脚本嵌入 assets | 需要把语法素材编译进二进制 |
| `regex-onig` | `["syntect/regex-onig"]` | syntect 用 **oniguruma（C 库）** 正则引擎 | **库用户必选其一** |
| `regex-fancy` | `["syntect/regex-fancy"]` | syntect 用 **fancy-regex（纯 Rust）** 引擎 | 同上，二选一 |

### 关键设计点

1. **feature 表本身是静态定义，没有"开关"一说**；变的是"哪些 feature 被激活"。
   激活一个 feature = 把等号右边数组里的依赖/特性拉进编译。
2. **`default = ["application", "git"]`**：默认构建出完整 bat。
   库用户要写 `default-features = false, features = ["regex-onig"]`（或 `regex-fancy`），
   把 `application`/`git`/`paging` 全砍掉，得到最干净的依赖图。
3. **`regex-onig` / `regex-fancy` 二选一**（第 39-41 行注释）：
   这是 syntect 的正则引擎，必须选一个，否则 syntect 编译不过。
   onig 快但要编 C 库；fancy 纯 Rust 省事。库用户显式选一个。

> feature 和你之前学的 `#[cfg(feature = "git")]`（`clap_app.rs` 里给 `--diff` 加参数）**是同一套**：
> feature 没开 → 对应的 `Arg` 定义、`gix` 依赖整段被裁掉，二进制里根本不存在。

---

## 第三部分：`[dependencies]`（第 43-92 行）—— 主程序/库的依赖

这是**主代码（`src/`）能 `use` 的库**。分"固定依赖"和"可选依赖（`optional = true`）"两类。

### 固定依赖（没有 optional，永远编译）

| 库 | 用途 |
|---|---|
| `nu-ansi-term`、`ansi_colours`、`terminal-colorsaurus` | 终端颜色/ANSI 序列 |
| `console` | 终端交互（尺寸、清屏） |
| `syntect`（第 84，`default-features=false, features=["parsing"]`） | **语法高亮核心**（只开 parsing，关默认特性省体积） |
| `content_inspector` | 探测二进制文件（bat 遇到二进制会提示而非乱打） |
| `encoding_rs` | 字符集/编码转换 |
| `unicode-width`、`unicode-segmentation` | 终端等宽对齐、按字符切分（处理全角字符） |
| `globset` | 通配符匹配（`*.rs` 这种） |
| `serde` / `serde_derive` / `serde_yaml` / `bincode` | 序列化（配置文件、assets 元数据） |
| `semver` | 版本号解析 |
| `once_cell` | 懒加载单例（build 生成的静态映射表） |
| `flate2` | 压缩（assets 以压缩形式嵌入） |
| `thiserror` | 优雅的错误类型 |
| `itertools` | 迭代器工具 |
| `bytesize` | 文件大小人类可读格式 |
| `path_abs`（`default-features=false`） | 绝对路径 |
| `clircle`（`default-features=false`） | 检测"输出重定向回自身"的 IO 死循环 |
| `wild`（`optional`，feature 激活） | 跨平台通配符展开（替代 `env::args_os`，之前学过） |

### 可选依赖（`optional = true`，被某 feature 激活才编译）

```toml
wild = { version = "2.2", optional = true }        # ← minimal-application 激活
shell-words = { version = "1.1.1", optional = true }# ← paging 激活
minus = { version = "5.7", optional = true, ... }   # ← paging 激活（内置分页器）
bugreport = { ..., optional = true }                # ← application 激活
etcetera = { ..., optional = true }                 # ← minimal-application 激活（XDG 目录）
grep-cli = { ..., optional = true }                 # ← paging 激活
regex = { ..., optional = true }                    # ← build-assets 激活
walkdir = { ..., optional = true }                  # ← build-assets 激活
execute = { ..., optional = true }                   # ← lessopen 激活（起子进程）
gix = { ..., optional = true, ... }                 # ← git 激活
clap = { ..., optional = true }                     # ← minimal-application 激活（命令行解析）
```

**`optional` 的效果**：这个库**默认不编译**。只有当某个 feature 的数组里写了它的名字（如 `git = ["gix"]`）且该 feature 被激活时，它才进依赖图。
这就是"库用户关掉 application → clap/gix/minus 全不编译"的机制根源。

### 三个"表格外单独声明"的依赖（第 78-92 行）

cargo 允许给复杂依赖单开一块（写法 `name = { ... }` 等价，只是分开排版更清晰）：

- **`[dependencies.gix]`（78）**：git 库，`optional`，`default-features=false`，只开 `["sha1","blob-diff"]`（最小化 git 能力，只要 blob diff 来算行改动）
- **`[dependencies.syntect]`（84）**：高亮核心，`default-features=false`，只开 `["parsing"]`
- **`[dependencies.clap]`（89）**：命令行解析，`optional`，开 `["wrap_help","cargo"]`

> `default-features = false` 值得记住：**默认关掉依赖 crate 自己的一整套特性**，只手动开需要的，
> 编译更快、二进制更小。这是控制依赖体积的常用手法。

---

## 第四部分：`[target.'cfg(...)'.dependencies]`（第 94-107 行）—— 平台相关依赖

cargo 支持"只在某目标平台编译"的依赖：

- **`[target.'cfg(target_os = "macos")'.dependencies]`（94）**：`plist`
  只有 **macOS** 编译时才拉进来（解析 macOS 语法定义里的 plist 元数据）。Windows/Linux 编译看不到它。
- **`[target.'cfg(unix)'.dev-dependencies]`（106）**：`nix`（`term` 特性）
  只有 **unix（Linux/macOS）** 编译**测试**时才用（操作终端）。

这和 feature 门控（`#[cfg(feature)]`）是**并行的另一套条件**：一个按功能开，一个按操作系统开。

---

## 第五部分：`[dev-dependencies]`（第 97-104 行）—— 测试/示例专用依赖

**只在 `cargo test` / `cargo bench` 编译**时可用，**不会**进你发布给用户的主程序。

| 库 | 用途 |
|---|---|
| `assert_cmd` | 跑二进制、断言其 stdout/退出码（集成测试） |
| `expect-test` | 快照测试（"输出和预期完全一致吗"） |
| `serial_test` | 强制某些测试串行跑（避免冲突） |
| `predicates` | 对输出做"包含/匹配"式断言 |
| `wait-timeout` | 等待子进程（测试分页器等场景） |
| `tempfile` | 临时文件（测试时造输入文件） |
| `serde`（`features=["derive"]`） | 测试里序列化 |

**小白易错点**：dev 依赖不是"开发依赖"（开发主程序时用的），而是"跑测试时用的"。
你写 `src/` 主代码时 `use` 不到它们，只有 `#[cfg(test)]` 和 `tests/` 目录能 `use`。

---

## 第六部分：`[build-dependencies]`（第 109-128 行）—— build 脚本专用依赖

`build/main.rs`（前面 `[package].build` 指的那个）**编译期**跑的脚本，它能用的库写在这。
**这些库不会链接进最终二进制**——它们只活在"编译过程"里。

| 库 | build 脚本里的用途 |
|---|---|
| `anyhow` | 错误处理（`main()` 返回 `anyhow::Result`） |
| `syn`（`full`）+ `quote` + `proc-macro2` | **程序化生成 Rust 代码**：把扩展名→语法映射构造成源码字符串 |
| `prettyplease` | 把 `syn` 生成的代码自动格式化（可读） |
| `indexmap`（`serde`） | 有序 map + 序列化，生成代码时保序 |
| `toml`（`preserve_order`） | 解析本项目的 `Cargo.toml` 读依赖信息 |
| `serde` / `serde_derive` / `serde_with` | 数据（assets 清单等）反序列化 |
| `regex` | 处理语法定义文件内容 |
| `itertools` / `walkdir` / `once_cell` | 文件遍历、迭代辅助 |
| `[build-dependencies.clap]`（125） | build 脚本在 `application` feature 下生成 man/补全脚本，需要读 clap 的命令定义 |

**为什么 clap 在 `[dependencies]` 和 `[build-dependencies]` 各出现一次？**
- `[dependencies.clap]`（89）：给**主程序**解析命令行
- `[build-dependencies.clap]`（125）：给 **build 脚本**生成补全/man（这些内容来自 clap 的参数定义）
两者 `optional`，都由 feature 门控，feature 关了就都不编译。

---

## 第七部分：`[profile.release]`（第 130-133 行）—— 发布版编译优化

只在 `cargo build --release` / `cargo install` 时生效：

| 配置 | 效果 |
|---|---|
| `lto = true` | **链接期优化**（Link-Time Optimization）：跨 crate 优化，二进制更快更小，代价是编译慢很多 |
| `strip = true` | 去掉符号表/调试信息，二进制更小 |
| `codegen-units = 1` | 只用一个代码生成单元，牺牲编译速度换极致优化（单元越少优化越透） |

**三样组合 = "编译慢一点，但发布出的 bat.exe 又快又小"**。这是 CLI 工具的典型 release 配方。

---

## 把整张表串成一张图

```
Cargo.toml
├─ [package]          身份卡（name/version/exclude/build/edition/MSRV）
├─ [features]         功能开关（application/git/paging/... → 决定哪些 optional 依赖进编译）
├─ [dependencies]     主代码依赖（固定 + optional，被 features 门控）
│  ├─ gix / syntect / clap 单独声明块（复杂配置）
├─ [target.'cfg(...)'] 平台相关（macos 用 plist、unix 测试用 nix）
├─ [dev-dependencies] 只测试用（assert_cmd/expect-test/...）
├─ [build-dependencies] 只 build 脚本用（syn/quote 生成代码，不进二进制）
└─ [profile.release]  发布优化（lto/strip/codegen-units）
```

**编译时的决策流**：
1. 定 feature（`default` 或用户指定）→ 决定哪些 `optional` 依赖、哪些 `#[cfg(feature)]` 代码被保留
2. 按目标平台过滤 `target.` 依赖
3. 先编译 build 脚本（`build/main.rs`，用 `[build-dependencies]`）→ 生成映射/man/补全
4. 编译主程序（用 `[dependencies]`）
5. `cargo test` 时额外编译测试（用 `[dev-dependencies]`）
6. `--release` 时套用 `[profile.release]` 优化

---

## 小白检验问题（读完自己答）

1. 别人把 bat 当**库**用，要最精简的依赖图，Cargo.toml 里那行该怎么写？（提示：`default-features` + 必选 feature）
2. `gix` 标了 `optional = true`，但 `git` feature 没开时，`gix` 会被编译进主程序吗？为什么？
3. `syn` / `quote` 出现在 `[dependencies]` 还是 `[build-dependencies]`？它们最终会链接进用户手里的 bat 可执行文件吗？
4. `[dev-dependencies]` 里的 `assert_cmd`，写 `src/main.rs` 主代码时能 `use` 到吗？
5. `cargo build --release` 比 `cargo build`（debug）编译慢很多，主要怪 `[profile.release]` 里哪几项？

> 参考答案：
> 1. `bat = { version = "0.26", default-features = false, features = ["regex-onig"] }`（regex-onig 或 regex-fancy 二选一）
> 2. 不会。feature 没激活 → 该 optional 依赖不进依赖图
> 3. 在 `[build-dependencies]`，只用于编译期生成代码，**不**进最终可执行文件
> 4. 不能。dev 依赖只在 `#[cfg(test)]` / `tests/` 可见
> 5. `lto` 和 `codegen-units = 1`（跨 crate 优化、单代码生成单元，都极耗时）
