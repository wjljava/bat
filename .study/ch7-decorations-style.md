# 第 7 章：装饰与样式（`Decoration` / `Colors` / `StyleComponent`）

这一章的目标：回头补一个被反复引用、一直没展开的细节——
**行左侧那些装饰（行号 / git 改动标记 / 网格竖线）到底怎么画**。
ch5 的 `print_line` 里 `decorations.iter().map(|d| d.generate(...))` 那一行
背后真正的"算宽度、出文本"逻辑都在这一章。读完你应该能回答：
"bat 每行左边那个 `42`（行号）+ `+`（git 标记）+ `│`（网格竖线）是怎么拼出来、
宽度怎么算的？"

## 涉及文件

- `src/decorations.rs` — `Decoration` trait + 三个实现（行号 / git / 网格竖线）
- `src/style.rs` — `StyleComponent` / `StyleComponents` / `StyleComponentList`
- `src/printer.rs` — `Colors`（装饰区用色）
- `src/diff.rs` — `LineChanges`（git 标记的数据来源，`gix` 算出来的）

## 全景图（先有这个概念，再看代码）

```
ch5 print_line 里：
  if self.panel_width > 0 {
      let decorations = self.decorations.iter()
          .map(|d| d.generate(line_number, false, self));
      for deco in decorations {
          write!(handle, "{} ", deco.text)?;     // "  42 + │ " 这类前缀
          cursor_max -= deco.width + 1;           // 内容区变窄
      }
  }
   │
   ▼  这些 decorations 从哪来？ch4 new() 里按 style_components 攒的：
   │
InteractivePrinter::new()（ch4 阶段 1）
   ├─ style_components.numbers()   → LineNumberDecoration（行号，宽 4）
   ├─ style_components.changes()   → LineChangesDecoration（git 标记，宽 1）
   └─ style_components.grid()      → GridBorderDecoration（竖线，宽 1）
   │
   ▼  每个装饰 generate(line_number, continuation, printer) 返回 DecorationText { text, width }
   │
   ▼  Colors（ch3 从主题抽色）：行号/gutter 灰、git 红绿黄、网格竖线色
```

ch5 的 `print_line` 里有一行 `decorations.iter().map(|d| d.generate(...))`——
那一行背后真正的"算宽度、出文本"逻辑都在这一章。

"装饰"是**行左侧的留白区**（panel）。bat 的输出每行其实是两段：

```
┌────────┬──────────────────────────────┐
│ 行号/git │  代码内容（ch4 高亮的部分）       │
│  42 +  │  fn main() {                 │  ← "装饰"= 左边 "  42 +"
└────────┴──────────────────────────────┘
```

装饰区由 `Decoration` trait 的多个实例拼成（行号一个、git 标记一个、
网格竖线一个），每个 `generate()` 返回 `(text, width)`——"这段画什么字、占几列"。
`panel_width` = 所有装饰宽度之和（ch4 算的）。ch5 把装饰 `write!` 到行首，
`cursor_max` 减掉装饰宽，剩下的才是"代码区能放多少列"。

**为什么用 trait 对象（`Vec<Box<dyn Decoration>>`）而不是直接写死？**
因为装饰**可组合**：开 `-n` 只加行号，开 `--diff` 再加 git 标记，开 grid 再加竖线。
每种装饰独立实现 `Decoration`，`new` 时按 `style_components` 该加哪些就加哪些。
这是 ch1 ripgrep 对比里提过的"动态分发"——bat 这里用得更轻（没有线程池那套，
只有"一组可插拔的行装饰"）。

---

## 阶段 1：`Decoration` trait（`src/decorations.rs:12`）

```rust
#[derive(Debug, Clone)]
pub(crate) struct DecorationText {
    pub width: usize,      // 这段装饰占几列（决定 panel_width）
    pub text: String,      // 实际写出去的字（含颜色转义）
}

pub(crate) trait Decoration {
    fn generate(
        &self,
        line_number: usize,        // 当前行号
        continuation: bool,        // 续行？（一行折成第二行时左边补空格/竖线）
        printer: &InteractivePrinter,  // 要读 colors / line_changes / panel_width
    ) -> DecorationText;
    fn width(&self) -> usize;      // 静态宽度（panel_width 计算用）
}
```

