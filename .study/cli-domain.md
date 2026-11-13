# CLI 业务知识地图：一条高亮命令背后"世界怎么运转"

> 定位：t01~t09 讲"代码怎么跑"，**这篇讲"跑的时候依赖的那些业务知识"**——Rust 官方教程不教、
> 但不懂就"知其然不知其所以然"的那一层。比如"为什么 bat 能按不同语言高亮"、
> "为什么要有退出码"、"为什么颜色是终端画的不是 bat 画的"。
> 每块都是 **领域知识（脱离代码讲清楚）→ 锚定 bat（落到 文件:行号）**。
> 阅读时机：任意时刻，配合 t 系列回查。读到 t 里某段"卡在这块知识上"就翻对应小节。

## 全图（先有地图）

一个 CLI 高亮工具（bat）背后，跨了 **7 片业务知识**：

```
① CLI 契约      退出码 / stdout·stderr / 可组合性        → 决定"bat 怎么和别的工具搭配"
② 配置业务      三级合并 / 12-factor / 为什么环境变量夹中间 → 决定"默认行为从哪来"
③ 语法高亮原理  语法定义=正则状态机 / scope / 检测顺序      → 决定"为什么能按语言上色"★你点名的
④ 主题与终端色  主题=scope→颜色 / ANSI 转义 / 8~24bit 色    → 决定"颜色实际由谁画"
⑤ 编译期资产    语法预编译成 .bin / 用户缓存 / 版本匹配     → 决定"重活挪到哪做"★你点名的
⑥ 终端与分页    TTY / 交互式 / less / 管道·重定向           → 决定"输出送到哪"
⑦ 字节与编码    文件=字节 / 文本=编码 / 二进制判定 / 全角宽 → 决定"读进来的是啥"
```

下面逐个钻。带 ★ 的是你明确问的（"为什么能按语言高亮"=③、"前置编译资产"=⑤）。

---

## ① CLI 契约：为什么 bat 长这样（退出码 / stdout·stderr / 可组合性）

**领域知识（脱离 bat）：**

命令行工具活在 shell 里，shell 靠三样东西跟它"说话"，这是几十年 Unix 惯例：

1. **退出码**（0~255）：程序"算成功还是失败"的信号。约定俗成：
   - `0` = 全好；`1` = 一般失败；`2`（及部分工具自定义）= 用法错误（参数不对）。
   - shell 里 `if cmd; then … fi` 判的就是退出码。
2. **stdout 和 stderr 分开**：stdout（文件描述符 1）走"正常结果"，stderr（描述符 2）走"错误/提示"。
   **管道 `|` 只接 stdout**。所以设计铁律：**报错别打 stdout**，否则 `bat x.rs | grep foo`
   会把错误信息也 grep 进去。
3. **可组合性（"像 cat 一样"）**：bat 的官方定位是 **"A cat(1) clone with wings"**
   （Cargo.toml:4 description）。`cat` 的精髓是"把内容原样吐到 stdout，能被管道串起来"。
   bat 的目标就是**保持这个契约**——`echo hi | bat`、`bat foo.rs | less`、`bat *.rs > out.txt`
   都得能像 cat 一样工作，只是"多了翅膀"（高亮、行号、分页）。

**锚定 bat：**
- 退出码三态在 t01 §1（`main.rs:474`）：`Ok(true)=0`、`Ok(false)=1`、`Err=1`。
  注意 bat 没用到"2=用法错误"，参数错误 clap 直接自己退出。
- stdout/stderr 分流在 t09：正常内容经 `OutputHandle` 走 stdout；
  `default_error_handler`（error.rs:67）错误打 stderr。
- 可组合性的直接体现：`loop_through`（t01 §2e）——管道场景切纯文本模式，
  就是为了让 `bat | 其他` 不被 ANSI 码/装饰干扰。

> **一句话**：bat 的所有"奇怪设计"（纯文本退化、错误走 stderr、分页可选）本质都是在**守 cat 的契约**。

---

## ② 配置业务：三级合并背后的"为什么"

**领域知识（脱离 bat）：**

正经 CLI 的配置几乎都遵循一条**优先级链**，从强到弱：

