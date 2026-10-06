// 进程级共享变量：跨 VM 共享与默认值/删除语义
use sflang::api::Sflang;

#[test]
fn test_process_var_cross_vm() {
    let mut a = Sflang::new();
    let mut b = Sflang::new();
    a.run_string(r#"setProcessVar("pv_test_hits", {"n": 1})"#).unwrap();
    let v = b.run_string(r#"getProcessVar("pv_test_hits")["n"]"#).unwrap();
    assert_eq!(v.inspect(), "1");
    // 引用共享：b 修改，a 可见
    b.run_string(r#"getProcessVar("pv_test_hits")["n"] = 42"#).unwrap();
    let v2 = a.run_string(r#"getProcessVar("pv_test_hits")["n"]"#).unwrap();
    assert_eq!(v2.inspect(), "42");
}

#[test]
fn test_process_var_default_and_delete() {
    let mut vm = Sflang::new();
    let v = vm.run_string(r#"getProcessVar("__nope__", "dft")"#).unwrap();
    assert_eq!(v.inspect(), "dft");
    let u = vm.run_string(r#"getProcessVar("__nope__")"#).unwrap();
    assert!(matches!(u, sflang::value::Value::Undefined));
    vm.run_string(r#"setProcessVar("__del__", 1); deleteProcessVar("__del__")"#).unwrap();
    let gone = vm.run_string(r#"getProcessVar("__del__")"#).unwrap();
    assert!(matches!(gone, sflang::value::Value::Undefined));
}
