# t03：分支③ cache 子命令（--build / --clear / 无参特判）

> 钻分派表的【分支③】到叶子：`bat cache` 的三种走向。
> 钻完返回 run() 分派表，下一站是分支④⑤（t04）。

## 你在执行树的哪个位置

```
main() → run()
   ├─【分支③】subcommand cache   ← 你在这里
   │    ├─ 有子参数 → run_cache_subcommand
   │    │    ├─ --build → build_assets → bat::assets::build   ★ 叶子
   │    │    └─ --clear → clear_assets                            ★ 叶子
   │    └─ 无子参数 → 把名为 "cache" 的文件当普通文件读（走主路⑦）
   └─ …
```

## 1. 分派入口的歧义消解（main.rs:415-434）

`bat cache ...` 会被 clap 解析成子命令。但有个**坑**：当前目录恰好有个叫
`cache` 的文件时，`bat cache` 到底是"运行 cache 子命令"还是"读这个文件"？
代码用 `args_present()`（main.rs:420）消歧：

```rust
Some(("cache", cache_matches)) => {
    if cache_matches.args_present() {
        run_cache_subcommand(cache_matches, config_dir, cache_dir)?;  // 带了 --build/--clear
        Ok(true)
    } else {
        let inputs = vec![Input::ordinary_file("cache")];            // 没带参数
        let config = app.config(&inputs)?;
        run_controller(inputs, &config, config_dir, cache_dir)       // ← 走主路⑦（t05）
    }
}
```

- 带参数 → 纯工具操作，`return` 结束，不碰 Controller。
- 不带参数 → **特判**：把它当成"要读一个叫 cache 的文件"，直接掉进主路⑦。
  这是整棵树里少见的"边路分支绕回主路"，记住这个回流点。

## 2. 叶子：`run_cache_subcommand`（main.rs:63）

```rust
fn run_cache_subcommand(matches, config_dir, default_cache_dir) -> Result<()> {
    let cache_dir = matches.get_one::<String>("target")
        .map(Path::new)
        .unwrap_or_else(|| default_cache_dir);   // 没给 --target 就用默认缓存目录

    if matches.get_flag("build") {
        #[cfg(feature = "build-assets")]
        build_assets(matches, config_dir, cache_dir)?;
        #[cfg(not(feature = "build-assets"))]
        writeln!(io::stdout(), "bat has been built without the 'build-assets' feature...")?;
    } else if matches.get_flag("clear") {
        clear_assets(cache_dir)?;
    }
    Ok(())
}
```

两个 flag 互斥走向：`--build` 重建、`--clear` 清除。`--target` 可选，重定向缓存目录。
feature `build-assets` 关着时，`--build` 只是打印一句"没编译进去"，不报错——
和 t02 分支①的 bugreport 同款模式（feature 关 → 友好提示，不是 panic）。

## 3. 叶子：`--build` 的调用链

`build_assets`（main.rs:48）→ `bat::assets::build`（定义 `src/assets/build_assets.rs:13`，
经 `src/assets.rs:24` re-export）：

```rust
bat::assets::build(
    source_dir,                    // --source 指定目录，没给就用 config_dir（main.rs:49-52）
    !matches.get_flag("blank"),    // 是否连内置资产一起打进缓存（--blank 时只打自定义）
    matches.get_flag("acknowledgements"),  // 是否顺带生成致谢文本
    cache_dir,
    clap::crate_version!(),        // 版本号：运行时用它校验缓存兼容性
)
```

钻到 `build()` 内部（build_assets.rs:13）依次四步：

1. `build_theme_set`（build_assets.rs:39）：把用户目录 + 内置的主题 `.tmTheme` 全读进来，
   序列化成一个二进制文件；
2. `build_syntax_set`：把所有语法定义（`.sublime-syntax`）编译成 syntect 能直接用的
   **预编译二进制**（`syntaxes.bin`）——运行时不再解析 YAML，秒开；
3. 可选的 acknowledgements：收集第三方语法/主题的归属文本；
4. `write_assets`：连同 `crate_version!()` 版本号写 `metadata.yaml`。

**为什么要编译期缓存**：syntect 语法定义是 YAML，运行时逐条解析几百个语法要好几秒。
`bat cache --build` 把这步一次性做完焊进二进制文件，平时 `assets_from_cache_or_binary`
（t05 讲）直接 `from_cache` 秒加载。代价是改了自定义语法要**手动重 build**——
所以 t05 会看到"缓存版本和二进制版本不匹配就**显式报错**"（不静默降级，architecture.md 判断 2）。

## 4. 叶子：`--clear`（`src/bin/bat/assets.rs:12`）

```rust
pub fn clear_assets(cache_dir: &Path) -> Result<()> {
    clear_asset(cache_dir.join("themes.bin"), "theme set cache")?;
    clear_asset(cache_dir.join("syntaxes.bin"), "syntax set cache")?;
    clear_asset(cache_dir.join("metadata.yaml"), "metadata file")?;
    Ok(())
}
```

逐个删三个缓存文件。`clear_asset` 的容错（assets.rs:52）：文件不存在
（`ErrorKind::NotFound`）就打印 "skipped" 而不是报错——`--clear` 在从没 build 过的机器上
也应该"成功"（幂等），否则"清一个本来就没有的东西"会莫名失败。

## 本区段 Rust 机制

1. **`Option::map + unwrap_or` 的默认值套路**（main.rs:68-71）：
   `get_one("target").map(Path::new).unwrap_or(default)` = "有就用、没有就用默认"，
   三行替代 if-let。全项目高频出现。
2. **feature 关时的"友好降级"模式**：`#[cfg(feature)]` 包住真实现，
   `#[cfg(not(feature))]` 包住一句提示。编译期裁剪 + 运行期不 panic 的组合。

## 检验问题

1. 当前目录有名为 `cache` 的文件，你敲 `bat cache`（不带参数），发生什么？这条路径和 `bat cache --build` 差在哪（会不会重建缓存）？
2. `bat cache --build` 为什么要在 `metadata.yaml` 里存 `crate_version!()`？t05 的 `assets_from_cache_or_binary` 靠它做什么检查？
3. `--clear` 在一个从没 build 过的机器上跑，退出码是 0 还是 1？代码哪一行保证了它？

## 返回与下一站

**分支③（含两个叶子 + 无参回流点）结束，返回 run() 分派表的 `_` 分支 if-else 串，
继续走分支④⑤。** → **t04**（list-languages / list-themes 的完整叶子，含"递归套自己"那处）。