```
命令行参数 > 环境变量 > 配置文件 > 硬编码默认值
（越靠近"当前这次调用"的，优先级越高）
```

为什么这样设计（**12-factor app 配置方法论**的影子）：
- **命令行最高**：用户在当前命令里明确敲的，意愿最强，必须盖住一切。
- **环境变量其次**：CI/CD、容器、`export BAT_THEME=...` 这类"部署期/会话期"的设置，
  比用户机器的静态配置文件更"现场"。这就是**环境变量夹在中间**的原因。
- **配置文件再低**：`~/.config/bat/config` 是你一次性设好的默认习惯。
- **默认值垫底**：啥都没设时的行为。

**反直觉的一点（bat 特有）**：bat 的"配置文件"**不是 TOML/YAML**，而是
**"一串 shell 命令行的词法"**——文件里写 `theme = "Monokai"` 等价于加了 `--theme=Monokai`。
bat 用 `shell_words` 库按 shell 规则切（处理引号），所以带空格的值要加引号。
这是"配置文件本质是'虚拟命令行参数'"的设计（t01 §2c 讲过机制，这里补"为什么"）。

**锚定 bat：**
- 三级合并的实现（t01 §2c，app.rs:199）：把三层**拍平成一条参数列表**再给 clap，
  靠 clap 的"后出现的覆盖先出现的"实现优先级，**没有任何"优先级表"数据结构**。
- 环境变量映射（config.rs:155 `get_args_from_env_vars`）：8 个 `BAT_*` → `--flag=value`。
- 配置文件读取（config.rs `get_args_from_config_file`）：系统级 `/etc/bat/config` +
  用户级 `~/.config/bat/config` 两级，后者盖前者。

> **一句话**：配置"盖"的本质是**参数拼接顺序**，不是运行时查优先级表。

---

## ③ ★ 语法高亮原理：为什么能"按不同语言上色"（你点名的核心）

**领域知识（脱离 bat）：**

这是整片知识里最该吃透的。"高亮"不是魔法，链条是**三样东西 + 一个引擎**：

### 3.1 语法定义 = "正则规则 + 状态机"

bat 用 **Sublime Text 的语法定义**（`.sublime-syntax`，YAML 格式，bat 内置 226 个，
`assets/syntaxes/`，git 子模块）。打开一个看结构（如 `assets/syntaxes/01_Packages/JSON/JSON.sublime-syntax`）：

```yaml
name: JSON
file_extensions: [json, ...]   # ★ 关键：这语言对应哪些扩展名
scope: source.json             # ★ 关键：这语言的整体 scope（主题据此上色）
contexts:                     # ★ 状态机：一堆"上下文"，每个是一组正则规则
  main:
    - include: value
  value:
    - include: string
    - include: number
  ...
```

- **`contexts` = 状态机的"状态"**，每个状态是一组正则。匹配到 `"` 就切到
  "字符串"状态，字符串里遇到转义/结尾才切回来——**这就是"有状态"**（跨行字符串靠它）。
- **`include`** = 状态间跳转（`main` 包含 `value`，`value` 包含 `string`/`number`…）。
- 每条规则匹配到的文本，会被打上一个 **scope**。

### 3.2 scope：上色的"锚点"

scope 是**层级命名**，如 `source.json`、`string.quoted`、`constant.numeric`、
`comment.line`。高亮引擎跑完，一行文本被切成若干段，每段带一个（或几个叠加的）scope。
**主题**（④）就是"scope → 颜色"的映射表。所以：

```
（语法定义 + 主题 + 输入文本） → syntect 引擎 → 一行切成 [ (scope=string, "hello"),
                                                        (scope=number, 42), ... ] → 按主题上色
```

### 3.3 检测顺序：怎么知道"这是哪种语言"（t07 §4 的四级）

**领域知识**：语言检测本质是"从线索推断"，优先级（从高到低）：
1. **用户显式 `-l Rust`**：最确定，直接信。
2. **文件扩展名/路径**：`.rs`→Rust。靠"扩展名→语法"映射表（build 篇生成的静态表，
   226 个语法的 `file_extensions` 汇总而成）。`-m '*.build:Python'` 是覆盖这张表。
