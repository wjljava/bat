# Cargo.toml 完全教学（给刚学完官方教程的你）

> **学完标准**：能独立读懂 bat 的 `Cargo.toml` 每一行，能写出任何场景下依赖方
> 该写的 `bat = {...}` 那一行，能解释"改一个配置 → 编译产物发生什么变化"。
> **读法**：按 1→13 顺序读，别跳。每节末尾有检验题，**合上文档答不上来就回去重读那节**。
> 所有行号指 `D:\RustProject\bat\Cargo.toml`，所有验证命令在 bat 仓库根目录敲。

---

## 1. 心智模型：Cargo.toml 是菜谱，不是代码

先纠正官方教程留下的模糊印象。你写过的是这种：

```toml
[dependencies]
serde = "1.0"
```

教程让你"加一行就有库可用"，但没告诉你**这行背后发生了什么**。补全它：

- **Cargo.toml 本身一行代码都不运行**。它是给 cargo 看的菜谱。
- cargo 读了菜谱后做四件事：**定身份**（包叫什么、谁写）、**拉依赖**（去 crates.io 下载/锁版本）、
  **排编译顺序**（谁先编谁后编）、**传参数给 rustc**（feature、cfg 开关、profile）。
- 你的 `src/` 代码里写 `use serde::...` 能成立，唯一原因是 `[dependencies]` 里有 serde。
  **菜谱里没有的食材，做菜时就是 `error[E0432]: unresolved import`。**

两个词从此要分清：

| 词 | 指什么 |
|---|---|
| **依赖（dependency）** | 你的 crate 用到的另一个 crate |
| **feature** | 某个 crate 身上的"编译期开关"，决定它内部编译哪些功能、拉哪些它自己的依赖 |

一个 crate 可以同时是"依赖"（对上游）又是"开 feature 的对象"（对它自己）。
比如 `gix` 是 bat 的依赖，`git` 是 bat 的 feature，`git` 开了才把 `gix` 拉进来——**feature 是拉依赖的开关**。这是第 5 节的主题。

**本节检验**：`src/lib.rs` 里 `use syntect::...` 能编译通过，依据是 Cargo.toml 的哪一块？（答案：第 84-87 行的 `[dependencies.syntect]`。）

---

## 2. `[package]`：包的身份证（第 1-14 行）

```toml
[package]
authors = ["David Peter <mail@david-peter.de>"]   # 谁写的
categories = ["command-line-utilities"]            # crates.io 上的分类标签
description = "A cat(1) clone with wings."         # crates.io 页面展示
homepage = "https://github.com/sharkdp/bat"        # 元数据
license = "MIT OR Apache-2.0"                      # 双许可：使用者二选一，不用两个都满足
name = "bat"                                        # 产物名 + 库名
repository = "https://github.com/sharkdp/bat"      # 元数据
version = "0.26.1"                                  # 语义化版本
exclude = ["assets/syntaxes/*", "assets/themes/*"]  # 发布到 crates.io 时不打进包的东西
build = "build/main.rs"                              # 编译前先跑这个脚本（build 篇主角）
edition = '2021'                                     # 语言版本
rust-version = "1.88"                                 # MSRV：最低支持 rustc 版本
```

逐个说**为什么有它**：

- **`name`**：三处生效。`cargo build` 产物叫 `bat`/`bat.exe`；别人写 `use bat::...`
  （bat 同时是库，见 `src/lib.rs`，可执行文件在 `src/bin/bat/`）；crates.io 上搜的就是这个名字。
- **`version` + 语义化版本（semver）规则**：`主.次.修订`。**主版本变 = 可以破坏兼容，
  次版本变 = 只加功能不破坏**。所以依赖方写 `serde = "1.0"` 时（下一节讲），cargo 允许
  升级到 1.x 任何版，但绝不允许跳到 2.x。bat 是 `0.26.1`——**0.x 比较特殊：次版本升级也算破坏性**，
  所以 0.x 项目间互相依赖时最好锁得更紧。
