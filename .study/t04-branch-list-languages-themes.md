# t04：分支④⑤ list-languages / list-themes（含"递归套自己"那处）

> 钻分派表的【分支④⑤】到叶子。这两个分支有个共同点：都**借用主路⑦的 `Controller`
> 来干活**——执行树在这里"自己套了一层自己"。钻完返回 run()，下一站是主路⑦（t05）。

## 你在执行树的哪个位置

```
main() → run()
   ├─【分支④】--list-languages   ← 你在这里
   │    └─ get_languages → 生成文本 → 塞回 run_controller（主路⑦）
   ├─【分支⑤】--list-themes      ← 你在这里
   │    └─ list_themes → 每个主题 new 一个 Controller 渲预览（主路⑦ 的子集）
   └─【主路⑦】run_controller     （t05）
```

## 1. 分派入口（main.rs:439-448）

`_` 分支（非 cache）先备好 `inputs` 和 `config`，再按 flag 串：

```rust
if app.matches.get_flag("list-languages") {
    let languages = get_languages(&config, config_dir, cache_dir)?;
    let inputs: Vec<Input> = vec![Input::from_reader(Box::new(languages.as_bytes()))];
    let mut plain_config = config.clone();
    plain_config.style_components = StyleComponents::new(StyleComponent::Plain.components(false));
    run_controller(inputs, &plain_config, config_dir, cache_dir)   // ← 回流主路⑦
} else if app.matches.get_flag("list-themes") {
    list_themes(&config, config_dir, cache_dir, app.theme_options())?;
    Ok(true)
}
```

**关键设计**（t01 §3 提过，这里钻实）：`--list-languages` **不是**自己打印语言表，
而是把语言表当"一段文本"包成 `Input::from_reader`，clone 一份 config 改成 plain 样式，
**回流到主路⑦ `run_controller` 去打印**。好处：

- 不用维护第二套"打印到终端/分页/上色"逻辑（语言表自动获得分页、行号、颜色）；
- 行为和"读一个文件"完全一致，测试也复用同一套。

代价：`plain_config` 必须 `clone()` 整份 `Config`（t01 §4 Rust 机制 3 讲过：`Config`
派生 `Clone`，深拷贝便宜）。只改 `style_components` 一个字段，故用
`StyleComponents::new(StyleComponent::Plain.components(false))` 替换成纯文本。

## 2. 叶子：`get_languages`（main.rs:100）

钻到叶子，它干四件事：

1. **拿资产 + 列语法**（main.rs:103-109）：
   `assets_from_cache_or_binary` → `assets.get_syntaxes()` → 过滤掉
   `hidden`（内部辅助语法）和**没有扩展名**的，collect 成 `Vec`。
2. **剔"伪扩展名"**（main.rs:112-128，issue #1076）：某些语法的"扩展名"其实是
   完整文件名（`CMakeLists.txt`、`Cargo.lock`）或隐藏文件（`.vimrc`）。
   对每个扩展名造 `Path::new("test").with_extension(它)`，若 `extension().is_some()`
   说明它**自己就带点**（不是真扩展名），保留；否则检查它会不会被映射到别的语法。
3. **排序 + 补自定义映射**（main.rs:130-139）：按名称大写序排，再把
   `--map-syntax`（`-m`）里"映射到某语法"的额外路径拼回该语法的扩展名列表。
4. **输出两种格式**（main.rs:141-193）：
   - `loop_through`（管道/批处理）→ `名字:ext1,ext2` 紧凑行；
   - 交互 → 按最长语言名对齐、扩展名超宽就**折行**（用 `saturating_sub` 防小终端宽度下
     `usize` 下溢 panic，main.rs:159-162 注释专门解释了这个坑）。

返回一个 `String`（整张表），交给分派入口包成 `Input`。

## 3. 叶子：`list_themes`（main.rs:202）★ 递归点

这是整棵树**最像"套娃"**的地方。钻到叶子：

