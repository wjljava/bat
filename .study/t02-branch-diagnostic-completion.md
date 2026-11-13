# t02：分派表前 6 个分支的叶子（① diagnostic / ② completion / ⑥ 五个小 flag）

> t01 的续：站在 `run()` 分派表前，先把**不碰核心渲染**的边路分支全部钻到叶子。
> 钻完这 3 个分支后，返回分派表，下一站是分支③ cache（t03）。

## 你在执行树的哪个位置

```
main() → run()
   ├─【分支①】--diagnostic        ← 你在这里（钻到叶子）
   ├─【分支②】--completion        ← 你在这里（钻到叶子）
   ├─【分支③】cache 子命令        （下一站 t03）
   ├─【分支⑥】五个小 flag          ← 你在这里（钻到叶子）
   └─【主路⑦】run_controller      （t05）
```

## 分支①：`--diagnostic` —— 生成 bug 报告（main.rs:394）

```rust
if app.matches.get_flag("diagnostic") {
    #[cfg(feature = "bugreport")]
    invoke_bugreport(&app, cache_dir)?;          // feature 开：收集报告打印到 stdout
    #[cfg(not(feature = "bugreport"))]
    writeln!(io::stdout(), "bat has been built without the 'bugreport' feature...")?;
    return Ok(true);                              // 钻完 → 返回 run()，直接退出
}
```

钻到叶子 `invoke_bugreport`（main.rs:303）：

- 组装 `bugreport!` 宏的 `ReportEntry` 列表：软件版本、OS、命令行、**白名单**环境变量
  （`BAT_*` 全套 + `COLORTerm`/`TERM`/`PAGER`…，main.rs:340-363——注意只列这几十个，
  不 dump 全部 env，避免泄密）、系统配置文件内容、用户配置文件内容、自定义 assets
  metadata 文件内容、缓存目录条目。
- 自定义 Collector（main.rs:310）：探测当前终端深浅色（dark/light/未检出），
  写成一行 `Text` 进报告——报 bug 时"我在深色终端"是关键信息。
- feature 开 `paging` 时额外跑一次 `less --version`（main.rs:374-381，用
  `grep_cli::resolve_binary` 按 `PAGER`/`BAT_PAGER` 找到的 less 来问版本）。
- 最后 `writeln!(stdout, "{}", report.format::<Markdown>())`——markdown 格式，
  用户直接贴进 GitHub issue。

**Rust 机制 · feature 门控的第一课**：`#[cfg(feature = "bugreport")]` 不是"运行时判断"，
是**编译期**——不开这个 feature 编译 bat，这段代码**根本不存在**于二进制里。
所以"feature 关了会怎样"不是返回默认值，而是那行代码没有。
（全项目 `git`/`paging`/`lessopen` 同理，t05~t08 会反复遇到 `#[cfg(feature = ...)]` 包裹的块。）

**分支①结束，返回 run()，继续走分支②。**

## 分支②：`--completion <shell>` —— 打印补全脚本（main.rs:403）

```rust
#[cfg(feature = "application")]
if let Some(shell) = app.matches.get_one::<String>("completion") {
    let completion = match shell.as_str() {
        "bash" => completions::BASH_COMPLETION,
        "fish" => completions::FISH_COMPLETION,
        "ps1"  => completions::PS1_COMPLETION,
        "zsh"  => completions::ZSH_COMPLETION,
        _ => unreachable!("No completion for shell '{shell}' available."),
    };
    writeln!(io::stdout(), "{completion}")?;
    return Ok(true);
}
```

叶子在 `src/bin/bat/completions.rs:3`，只有 4 行却值得讲透：

```rust
pub const BASH_COMPLETION: &str = include_str!(env!("BAT_GENERATED_COMPLETION_BASH"));
```

两个宏串起来的**编译期管线**（对应 .study/build.md 的第 2~3 节）：