- **`exclude`**：`assets/` 下是 git 子模块拉来的几百个语法文件，体积巨大。发布包不带上，
  因为 build 篇讲过——它们编译时已被 `include!` **焊进二进制**，发布包用不到原件。
- **`build`**：不写这行 cargo 找不到的 `build/main.rs` 就不跑了；它生成"扩展名→语法"映射表，
  主程序 `include_str!` 依赖它 → 删了这行 = 编译直接失败。
- **`edition`**：Rust 语言分版。edition 2021 才有 `dyn Trait` 空方括号语法、
  `async` 块进标准库等。同一份代码在不同 edition 下编译规则不同，写死 2021 = 消除歧义。
  官方教程默认就给你 2021，所以你可能没意识过它的存在。
- **`rust-version`（MSRV）**：cargo 选依赖版本时参考它——"别给我挑一个需要 rustc 1.99 的
  库，因为 1.88 的机器也要能编"。bat 升它 = 所有依赖 bat 的人被迫升级 rustc。

**验证**：

```powershell
cargo package --list     # 看发布包里到底排除了什么（exclude 生效证明）
```

**本节检验**：

1. `license = "MIT OR Apache-2.0"` 对使用者意味着什么？（两个许可里选一个遵守即可）
2. 删掉 `build = "build/main.rs"` 会发生什么？（build 脚本不跑 → 映射表没生成 → 主程序 `include_str!` 编译失败）

---

## 3. 依赖版本约束：`"1.0"` 这四个字符到底是什么意思

官方教程只说"加个 `serde = "1.0"`"，没解释引号里是**约束**不是**精确版本**。这是你以后读
所有 Cargo.toml 的通用语法，必须拿下：

```toml
bincode = "1.0"                    # 简写！= ^1.0，即 >=1.0.0 <2.0.0
ansi_colours = "^1.2"             # 显式 ^，即 >=1.2.0 <2.0.0
gix: version = "0.86"             # 0.x 特例！= >=0.86.0 <0.87.0（0 的次版本升级也算破坏）
```

| 写法 | 展开成 | 允许的版本范围 |
|---|---|---|
| `"1.0"` | `^1.0` | `>=1.0.0, <2.0.0` |
| `"0.50.3"` | `^0.50.3` | `>=0.50.3, <0.51.0` |
| `"=0.5"` | 精确锁 | 只有 `0.5.0` |
| `"~1.4"` | 波浪号 | `>=1.4.0, <1.5.0` |
| `">=1.4, <2.0"` | 手写区间 | 任意复杂区间 |

**谁做最终决定？** 你写的是"可接受范围"，**`Cargo.lock` 才是答案**。
bat 仓库的 `Cargo.lock` 里每个库锁死了具体版本（比如 `bincode 1.3.3`），
整个项目所有人、CI、编译期看到的都是同一份答案——这就是 reproducible build 的来源。
库项目发布时不带 lock（让下游自己解），二进制项目必须带（锁死）。

**验证**：

```powershell
# 看约束（Cargo.toml）vs 实际锁定的版本（Cargo.lock）
Select-String -Path Cargo.lock -Pattern '^name = "bincode"' -Context 0,1
# 输出 bincode 后面紧跟的 version = "1.3.3"，对照第 3 节表格：1.3.3 ∈ ^1.0 ✓
```

**本节检验**：`nu-ansi-term = "0.50.3"`（:44）允许升到 `0.50.9` 吗？允许升到 `0.51.0` 吗？
（允许；不允许——0.x 的次版本是破坏边界。）

---

## 4. 固定依赖 vs 可选依赖：`optional` 是"默认关"

`[dependencies]`（:43-76）里的库分两类：

**固定依赖**——直接 `库名 = "版本"` 写法，任何构建都编：

```toml
syntect = ...        # 高亮核心，砍不掉
serde = "1.0"       # 序列化
globset = "0.4"     # 通配符匹配
thiserror = "2.0"   # 派生错误类型（architecture 篇的判断 7 全靠它）
once_cell = "1.20"  # 懒加载单例（build 生成表的首次初始化靠它）
```

