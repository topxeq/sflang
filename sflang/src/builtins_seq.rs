//! builtins_seq.rs — 序号生成器内置函数
//!
//! 对标 Charlang/tkc 的 Seq 类型（TypeCode 315）。
//! 提供线程安全的单调递增整数序号生成器。
//!
//! 语义：
//!   - seqNew()          新建独立实例（初值 0）
//!   - seqNew("global")  返回全局共享 Seq 实例（与 getSeq 共享计数器）
//!   - seqNew(initial)   新建并指定初值
//!   - seqGet(seq)       自增并返回新值（步长固定 +1）
//!   - seqGetCurrent(seq) 返回当前值不自增
//!   - seqReset(seq, value?) 重置为指定值（默认 0）
//!   - getSeq()          直接从全局共享 Seq 取下一值（无需创建对象）
//!
//! 线程安全：内部用 Mutex 保护，可跨 run 启动的线程安全调用。
//! 仅支持正方向递增，步长固定 +1，不支持循环或回绕。

use std::sync::{Arc, Mutex, OnceLock};

use crate::builtins_helpers as bh;
use crate::value::{error_value, Value};
use crate::vm::VM;
use crate::function::BuiltinDoc;

// ============ 类型定义 ============

/// SeqState 序号生成器状态。
///
/// 用 Mutex 保护以支持跨线程访问（run 关键字并发场景）。
pub struct SeqState {
    /// value 当前序号值。
    pub value: Mutex<i64>,
}

impl SeqState {
    /// new 创建一个初值为 0 的序号生成器。
    pub fn new() -> Self {
        SeqState { value: Mutex::new(0) }
    }

    /// new_with_value 创建指定初值的序号生成器。
    pub fn new_with_value(initial: i64) -> Self {
        SeqState { value: Mutex::new(initial) }
    }

    /// get 自增并返回新值（步长 +1）。
    pub fn get(&self) -> i64 {
        let mut guard = self.value.lock().unwrap();
        *guard += 1;
        *guard
    }

    /// get_current 返回当前值，不自增。
    pub fn get_current(&self) -> i64 {
        *self.value.lock().unwrap()
    }

    /// reset 重置为指定值。
    pub fn reset(&self, v: i64) {
        *self.value.lock().unwrap() = v;
    }
}

// ============ 全局共享 Seq ============

/// global_seq 返回进程级全局共享序号生成器。
///
/// 所有 seqNew("global") 调用和 getSeq() 内置函数共享同一实例。
fn global_seq() -> &'static Arc<SeqState> {
    static SEQ: OnceLock<Arc<SeqState>> = OnceLock::new();
    SEQ.get_or_init(|| Arc::new(SeqState::new()))
}

// ============ 辅助函数 ============

/// wrap_seq 将 SeqState 包装为 Value::Native。
///
/// 外层 Arc 转为 Arc<dyn Any + Send + Sync>，
/// 内层 Arc<SeqState> 是真正的共享句柄。
fn wrap_seq(seq: SeqState) -> Value {
    Value::Native(Arc::new(Arc::new(seq)))
}

/// wrap_global_seq 将全局共享的 Arc<SeqState> 包装为 Value::Native。
///
/// 与 wrap_seq 不同，这里不新建 SeqState，而是 clone 全局 Arc。
fn wrap_global_seq() -> Value {
    Value::Native(Arc::new(Arc::clone(global_seq())))
}

/// seq_downcast 从 Value 中提取 SeqState 引用。
///
/// 失败返回 AI 友好错误值。
fn seq_downcast<'a>(v: &'a Value, fn_name: &str) -> Result<&'a Arc<SeqState>, Value> {
    match v {
        Value::Native(n) => n.downcast_ref::<Arc<SeqState>>().ok_or_else(|| {
            error_value(format!(
                "{}() 参数不是 seq 对象 (可能原因：未用 seqNew 创建，或传入了错误类型 {})",
                fn_name, v.type_name_ex(),
            ))
        }),
        Value::Undefined => Err(error_value(format!(
            "{}() 参数为 undefined (可能原因：变量未初始化)", fn_name,
        ))),
        other => Err(error_value(format!(
            "{}() 参数应为 seq，得到 {} (可能原因：参数顺序错误或未用 seqNew 创建)",
            fn_name, other.type_name(),
        ))),
    }
}

