//! builtins_containers.rs — 容器类型内置函数（Stack / Queue）
//!
//! 提供脚本级容器类型，语义清晰，区别于普通数组：
//!   - Stack：后进先出（LIFO），push/pop/peek/len/clear
//!   - Queue：先进先出（FIFO），可设容量上限，push/pop/peek/len/clear
//!
//! # 实现说明
//!
//! Stack 和 Queue 用 Native 包装 Arc<Mutex<...>>，与 Sflang 容器线程安全风格一致。
//! Queue 的容量上限为可选，push 时超限返回 error。

use std::sync::{Arc, Mutex};

use crate::value::{Value, error_value};
use crate::vm::VM;

// ===========================================================================
// Stack 类型
// ===========================================================================

/// StackT 栈内部状态（LIFO）。
pub struct StackT {
    items: Mutex<Vec<Value>>,
}

/// new_stack 创建栈对象。
pub fn new_stack() -> Arc<StackT> {
    Arc::new(StackT { items: Mutex::new(Vec::new()) })
}

/// extract_stack 从 Value 提取 StackT 引用。
fn extract_stack<'a>(v: &'a Value, fn_name: &str) -> Result<&'a Arc<StackT>, Value> {
    match v {
        Value::Native(n) => n.downcast_ref::<Arc<StackT>>().ok_or_else(|| {
            error_value(format!(
                "{}() 参数应为 stack 对象，得到 {} (可能原因：传入了其他类型或参数顺序错误)",
                fn_name, v.type_name_ex(),
            ))
        }),
        _ => Err(error_value(format!(
            "{}() 参数应为 stack 对象，得到 {}",
            fn_name, v.type_name(),
        ))),
    }
}

// ===========================================================================
// Queue 类型
// ===========================================================================

/// QueueT 队列内部状态（FIFO）。
///
/// capacity 为 0 表示无上限。
pub struct QueueT {
    items: Mutex<Vec<Value>>,
    capacity: usize,
}

/// new_queue 创建队列对象。
pub fn new_queue(capacity: usize) -> Arc<QueueT> {
    Arc::new(QueueT {
        items: Mutex::new(Vec::new()),
        capacity,
    })
}

/// extract_queue 从 Value 提取 QueueT 引用。
fn extract_queue<'a>(v: &'a Value, fn_name: &str) -> Result<&'a Arc<QueueT>, Value> {
    match v {
        Value::Native(n) => n.downcast_ref::<Arc<QueueT>>().ok_or_else(|| {
            error_value(format!(
                "{}() 参数应为 queue 对象，得到 {} (可能原因：传入了其他类型或参数顺序错误)",
                fn_name, v.type_name_ex(),
            ))
        }),
        _ => Err(error_value(format!(
            "{}() 参数应为 queue 对象，得到 {}",
            fn_name, v.type_name(),
        ))),
    }
}

// ===========================================================================
// 注册
// ===========================================================================

/// register 注册 Stack 和 Queue 相关内置函数。
pub fn register(vm: &mut VM) {
    // Stack
    vm.register_builtin("newStack", bi_new_stack);
    vm.register_builtin("stackPush", bi_stack_push);
    vm.register_builtin("stackPop", bi_stack_pop);
    vm.register_builtin("stackPeek", bi_stack_peek);
    vm.register_builtin("stackLen", bi_stack_len);
    vm.register_builtin("stackClear", bi_stack_clear);

    // Queue
    vm.register_builtin("newQueue", bi_new_queue);
    vm.register_builtin("queuePush", bi_queue_push);
    vm.register_builtin("queuePop", bi_queue_pop);
    vm.register_builtin("queuePeek", bi_queue_peek);
    vm.register_builtin("queueLen", bi_queue_len);
    vm.register_builtin("queueClear", bi_queue_clear);
}

// ===========================================================================
// Stack 内置函数
// ===========================================================================

/// bi_new_stack 创建空栈。
fn bi_new_stack(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Native(Arc::new(new_stack())))
}

/// bi_stack_push 压入元素到栈顶。
fn bi_stack_push(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let stack = extract_stack(&args[0], "stackPush")?;
    if args.len() < 2 {
        return Err(error_value("stackPush() 需要 2 个参数 (stack, value)"));
    }
    stack.items.lock().unwrap().push(args[1].clone());
    Ok(args[0].clone())
}

/// bi_stack_pop 弹出栈顶元素并返回；空栈返回 undefined。
fn bi_stack_pop(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let stack = extract_stack(&args[0], "stackPop")?;
    let mut guard = stack.items.lock().unwrap();
    Ok(guard.pop().unwrap_or(Value::Undefined))
}

/// bi_stack_peek 查看栈顶元素但不弹出；空栈返回 undefined。
fn bi_stack_peek(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let stack = extract_stack(&args[0], "stackPeek")?;
    let guard = stack.items.lock().unwrap();
    Ok(guard.last().cloned().unwrap_or(Value::Undefined))
}

/// bi_stack_len 返回栈中元素数量。
fn bi_stack_len(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let stack = extract_stack(&args[0], "stackLen")?;
    Ok(Value::Int(stack.items.lock().unwrap().len() as i64))
}

/// bi_stack_clear 清空栈。
fn bi_stack_clear(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let stack = extract_stack(&args[0], "stackClear")?;
    stack.items.lock().unwrap().clear();
    Ok(Value::Undefined)
}

// ===========================================================================
// Queue 内置函数
// ===========================================================================

/// bi_new_queue 创建空队列。
///
/// 用法：newQueue() 无上限，或 newQueue(capacity) 设置容量上限。
fn bi_new_queue(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let cap = match args.get(0) {
        Some(Value::Int(c)) => {
            if *c < 0 {
                return Err(error_value(format!(
                    "newQueue() 容量不能为负数: {}", c,
                )));
            }
            *c as usize
        }
        _ => 0,
    };
    Ok(Value::Native(Arc::new(new_queue(cap))))
}

/// bi_queue_push 入队元素。
///
/// 若设置了容量上限且已满，返回 error。
fn bi_queue_push(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let queue = extract_queue(&args[0], "queuePush")?;
    if args.len() < 2 {
        return Err(error_value("queuePush() 需要 2 个参数 (queue, value)"));
    }
    let mut guard = queue.items.lock().unwrap();
    if queue.capacity > 0 && guard.len() >= queue.capacity {
        return Err(error_value(format!(
            "queuePush() 队列已满 (容量 {}，当前 {})",
            queue.capacity, guard.len(),
        )));
    }
    guard.push(args[1].clone());
    Ok(args[0].clone())
}

/// bi_queue_pop 出队元素并返回；空队返回 undefined。
fn bi_queue_pop(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let queue = extract_queue(&args[0], "queuePop")?;
    let mut guard = queue.items.lock().unwrap();
    if guard.is_empty() {
        Ok(Value::Undefined)
    } else {
        Ok(guard.remove(0))
    }
}

/// bi_queue_peek 查看队首元素但不出队；空队返回 undefined。
fn bi_queue_peek(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let queue = extract_queue(&args[0], "queuePeek")?;
    let guard = queue.items.lock().unwrap();
    Ok(guard.first().cloned().unwrap_or(Value::Undefined))
}

/// bi_queue_len 返回队列中元素数量。
fn bi_queue_len(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let queue = extract_queue(&args[0], "queueLen")?;
    Ok(Value::Int(queue.items.lock().unwrap().len() as i64))
}

/// bi_queue_clear 清空队列。
fn bi_queue_clear(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let queue = extract_queue(&args[0], "queueClear")?;
    queue.items.lock().unwrap().clear();
    Ok(Value::Undefined)
}