3. **首行嗅探（shebang）**：`#!/usr/bin/env python` → Python。没扩展名的脚本靠这个。
4. **`--fallback-syntax` 兜底**：全猜不到就用它。

**为什么"扩展名优先于内容"**：`foo.sh` 就算首行没 shebang 也该当 Shell——扩展名是
**更强的信号**（你"说"它是 sh）。stdin（无路径）才退到首行嗅探 + 兜底。

**锚定 bat：**
- 语法定义文件：`assets/syntaxes/**/*.sublime-syntax`（226 个，YAML）。
- 检测四级：t07 §4（`assets.get_syntax`，assets.rs:216）。
- 有状态高亮器：t07 §5（`HighlightLines`，syntect 的状态机实例，跨行复用）。
- 为什么"每文件一个"：状态机的上下文（上一行结尾在哪个 scope）必须跨行保留，
  否则跨行字符串/注释高亮就错（architecture.md 判断 5）。

> **一句话**：**语法定义（正则状态机，产出 scope）+ 主题（scope→颜色）+ 检测（选哪个语法）**
> 三者 + syntect 引擎，就是"按语言上色"的全部。

---

## ④ 主题与终端色：颜色其实是"终端画的"，不是 bat 画的

**领域知识（脱离 bat）：**

新手最大的误解是"bat 在终端上画颜色"。**实际是：bat 只往 stdout 写"带颜色码的文本"，
真正上色的是终端模拟器**（iTerm2/Windows Terminal/vim 里的终端）。中间靠**ANSI 转义序列**：

- ANSI 转义 = 一类控制字符，如 `\x1b[31m`（红色）、`\x1b[0m`（重置）。
  bat 把"这段文字该红"翻译成 `\x1b[31m这段文字\x1b[0m` 写出去，终端见到就渲染。
- **终端颜色能力分档**：`8 色 → 16 色（含粗体）→ 256 色 → 24-bit（1670 万色，叫 truecolor）`。
  同一个"主题要的颜色"，在只支持 16 色的终端上会**降级**到最近的 16 色调色板色。
- **主题文件**：bat 的主题是 `.tmTheme`（plist/XML，在 `assets/themes/*/`），
  本质就是**一张 scope→颜色 表**：`<key>string</key> → 某 RGB 值`。
  这就是 ③ 里"主题 = scope→颜色映射"的文件形态。
- **深浅色自动**：`--theme auto` 时 bat 探测终端背景是深是浅（`COLORTERM`/查询序列），
  挑对应的主题。

**锚定 bat：**
- "翻译 scope/RGB → ANSI 码"在 t08 §⑤（`as_terminal_escaped`，printer.rs 底 + terminal.rs）：
  这是 syntect 的样式 → 终端字符串的**最后一跳**。
- `true_color`（t01 §2e 派生字段，读 `COLORTERM`）决定"24bit 原色 or 降 16 色"。
- `colored_output` 为 false（管道/`--color=never`）时**干脆不包任何颜色码**——
  因为管道下游不是终端，ANSI 码会变乱码。

> **一句话**：bat 发"颜色指令"（ANSI 码），**终端执行上色**；主题文件就是"该发什么颜色"的映射表。

---

## ⑤ ★ 编译期资产：为什么把 226 个语法"预编译"进二进制（你点名的）

**领域知识（脱离 bat）：**

核心权衡：**"把重活挪到编译期，换取运行期秒开"**。展开：

- 语法定义是 **YAML**（226 个文件）。syntect 引擎**每次运行时现解析**几百个 YAML 要好几秒
  ——bat 启动就卡住，不可接受。
- 所以 bat 在 **build 期**把语法定义**预处理成二进制**（syntect 内部格式），
  运行期直接反序列化，秒开。同理主题也预处理。
- 这些预处理产物被 `include_bytes!` **焊进可执行文件**（build 篇讲过 `include_*!` 是编译期嵌入）。
  结果：`bat` 单个 exe 就能跑，不依赖外部素材文件。