**可选依赖**——`optional = true`，**默认不编**，要等一个 feature 把它拉进来：

```toml
wild = { version = "2.2", optional = true }          # :51
gix = { version = "0.86", optional = true, ... }     # :78（独立声明块写法）
clap = { version = "4.6.1", optional = true, ... }  # :89
```

全仓带 `optional` 的共 **12 个**：`wild`、`shell-words`、`minus`、`grep-cli`、
`bugreport`、`etcetera`、`regex`、`walkdir`、`execute`、`gix`、`clap`（:89）、`clap`（:125，build 依赖）。
每个都在等开关——下一节讲开关在哪。

**写法细节**：`库 = { version = "...", optional = true }` 和下面这种**完全等价**，
只是排版（库多特性时单独成块更好读）：

```toml
[dependencies.gix]
version = "0.86"
optional = true
default-features = false
features = ["sha1", "blob-diff"]
```

**本节检验**：`syntect`（:84-87）是可选依赖吗？（不是——块里没有 `optional`，
它是固定依赖，砍不掉。注意它身上的 `default-features = false` 只关 syntect 自己的特性，不影响"进不进图"。）

---

## 5. feature：把"可选依赖"和"开关"连起来

### 5.1 最小例子（从零搭，不看 bat）

假设你要写个带可选功能的库：

```toml
[dependencies]
# 固定依赖
log = "0.4"
# 可选依赖：默认不编
reqwest = { version = "1", optional = true }

[features]
http = ["reqwest"]        # ← 开关名叫 http，按下它 = 把 reqwest 拉进编译
```

此时：

- `cargo build` → reqwest 没被任何人拉 → **不编译**，源码里 `use reqwest` 的行要么有 `#[cfg(feature="http")]` 要么根本不存在
- `cargo build --features http` → 按下开关 → reqwest 进依赖图 → 源码里 `#[cfg(feature = "http")]` 的代码块开始参与编译

**feature 表本身是静态定义**；动态的是"哪些开关被按下"，被按下的来源有四个：

```
命令行        cargo build --features git,paging     ← 手动按
default       cargo build 自动按                     ← 不传参时的默认
依赖方传递     你的下游写 features = ["..."]          ← 库用户替你按
跨 crate      "syntect/yaml-load"                    ← 替别人按（见 5.4）
```

### 5.2 `default`：不是 feature，是"默认按哪些"的清单

bat 的 :17：

```toml
default = ["application", "git"]
```

读法：**不带任何参数 `cargo build` 时，自动按下 `application` 和 `git` 两个开关。**
（注意 `default` 这条本身也写在 `[features]` 表里，但它是特殊的"默认清单"，不是功能开关。）

对应 bat 的两个开关（:34-35）：

```toml
git = ["gix"]                    # 按下 git → 拉 gix → 行旁能画 +/− git 改动
paging = [ "shell-words", "grep-cli", "minus"]   # 按下 paging → 拉分页三件套
```

**验证**（两条命令，输出差异就是 feature 的效果）：

```powershell
cargo tree --depth 1                                  # 默认：有 gix、minus、clap...
cargo tree --no-default-features --features regex-onig --depth 1   # 精简：它们全没了
```

`--no-default-features` = 清单一个都不按；`--features a,b` = 手动按 a 和 b。

### 5.3 feature 合键（嵌套）：一个开关带一串

:20-33：

```toml
application = [
    "bugreport",
    "build-assets",
    "minimal-application",
]
minimal-application = [
    "clap", "etcetera", "paging", "regex-onig", "wild",
]
```

feature 的右边数组里可以写**别的 feature 名**——按下 `application` 等于同时按下三个。
像遥控器上的"一键全开"。bat 的完整链条：

