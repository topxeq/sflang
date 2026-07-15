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
use crate::function::BuiltinDoc;

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

static DOC_NEWSTACK: BuiltinDoc = BuiltinDoc {
    category: "containers",
    signature: "newStack() -> stack",
    summary: "创建栈（后进先出 LIFO）。",
    params: &[],
    returns: "stack 对象",
    examples: &["var s = newStack()"],
    errors: &[],
};

static DOC_STACKPUSH: BuiltinDoc = BuiltinDoc {
    category: "containers",
    signature: "stackPush(s, val) -> undefined",
    summary: "入栈。",
    params: &[("s", "stack 对象"), ("val", "要压入的值")],
    returns: "undefined",
    examples: &["stackPush(s, 42)"],
    errors: &[],
};

static DOC_STACKPOP: BuiltinDoc = BuiltinDoc {
    category: "containers",
    signature: "stackPop(s) -> value",
    summary: "出栈（弹出栈顶元素）。",
    params: &[("s", "stack 对象")],
    returns: "栈顶值",
    examples: &["var v = stackPop(s)"],
    errors: &["空栈 pop 会返回 undefined 或报错"],
};

static DOC_STACKPEEK: BuiltinDoc = BuiltinDoc {
    category: "containers",
    signature: "stackPeek(s) -> value",
    summary: "查看栈顶（不出栈）。",
    params: &[("s", "stack 对象")],
    returns: "栈顶值",
    examples: &["var v = stackPeek(s)"],
    errors: &[],
};

static DOC_STACKLEN: BuiltinDoc = BuiltinDoc {
    category: "containers",
    signature: "stackLen(s) -> int",
    summary: "返回栈元素数。",
    params: &[("s", "stack 对象")],
    returns: "int",
    examples: &["stackLen(s)"],
    errors: &[],
};

static DOC_STACKCLEAR: BuiltinDoc = BuiltinDoc {
    category: "containers",
    signature: "stackClear(s) -> undefined",
    summary: "清空栈。",
    params: &[("s", "stack 对象")],
    returns: "undefined",
    examples: &["stackClear(s)"],
    errors: &[],
};

static DOC_NEWQUEUE: BuiltinDoc = BuiltinDoc {
    category: "containers",
    signature: "newQueue() -> queue",
    summary: "创建队列（先进先出 FIFO）。",
    params: &[],
    returns: "queue 对象",
    examples: &["var q = newQueue()"],
    errors: &[],
};

static DOC_QUEUEPUSH: BuiltinDoc = BuiltinDoc {
    category: "containers",
    signature: "queuePush(q, val) -> undefined",
    summary: "入队。",
    params: &[("q", "queue 对象"), ("val", "值")],
    returns: "undefined",
    examples: &["queuePush(q, 1)"],
    errors: &[],
};

static DOC_QUEUEPOP: BuiltinDoc = BuiltinDoc {
    category: "containers",
    signature: "queuePop(q) -> value",
    summary: "出队（弹出队首元素）。",
    params: &[("q", "queue 对象")],
    returns: "队首值",
    examples: &["var v = queuePop(q)"],
    errors: &[],
};

static DOC_QUEUEPEEK: BuiltinDoc = BuiltinDoc {
    category: "containers",
    signature: "queuePeek(q) -> value",
    summary: "查看队首（不出队）。",
    params: &[("q", "queue 对象")],
    returns: "队首值",
    examples: &["queuePeek(q)"],
    errors: &[],
};

static DOC_QUEUELEN: BuiltinDoc = BuiltinDoc {
    category: "containers",
    signature: "queueLen(q) -> int",
    summary: "返回队列元素数。",
    params: &[("q", "queue 对象")],
    returns: "int",
    examples: &["queueLen(q)"],
    errors: &[],
};

static DOC_QUEUECLEAR: BuiltinDoc = BuiltinDoc {
    category: "containers",
    signature: "queueClear(q) -> undefined",
    summary: "清空队列。",
    params: &[("q", "queue 对象")],
    returns: "undefined",
    examples: &["queueClear(q)"],
    errors: &[],
};

/// register 注册 Stack 和 Queue 相关内置函数。
pub fn register(vm: &mut VM) {
    // Stack
    vm.register_builtin_doc("newStack", bi_new_stack, &DOC_NEWSTACK);
    vm.register_builtin_doc("stackPush", bi_stack_push, &DOC_STACKPUSH);
    vm.register_builtin_doc("stackPop", bi_stack_pop, &DOC_STACKPOP);
    vm.register_builtin_doc("stackPeek", bi_stack_peek, &DOC_STACKPEEK);
    vm.register_builtin_doc("stackLen", bi_stack_len, &DOC_STACKLEN);
    vm.register_builtin_doc("stackClear", bi_stack_clear, &DOC_STACKCLEAR);

    // Queue
    vm.register_builtin_doc("newQueue", bi_new_queue, &DOC_NEWQUEUE);
    vm.register_builtin_doc("queuePush", bi_queue_push, &DOC_QUEUEPUSH);
    vm.register_builtin_doc("queuePop", bi_queue_pop, &DOC_QUEUEPOP);
    vm.register_builtin_doc("queuePeek", bi_queue_peek, &DOC_QUEUEPEEK);
    vm.register_builtin_doc("queueLen", bi_queue_len, &DOC_QUEUELEN);
    vm.register_builtin_doc("queueClear", bi_queue_clear, &DOC_QUEUECLEAR);
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