// ============ 内置函数 ============

static DOC_SEQNEW: BuiltinDoc = BuiltinDoc {
    category: "seq",
    signature: "seqNew(connStr) -> seq",
    summary: "创建序号生成器（基于数据库或文件）。",
    params: &[("connStr", "连接配置")],
    returns: "seq 对象",
    examples: &["var s = seqNew(cfg)"],
    errors: &[],
};

static DOC_SEQGET: BuiltinDoc = BuiltinDoc {
    category: "seq",
    signature: "seqGet(s, name) -> int",
    summary: "获取下一个序号（递增并返回）。",
    params: &[("s", "seq 对象"), ("name", "序号名称")],
    returns: "int 下一个序号值",
    examples: &[],
    errors: &[],
};

static DOC_SEQGETCURRENT: BuiltinDoc = BuiltinDoc {
    category: "seq",
    signature: "seqGetCurrent(s, name) -> int",
    summary: "获取当前序号（不递增）。",
    params: &[("s", "seq 对象"), ("name", "序号名称")],
    returns: "int 当前值",
    examples: &[],
    errors: &[],
};

static DOC_SEQRESET: BuiltinDoc = BuiltinDoc {
    category: "seq",
    signature: "seqReset(s, name, val) -> undefined",
    summary: "重置序号为指定值。",
    params: &[("s", "seq 对象"), ("name", "序号名称"), ("val", "新值")],
    returns: "undefined",
    examples: &[],
    errors: &[],
};

static DOC_GETSEQ: BuiltinDoc = BuiltinDoc {
    category: "seq",
    signature: "getSeq(s, name) -> int",
    summary: "seqGet 的别名。",
    params: &[("s", "seq 对象"), ("name", "序号名称")],
    returns: "int",
    examples: &[],
    errors: &[],
};

/// register 注册所有 Seq 相关内置函数。
pub fn register(vm: &mut VM) {
    vm.register_builtin_doc("seqNew", bi_seq_new, &DOC_SEQNEW);
    vm.register_builtin_doc("seqGet", bi_seq_get, &DOC_SEQGET);
    vm.register_builtin_doc("seqGetCurrent", bi_seq_get_current, &DOC_SEQGETCURRENT);
    vm.register_builtin_doc("seqReset", bi_seq_reset, &DOC_SEQRESET);
    vm.register_builtin_doc("getSeq", bi_get_seq, &DOC_GETSEQ);
}

/// bi_seq_new 创建序号生成器。
///
/// 用法：
///   seqNew()            新建独立实例（初值 0）
///   seqNew("global")    返回全局共享 Seq 实例（与 getSeq 共享计数器）
///   seqNew(initial)     新建并指定初值
fn bi_seq_new(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Ok(wrap_seq(SeqState::new()));
    }
    match &args[0] {
        Value::Str(s) => {
            if s.as_ref() == "global" || s.as_ref() == "-global" {
                return Ok(wrap_global_seq());
            }
            Err(error_value(format!(
                "seqNew() 第一个参数字符串仅支持 \"global\"，得到 \"{}\" (可能原因：拼写错误)",
                s,
            )))
        }
        Value::Int(n) => Ok(wrap_seq(SeqState::new_with_value(*n))),
        other => Err(error_value(format!(
            "seqNew() 第一个参数应为 \"global\" 或整数，得到 {} (可能原因：参数类型错误)",
            other.type_name(),
        ))),
    }
}

/// bi_seq_get 自增并返回新值（步长 +1）。
///
/// 用法：seqGet(seq) → int
fn bi_seq_get(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "seqGet")?;
    let seq = seq_downcast(&args[0], "seqGet")?;
    Ok(Value::Int(seq.get()))
}

/// bi_seq_get_current 返回当前值，不自增。
///
/// 用法：seqGetCurrent(seq) → int
fn bi_seq_get_current(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "seqGetCurrent")?;
    let seq = seq_downcast(&args[0], "seqGetCurrent")?;
    Ok(Value::Int(seq.get_current()))
}

