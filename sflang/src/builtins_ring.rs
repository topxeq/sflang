//! builtins_ring.rs — Ring 环形缓冲区内置函数
//!
//! 对标 Charlang/tkc 的 AnyQueue + StringRing + ByteQueue。
//! 用一个通用 Ring 类型替代三者，可存储任意 Value。

use std::sync::{Arc, Mutex};

use crate::builtins_helpers as bh;
use crate::ring::{ring_downcast, ring_value, Ring};
use crate::value::Value;
use crate::function::BuiltinDoc;

static DOC_NEWRING: BuiltinDoc = BuiltinDoc {
    category: "ring",
    signature: "newRing(size) -> ring",
    summary: "创建固定容量的环形缓冲区。",
    params: &[("size", "容量（int）")],
    returns: "ring 对象",
    examples: &["var r = newRing(100)"],
    errors: &[],
};

static DOC_RINGPUSH: BuiltinDoc = BuiltinDoc {
    category: "ring",
    signature: "ringPush(r, val) -> undefined",
    summary: "向环形缓冲写入值（满时覆盖最旧）。",
    params: &[("r", "ring 对象"), ("val", "值")],
    returns: "undefined",
    examples: &["ringPush(r, 42)"],
    errors: &[],
};

static DOC_RINGPOP: BuiltinDoc = BuiltinDoc {
    category: "ring",
    signature: "ringPop(r) -> value",
    summary: "弹出最新写入的值。",
    params: &[("r", "ring 对象")],
    returns: "值",
    examples: &["var v = ringPop(r)"],
    errors: &[],
};

static DOC_RINGPICK: BuiltinDoc = BuiltinDoc {
    category: "ring",
    signature: "ringPick(r) -> value",
    summary: "查看最新值（不弹出）。",
    params: &[("r", "ring 对象")],
    returns: "最新值",
    examples: &["ringPick(r)"],
    errors: &[],
};

static DOC_RINGGET: BuiltinDoc = BuiltinDoc {
    category: "ring",
    signature: "ringGet(r, idx) -> value",
    summary: "按索引读取元素。",
    params: &[("r", "ring 对象"), ("idx", "索引（int）")],
    returns: "值",
    examples: &["ringGet(r, 0)"],
    errors: &[],
};

static DOC_RINGSET: BuiltinDoc = BuiltinDoc {
    category: "ring",
    signature: "ringSet(r, idx, val) -> undefined",
    summary: "按索引设置元素。",
    params: &[("r", "ring 对象"), ("idx", "索引"), ("val", "值")],
    returns: "undefined",
    examples: &["ringSet(r, 0, 99)"],
    errors: &[],
};

static DOC_RINGINSERT: BuiltinDoc = BuiltinDoc {
    category: "ring",
    signature: "ringInsert(r, idx, val) -> undefined",
    summary: "在指定位置插入元素。",
    params: &[("r", "ring 对象"), ("idx", "索引"), ("val", "值")],
    returns: "undefined",
    examples: &["ringInsert(r, 0, 1)"],
    errors: &[],
};

static DOC_RINGREMOVE: BuiltinDoc = BuiltinDoc {
    category: "ring",
    signature: "ringRemove(r, idx) -> value",
    summary: "删除指定位置元素并返回。",
    params: &[("r", "ring 对象"), ("idx", "索引")],
    returns: "被删除的值",
    examples: &["ringRemove(r, 0)"],
    errors: &[],
};

static DOC_RINGSIZE: BuiltinDoc = BuiltinDoc {
    category: "ring",
    signature: "ringSize(r) -> int",
    summary: "返回当前元素数。",
    params: &[("r", "ring 对象")],
    returns: "int",
    examples: &["ringSize(r)"],
    errors: &[],
};

static DOC_RINGCLEAR: BuiltinDoc = BuiltinDoc {
    category: "ring",
    signature: "ringClear(r) -> undefined",
    summary: "清空环形缓冲。",
    params: &[("r", "ring 对象")],
    returns: "undefined",
    examples: &["ringClear(r)"],
    errors: &[],
};

static DOC_RINGTOLIST: BuiltinDoc = BuiltinDoc {
    category: "ring",
    signature: "ringToList(r) -> array",
    summary: "将环形缓冲转为数组。",
    params: &[("r", "ring 对象")],
    returns: "array",
    examples: &["var a = ringToList(r)"],
    errors: &[],
};

/// register 注册所有 Ring 相关内置函数。
pub fn register(vm: &mut crate::vm::VM) {
    vm.register_builtin_doc("newRing", bi_new_ring, &DOC_NEWRING);
    vm.register_builtin_doc("ringPush", bi_ring_push, &DOC_RINGPUSH);
    vm.register_builtin_doc("ringPop", bi_ring_pop, &DOC_RINGPOP);
    vm.register_builtin_doc("ringPick", bi_ring_pick, &DOC_RINGPICK);
    vm.register_builtin_doc("ringGet", bi_ring_get, &DOC_RINGGET);
    vm.register_builtin_doc("ringSet", bi_ring_set, &DOC_RINGSET);
    vm.register_builtin_doc("ringInsert", bi_ring_insert, &DOC_RINGINSERT);
    vm.register_builtin_doc("ringRemove", bi_ring_remove, &DOC_RINGREMOVE);
    vm.register_builtin_doc("ringSize", bi_ring_size, &DOC_RINGSIZE);
    vm.register_builtin_doc("ringClear", bi_ring_clear, &DOC_RINGCLEAR);
    vm.register_builtin_doc("ringToList", bi_ring_to_list, &DOC_RINGTOLIST);
}

