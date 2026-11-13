# Cargo.toml（效果视角）：每个配置项"这么配 → 编译出的 bat 长什么样"

> 定位：`.study` 的**编译期配置**补充，和 `architecture.md` 并列。建议**读完 t09 + build 篇之后**看。
> 组织方式：不再逐项罗列，而是**每个配置项都落到"配了它 → 产生什么后果"**——
> 后果用可验证的方式锚定（`cargo tree` 看到的依赖、二进制里 `#[cfg]` 代码块存不存在、
> 编译速度/体积变化），不是干巴巴解释名词。
>
> 读这一篇的检验标准：看完应该能回答"我把 feature 从 default 换成 `default-features=false,
> features=["regex-onig"]`，编译出的东西和默认 build 差在哪？哪些代码没了？"

## 0. 先立总图：Cargo.toml 在"编译流水线"的哪一环

Cargo.toml 本身**不运行**，它是给 cargo/rustc 看的"配方"。一次 `cargo build` 里它被读的顺序：

```
cargo build
  │
  ├─ 读 [package]（身份 + MSRV + build 脚本入口）
  ├─ 定 feature（default 或用户指定）→ 决定"哪些 optional 依赖进依赖图、哪些 #[cfg(feature)] 代码保留"
  ├─ 按 target. 过滤平台依赖（Windows 只拉 windows 需要的）
  ├─ 先编 build 脚本（[build-dependencies]）→ 生成映射表/补全（build 篇讲过）
  ├─ 编主程序 [dependencies]（feature 门控决定哪些代码块存在）
  ├─ cargo test 才额外编 [dev-dependencies]
  └─ --release 才套 [profile.release]
```

**核心心智**：Cargo.toml 的每个配置，效果都体现在"**编译出的二进制里，哪些代码/库在、哪些不在**"。
下面每个部分都按这个落点讲。

---

## 1. `[package]`（Cargo.toml:1-14）—— 配了它，"编译产物叫什么、能不能在老 rustc 上编"

| 配置（行） | 这么配 → 效果 |
|---|---|
| `name = "bat"`（7） | 产物叫 `bat`；库被 `use bat::...` 引入；crates.io 上名字就是 bat |
| `version = "0.26.1"`（9） | 别人写 `bat = "0.26"` 依赖时按此匹配；build 脚本把它 `cargo:rustc-env` 给主程序 |
| `license = "MIT OR Apache-2.0"`（6） | **双许可**：用户可在两个许可里任选其一，无需同时满足 |
| `exclude = ["assets/syntaxes/*", "assets/themes/*"]`（10） | 发布到 crates.io 时**不把**语法/主题文件打包（体积巨大且是 git 子模块）。**效果**：你 `cargo install bat` 拿到的包**没有**这些素材——但没关系，build 篇讲过它们在**编译期**被 `include` 焊进二进制，发布包不需要 |
| `build = "build/main.rs"`（11） | **效果**：cargo 编译主程序前先跑 build 脚本（build 篇整条链的入口）。删掉这行 = 不生成映射表/补全，`include_str!` 会失败 |
| `edition = '2021'`（12） | 用 Rust 2021 语言版本（`dyn` 语法、空 struct 更新等）。**效果**：同一份代码在不同 edition 下行为/语法不同，固定 2021 避免歧义 |
| `rust-version = "1.88"`（14，MSRV） | **效果**：cargo 用它在解析 `Cargo.lock` 时挑依赖的兼容版本，保证"1.88 能编译"。升了它 = 逼所有依赖方升级 rustc |

---

## 2. `[features]`（Cargo.toml:16-41）—— 配了它，"哪些库进依赖图、哪些 `#[cfg]` 代码块存/亡"

**feature 是编译期开关**。效果分两层（这是本篇最该吃透的）：

- **依赖层**：激活一个 feature = 把它等号右边数组里的**可选依赖**拉进依赖图。
  用 `cargo tree | grep gix` 能看到：开了 `git` 才有 `gix`，关了就没有。
- **代码层**：主程序里的 `#[cfg(feature = "git")]` 代码块（t06/t07 的 git diff、
  `LineChangesDecoration`）**只有 feature 开时才编译进去**，关了物理不存在。

### 2a. 两种写法（先看机制再逐个）

