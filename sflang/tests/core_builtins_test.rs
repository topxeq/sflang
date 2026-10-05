//! core_builtins_test.rs — 全局核心内置函数表（阶段0a）回归测试
//!
//! 验证点：
//!   1. 新建 VM 无需逐 VM 注册即可使用全部核心内置函数（全局表生效）
//!   2. 自定义内置函数可注册到单个 VM（extra_builtins 生效）
//!   3. 自定义函数可覆盖核心同名函数（查找顺序：extra → core）
//!   4. help 体系（names/doc/categories）合并两张表
//!   5. 多个 VM 共享全局表（两个独立 VM 行为一致）

use sflang::value::Value;
use sflang::Sflang;

/// bi_double 测试用自定义内置函数：参数乘 2。
fn bi_double(_vm: &mut sflang::VM, args: &[Value]) -> Result<Value, Value> {
    let n = args[0].to_int().unwrap_or(0);
    Ok(Value::Int(n * 2))
}

/// bi_len_fake 测试用覆盖函数：恒返回 -1（验证覆盖语义用）。
fn bi_len_fake(_vm: &mut sflang::VM, _args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Int(-1))
}

/// test_global_table_fresh_vm 新建 VM 直接调用核心内置函数。
#[test]
fn test_global_table_fresh_vm() {
    let mut sf = Sflang::new();
    assert_eq!(sf.run_string("strRepeat(\"ab\", 3)").unwrap().to_str(), "ababab");
    assert_eq!(sf.run_string("len([1,2,3])").unwrap().to_str(), "3");
}

/// test_custom_builtin_registration 自定义内置函数注册到单个 VM。
#[test]
fn test_custom_builtin_registration() {
    let mut sf = Sflang::new();
    sf.vm_mut().register_builtin("double", bi_double);
    assert_eq!(sf.run_string("double(21)").unwrap().to_str(), "42");

    // 其他 VM 不受影响（自定义函数不进全局表）
    let mut sf2 = Sflang::new();
    assert!(!sf2.vm_mut().builtin_exists("double"));
}

/// test_custom_builtin_overrides_core 自定义函数覆盖核心同名函数。
#[test]
fn test_custom_builtin_overrides_core() {
    let mut sf = Sflang::new();
    sf.vm_mut().register_builtin("len", bi_len_fake);
    assert_eq!(sf.run_string("len([1,2,3])").unwrap().to_str(), "-1"); // 覆盖生效
    assert_eq!(sf.run_string("trim(\"  x  \")").unwrap().to_str(), "x"); // 其余核心函数不受影响

    // 新 VM 恢复核心行为（覆盖不进全局表）
    let mut sf2 = Sflang::new();
    assert_eq!(sf2.run_string("len([1,2,3])").unwrap().to_str(), "3");
}

/// test_help_views_merge_tables help 体系（names/doc/categories）合并两张表。
#[test]
fn test_help_views_merge_tables() {
    let mut sf = Sflang::new();
    sf.vm_mut().register_builtin("double", bi_double);
    let vm = sf.vm_mut();

    // names 含核心函数与自定义函数
    let names = vm.builtin_names();
    assert!(names.contains(&"len"));
    assert!(names.contains(&"double"));

    // doc 查询两表都可用
    assert!(vm.builtin_doc("len").is_some());
    assert!(vm.builtin_doc("double").is_none()); // 无文档的自定义函数

    // categories 含自定义函数（归入 uncategorized）
    let cats = vm.builtin_categories();
    let unc = cats.iter().find(|(c, _)| *c == "(uncategorized)").unwrap();
    assert!(unc.1.contains(&"double"));
}

/// test_shared_table_across_vms 多个 VM 共享全局表且行为一致。
#[test]
fn test_shared_table_across_vms() {
    let mut sf1 = Sflang::new();
    let mut sf2 = Sflang::new();
    let n1 = sf1.vm_mut().builtin_names().len();
    let n2 = sf2.vm_mut().builtin_names().len();
    assert_eq!(n1, n2);
    assert_eq!(
        sf1.run_string("strRepeat(\"x\", 2)").unwrap().to_str(),
        sf2.run_string("strRepeat(\"x\", 2)").unwrap().to_str()
    );
}

/// test_math_constants_per_vm 预定义全局常量（piG/eG）每 VM 私有预置。
#[test]
fn test_math_constants_per_vm() {
    let mut sf = Sflang::new();
    assert_eq!(sf.run_string("piG > 3.14 && piG < 3.15").unwrap().to_str(), "true");
}
