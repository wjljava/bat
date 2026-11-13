# bat 完整学习路线（DFS 执行树版）

> 目标：把 bat 学通透。组织方式 = **深度优先执行树**：从 `main()` 出发，
> 每到一个分支就钻到叶子讲清楚，钻完**返回父分支**继续，像递归一样。
> 每篇开头有"你在执行树的哪个位置"定位图，结尾有"返回/下一站"指针。
> 遇到高级 Rust 特性（生命周期、trait 对象、feature 门控、有状态对象）就地讲透。

## 执行树总览（先记这张图，再逐篇钻）

```
main()  [bin/bat/main.rs:471]
└─ run()  [main.rs:389]
   │  前置：App::new()（解析+三级配置合并）+ config_dir/cache_dir
   ├─【分支①】--diagnostic → invoke_bugreport → 返回
   ├─【分支②】--completion → 打印编译期嵌入的补全脚本 → 返回
   ├─【分支③】subcommand cache
   │    ├─ 有子参数 → run_cache_subcommand（--build 重建 / --clear 清除）→ 返回
   │    └─ 无子参数 → 把名为 "cache" 的文件当普通文件读（回流主路⑦）
   ├─【分支④】--list-languages → get_languages → 塞回主路⑦打印 → 返回
   ├─【分支⑤】--list-themes → list_themes（每主题 new 一个 Controller 渲预览）→ 返回
   ├─【分支⑥】--config-file/--generate-config-file/--config-dir/--cache-dir/--acknowledgements → 各一行输出 → 返回
   └─【主路⑦】run_controller [main.rs:289]  ★ 最常见
      ├─ assets_from_cache_or_binary（缓存 / 内置二进制 二选一）
      ├─ Controller::new(config, &assets)   ← 'a 绑住两者
      └─ Controller::run → run_with_error_handler [controller.rs:47]
         ├─【子分支】OutputType::from_mode 三选一（stdout / less 子进程 / 内置 minus）
         │    + 叶子特判：无文件退 Never、防 IO 圈、找不到 less 退 stdout
         ├─ for input: print_input [controller.rs:141]
         │    ├─ Input::open → 编码嗅探全分支（UTF-8/16/二进制，t06）
         │    ├─ git diff 获取（feature 门控，t06）
         │    ├─【分支】Printer 二选一（t07）
         │    │    └─ InteractivePrinter::new：theme/syntax 四级优先级/装饰器/有状态高亮器
         │    └─ print_file → print_file_ranges → 逐行 print_line（t08）
         │         └─ 六步全叶子（解码/高亮/挤空行/装饰/NoWrapping vs 换行两路/上色）
         └─ 循环结束 → Ok(no_errors)
   返回 main() → 定退出码（t09：Ok(true)=0 / Ok(false)=1 / Err=1+stderr）
```

## 篇索引（按 DFS 顺序）

| 篇 | 覆盖执行树区段 | 核心文件 | 状态 |
|---|---------------|---------|------|
| t01 | main() + run() 分派表总览（树的根，7 个分支摆出来） | `bin/bat/{main,app}.rs` | [x] |
| t02 | 分支①②⑥ 叶子（diagnostic / completion / 五个小 flag） | `main.rs:303,403`、`completions.rs` | [x] |
| t03 | 分支③ cache（--build 调用链 / --clear / 无参特判） | `main.rs:48,63`、`bin/bat/assets.rs` | [x] |
| t04 | 分支④⑤（get_languages / list_themes，含"递归套自己"那处） | `main.rs:100,202` | [x] |
| t05 | 主路⑦前半：run_controller → Controller → OutputType 三选一全叶子 | `controller.rs`、`output.rs`、`pager.rs`、`less.rs` | [x] |
| t06 | print_input → Input::open 三分支 + 编码嗅探 + git diff | `controller.rs:141`、`input.rs` | [x] |
| t07 | Printer 二选一 + InteractivePrinter::new 全叶子（theme/syntax/装饰/高亮器） | `printer.rs:227`、`assets.rs:216` | [x] |
| t08 | print_file → print_file_ranges → print_line 六步全叶子 | `controller.rs:214`、`printer.rs:656` | [x] |
| t09 | 收尾：退出码 + default_error_handler + 一条命令的一生复盘 | `main.rs:471`、`error.rs:67` | [x] |