```
default ─┬─► application ─┬─► bugreport                    （开关→bugreport 库）
         │                ├─► build-assets ─► syntect/yaml-load、syntect/plist-load、regex、walkdir
         │                └─► minimal-application ─┬─► clap        （→clap 库）
         │                                          ├─► etcetera    （→etcetera 库）
         │                                          ├─► wild        （→wild 库）
         │                                          ├─► regex-onig ──► syntect/regex-onig（syntect 选引擎）
         │                                          └─► paging ─► shell-words、grep-cli、minus
         └─► git ─► gix
```

**结论（回答你最早那个问题）**："库用户写 `default-features = false, features = ["regex-onig"]`
→ 和默认 build 差在哪？" 把 `application`/`git` 的链**全断**：`clap`/`gix`/`minus`/`wild`…
不进依赖图；源码里挂 `#[cfg(feature="git")]` 等的代码块物理消失；`regex-onig` 手动补上，
保证 syntect 的正则引擎不缺。二进制只剩"高亮核心 + 引擎"。

**判断技巧（反向找）**：想知道某库归谁管，看哪个 feature 的数组里有它的名字。
`minus` → 在 `paging` 数组里 → `paging` 被 `minimal-application` 合着 → 再被 `application` 合着
→ 在 `default` 里。链条任何一环断了，`minus` 就没。

**验证**：

```powershell
# 加开一个非默认 feature（lessopen 不在 default 清单里），看它单独拉进来什么：
cargo tree --no-default-features --features lessopen --depth 1   # 应只多出 execute
```

### 5.4 跨 crate feature：斜杠写法 `syntect/yaml-load`

:40：

```toml
regex-onig = ["syntect/regex-onig"]
```

读法：**替 syntect 按下它家的 `regex-onig` 开关**。斜杠左边是"谁家的库"，右边是"它家的开关"。
bat 不自己实现正则，syntect 高亮需要正则引擎，而 syntect 把它做成两个 feature 让上游选：

```toml
regex-onig  = ["syntect/regex-onig"]   # oniguruma：C 库，编译要 C 工具链，运行快
regex-fancy = ["syntect/regex-fancy"]  # fancy-regex：纯 Rust，省心，稍慢
```

**硬性规则**：syntect 的正则引擎**必须选一个**——都不选 syntect 编译不过；都选冲突。
所以 bat 在注释里（:39）明说：当库用我时，你至少开 `regex-onig` 或 `regex-fancy` 其一。
这就是库用户那行 `features = ["regex-onig"]` 里必须有它的原因。

**另一个坑（feature 统一）**：feature 是**加法且幂等**的——同一个构建里两条路都能按到
`syntect/regex-onig` 就合并成一次，不冲突；但**不同构建**（比如你的二进制 + 它依赖的库各自解
feature）可能解出不同组合，这是"我在本地好好的，CI 上挂了"的经典来源。知道有此现象即可，
bat 项目里不会碰到。

### 5.5 机制：`#[cfg(feature = ...)]` 是怎么和 Cargo.toml 对接的

**核心事实：rustc（编译器）根本不读 Cargo.toml。** rustc 只认命令行参数，
对"配置文件"一无所知。Cargo.toml 和源码里 `#[cfg]` 之间的桥，**全部由 cargo 搭**：

```
你按开关                cargo                                rustc
─────────              ──────                               ─────
--features http  ─►  读 [features] + default 清单 + 命令行  ─►  在 rustc 命令行上
(默认清单自动按)        算出"最终被按下的集合"                   为每个按下的 feature X
                                                              加一个 --cfg 'feature="X"'
                                                                  │
                                                                  ▼
                                              源码里 #[cfg(feature = "http")] 在
                                              预处理阶段对照命令行 → 保留或删除该代码块
```

**证据（5 行自建，亲眼跑一遍）**：在仓库外建个最小项目（约 2 分钟）：

```powershell
# 1. 建项目
cargo new cfgdemo && cd cfgdemo
# 2. Cargo.toml 的 [features] 加一行： http = []
# 3. src/main.rs 里加：
#    #[cfg(feature = "http")] { println!("http 块编译进来了"); }
# 4. 两次编译对比 rustc 命令行：
cargo build -v | findstr "Running"        # 默认：只有 --check-cfg，没有 --cfg feature
cargo build -v --features http | findstr "Running"
#      → 多了 --cfg 'feature="http"' 这一个参数
```