```toml
git = ["gix"]                       # 开 git → 把可选依赖 gix 拉进来（依赖层）
build-assets = ["syntect/yaml-load", ...]   # 开 build-assets → 连打开 syntect 这个依赖的 yaml-load 特性（跨 crate feature）
```
等号右边数组 = "开了这个 feature 时连带激活的东西"，可以是自己的可选依赖，
也可以是**别的 crate 的 feature**（`syntect/yaml-load` = 打开 syntect 的 yaml-load 特性）。

### 2b. 逐个 feature（配了 → 效果）

| feature（行） | 这么配 → 效果（`cargo tree` / 二进制里看到什么） |
|---|---|
| `default = ["application","git"]`（17） | **默认 `cargo build` 产物**：完整 bat + git 标记。`cargo tree` 里有 `gix`/`minus`/`clap` |
| `application`（20） | 应用所需全开（bugreport+build-assets+minimal-application）。**注释明说：当库用时该关它** |
| `minimal-application`（27） | 拉进 `clap`/`etcetera`/`paging`/`regex-onig`/`wild`——最小可运行程序骨架 |
| `git`（34） | 拉进 `gix` + 让 t06/t07 的 `#[cfg(feature="git")]` 块编译进去。效果：行旁能画 `+`/`-` git 改动 |
| `paging`（35） | 拉进 `minus`/`grep-cli`/`shell-words`。效果：t05 的 `OutputType` 能起 less/内置分页；关了就永远 stdout |
| `lessopen`（36） | 拉进 `execute`。效果：t06 的 lessopen 预处理器存在 |
| `build-assets`（37） | 打开 syntect 的 yaml/plist 加载 + 拉 `regex`/`walkdir`。效果：build 脚本能把语法素材编译进二进制 |
| `regex-onig`（40） | syntect 用 **oniguruma（C 库）** 正则引擎。**库用户必选 onig/fancy 其一** |
| `regex-fancy`（41） | syntect 用 **fancy-regex（纯 Rust）**。同上，二选一 |

### 2c. 三个关键"配了 → 效果"判断

1. **`default = ["application","git"]` 的含义**：不带参数 `cargo build` 得到完整版。
   库用户要写 `bat = { default-features = false, features = ["regex-onig"] }`，
   **效果**：`application`/`git`/`paging` 全砍 → `clap`/`gix`/`minus` 都不进依赖图，
   二进制只剩高亮核心，`cargo tree` 会明显变短。
2. **`regex-onig` / `regex-fancy` 二选一**（40-41）：syntect 的正则引擎**必须**选一个，
   否则 syntect 编译不过。onig 快但要编 C 库；fancy 纯 Rust 省事。**效果**：选错（都不选）
   = 编译报错；两个都选 = 冲突。
3. **feature 是"拉依赖"不是"开关本身"**：feature 表是静态定义，变的是"哪些被激活"。
   激活 = 把右边数组拉进编译。没有 feature 激活某 optional 依赖，那个依赖就永远不进二进制。

> 呼应 t 系列：feature 和你在 t06/t07 看到的 `#[cfg(feature = "git")]` 是**同一套**——
> feature 没开 → 对应的 `Arg` 定义、`gix` 依赖、git diff 代码块**整段被裁掉**，二进制里根本没有。

---

## 3. `[dependencies]`（Cargo.toml:43-92）—— 配了它，"主程序能 `use` 哪些库、哪些默认不编"

分**固定**（永远编）和**可选**（`optional=true`，被 feature 拉进来才编）两类。

### 3a. 固定依赖（没有 optional → 任何构建都有）

| 库 | 配了它 → 效果（主程序里干什么） |
|---|---|
| `syntect`（84-87，`default-features=false, features=["parsing"]`） | **高亮核心**。`default-features=false` 只开 parsing → 二进制小、编译快（t07 的高亮器就靠它） |
| `nu-ansi-term`/`ansi_colours`/`terminal-colorsaurus` | 终端颜色/ANSI 序列（t08 上色的底层） |
| `content_inspector` | 探测二进制文件（t06 编码嗅探的判定依据） |
| `unicode-width`/`unicode-segmentation` | 全角字符宽度、按 grapheme 切（t08 换行"按字形"就靠它） |
| `globset` | 通配符匹配（`*.rs`、`-m '*.build:Python'` 的匹配引擎） |
| `serde`/`serde_yaml`/`bincode` | 序列化（配置、assets 元数据 metadata.yaml） |
| `thiserror` | 派生错误类型（architecture 判断 7 的库 `Error`） |
| `once_cell` | 懒加载单例（build 生成的静态映射表首次用时才初始化） |
| `clircle`（`default-features=false`） | 检测"输出重定向回自身"（t05 防 IO 圈） |
| `path_abs`（`default-features=false`） | 绝对路径（t07 选 syntax 解析符号链接用） |