/// bi_new_ring 创建环形缓冲区。
///
/// 用法：newRing(cap) 或 newRing()
/// - cap > 0：固定容量，超容量 Push 自动淘汰头部
/// - cap <= 0 或缺省：无限制（默认 10）
fn bi_new_ring(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    let cap = if args.is_empty() {
        10i64
    } else {
        bh::as_int(args, 0, "newRing")?
    };
    Ok(ring_value(Ring::new(cap)))
}

/// bi_ring_push 尾部追加元素，超容量淘汰头部。
///
/// 用法：ringPush(ring, value)
fn bi_ring_push(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "ringPush")?;
    bh::require_arg(args, 1, "ringPush")?;
    let ring = ring_downcast(&args[0], "ringPush")?;
    let v = args[1].clone();
    ring.lock().unwrap().push(v);
    Ok(Value::Undefined)
}

/// bi_ring_pop 取出尾部元素（删除）。空则返回 undefined。
///
/// 用法：ringPop(ring) → value 或 undefined
fn bi_ring_pop(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "ringPop")?;
    let ring = ring_downcast(&args[0], "ringPop")?;
    Ok(ring.lock().unwrap().pop().unwrap_or(Value::Undefined))
}

/// bi_ring_pick 取出头部元素（删除）。空则返回 undefined。
///
/// 用法：ringPick(ring) → value 或 undefined
fn bi_ring_pick(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "ringPick")?;
    let ring = ring_downcast(&args[0], "ringPick")?;
    Ok(ring.lock().unwrap().pick().unwrap_or(Value::Undefined))
}

/// bi_ring_get 查看指定位置元素（不删除）。
///
/// 用法：
///   ringGet(ring)       → 头部元素
///   ringGet(ring, idx)  → 指定位置（-1 取尾部）
/// 越界或空返回 undefined。
fn bi_ring_get(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "ringGet")?;
    let ring = ring_downcast(&args[0], "ringGet")?;
    let idx = if args.len() > 1 {
        bh::as_int(args, 1, "ringGet")?
    } else {
        0
    };
    Ok(ring.lock().unwrap().get(idx).unwrap_or(Value::Undefined))
}

/// bi_ring_set 修改指定位置的元素值。
///
/// 用法：ringSet(ring, idx, value) → bool（是否成功）
fn bi_ring_set(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "ringSet")?;
    bh::require_arg(args, 2, "ringSet")?;
    let ring = ring_downcast(&args[0], "ringSet")?;
    let idx = bh::as_int(args, 1, "ringSet")?;
    let v = args[2].clone();
    Ok(Value::Bool(ring.lock().unwrap().set(idx, v)))
}

/// bi_ring_insert 在指定位置插入元素。
///
/// 用法：ringInsert(ring, idx, value) → bool（是否成功）
/// 超容量时先淘汰尾部。
fn bi_ring_insert(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "ringInsert")?;
    bh::require_arg(args, 2, "ringInsert")?;
    let ring = ring_downcast(&args[0], "ringInsert")?;
    let idx = bh::as_int(args, 1, "ringInsert")?;
    let v = args[2].clone();
    Ok(Value::Bool(ring.lock().unwrap().insert(idx, v)))
}

/// bi_ring_remove 删除指定位置的元素。
///
/// 用法：ringRemove(ring, idx) → bool（是否成功）
fn bi_ring_remove(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "ringRemove")?;
    let ring = ring_downcast(&args[0], "ringRemove")?;
    let idx = bh::as_int(args, 1, "ringRemove")?;
    Ok(Value::Bool(ring.lock().unwrap().remove(idx)))
}

/// bi_ring_size 返回当前元素数量。
///
/// 用法：ringSize(ring) → int
fn bi_ring_size(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "ringSize")?;
    let ring = ring_downcast(&args[0], "ringSize")?;
    Ok(Value::Int(ring.lock().unwrap().size() as i64))
}

/// bi_ring_clear 清空所有元素。
///
/// 用法：ringClear(ring)
fn bi_ring_clear(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "ringClear")?;
    let ring = ring_downcast(&args[0], "ringClear")?;
    ring.lock().unwrap().clear();
    Ok(Value::Undefined)
}

/// bi_ring_to_list 转为数组（从头到尾顺序）。
///
/// 用法：ringToList(ring) → array
fn bi_ring_to_list(_vm: &mut crate::vm::VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "ringToList")?;
    let ring = ring_downcast(&args[0], "ringToList")?;
    let list = ring.lock().unwrap().to_list();
    Ok(Value::Array(Arc::new(Mutex::new(list))))
}