同一份源码、同一个目录，两次编译唯一的区别就是那个 `--cfg 'feature="http"'`——
**这就是"配置文件配合源码"的全部机制**（旁边的 `--check-cfg` 是 cargo 告诉 rustc
"feature 条件只可能有这几个值"，所以你拼错 `#[cfg(feature = "htttp")]` 会收到警告，
而不是被静默删代码。跑完删掉 cfgdemo 即可。）

**`#[cfg]` 在干什么：预处理，不是判断。** rustc 在类型检查之前先做一道条件裁剪：
不满足条件的代码整块删掉，剩下的才参与名称解析/类型检查。两个直接推论：

1. **被删掉的代码不需要能通过编译**——`use gix::...`、git diff 代码可以留在源码里，
   开关没按时它们被裁掉，不会报 `unresolved import`；同时 cargo 也没把 `gix` 编译进依赖图。
   **依赖层和代码层被同一个开关统一裁掉，闭环**。
2. **被删掉的代码在二进制里一个字节都没有**——不是运行时 `if` 跳过执行，是物理不存在。
   关掉 `git` feature 后 `bat.exe` 里没有 `--diff` 参数（`clap_app.rs:171` 的定义被裁了，连注册都没有）。

**`#[cfg]` 不只给 feature 用**，同一个机制、开关来源不同：

```rust
#[cfg(test)]              // cargo test 编译时传 --cfg test——你写单元测试用的就是它，
                          //   "开关是谁按的"答案就在这里
#[cfg(debug_assertions)]  // cargo build（debug）自动传，--release 不传
#[cfg(windows)]           // rustc 按编译目标（OS/架构）自动传
#[cfg(feature = "git")]   // cargo 按 [features] 传
```

**bat 里走一遍这个闭环**（feature 两层效果的完整链路）：

```
Cargo.toml:34  git = ["gix"]
   │  cargo 按下 git 开关
   ├─► ① gix（optional 依赖）进依赖图，被编译
   └─► ② rustc 命令行加 --cfg 'feature="git"'
         ▼
   源码里所有 #[cfg(feature = "git")] 的代码块（clap --diff 参数、diff 计算）
   被 rustc 预处理保留，参与编译
         ▼
   bat.exe 有 git diff 功能，链接了 gix
```

开关没按：①② 同时消失，代码和库一起被裁，二进制里干干净净。

全仓 `#[cfg(feature` 共 20+ 处，随手可 grep：

```powershell
(Select-String -Path src\*.rs,src\bin\bat\*.rs -Pattern '#\[cfg\(feature' | Measure-Object).Count
```

**本节检验**：

1. `cargo build --features lessopen`（在默认基础上**加开** lessopen）后，`execute` 库进依赖图吗？（进——feature 是加法，lessopen 按下拉 execute；默认的 application/git 照开）
2. 为什么库用户必须二选一选 regex 引擎？（syntect 高亮依赖正则，引擎是它家 feature，不选一个它编译不过）
3. `git` 开关没按时，`--diff` 参数在 `bat.exe --help` 里会出现吗？（不会——clap_app.rs:171 的参数定义被 `#[cfg]` 裁掉，连注册都没有）

---

## 6. 依赖的其他五张表：作用域问题

机制（第 4、5 节）会了，剩下五张表只是"**谁能 use 它、什么时候进编译**"。一张总表：

| 表 | bat 行号 | 谁能 `use` | 何时进编译 | 一眼验证 |
|---|---|---|---|---|
| `[dependencies]` | 43-92 | 主程序/库 | 永远（optional 除外） | `cargo tree --depth 1` 的主体 |
| `[dev-dependencies]` | 97-104 | **只有**测试/示例 | 仅 `cargo test` 编译测试目标时 | `cargo tree --depth 1 --edges dev` |
| `[build-dependencies]` | 109-128 | **只有** build 脚本 | 仅编译 build 脚本时 | `cargo tree --depth 1 --edges build` |
| `[target.'cfg(...)'.dependencies]` | 94 | 平台匹配时 | 仅对应 OS 编译时 | Windows 上 `cargo tree` 无 `plist` |
| `[target.'cfg(unix)'.dev-dependencies]` | 106 | unix 且测试 | 仅 unix + test | 你现在永远看不到 `nix` |