### 3b. 可选依赖（`optional=true` → 默认不编，被 feature 拉进来才编）

```toml
wild      = { ..., optional = true }   # minimal-application 激活
shell-words / minus / grep-cli = {..., optional}   # paging 激活
bugreport / etcetera = {..., optional}             # application / minimal-application 激活
regex / walkdir = {..., optional}                   # build-assets 激活
execute = {..., optional}                            # lessopen 激活
gix = {..., optional}                                # git 激活
clap = {..., optional}                               # minimal-application 激活
```

**配了 `optional=true` → 效果**：这个库**默认不进依赖图**。只有当某 feature 的数组里写了它
（如 `git = ["gix"]`）且该 feature 激活时才编译。**这就是"库用户关 application →
clap/gix/minus 全不编"的机制根源**——`cargo tree` 在两种配置下数量差很多。

### 3c. 三处"单独声明块"（78-92，写法 `name = {...}` 等价，只是排版清晰）

- `[dependencies.gix]`（78）：`optional` + `default-features=false` + 只开 `["sha1","blob-diff"]`
  → **效果**：git 库只保留"算行改动"所需的最小能力，其余 git 功能全裁掉，二进制更小。
- `[dependencies.syntect]`（84）：只开 `["parsing"]` → 同上，最小化高亮核心。
- `[dependencies.clap]`（89）：`optional` + `["wrap_help","cargo"]` → 命令行解析，
  关掉 minimal-application 就没有。

> **`default-features=false` 值得单独记**：默认关掉依赖 crate 自己的一整套特性，只手动开需要的，
> 编译更快、二进制更小。这是控制依赖体积的常用手法（`gix`/`syntect`/`clap`/`path_abs`/`clircle`
> 都用了）。

---

## 4. `[target.'cfg(...)'.dependencies]`（Cargo.toml:94-107）—— 配了它，"某操作系统才有的依赖"

| 配置（行） | 这么配 → 效果 |
|---|---|
| `[target.'cfg(target_os="macos")'.dependencies]` `plist`（94） | **只在 macOS 编译时**拉进来（解析 macOS 语法定义里的 plist 元数据）。Windows/Linux 编译时 `cargo tree` 里**没有** plist |
| `[target.'cfg(unix)'.dev-dependencies]` `nix`（106） | **只在 unix 且跑测试时**用（操作终端）。Windows 上、或非测试构建都看不到 |

**效果 vs feature**：feature 按"功能"开关，`target.` 按"操作系统"开关——**两套并行的条件**。
同一个库可能"feature 开了 且 平台匹配"才真正进编译。

---

## 5. `[dev-dependencies]`（Cargo.toml:97-104）—— 配了它，"只有测试/示例能用，不进用户拿到的 bat"

| 库 | 配了它 → 效果 |
|---|---|
| `assert_cmd` / `predicates` / `expect-test` | **只在 `cargo test`/集成测试**里跑二进制、断言 stdout/退出码、快照比对 |
| `serial_test` | 强制某些测试串行（避免冲突） |
| `wait-timeout` | 等子进程（测分页器场景） |
| `tempfile` | 造临时输入文件 |

**易错点 + 效果**：dev 依赖不是"开发主程序用的"，是"**跑测试时用的**"。你写 `src/` 主代码时
`use` 不到它们，只有 `#[cfg(test)]` 和 `tests/` 目录能 `use`。
`cargo tree`（非 test）里看不到 dev 依赖。

---

## 6. `[build-dependencies]`（Cargo.toml:109-128）—— 配了它，"build 脚本能用、但绝不进最终二进制"

build 篇讲过：build 脚本先跑、跑完即弃。它能用的库写在这，**这些库不链接进 bat.exe**。