```rust
pub fn list_themes(cfg, config_dir, cache_dir, theme_options) -> Result<()> {
    let assets = assets_from_cache_or_binary(cfg.use_custom_assets, ...)?;
    let mut config = cfg.clone();
    config.language = Some("Rust");                        // 预览文件是 Rust 代码
    config.style_components = StyleComponents(style);      // Plain

    let default_theme_name = theme(theme_options).to_string();   // 当前默认主题
    let mut handle = OutputHandle::FmtWrite(&mut buf);    // 先写进内存 buf

    for theme in assets.themes() {
        // 标注 (default)/(default dark)/(default light)（main.rs:220-228）
        if config.colored_output {
            handle.write_fmt(... 加粗的主题名 ...)?;
            config.theme = theme.to_string();
            Controller::new(&config, &assets)               // ★ 每个主题 new 一个 Controller
                .run(vec![theme_preview_file()], Some(&mut handle))
                .ok();                                     // 渲染一段 Rust 预览到 buf
        }
    }
    // 最后把整个 buf 经 OutputType::from_mode 分页写出（main.rs:257-260）
}
```

逐点钻：

- **`theme_preview_file()`**（main.rs:198）= `Input::from_reader`，读的是
  `THEME_PREVIEW_DATA`（main.rs:45 `include_bytes!("../../../assets/theme_preview.rs")`）——
  一段**编译期焊进二进制**的 Rust 示例代码。每个主题就用它做预览。
- **循环内 `Controller::new(&config, &assets).run(...)`**：注意 `output_handle` 传的是
  `Some(&mut handle)`（不是 `None`）——意思是"**别自己决定输出目标，直接写进我给的 buf**"。
  这是 t05 要讲的 `OutputType` 的关键分支：`run_with_error_handler` 收到 `Some(handle)` 时
  **跳过**自己建 `OutputType`，直接复用调用者给的。
- **`config.theme = theme.to_string()` 写在循环体里**：每个主题换一次 config 的 theme
  字段，再 `Controller::new`——因为 Controller 读的是 `config.theme` 选主题。
  `config` 是 `mut` 的 clone，所以可改。
- **`.ok()` 吞错**（main.rs:238）：某个主题渲染失败不该让整个 list 崩掉，
  `ok()` 把 `Result` 变 `Option` 丢弃——"尽力渲染"语义。
- 最后（main.rs:257-260）整张预览表 `buf` 才经 `OutputType::from_mode` 决定
  stdout 还是分页器**一次性写出**。

**递归点本质**：`list_themes` 没有自己实现渲染，它对**每个**主题都走了一遍
`Controller::run`（= 主路⑦ 的核心，t05 要钻的）——只是把"输入"换成同一段
预览 Rust 代码、把"主题"逐个换。所以 t05 读懂主路后，这里会瞬间全通。

## 本区段 Rust 机制

1. **`Option::ok()` / `ok_or` 的错误"降级"**：`list_themes` 用 `.ok()` 把渲染错误吞掉
   （尽力而为），而 `get_languages` 用 `?` 上抛（必须成功）。**同一个函数体里
   "哪些错该炸、哪些该忍"是设计判断**，不是机械写 `?`。
2. **clone 改单字段再传**：`config.clone()` 后只改 `theme`/`style_components`，
   其余 26 字段不动。`Clone` 派生保证这是值拷贝（不是浅引用），安全。

## 检验问题

1. `--list-languages` 为什么 `clone` 一份 config 改成 plain 就回流 `run_controller`，而不直接 `println!` 语言表？这样做的代价是什么（要 `Clone` 整份 Config）？
2. `list_themes` 循环里为什么每次 `Controller::new` 前要先 `config.theme = ...`？`Controller` 读哪个字段来决定主题？
3. `list_themes` 传 `Some(&mut handle)` 给 `run`，和主路⑦传 `None`，区别是什么（谁决定输出目标）？

## 返回与下一站

**分支④⑤结束，返回 run() 的 `_` 分支。六个边路分支全部钻完，现在走【主路⑦】
`run_controller`** —— 真正"读文件→高亮→渲染→输出"的主线。→ **t05**。