### 6.1 `[dev-dependencies]`：测试工具区（:97-104）

```toml
assert_cmd = "2.0.16"     # 把 bat.exe 当子进程跑、断言 stdout/退出码
expect-test = "1.5.0"     # 快照测试（输出和存档比对）
serial_test = ...         # 强制某些测试串行（防抢终端）
wait-timeout = "0.2.1"    # 等分页器子进程退出
tempfile = "3.27.0"       # 造临时输入文件
```

**关键**：它们**不是**"开发主程序用的库"。你写 `src/main.rs` 时 `use assert_cmd` 直接编译失败，
只有 `tests/` 目录和 `#[cfg(test)]` 模块能见到它们。
注意：**默认** `cargo tree`（`--edges all`）会**显示** dev 依赖（你跑过就会发现有 `assert_cmd`），
要看"主程序真正链接了哪些"，加 `--edges normal` 过滤。
记忆钩子：dev 依赖 = "**测你的东西时**用的东西"。

### 6.2 `[build-dependencies]`：编译期工具区（:109-128）

build 篇讲过：build 脚本先跑、跑完即弃。它用的库写在这张表：

```
syn + quote + proc-macro2   ← 程序化"生成 Rust 代码"（扩展名→语法映射源码）
prettyplease                ← 把生成代码格式化
indexmap / toml             ← 有序 map + 解析本项目 Cargo.toml
anyhow                      ← build 脚本的错误处理（build/main.rs:6 的 main 返回 anyhow::Result）
```

**关键效果**：`syn`/`quote` **只活在编译期**，绝不出现在用户手里的 bat.exe。
验证：`cargo tree --depth 1 --edges normal`（只看主程序边）里找不到 syn——
它只在 `--edges build`（或默认输出末尾的 build-dependencies 区块）里出现。

**`clap` 出现两次**（:89 和 :125）不矛盾：
- `:89` `[dependencies.clap]` → 给**主程序**解析命令行（feature 门控，`minimal-application` 才有）
- `:125` `[build-dependencies.clap]` → 给 **build 脚本**生成 man 页/补全脚本（补全内容来自 clap 参数定义）

两处独立的库副本、各自门控。同一库出现在两张表 = 两个不同的"使用方"，不是重复。

### 6.3 `[target]`：按操作系统开关（:94, :106）

```toml
[target.'cfg(target_os = "macos")'.dependencies]
plist = "1.9.0"                     # 解析 macOS 语法定义里的 plist 元数据——只有 macOS 需要

[target.'cfg(unix)'.dev-dependencies]
nix = { version = "0.31", ... }     # 操作终端——unix 测试才用
```

**和 feature 是两套并行的条件**：feature 按"功能"开关，`target` 按"操作系统"开关。
同一个库要进编译，可能得"**feature 开着 且 平台匹配**"。你 Windows 上 `cargo build`：
`plist` 不进（macOS 专属）、`nix` 不进（非 unix 且非测试）。

**验证**：`cargo tree --depth 1` 的默认输出里没有 `plist`、没有 `nix`——这就是 target 过滤的现场。

**本节检验**：

1. 你写 `src/main.rs` 时能 `use tempfile::tempdir()` 吗？（不能，dev-dependencies 只给测试）
2. `syn` 最后出现在用户下载安装的 `bat.exe` 里吗？（不，build-dependencies 只活在编译期）
3. 为什么 plist 能"只在 mac 编译"而不用 feature 开关？（它是平台条件不是功能条件，用 `[target]` 表达更直接——写了 feature 你也得在 mac 才开，绕远）

---