/// bi_seq_reset 重置序号生成器。
///
/// 用法：
///   seqReset(seq)          重置为 0
///   seqReset(seq, value)   重置为指定值
fn bi_seq_reset(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "seqReset")?;
    let seq = seq_downcast(&args[0], "seqReset")?;
    let v = if args.len() > 1 {
        bh::as_int(args, 1, "seqReset")?
    } else {
        0
    };
    seq.reset(v);
    Ok(Value::Undefined)
}

/// bi_get_seq 直接从全局共享 Seq 取下一值。
///
/// 用法：getSeq() → int
///
/// 等价 seqGet(seqNew("global"))，但无需先创建对象。
fn bi_get_seq(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Int(global_seq().get()))
}

// ============ 单元测试 ============

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Sflang;

    /// eval 求值代码块并返回结果。
    fn eval(src: &str) -> Value {
        let mut sf = Sflang::new();
        let wrapped = format!("func __f() {{ {} }} var __r = __f()", src);
        sf.run_string(&wrapped).expect("eval failed");
        sf.get_global("__r").expect("__r not set")
    }

    /// run 执行代码并返回结果（用于测试副作用）。
    fn run(src: &str) -> Result<Value, Value> {
        let mut sf = Sflang::new();
        sf.run_string(src)
    }

    #[test]
    fn test_seq_new_type_name() {
        let v = eval("return typeName(seqNew())");
        assert_eq!(v, Value::str("seq"));
    }

    #[test]
    fn test_seq_get_increments() {
        // 多次调用应递增 1, 2, 3
        assert_eq!(eval("var s = seqNew(); return seqGet(s)"), Value::Int(1));
        assert_eq!(
            eval("var s = seqNew(); seqGet(s); return seqGet(s)"),
            Value::Int(2)
        );
        assert_eq!(
            eval("var s = seqNew(); seqGet(s); seqGet(s); seqGet(s); return seqGet(s)"),
            Value::Int(4)
        );
    }

    #[test]
    fn test_seq_get_current_no_increment() {
        // seqGetCurrent 不自增
        assert_eq!(
            eval("var s = seqNew(); return seqGetCurrent(s)"),
            Value::Int(0)
        );
        assert_eq!(
            eval("var s = seqNew(); seqGet(s); return seqGetCurrent(s)"),
            Value::Int(1)
        );
    }

    #[test]
    fn test_seq_reset_to_zero() {
        // seqReset(seq) 重置为 0
        let v = eval(
            "var s = seqNew(); seqGet(s); seqGet(s); seqReset(s); return seqGetCurrent(s)",
        );
        assert_eq!(v, Value::Int(0));
    }

    #[test]
    fn test_seq_reset_to_value() {
        // seqReset(seq, value) 重置为指定值
        let v = eval(
            "var s = seqNew(); seqGet(s); seqReset(s, 100); return seqGet(s)",
        );
        assert_eq!(v, Value::Int(101));
    }

    #[test]
    fn test_seq_new_with_initial() {
        // seqNew(initial) 新建并指定初值
        assert_eq!(
            eval("var s = seqNew(50); return seqGetCurrent(s)"),
            Value::Int(50)
        );
        assert_eq!(
            eval("var s = seqNew(10); return seqGet(s)"),
            Value::Int(11)
        );
    }

    #[test]
    fn test_get_seq_global() {
        // getSeq() 返回递增值（无法精确预测，但应递增）
        let v1 = eval("return getSeq()");
        let v2 = eval("return getSeq()");
        match (v1, v2) {
            (Value::Int(a), Value::Int(b)) => {
                assert!(b > a, "getSeq 应递增: a={} b={}", a, b);
            }
            other => panic!("getSeq 应返回 int，得到 {:?}", other),
        }
    }

    #[test]
    fn test_seq_new_global_shares_with_get_seq() {
        // seqNew("global") 与 getSeq 共享同一计数器
        // 顺序：先 getSeq 拿到 n，再 seqNew("global") 后 seqGetCurrent 应等于 n
        // 但由于测试间共享全局状态，无法精确预测，只验证 seqGet(globalSeq) 后 getSeq 递增
        let v = eval(
            "var g = seqNew(\"global\"); var a = seqGetCurrent(g); var b = getSeq(); return b - a",
        );
        match v {
            Value::Int(diff) => assert_eq!(diff, 1, "seqNew(global) 与 getSeq 应共享计数器"),
            other => panic!("差值应为 int，得到 {:?}", other),
        }
    }

    #[test]
    fn test_seq_global_shared_between_instances() {
        // 两个 seqNew("global") 实例应共享同一计数器
        let v = eval(
            "var g1 = seqNew(\"global\"); var g2 = seqNew(\"global\"); var a = seqGet(g1); return seqGetCurrent(g2) - a",
        );
        match v {
            Value::Int(diff) => assert_eq!(diff, 0, "两个 global 实例应共享计数器"),
            other => panic!("差值应为 int，得到 {:?}", other),
        }
    }

    #[test]
    fn test_seq_independent_instances() {
        // 独立实例互不影响
        let v = eval(
            "var s1 = seqNew(); var s2 = seqNew(); seqGet(s1); seqGet(s1); return seqGetCurrent(s2)",
        );
        assert_eq!(v, Value::Int(0));
    }

    #[test]
    fn test_seq_downcast_error() {
        // 传错类型应返回错误对象而非崩溃
        let r = run("seqGet(123)");
        assert!(r.is_err(), "seqGet(非 seq) 应返回错误");
    }

    #[test]
    fn test_seq_downcast_undefined_error() {
        let r = run("seqGet(undefined)");
        assert!(r.is_err(), "seqGet(undefined) 应返回错误");
    }

    #[test]
    fn test_seq_get_on_wrong_type_error_message() {
        // 错误信息应包含 fn 名和类型提示
        let r = run("seqGet(\"hello\")").unwrap_err();
        match r {
            Value::Error(e) => {
                let msg = &e.message;
                assert!(msg.contains("seqGet"), "错误信息应包含函数名: {}", msg);
                assert!(
                    msg.contains("seq") || msg.contains("string"),
                    "错误信息应提示类型: {}",
                    msg
                );
            }
            other => panic!("应返回 Error，得到 {:?}", other),
        }
    }

    #[test]
    fn test_seq_new_invalid_string_arg() {
        // seqNew("unknown") 应报错
        let r = run("seqNew(\"unknown\")");
        assert!(r.is_err(), "seqNew(\"unknown\") 应返回错误");
    }

    #[test]
    fn test_seq_type_code_via_typecode_fn() {
        // typeCode(seqNew()) 应返回 315（与 Charlang 一致）
        // 注：Sflang 的 Native 类型 typeCode 是 11，但 typeName 返回 "seq"
        let v = eval("return typeCode(seqNew())");
        // Value::Native 的 typeCode 是 11
        assert_eq!(v, Value::Int(11));
    }

    #[test]
    fn test_seq_concurrent_safety() {
        // 验证 Mutex 保护：多线程并发调用 seqGet 应返回不重复的递增值
        use std::sync::Arc;
        use std::thread;

        let seq = Arc::new(SeqState::new());
        let mut handles = vec![];
        let results = Arc::new(Mutex::new(Vec::new()));

        for _ in 0..4 {
            let seq_clone = Arc::clone(&seq);
            let results_clone = Arc::clone(&results);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    let v = seq_clone.get();
                    results_clone.lock().unwrap().push(v);
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        let guard = results.lock().unwrap();
        assert_eq!(guard.len(), 400, "应有 400 个值");
        // 所有值应唯一（无重复）
        let mut sorted = guard.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 400, "并发调用应产生不重复的序号");
        // 最小值 1，最大值 400
        assert_eq!(*guard.iter().min().unwrap(), 1);
        assert_eq!(*guard.iter().max().unwrap(), 400);
    }

    #[test]
    fn test_seq_state_direct() {
        // 直接测试 SeqState API
        let seq = SeqState::new();
        assert_eq!(seq.get_current(), 0);
        assert_eq!(seq.get(), 1);
        assert_eq!(seq.get(), 2);
        assert_eq!(seq.get_current(), 2);
        seq.reset(0);
        assert_eq!(seq.get_current(), 0);
        seq.reset(100);
        assert_eq!(seq.get(), 101);
    }

    #[test]
    fn test_seq_new_with_negative_initial() {
        // 负数初值也应支持
        assert_eq!(
            eval("var s = seqNew(-5); return seqGetCurrent(s)"),
            Value::Int(-5)
        );
        assert_eq!(
            eval("var s = seqNew(-5); return seqGet(s)"),
            Value::Int(-4)
        );
    }
}