**`continuation` 参数的意义**（小白易懵）：
一行代码若**超过终端宽度**被折成两行（ch5 路 B），第二行左边也要对齐——
但第二行**不该**再画一遍行号（行号只标第一行）。
所以 `continuation=true` 时装饰组件要返回"续行版"：
- `LineNumberDecoration`：返回**空格**（对齐，但没数字）
- `GridBorderDecoration`：返回竖线 `│`（竖线跨折行延续）
- `LineChangesDecoration`：返回**空**（git 标记只标第一行）

ch5 路 B 的 `panel_wrap` 懒生成调的正是 `d.generate(line_number, true, self)`。

---

## 阶段 2：三个装饰实现

### 2a. `LineNumberDecoration`（decorations.rs:22）

```rust
pub(crate) struct LineNumberDecoration {
    color: Style,
    cached_wrap: DecorationText,
    cached_wrap_invalid_at: usize,
}

impl LineNumberDecoration {
    pub(crate) fn new(colors: &Colors) -> Self {
        LineNumberDecoration {
            color: colors.line_number,     // ch4 Colors 抽的行号色（终端灰）
            cached_wrap_invalid_at: 10000,  // 行号到 10000 之后续行空格要变宽
            cached_wrap: DecorationText {
                text: colors.line_number.paint(" ".repeat(4)).to_string(),  // "    "（4列）
                width: 4,
            },
        }
    }
}

impl Decoration for LineNumberDecoration {
    fn generate(&self, line_number: usize, continuation: bool, _printer: &InteractivePrinter) -> DecorationText {
        if line_number == 0 {
            // -b（number_nonblank）的空行：不显示行号，显示空白
            return DecorationText {
                text: self.color.paint(" ".repeat(self.width())).to_string(),
                width: self.width(),
            };
        }
        if continuation {
            // 续行：显示对齐的空格。行号 >= 10000 时行号从 4 位变 5 位，缓存作废
            if line_number >= self.cached_wrap_invalid_at {
                let new_width = self.cached_wrap.width + 1;
                return DecorationText {
                    text: self.color.paint(" ".repeat(new_width)).to_string(),
                    width: new_width,
                };
            }
            self.cached_wrap.clone()        // 命中缓存（绝大多数续行走这）
        } else {
            let plain: String = format!("{line_number:4}");   // 右对齐 4 列："  42"
            DecorationText {
                width: plain.len(),
                text: self.color.paint(plain).to_string(),    // 上色（gutter 灰）
            }
        }
    }
    fn width(&self) -> usize { 4 }   // 行号区固定 4 列
}
```

**为什么 `format!("{line_number:4}")`？** 行号右对齐、占满 4 列：
第 1 行 → `"   1"`（3 空格+1），第 12 行 → `"  12"`。
这样**所有行号列对齐**，视觉上像表格。行号到 10000 会变 5 位，
`cached_wrap_invalid_at` 就是为这个"缓存宽度变了"预留的（极罕见）。

### 2b. `GridBorderDecoration`（decorations.rs:138）

```rust
pub(crate) struct GridBorderDecoration {
    cached: DecorationText,
}
impl GridBorderDecoration {
    pub(crate) fn new(colors: &Colors) -> Self {
        GridBorderDecoration {
            cached: DecorationText {
                text: colors.grid.paint("│").to_string(),   // 竖线"│"
                width: 1,
            },
        }
    }
}
impl Decoration for GridBorderDecoration {
    fn generate(&self, _line_number: usize, _continuation: bool, _printer: &InteractivePrinter) -> DecorationText {
        self.cached.clone()   // 永远就一个竖线，缓存直接返
    }
    fn width(&self) -> usize { self.cached.width }   // 1
}
```
最傻的一个：行号区和代码区之间画一根竖线 `│`。
`_continuation` 没用——竖线在折行时**要延续**（续行也画），所以 `continuation` 不影响它。

