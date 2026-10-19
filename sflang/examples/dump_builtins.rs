// dump_builtins — Sflang 内置函数参考文档生成器
// ==================================
// 从 VM 内嵌的 BuiltinDoc（help() 系统的同一数据源）导出全部内置函数文档为
// Markdown，供 Sflang 专区（https://magicdo.top/sf/exam/）的 reference-builtins.md
// 使用。数据源与 help("函数名") 完全一致，新增/修改内置函数文档后重跑本例即可再生成。
//
// 用法：
//   cargo run -p sflang --example dump_builtins > reference-builtins.md

use std::fmt::Write as _;

use sflang::Sflang;

/// main 遍历全部分类与函数，把每个内置函数的文档渲染为 Markdown 输出到 stdout。
fn main() {
    let mut sf = Sflang::new();
    let vm = sf.vm_mut();
    let cats = vm.builtin_categories();
    let total = vm.builtin_names().len();

    let mut out = String::new();
    writeln!(out, "# Sflang 内置函数参考（{} 个）", total).unwrap();
    writeln!(out).unwrap();
    writeln!(out, "本文件由 `cargo run -p sflang --example dump_builtins` 从二进制内嵌的函数文档自动生成，").unwrap();
    writeln!(out, "与运行时的 `help(\"函数名\")` 输出同源。").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "运行时自省：`help()` 全部分类一览，`help(\"分类\")` 浏览分类，`help(\"函数名\")` 查详情；").unwrap();
    writeln!(out, "`sf --list-builtins [分类]` 从命令行列出。").unwrap();

    for (cat, names) in &cats {
        writeln!(out).unwrap();
        writeln!(out, "## {}（{} 个）", cat, names.len()).unwrap();
        for name in names {
            let doc = match vm.builtin_doc(name) {
                Some(d) => d,
                None => continue,
            };
            writeln!(out).unwrap();
            writeln!(out, "### `{}`", name).unwrap();
            writeln!(out).unwrap();
            writeln!(out, "- 签名：`{}`", doc.signature).unwrap();
            writeln!(out, "- 简介：{}", doc.summary).unwrap();
            if !doc.params.is_empty() {
                writeln!(out, "- 参数：").unwrap();
                for (p, desc) in doc.params {
                    writeln!(out, "  - `{}`：{}", p, desc).unwrap();
                }
            }
            writeln!(out, "- 返回：{}", doc.returns).unwrap();
            if !doc.examples.is_empty() {
                writeln!(out, "- 示例：").unwrap();
                for e in doc.examples {
                    writeln!(out, "  - `{}`", e).unwrap();
                }
            }
            if !doc.errors.is_empty() {
                writeln!(out, "- 常见错误：").unwrap();
                for e in doc.errors {
                    writeln!(out, "  - {}", e).unwrap();
                }
            }
        }
    }

    print!("{}", out);
}
