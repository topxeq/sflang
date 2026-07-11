//! builtins_test.rs — 脚本内断言测试框架
//!
//! 设计要点：
//!   - 提供 testByText/testByContains/testByReg 三种断言方式
//!   - 断言失败抛出异常（含期望值与实际值对比），脚本用 try/catch 收集
//!   - 配合 assert 函数（已有）使用，覆盖更复杂场景
//!   - 保持简单：不引入隐式测试状态收集器，失败即抛异常
//!
//! 函数列表：
//!   testByText(actual, expected)         — 断言实际值文本等于期望值
//!   testByContains(actual, substring)   — 断言实际值包含子串
//!   testByReg(actual, pattern)          — 断言实际值匹配正则
//!
//! 三者均接受可选第三参数 message，作为失败时的附加说明。
//! 断言通过返回 undefined，失败抛出 error。

use crate::builtins_helpers as bh;
use crate::value::{Value, error_value};
use crate::vm::VM;

/// register 注册测试内置函数。
pub fn register(vm: &mut VM) {
    vm.register_builtin("testByText", bi_test_by_text);
    vm.register_builtin("testByContains", bi_test_by_contains);
    vm.register_builtin("testByReg", bi_test_by_reg);
}

/// extract_message 从可选第三参数提取失败说明。
fn extract_message(args: &[Value]) -> String {
    if args.len() > 2 {
        match &args[2] {
            Value::Str(s) => format!(" — {}", s),
            _ => format!(" — {}", args[2].inspect()),
        }
    } else {
        String::new()
    }
}

/// bi_test_by_text 断言实际值文本等于期望值。
///
/// 用法：testByText("hello", "hello")  // 通过
///       testByText(getValue(), "42")  // 通过
fn bi_test_by_text(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 1, "testByText")?;
    let actual = args[0].to_str();
    let expected = args[1].to_str();
    if actual != expected {
        let msg = extract_message(args);
        return Err(error_value(format!(
            "testByText 失败{}: 期望 {:?} 实际 {:?} (可能原因：实现错误或数据格式不一致)",
            msg, expected, actual,
        )));
    }
    Ok(Value::Undefined)
}

/// bi_test_by_contains 断言实际值包含子串。
///
/// 用法：testByContains("hello world", "world")  // 通过
///       testByContains(resp["body"], "success")  // 通过
fn bi_test_by_contains(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 1, "testByContains")?;
    let actual = args[0].to_str();
    let substring = args[1].to_str();
    if !actual.contains(&substring) {
        let msg = extract_message(args);
        return Err(error_value(format!(
            "testByContains 失败{}: 期望包含 {:?}，实际 {:?} (可能原因：返回内容缺少预期片段)",
            msg, substring, actual,
        )));
    }
    Ok(Value::Undefined)
}

/// bi_test_by_reg 断言实际值匹配正则。
///
/// 用法：testByReg("abc123", `\d+`)  // 通过
///       testByReg(email, `^[\w.]+@[\w.]+$`)  // 通过
fn bi_test_by_reg(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 1, "testByReg")?;
    let actual = args[0].to_str();
    let pattern = args[1].to_str();
    let re = regex::Regex::new(&pattern).map_err(|e| {
        error_value(format!(
            "testByReg() 正则编译失败: {} (可能原因：正则语法错误，需转义特殊字符如 \\[ \\] \\( \\))",
            e,
        ))
    })?;
    if !re.is_match(&actual) {
        let msg = extract_message(args);
        return Err(error_value(format!(
            "testByReg 失败{}: 期望匹配 /{}/ ，实际 {:?} (可能原因：返回内容不符合预期模式)",
            msg, pattern, actual,
        )));
    }
    Ok(Value::Undefined)
}