| 库 | 配了它 → 效果（build 脚本里干什么） |
|---|---|
| `syn`（`full`）+ `quote` + `proc-macro2` | **程序化生成 Rust 代码**：把扩展名→语法映射构造成源码（build 篇分支A） |
| `prettyplease` | 把 `syn` 生成的代码自动格式化（可读） |
| `indexmap`/`toml`（`preserve_order`） | 有序 map + 解析本项目 Cargo.toml，生成时保序 |
| `anyhow` | build 脚本的错误处理（`main()` 返回 `anyhow::Result`，见 build/main.rs:6） |

**为什么 `clap` 出现两次**（`[dependencies.clap]`:89 和 `[build-dependencies.clap]`:125）：
- `:89` 给**主程序**解析命令行（feature 门控，minimal-application 才有）；
- `:125` 给 **build 脚本**生成 man/补全脚本（补全内容来自 clap 的参数定义）。
两处各自 optional、各自 feature 门控。

**关键效果**：`syn`/`quote`/`prettyplease` **只活在编译期**（build 脚本里），
**不会**出现在用户手里的 bat 可执行文件。`cargo tree`（看主程序依赖）里根本没有它们。

---

## 7. `[profile.release]`（Cargo.toml:130-133）—— 配了它，"`--release` 产物又快又小、但编译更慢"

| 配置（行） | 这么配 → 效果（只在 `cargo build --release`/`cargo install` 时生效） |
|---|---|
| `lto = true`（131） | **链接期优化**（跨 crate 内联/去死码）。效果：二进制更快更小；代价：链接阶段极慢 |
| `strip = true`（132） | 去掉符号表/调试信息。效果：二进制更小、`bat.exe` 里没有可读符号（排障时看不到函数名） |
| `codegen-units = 1`（133） | 只用 1 个代码生成单元。效果：优化最透；代价：编译时**不能并行**，编译显著变慢 |

**三样组合的净效果**：debug build（`cargo build`）快、二进制大、`#[cfg]` 检查开（越界会 panic）；
release build（`cargo build --release`）编译慢很多，但产物又快又小。CLI 工具发布用 release。

---

## 8. 把"配置 → 效果"收成一张决策流

```
你要"当库用最精简" → 依赖方写 default-features=false + features=["regex-onig"]
   效果：clap/gix/minus 全不进依赖图；#[cfg(feature=git/paging/lessopen)] 代码块全裁掉；
        二进制只剩 syntect 高亮核心 + 你选的 regex 引擎

你要"Windows 上的完整 bat" → cargo build（default）
   效果：application+git 全开；target.macos 的 plist 不拉（因为是 Windows）；
        build 脚本生成 Windows 子目录的语法映射（build 篇 §分支A）

你要"发布版" → cargo build --release
   效果：套 [profile.release]，lto+strip+codegen-units=1，编译慢、产物小快
```

## 检验问题（配了 → 效果，自己答）

1. 别人把 bat 当**库**用最精简，Cargo.toml 那行怎么写？效果上 `cargo tree` 里会**没有**哪几个库？
   （`bat = { default-features=false, features=["regex-onig"] }`；没有 clap/gix/minus）
2. `gix` 标了 `optional=true`，但 `git` feature 没开时，`gix` 会被编译进主程序吗？
   （不会——feature 没激活，optional 依赖不进依赖图）
3. `syn`/`quote` 出现在哪张表？它们最终会进用户手里的 bat 可执行文件吗？
   （`[build-dependencies]`；不会，只活在编译期 build 脚本里）
4. `[dev-dependencies]` 的 `assert_cmd`，写 `src/main.rs` 主代码时能 `use` 到吗？
   （不能，只在 `#[cfg(test)]`/`tests/` 可见）
5. `cargo build --release` 比 debug 编译慢很多，主要怪 `[profile.release]` 哪几项？
   （`lto` + `codegen-units=1`：跨 crate 优化 + 单代码生成单元不可并行）
6. **综合**：把 `default` 的 `["application","git"]` 改成只留 `["build-assets"]`，
   编译出的 bat 在 t 系列里会"失去"哪些能观测到的行为？（提示：t02 的 --diagnostic、
   t04 的 list-languages/list-themes、t05 的分页、t06 的 git diff 还会在吗？）