## 7. 依赖方视角：别人怎么依赖 bat（你以后要会写）

bat 既是库（`src/lib.rs`）又是可执行文件。库用户的 Cargo.toml 会这么写：

```toml
# 完整应用（等价于 cargo 默认）
bat = "0.26"

# 当库用最精简——官方教程从不教、但第 5 节全铺垫好了：
bat = { default-features = false, features = ["regex-onig"] }
```

逐项拆解：

- **`default-features = false`**：把 bat 的 `default` 清单（application+git）**一个都不按**
  → 第 5.3 的链条全断 → clap/gix/minus 等不进图
- **`features = ["regex-onig"]`**：手动按下引擎开关（补 syntect 的必需项）
- **`version = "0.26"`**：按第 3 节，0.x 下等价 `>=0.26.0 <0.27.0`——0 的次版本是破坏边界

**"效果"一句话说清**：依赖图的直接数量少一大截（两份 `cargo tree` 的差异就是它），
源码里所有 `#[cfg(feature="git")]` 等块被裁掉，二进制只剩高亮核心 + onig 引擎。

---

## 8. `[profile]`：不改"有什么"，改"怎么编"

```toml
[profile.release]        # 只在 --release / cargo install 时生效
lto = true              # 链接期跨 crate 内联/去死码
strip = true            # 删符号表/调试信息
codegen-units = 1       # 代码生成单元数：1 = 优化最透但不能并行
```

| 项 | 产物效果 | 编译代价 |
|---|---|---|
| `lto` | 更快更小（跨 crate 优化，砍掉用不到的代码） | 链接阶段极慢 |
| `strip` | 更小（没有可读函数名） | 排障时看不到符号 |
| `codegen-units = 1` | 优化最透 | 代码生成串行，编译显著变慢 |

对照表记牢：

```
cargo build          → 套默认 [profile.dev]：编译快、产物大、边界检查开（越界 panic）
cargo build --release → 套 [profile.release]：编译慢很多、产物又快又小
```

CLI 工具发布永远用 release。你日常开发迭代永远用 debug（秒级重编 vs 分钟级重编）。

**本节检验**：`cargo build --release` 比 debug 慢很多，主要怪哪几项？（`lto` + `codegen-units = 1`；`strip` 基本不花时间。）

---

## 9. 工具箱：随时把"效果"变成眼前

```powershell
# ① 看依赖图（直接依赖 / 全量 / 某个库的子树 / 为什么它被拉进来）
cargo tree --depth 1
cargo tree
cargo tree -p syntect
cargo tree -i gix          # "into"：反向追溯 gix 是谁拉进来的（feature 链条的活体演示）

# ② 看 feature 解出来的最终结果（"配置→效果"的最终裁判）
cargo metadata --format-version 1   # 输出 JSON，里面 resolve.nodes 是实解依赖图
# 更轻量：cargo build --features lessopen --dry-run? （没有 dry-run，用 ① 的两条对比法）

# ③ 版本约束检查
cargo update -p syntect --dry-run   # 不真改 lock，只看"能不能升到 5.x 的更新版"

# ④ 发布体检
cargo package --list                # 发布包实际包含哪些文件（验证 exclude）
```

**学习姿势**：以后每读一个新项目的 Cargo.toml，先跑 `cargo tree --depth 1`，
再找它的 `[features]` 表，把"哪个 feature 拉哪个库"反向连一遍（5.3 的判断技巧）。
十分钟内任何项目的 feature 结构都能摸清。

---

## 10. 实战：自己造一个"库用户"项目（10 分钟，强烈建议做）