1. `build/application.rs:55-64`：cargo 编译**先**跑 build 脚本，把
   `assets/completions/*.in` 模板渲染到 `OUT_DIR`；
2. `build/application.rs:66-81`：build 脚本发 `cargo:rustc-env=BAT_GENERATED_COMPLETION_BASH=<路径>`，
   把文件路径告诉 rustc；
3. 主程序编译时 `env!("...")` 取出路径，`include_str!` 把渲染好的补全脚本**整段文本
   焊进二进制**；
4. 运行时零 IO——`writeln!` 直接打这个 `&'static str`。

**Rust 机制 · 常量与 `&'static str`**：`BASH_COMPLETION` 是 `const &str`，指向二进制
`.rodata` 段里的文本，寿命 = 程序全程（`'static`）。`match` 四个分支返回的都是
`&'static str`，类型统一。"编译期素材焊进二进制"这套 build 脚本 + `include_str!`
模式，tokei（400 种语言统计器）用得比 bat 更极端，见 architecture.md 对比表。

**分支②结束，返回 run()，继续走分支③（t03）。**

## 分支⑥：五个"一行输出"小 flag（main.rs:449-463）

```rust
else if app.matches.get_flag("config-file") {
    writeln!(io::stdout(), "{}", config_file().to_string_lossy())?;      // 配置文件路径
    Ok(true)
} else if app.matches.get_flag("generate-config-file") {
    generate_config_file()?;                                              // 生成默认配置文件
    Ok(true)
} else if app.matches.get_flag("config-dir") {
    writeln!(io::stdout(), "{}", config_dir.to_string_lossy())?;        // 配置目录
    Ok(true)
} else if app.matches.get_flag("cache-dir") {
    writeln!(io::stdout(), "{}", cache_dir.to_string_lossy())?;         // 缓存目录
    Ok(true)
} else if app.matches.get_flag("acknowledgements") {
    writeln!(io::stdout(), "{}", bat::assets::get_acknowledgements())?;  // 第三方致谢
    Ok(true)
}
```

五个里四个是"打印一个路径"，唯一的叶子是 `generate_config_file`
（`src/bin/bat/config.rs:32`）：把 bat 支持的全部配置项连同注释写成一份 `bat.toml`
文本，写到用户配置路径——"给我一份配置模板"。`to_string_lossy()` 把 `Path` 转
`String`（非 UTF-8 路径用 U+FFFD 替换，实际用户路径几乎不会触发）。

这五个分支的共同点：**全是纯查询/工具操作，不碰输入、不碰 Controller**。
它们和分支①②一样，`return Ok(true)` 直接结束——主路⑦根本不会执行。

**分支⑥结束，返回 run() 的 `_` 分支 if-else 串，继续走分支④（t04）。**

## 本区段 Rust 机制回顾

1. **feature 门控 = 编译期裁剪**（分支①②都有 `#[cfg]`）：关掉的 feature 对应代码
   不存在于二进制；库调用方（bat 的库身份，architecture.md 判断 1）不被 CLI 依赖拖大。
2. **`#[cfg(windows)]` 同款**：`App::new` 开头 Windows 上开 ANSI 支持（app.rs:70），
   非 Windows 编译时整行消失。平台差异用 `cfg`，功能差异用 feature，两种 `cfg` 一套语法。
3. **`const &str` + `include_str!`**：编译期数据嵌入的完整链路见 build.md，
   这里只立"运行时零 IO"这个认知。

## 检验问题

1. 关掉 `bugreport` feature 编译 bat，再敲 `bat --diagnostic` 会发生什么？代码层面"发生"在哪一步（编译期还是运行期）？
2. `BASH_COMPLETION` 指向的文本存在哪里（哪个二进制段）？它的生命周期是多少？
3. `--completion` 为什么整个 if 块被 `#[cfg(feature = "application")]` 包着？什么场景会关掉 application feature？

## 下一站

分支③ cache 子命令（--build / --clear / 无参特判的完整叶子）→ **t03**。