- **用户自定义怎么办**（不想重编 bat）：`bat cache --build` 把你 `~/.config/bat` 下自定义的
  语法/主题也预处理，存进**用户缓存**（`~/.cache/bat/*.bin`）。运行时"缓存优先、内置兜底"。
- **版本匹配**：缓存是某版 bat 生成的，和当前 bat 版本不符会**显式报错**（不静默降级），
  让你 rebuild/clear。

**锚定 bat：**
- 内置资产三个 `.bin` 文件（**check-in 进仓库**）：`assets/syntaxes.bin`、`themes.bin`、
  `acknowledgements.bin`。编译期经 `src/assets.rs:378/382/386` 的 `include_bytes!` 焊进二进制，
  内容可能 **zlib 压缩**（assets.rs:402-403 解压）。
- "缓存优先、内置兜底"：t05 §2（`assets_from_cache_or_binary`，bin/bat/assets.rs:19）。
- 版本不匹配报错：assets.rs:29-41（t05 讲过）。
- build 脚本怎么生成：build 篇分支A/B。

> **一句话**：YAML 语法定义 → build 期预处理成二进制 `.bin` → `include_bytes!` 焊进 exe →
> 运行期秒开；用户自定义走 `cache --build` 生成缓存，**版本必须匹配**。

---

## ⑥ 终端与分页：输出到底"送到哪"（TTY / 交互式 / less）

**领域知识（脱离 bat）：**

程序"输出"不总到屏幕，取决于 stdout 接到什么（**三种去向**）：

| stdout 接到 | 术语 | bat 该怎么做 |
|---|---|---|
| 交互终端（你盯着看） | **TTY** | 高亮 + 分页 + 行号（"翅膀"全开） |
| 管道 `| 其他` | 管道 | 纯文本透传（别发 ANSI 码，下游会乱） |
| 重定向 `> file` | 文件 | 可保留颜色（文件里存 ANSI 码，回看时终端再渲染） |

`is_terminal()`（t01 §2a）就是在判"stdout 是不是 TTY"。

**分页（pager）为什么存在**：内容超过一屏时，终端会**滚出去看不到上面**。
pager（`less`）起个子进程把内容接管，可上下翻、搜索。bat 默认交互时**自动**起 less，
且加 `-F`（内容不到一屏就直接退，不卡住你按 q）——这就是 `echo short | bat` 不卡的原因。

**less 的参数为什么这么配**（t05 §5c 详解，这里给"为什么"）：
- `-R`：让 less 认 ANSI 颜色（否则颜色码显示成乱码）。
- `-F`：一屏就退（`--quit-if-one-screen`）。
- `-S`：长行截断不折（`--chop-long-lines`，配 `--wrap=never` 时）。
- `-K`：Ctrl-C 时 less 跟 bat 一起退（不然 bat 退了 less 还挂着）。

**管道契约的边角**：`bat | head` 时 head 读够就关，bat 再写收到 **BrokenPipe**——
这不是 bat 的错，所以 `exit(0)`（t09 / error.rs:71）。

**锚定 bat：**
- 输出去向三选一：t05 §5（`OutputType`：Stdout / Pager(less 子进程) / BuiltinPager(minus 线程)）。
- 分页模式 `PagingMode`（Never/Always/QuitIfOneScreen）在 t01 §3a 由 config 算出。
- 内嵌 minus 线程（`--pager=builtin`，不起子进程）：t05 §5b。

> **一句话**：bat 先判"输出到哪"（TTY/管道/文件），再决定"高亮全开 or 纯文本"；
> 交互且超一屏就起 less 分页，less 参数 `-R -F -S -K` 各司其职。

---

## ⑦ 字节与编码：读进来的"到底是什么"

**领域知识（脱离 bat）：**

- **文件在磁盘上是字节**，没有"文本/二进制"之分——"文本"是**一种编码约定**。
  同一串字节，当 UTF-8 解是一个样、当 UTF-16 解是另一个样。
- **常见编码**：UTF-8（网页/代码主流）、UTF-16（Windows 老文件）、ASCII（纯英文子集）。
  **BOM**（Byte Order Mark，文件头几个特殊字节）是"这个文件是什么编码"的自报家门。