这是把第 7 节从"看懂"变"会用"的一步。在**bat 仓库外面**（比如 `D:\RustProject\bat-as-lib\`）：

```powershell
cargo new bat-demo --bin
cd bat-demo
# 编辑 Cargo.toml，[dependencies] 里写：
#   bat = { path = "..\\bat", default-features = false, features = ["regex-onig"] }
# main.rs:
#   use bat::Printer;   // 库入口，能编译过即成功
cargo tree --depth 1
```

做三件事并观察 `cargo tree` 的变化：

1. **默认配法**（上面那行）→ 直接依赖应该只有 bat 及其"核心"（syntect、serde…），
   没有 `clap`、没有 `gix`
2. 把 `default-features = false` 删掉 → 重新 `cargo tree` → 冒出 `clap`、`gix`、`minus`…
   （application 链条被默认按下了）
3. 把 features 换成 `["regex-fancy"]` → `cargo build` 依然能过（另一个引擎），
   但注意 syntect 子树里的引擎变了

做完这三个对比，第 5 节的链条图就从"纸上"变成"眼前"了。
（编译 bat 全套要几分钟，耐心等一次即可，后面改配置是增量编译。）

---

## 11. 把 10 节收成一张速查表（读完全文后再看它才不晕）

| 概念 | 一句话 | bat 锚点 |
|---|---|---|
| Cargo.toml = 菜谱 | 不运行，cargo 读它定身份/拉依赖/排顺序/传参 | 全文 |
| 版本是约束不是精确值 | `"1.0"` = `^1.0`；`Cargo.lock` 才是答案 | :44-76、Cargo.lock |
| 固定 vs 可选 | `optional = true` = 默认不编，等 feature 拉 | :51、:78 |
| feature = 编译期开关 | 按下 → 拉依赖 + 放行 `#[cfg]` 代码块 | :16-41 |
| `default` = 默认按哪些 | 不带参 build 自动按 | :17 |
| feature 合键 | 右边数组写别的 feature 名 = 一键带一串 | :20-33 |
| 跨 crate 斜杠 | `syntect/regex-onig` = 替下游按开关 | :40-41 |
| 引擎二选一 | syntect 必须选 onig 或 fancy | :40-41 |
| 代码层裁剪 | `#[cfg(feature="x")]` 没开 = 物理不存在 | clap_app.rs:171 等 20+ 处 |
| 五张依赖表 = 作用域 | 主程序 / 测试 / build 脚本 / mac / unix+test | :43、:97、:109、:94、:106 |
| `[target]` vs feature | 一套按功能、一套按操作系统，并行生效 | :94、:106 |
| `default-features = false` | 依赖方"不按你默认清单"的写法 | 第 7、10 节 |
| `[profile.release]` | 只影响 --release 怎么编（慢换小快） | :130-133 |
| 反向找链条 | 库归谁管 = 哪个 feature 数组里有它 | 5.3 技巧 + `cargo tree -i` |

## 12. 毕业检验（全部不看文档答上 = 通过）

1. `bat = "0.26"` 作为依赖方写法，允许解析到 `0.26.9` 吗？`0.27.0` 呢？
2. `wild` 库默认 build 有、`--no-default-features` 没有——把从"谁拉它"到"链断在哪"的完整路径说出来。（wild ← minimal-application ← application ← default）
3. `gix` 块里 `default-features = false, features = ["sha1","blob-diff"]` 分别是什么意思？效果各是什么？
4. 为什么 `cargo test` 会编译出比 `cargo build` 多出来的库？哪些？
5. 你在 Windows 上 `cargo build --release`，`plist` 和 `nix` 会进产物吗？为什么？
6. 改 `edition = '2021'` 为 `'2024'`，最可能踩到什么？（edition 切换有语法/语义行为变化，如 `unsafe extern` 块要求——bat 未必全兼容）
7. **综合**：把 `default` 从 `["application","git"]` 改成 `["build-assets"]`，bat 会失去哪些**用户可观测**的行为？（命令行解析没了？分页没了？git 标记没了？`list-languages` 还在吗？顺着链条推）

## 13. 本篇在 .study 里的位置

- 读完 build 篇再来（`build-dependencies` 的作用靠它铺底）
- 读 t 系列遇到 `#[cfg(feature = "git")]` → 回第 5.5 节
- 读 t 系列问"这个 feature 没开我失去了什么" → 回第 5.3 链条图
- 想深挖依赖图解法细节 → 第 9 节工具箱