### 2c. `LineChangesDecoration`（decorations.rs:81，git feature）

```rust
pub(crate) struct LineChangesDecoration {
    cached_none: DecorationText,            // 空格（无改动）
    cached_added: DecorationText,           // "+" 绿
    cached_removed_above: DecorationText,   // "‾" 红（行上方有删除）
    cached_removed_below: DecorationText,   // "_" 红（行下方有删除）
    cached_modified: DecorationText,        // "~" 黄
}
impl LineChangesDecoration {
    pub(crate) fn new(colors: &Colors) -> Self {
        LineChangesDecoration {
            cached_none: Self::generate_cached(Style::default(), " "),
            cached_added: Self::generate_cached(colors.git_added, "+"),
            cached_removed_above: Self::generate_cached(colors.git_removed, "‾"),
            cached_removed_below: Self::generate_cached(colors.git_removed, "_"),
            cached_modified: Self::generate_cached(colors.git_modified, "~"),
        }
    }
}
impl Decoration for LineChangesDecoration {
    fn generate(&self, line_number: usize, continuation: bool, printer: &InteractivePrinter) -> DecorationText {
        if !continuation {
            // 只看"第一行"（续行不重复画 git 标记）
            if let Some(ref changes) = printer.line_changes {
                return match changes.get(&(line_number as u32)) {
                    Some(&LineChange::Added) => self.cached_added.clone(),
                    Some(&LineChange::RemovedAbove) => self.cached_removed_above.clone(),
                    Some(&LineChange::RemovedBelow) => self.cached_removed_below.clone(),
                    Some(&LineChange::Modified) => self.cached_modified.clone(),
                    _ => self.cached_none.clone(),
                };
            }
        }
        self.cached_none.clone()   // 无 git 改动 / 续行 → 空白
    }
    fn width(&self) -> usize { self.cached_none.width }   // 1
}
```

**`LineChange` 四种状态**（diff.rs:11）：
```rust
pub enum LineChange {
    Added,            // 这行是新增的（git 里原来没有）
    RemovedAbove,     // 这行**上方**有行被删了（用 ‾ 标在这行顶部）
    RemovedBelow,     // 这行**下方**有行被删了（用 _ 标在这行底部）
    Modified,         // 这行被改过
}
```
`LineChanges`（diff.rs:18）是 `HashMap<u32 行号, LineChange>`。
`get_git_diff`（diff.rs:43）用 **gix 库**（Rust 写的 git 库）对"当前工作树"
和"git index"做 diff，算出每个行号属于哪种改动。
ch4 `Controller::print_input` 里 `get_git_diff(path)` 产出它，
塞进 `InteractivePrinter.line_changes`，这里查表画标记。

**小白会问：`RemovedAbove` / `RemovedBelow` 为什么分两个？**
git 删了一行，diff 只说"在第 N 行附近删了"，但**哪一侧**（这行上面还是下面）
取决于删除行原本的位置。bat 按位置决定 `‾`（标在保留行的上边）还是
`_`（标在下边），让删除的"位置感"更准。

---

## 阶段 3：`Colors` —— 装饰区用什么色

`src/printer.rs:996`（ch4 `new` 里 `Colors::colored(theme, true_color)` 产出的）：