- **二进制 vs 文本**：含 `NUL` 字节（0x00）的通常判二进制；或 ZIP/PE 等**魔数**
  （文件头固定字节，如 ZIP 是 `PK\x03\x04`）判二进制。bat 遇到二进制默认**只打警告不打印**
  （`--binary` 可强转文本）。
- **UTF-16 为什么特殊**：UTF-16 每个字符 2 字节，连换行符 `\n` 都是 **2 字节**
  （`\x00\x0A` 或 `\x0A\x00`），普通按 `\n`（1 字节）切行会切错，要专门处理（t06 `read_utf16_line`）。
- **全角/半角宽度**：终端是**等宽字符网格**，一个英文字母 1 格，一个汉字 **2 格**。
  算行宽/换行必须按"显示宽度"不是"字符数"（t08 用 `unicode-width` + `unicode-segmentation`）。

**锚定 bat：**
- 编码嗅探：t06 §3（`InputReader::try_new`，input.rs:267；`inspect_content_type` 判
  UTF-8/16/二进制，ZIP 魔数特判 input.rs:349）。
- 逐行读按编码分路：t06 §5（UTF-16 换行 2 字节）。
- 全角宽度算换行：t08 §⑤路B（`grapheme_width`，按 grapheme 不是按 char）。

> **一句话**：bat 读进来的是字节，先**嗅探编码**（UTF-8/16/二进制，看 BOM/NUL/魔数），
> 再按编码切行（UTF-16 换行是 2 字节）、按**显示宽度**换行（汉字占 2 格）。

---

## 把 7 片拼起来：一条 `bat -n --theme Monokai main.rs` 用上了哪些业务知识

```
① CLI 契约   → 结果走 stdout、错误走 stderr、退出码 0（全好）
② 配置       → 没在命令行给 --theme，就从 配置文件/环境变量 拿；都没就给默认
③ 语法高亮   → main.rs 扩展名 .rs → 查"扩展名→语法"表 → Rust 语法（正则状态机）
               → 高亮器跨行记状态 → 每行切成带 scope 的片段
④ 主题与色   → Monokai 主题（scope→颜色）→ syntect 产出样式 → 翻译成 ANSI 码
               → truecolor 终端原色 / 16 色终端降级 → 终端真正上色
⑤ 编译期资产 → Rust 语法是 build 期预处理成 .bin 焊进 exe 的（秒开）；
               若你 cache 过自定义语法则用缓存（版本须匹配）
⑥ 终端分页   → stdout 是 TTY（交互式）→ 高亮全开；若超一屏 → 起 less(-R -F -S -K)
⑦ 字节编码   → main.rs 是普通 UTF-8 文本 → 按 \n 切行、按显示宽度换行
```

**这套知识是"可迁移"的**：换个 CLI 工具（ripgrep/fd/gh）也适用——
①②⑥⑦ 是所有 Unix CLI 的通用契约，③④⑤ 是"语法高亮类工具"（bat/atom/synvert）专属。
读懂它，你看任何终端工具的"怪行为"都能解释。

---

## 检验（脱离代码答）

1. 为什么 `bat x.rs | grep foo` 时，bat 的**报错信息**不会出现在 grep 结果里？（①）
2. 配置文件里写 `theme = "Sublime Snazzy"`（带空格），为什么 bat 要能正确读？
   bat 的配置文件是 TOML 吗？（②）
3. "语法定义"本质是什么（正则还是状态机）？一个跨 50 行的多行字符串为什么能整块正确上色？（③）
4. bat 写的是"颜色"吗？如果 bat 往**纯文本文件**（非终端）写，还能带 ANSI 码吗？会怎样？（④）
5. 为什么 bat 不在运行时现解析那 226 个语法定义文件？用户加了个新语法又不想重编 bat，怎么办？（⑤）
6. `bat *.rs` 输出接到管道时为什么变纯文本、接到终端就带行号？判据是哪个调用？（⑥）
7. 一个 Java 文件含中文字符串，为什么它的高亮换行不会把汉字从中间切两半？（⑦）