## 编译期支线（非编号，读完 t09 后再看）

| 支线 | 主题 | 核心文件 | 状态 |
|---|------|---------|------|
| build | **按 DFS 执行树**讲 build 脚本：从 `cargo build` 到 build/main.rs 的两个分支（映射表总是跑 / 补全 man 受 application feature 门控），钻到"cargo:rustc-env 传路径 + include_str!/include! 焊进二进制"的叶子 | `build/{main,application,util,syntax_mapping}.rs`、`Cargo.toml` 的 `[build-dependencies]` | [x] |

## 配套（不进编号序列）

- **`architecture.md`**：7 个设计判断的"为什么"（`#[cfg]`/`Box<dyn>`/缓存双来源/有状态
  高亮器的考量）+ 仓库布局 + 跨项目对比。**读 t 系列卡"为什么"时回查**。
- **`cargo-toml.md`（效果视角）**：每个配置项都落到"这么配 → 编译出的二进制/行为发生什么变化"
  （feature 开/关哪些依赖进 `cargo tree`、哪些 `#[cfg]` 代码块存亡）。t 系列里遇到 `#[cfg(feature)]`
  时回这份看"这个 feature 没开会失去什么"。
- **`build.md`（DFS 支线）**：build 脚本执行树 + 嵌入机制——**读完 t09 再看**。
- **`cli-domain.md`（业务知识地图）**：t 系列讲"代码怎么跑"，这篇讲"跑时依赖的领域知识"——
  CLI 契约（退出码/stderr/可组合性）、三级配置、**语法高亮原理（为什么能按语言上色）**、
  主题与终端色（ANSI/truecolor）、**编译期资产（为什么把语法预编译进二进制）**、
  终端与分页（TTY/less）、字节与编码（UTF-8/16/二进制/全角宽）。
  每块"脱离代码讲清世界 → 锚定 bat 的 文件:行号"。t 里卡在某块知识上就翻对应小节。
- **`cli-yourself.md`（动手桥接篇）**：前面全在"读懂 bat"，这篇补最后一环——**把读懂的
  变成"能写自己的 CLI"**。给一个能 `cargo run` 的 minicat 骨架、4 层升级（抄 bat 哪个模式）、
  "该抄/别抄"判断表、3 个递进练习。**学完 t09 + 读完 cli-domain 后动手做**，做完再回头看
  bat 会通很多。
- **`minicat/`（可运行项目）**：`cli-yourself.md` 那个骨架的真身，零依赖、已实测通过
  （正常=0 / 缺文件=1 且提示在 stderr / 管道可读）。`cd .study/minicat && cargo run -- a.txt`。
  动手从这里开始，每段 `main.rs` 都注释了对应 bat 哪篇。

> **全套文档的角色分工**：`t01~t09`＝读懂 bat 的执行流（主干）；`cli-domain`＝读懂背后的
> 领域知识；`architecture`＝读懂"为什么这么设计"；`cargo-toml`/`build`＝读懂编译期配置与资产；
> **`cli-yourself`＝把读懂的变成动手写**（最后一块拼图）。

## 学习节奏约定（DFS 版）

1. **按 t01→t09 顺序读**。每篇开头"你在树的哪个位置"先扫一遍定位，再往下钻。
2. 每篇遇到子分支，**先钻到底再返回**；篇末"返回/下一站"指针会把你接回父分支或推进。
3. 每篇有"本区段 Rust 机制"小节（生命周期/trait 对象/feature 门控/有状态对象…），
   这是给"刚学完官方教程"的你补的——**卡住就先读这里，再回去读代码**。
4. 每篇末尾 3-5 道检验问题，**能不看文档答上来才算过**；答不出的回对应区段重读。
5. 卡壳随时回来问（贴代码行号），全部以 t 系列为准（旧 ch1~ch8 已删）。