```rust
#[derive(Debug, Default)]
pub struct Colors {
    pub grid: Style,            // 网格竖线 / 横线
    pub rule: Style,            // 文件间横线
    pub header_value: Style,    // header 里文件名/大小（粗体）
    pub git_added: Style,       // 绿（+ 标记）
    pub git_removed: Style,    // 红（‾ _ 标记）
    pub git_modified: Style,    // 黄（~ 标记）
    pub line_number: Style,     // 行号（gutter 灰）
}

impl Colors {
    fn plain() -> Self { Colors::default() }   // 不上色 → 全默认

    fn colored(theme: &Theme, true_color: bool) -> Self {
        // 行号/网格用"终端 gutter 色"。主题可指定；否则降级成固定灰 #238(=238号)
        let gutter_style = Style {
            foreground: match theme.settings.gutter_foreground {
                Some(c) => to_ansi_color(c, true_color),   // ch5 讲的颜色转换
                None => Some(Fixed(DEFAULT_GUTTER_COLOR)), // 238号灰（256色板）
            },
            ..Style::default()
        };
        Colors {
            grid: gutter_style,
            rule: gutter_style,
            header_value: Style::new().bold(),       // header 值加粗
            git_added: Green.normal(),               // 绿
            git_removed: Red.normal(),              // 红
            git_modified: Yellow.normal(),          // 黄
            line_number: gutter_style,              // 行号=gutter 色
        }
    }
}
```

**`gutter_foreground` 的小细节**（terminal.rs:6 的 `to_ansi_color` 回看）：
主题能用 `#00000001`（alpha=1）编码"用终端默认前景"。
`to_ansi_color` 返回 `None` → `gutter_style.foreground` 保持默认
（终端自己的前景色，不是硬编码色）。
这就是为什么 bat 的行号颜色能**跟着你的终端主题走**而不是写死。

---

## 阶段 4：`StyleComponent` / `StyleComponents` —— "哪些装饰要开"

`src/style.rs`。这是"装饰总开关"的**枚举定义**（ch2 讲过 `style_components` 字段，
这里讲它内部怎么展开）。

```rust
#[non_exhaustive]
#[derive(Debug, Eq, PartialEq, Copy, Clone, Hash)]
pub enum StyleComponent {
    Auto,                 // 交互→Default，非交互→Plain（"看情况"）
    #[cfg(feature = "git")]
    Changes,              // git 改动标记
    Grid,                 // 网格竖线+横线
    Rule,                 // 文件间横线
    Header,               // 文件头（= HeaderFilename 的别名）
    HeaderFilename,
    HeaderFilesize,
    LineNumbers,          // 行号
    Snip,                 // 行范围分隔线（"..." snip）
    Full,                 // 全开
    Default,             // 默认那套（changes+grid+header-filename+numbers+snip）
    Plain,               // 全关
}
```

`components(interactive_terminal)` 把"预设"展开成"具体组件集合"（style.rs:25）：
```rust
pub fn components(self, interactive_terminal: bool) -> &'static [StyleComponent] {
    match self {
        Auto => {
            // 交互终端 → 全装饰；管道/非交互 → 纯文本
            if interactive_terminal { StyleComponent::Default.components(interactive_terminal) }
            else { StyleComponent::Plain.components(interactive_terminal) }
        }
        Changes => &[StyleComponent::Changes],
        Grid => &[StyleComponent::Grid],
        Rule => &[StyleComponent::Rule],
        Header => &[StyleComponent::HeaderFilename],   // 别名
        HeaderFilename => &[StyleComponent::HeaderFilename],
        HeaderFilesize => &[StyleComponent::HeaderFilesize],
        LineNumbers => &[StyleComponent::LineNumbers],
        Snip => &[StyleComponent::Snip],
        Full => &[                          // 全开
            #[cfg(feature = "git")] StyleComponent::Changes,
            StyleComponent::Grid, StyleComponent::HeaderFilename,
            StyleComponent::HeaderFilesize, StyleComponent::LineNumbers, StyleComponent::Snip],
        Default => &[                        // 默认（比 Full 少 HeaderFilesize）
            #[cfg(feature = "git")] StyleComponent::Changes,
            StyleComponent::Grid, StyleComponent::HeaderFilename,
            StyleComponent::LineNumbers, StyleComponent::Snip],
        Plain => &[],                          // 啥都不画
    }
}
```

**`StyleComponents`**（style.rs:89）就是个 `HashSet<StyleComponent>` 的壳：
```rust
pub struct StyleComponents(pub HashSet<StyleComponent>);
impl StyleComponents {
    pub fn numbers(&self) -> bool { self.0.contains(&StyleComponent::LineNumbers) }
    pub fn grid(&self) -> bool { self.0.contains(&StyleComponent::Grid) }
    pub fn changes(&self) -> bool { self.0.contains(&StyleComponent::Changes) }  // git feature
    // … 每个组件一个查询方法
}
```
ch4 `InteractivePrinter::new` 里 `if config.style_components.numbers()` 就是查它。
`StyleComponentList::from_str`（style.rs:222）把 `--style "grid,+numbers,-snip"`
按逗号切、按前缀记 `Add/Remove/Override`，多组 `--style` 按"后盖前"叠加
（ch1 讲过）。

**`Auto` 是"魔法"组件**：它让"交互终端 vs 管道"自动切换全装饰/纯文本，
所以 `bat foo.rs`（终端里）有行号网格，`bat foo.rs | grep x`（管道）自动变纯文本。
ch2 的 `loop_through` 和这里的 `Auto` 是**两套**自动机制——
`loop_through` 决定"用不用 InteractivePrinter"，`Auto` 决定"用哪个组件集合"。

---

## 这一章把 ch5 那行"装饰"补全了

回看 ch5 `print_line` 的装饰段：
```rust
if self.panel_width > 0 {
    let decorations = self.decorations.iter()
        .map(|d| d.generate(display_line_number, false, self));
    for deco in decorations {
        write!(handle, "{} ", deco.text)?;     // "  42 +" 这类前缀
        cursor_max -= deco.width + 1;
    }
}
```
现在你懂了：
- `self.decorations` 是 ch4 `new` 里按 `style_components` 攒的
  `Vec<Box<dyn Decoration>>`（行号 / git / 网格竖线，按需）
- 每个 `generate` 返回 `(text, width)`，text 是上色的 `DecorationText`
- `panel_width` 是它们宽度之和，ch5 用它算内容区能放多少列
- `continuation=true`（折行续行）时，行号变空格、竖线延续、git 标记清空

---

## 本章涉及的 Rust 机制（小白需要补的知识点）

1. **`Vec<Box<dyn Decoration>>`（trait 对象）**：装饰可插拔（行号/git/网格任意组合），
   用 `Box<dyn>` 而非枚举，加新装饰只加实现 + push，不改 ch5 渲染循环（开闭原则）。
   代价是每次 `generate` 走 vtable（动态分发），但装饰数量极少、非热点。
2. **`#[cfg(feature = "git")]` 门控**：`LineChangesDecoration` 只在 git feature
   开时存在（依赖 `gix` 库算 diff）。关 feature 时整段代码不编译（ch9 详讲）。
3. **`Colorable` trait（`lscolors` 库）**：`DirEntry` 实现它，让 `LsColors`
   能按"文件类型"给路径上色（ch5 用过）。装饰的 `Colors` 也靠它从主题抽色。
4. **`CachedDecorationText`（缓存模式）**：`LineNumberDecoration` 的 `cached_wrap`
   和 `GridBorderDecoration` 的 `cached` 都是"提前算好、重复返回"的缓存——
   续行空格、网格竖线每次都是同一个值，缓存省得每行重算。

## 检验问题

1. 为什么装饰用 `Vec<Box<dyn Decoration>>` 而不是直接写死"行号+网格"？
2. `continuation=true` 时三个装饰各返回什么？为什么 `GridBorderDecoration` 不受影响？
3. `LineNumberDecoration` 为什么固定 4 列宽？行号到 10000+ 会怎样（`cached_wrap_invalid_at`）？
4. `StyleComponent::Auto` 在"管道输出"时展开成什么？它和 `loop_through` 的区别？
5. `Colors` 里行号/gutter 色为什么可能返回 `None`（终端默认色）？主题怎么用 `#00000001` 做到？
6. `LineChange::RemovedAbove` 和 `RemovedBelow` 画出来分别是 `‾` 还是 `_`？

## 下一步

**ch8 — 端到端复盘**：把一条真实命令 `bat -n --theme=Monokai foo.rs`
从 `main` 一路走到 stdout/less，串起前 7 章。最后 8 道检验题收尾。
