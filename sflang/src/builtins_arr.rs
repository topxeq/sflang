//! builtins_arr.rs — 数组处理内置函数
//!
//! 设计要点（来自 AGENTS.md）：
//!   - 覆盖常见数组操作：排序、反转、查找、切片、拼接、增删
//!   - 排序采用稳定的"自然序"：纯数字按数值、纯字符串按字典序；混合时数字在前
//!   - 错误信息 AI 友好（复用 builtins_helpers）
//!
//! 函数列表：
//!   sort reverse contains indexOf slice concat insert remove
//!   appendArray removeItems shuffle

use std::sync::{Arc, Mutex};

use crate::builtins_helpers as bh;
use crate::function::BuiltinDoc;
use crate::value::Value;
use crate::vm::VM;

// ---- 数组函数文档 ----

static DOC_SORT: BuiltinDoc = BuiltinDoc {
    category: "array",
    signature: "sort(arr, descending?) -> array",
    summary: "原地按自然序排序（数字按数值、字符串按字典序；混合时数字在前），返回数组本身。",
    params: &[
        ("arr", "待排序的数组"),
        ("descending", "可选 bool：true 表示降序（默认升序）"),
    ],
    returns: "array：排序后的同一数组（原地修改）",
    examples: &[
        "sort([3, 1, 2])           → [1, 2, 3]",
        "sort([3, 1, 2], true)     → [3, 2, 1]（降序）",
        "sort([\"b\", \"a\"])       → [\"a\", \"b\"]",
    ],
    errors: &["第一个参数必须是数组"],
};

static DOC_SORT_BY_FUNC: BuiltinDoc = BuiltinDoc {
    category: "array",
    signature: "sortByFunc(arr, cmpFn) -> array",
    summary: "用自定义比较函数原地排序，返回数组本身。",
    params: &[
        ("arr", "待排序的数组"),
        ("cmpFn", "比较函数 (a, b) -> int：负数 a 在前、0 相等、正数 a 在后"),
    ],
    returns: "array：排序后的同一数组（原地修改）",
    examples: &[
        "sortByFunc([3,1,2], func(a,b){ return a - b })  → [1,2,3]（升序）",
        "sortByFunc([3,1,2], func(a,b){ return b - a })  → [3,2,1]（降序）",
    ],
    errors: &[
        "比较函数返回值必须可转为 int（负/零/正）",
        "比较函数在排序期间被逐对调用，避免在其中修改原数组",
    ],
};

static DOC_REVERSE: BuiltinDoc = BuiltinDoc {
    category: "array",
    signature: "reverse(arr) -> array | reverse(str) -> string",
    summary: "数组原地反转（返回本身）；字符串返回反转后的副本。",
    params: &[("arr", "数组（原地反转）或字符串（返回新串）")],
    returns: "array 数组本身；或 string 反转后的新字符串",
    examples: &[
        "reverse([1, 2, 3])    → [3, 2, 1]",
        "reverse(\"abc\")        → \"cba\"",
    ],
    errors: &["参数必须是 array 或 string"],
};

static DOC_CONTAINS: BuiltinDoc = BuiltinDoc {
    category: "array",
    signature: "contains(arr, x) -> bool | contains(str, sub) -> bool",
    summary: "数组判断是否含某元素（按值相等）；字符串判断是否含子串。",
    params: &[
        ("arr", "数组（元素匹配）或字符串（子串匹配）"),
        ("x", "数组：要查找的元素；字符串：要查找的子串"),
    ],
    returns: "bool：存在返回 true",
    examples: &[
        "contains([1, 2, 3], 2)   → true",
        "contains([1, 2, 3], 9)   → false",
        "contains(\"hello\", \"ell\") → true",
    ],
    errors: &["第一个参数必须是 array 或 string"],
};

static DOC_STR_CONTAINS: BuiltinDoc = BuiltinDoc {
    category: "array",
    signature: "strContains(str, sub) -> bool",
    summary: "判断字符串是否包含子串（contains 的 str 前缀别名，命名一致性）。",
    params: &[
        ("str", "被搜索的字符串"),
        ("sub", "要查找的子串"),
    ],
    returns: "bool：包含返回 true",
    examples: &[
        "strContains(\"hello\", \"ell\") → true",
        "strContains(\"hello\", \"xyz\") → false",
    ],
    errors: &["等价于 contains(str, sub)"],
};

static DOC_INDEX_OF: BuiltinDoc = BuiltinDoc {
    category: "array",
    signature: "indexOf(arr, x) -> int",
    summary: "查找元素首次出现的索引；未找到返回 -1。",
    params: &[
        ("arr", "待搜索的数组"),
        ("x", "要查找的元素"),
    ],
    returns: "int：首次出现的索引（从 0 起）；未找到返回 -1",
    examples: &[
        "indexOf([\"a\", \"b\", \"a\"], \"a\") → 0",
        "indexOf([1, 2, 3], 9)             → -1",
    ],
    errors: &["第一个参数必须是数组"],
};

static DOC_SLICE: BuiltinDoc = BuiltinDoc {
    category: "array",
    signature: "slice(arr, start, end?) -> array",
    summary: "返回子切片副本 [start, end)；end 省略取到末尾，负索引按距末端解释。",
    params: &[
        ("arr", "数组或 byteArray（类型一致返回）"),
        ("start", "起始索引（含），负数表示距末端"),
        ("end", "结束索引（不含），省略取到末尾，负数表示距末端"),
    ],
    returns: "array 或 byteArray：切片副本（类型与输入一致）；空范围为空集合",
    examples: &[
        "slice([1,2,3,4], 1, 3)  → [2, 3]",
        "slice([1,2,3,4], 2)     → [3, 4]",
        "slice([1,2,3,4], -2)    → [3, 4]",
    ],
    errors: &["索引越界会自动截断到有效范围，不报错"],
};

static DOC_CONCAT: BuiltinDoc = BuiltinDoc {
    category: "array",
    signature: "concat(arr1, arr2, ...) -> array",
    summary: "拼接多个数组为一个新数组（原数组不变）。",
    params: &[("arr1, arr2, ...", "至少一个数组；所有参数都必须是数组")],
    returns: "array：拼接后的新数组",
    examples: &[
        "concat([1,2], [3,4])        → [1, 2, 3, 4]",
        "concat([1], [2], [3])       → [1, 2, 3]",
        "concat([1,2])               → [1, 2]",
    ],
    errors: &["每个参数都必须是数组（不会自动展开非数组值）"],
};

static DOC_INSERT: BuiltinDoc = BuiltinDoc {
    category: "array",
    signature: "insert(arr, idx, val) -> array",
    summary: "在指定索引处插入元素（原地修改），返回数组本身。",
    params: &[
        ("arr", "待插入的数组"),
        ("idx", "插入位置；负数按距末端计算（-1 表示追加到末尾）"),
        ("val", "要插入的值"),
    ],
    returns: "array：插入后的同一数组（原地修改）",
    examples: &[
        "insert([1,2,3], 1, 9)   → [1, 9, 2, 3]",
        "insert([1,2,3], -1, 9)  → [1, 2, 3, 9]（-1 追加）",
    ],
    errors: &["越界索引会自动截断为有效位置（追加到末尾），不报错"],
};

static DOC_REMOVE: BuiltinDoc = BuiltinDoc {
    category: "array",
    signature: "remove(arr, idx) -> value",
    summary: "移除指定索引处的元素并返回它（原地修改）。",
    params: &[
        ("arr", "待移除元素的数组"),
        ("idx", "要移除的索引；负数表示距末端"),
    ],
    returns: "被移除的元素值",
    examples: &[
        "a := [1, 2, 3]; remove(a, 1)  → 2（a 变为 [1, 3]）",
        "a := [1, 2, 3]; remove(a, -1) → 3（a 变为 [1, 2]）",
    ],
    errors: &["索引越界（含负索引超出范围）返回 error"],
};

static DOC_APPEND_ARRAY: BuiltinDoc = BuiltinDoc {
    category: "array",
    signature: "appendArray(arr, items) -> array",
    summary: "批量追加到数组末尾（原地修改）：items 为数组则展开追加，否则作单个元素追加。",
    params: &[
        ("arr", "目标数组（原地修改）"),
        ("items", "数组：展开追加每个元素；其他值：作为单个元素追加"),
    ],
    returns: "array：修改后的同一数组",
    examples: &[
        "appendArray([1,2], [3,4])   → [1, 2, 3, 4]（展开）",
        "appendArray([1,2], 9)       → [1, 2, 9]（单值）",
    ],
    errors: &[
        "与 push 的区别：push 仅追加单个元素，appendArray 会展开数组批量追加",
    ],
};

static DOC_REMOVE_ITEMS: BuiltinDoc = BuiltinDoc {
    category: "array",
    signature: "removeItems(arr, start, count) -> array",
    summary: "从 start 索引开始移除 count 个元素（原地修改），返回数组本身。",
    params: &[
        ("arr", "目标数组（原地修改）"),
        ("start", "起始索引；负数按距末端计算"),
        ("count", "要移除的元素个数；超过可移除范围自动截断"),
    ],
    returns: "array：修改后的同一数组",
    examples: &[
        "removeItems([1,2,3,4,5], 1, 2)  → [1, 4, 5]",
        "removeItems([1,2,3,4,5], -2, 2) → [1, 2, 3]",
    ],
    errors: &[
        "count 不能为负数（参数顺序为 arr, start, count）",
        "start 越界时静默不移除（不报错）",
    ],
};

static DOC_SHUFFLE: BuiltinDoc = BuiltinDoc {
    category: "array",
    signature: "shuffle(arr) -> array",
    summary: "原地用 Fisher-Yates 算法随机打乱数组，返回数组本身。",
    params: &[("arr", "待打乱的数组（原地修改）")],
    returns: "array：打乱后的同一数组",
    examples: &[
        "shuffle([1, 2, 3, 4, 5])  → 顺序随机（元素不变）",
    ],
    errors: &["第一个参数必须是数组"],
};

/// register 注册所有数组内置函数到 VM。
pub fn register(vm: &mut VM) {
    vm.register_builtin_doc("sort", bi_sort, &DOC_SORT);
    vm.register_builtin_doc("sortByFunc", bi_sort_by_func, &DOC_SORT_BY_FUNC);
    vm.register_builtin_doc("reverse", bi_reverse, &DOC_REVERSE);
    vm.register_builtin_doc("contains", bi_contains, &DOC_CONTAINS);
    vm.register_builtin_doc("strContains", bi_contains, &DOC_STR_CONTAINS);
    vm.register_builtin_doc("indexOf", bi_index_of, &DOC_INDEX_OF);
    vm.register_builtin_doc("slice", bi_slice, &DOC_SLICE);
    vm.register_builtin_doc("concat", bi_concat, &DOC_CONCAT);
    vm.register_builtin_doc("insert", bi_insert, &DOC_INSERT);
    vm.register_builtin_doc("remove", bi_remove, &DOC_REMOVE);
    vm.register_builtin_doc("appendArray", bi_append_array, &DOC_APPEND_ARRAY);
    vm.register_builtin_doc("removeItems", bi_remove_items, &DOC_REMOVE_ITEMS);
    vm.register_builtin_doc("shuffle", bi_shuffle, &DOC_SHUFFLE);
}

/// sort_key 为元素生成可比较的排序键。
///
/// 约定：数字（Int/Float）映射为 (0, f64)；字符串映射为 (1, str)；
/// 其他类型映射为 (2, inspect 串)。同类型之间可直接比较，跨类型按组别稳定排序。
fn sort_key(v: &Value) -> (u8, Option<f64>, String) {
    match v {
        Value::Int(i) => (0, Some(*i as f64), String::new()),
        Value::Float(f) => (0, Some(*f), String::new()),
        Value::Str(s) => (1, None, s.as_ref().to_string()),
        other => (2, None, other.to_str()),
    }
}

/// compare 按 sort_key 比较，返回 std::cmp::Ordering。
fn compare(a: &Value, b: &Value) -> std::cmp::Ordering {
    let ka = sort_key(a);
    let kb = sort_key(b);
    ka.0.cmp(&kb.0).then_with(|| {
        if ka.0 == 0 {
            // 数字组：按 f64 全序比较
            ka.1.unwrap_or(0.0).partial_cmp(&kb.1.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        } else {
            // 字符串/其他组：按字典序
            ka.2.cmp(&kb.2)
        }
    })
}

/// bi_sort 排序（原地，返回排序后的同一数组）。
///
/// 第二个可选参数为布尔值：true 表示降序。
fn bi_sort(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let arr = bh::as_array(args, 0, "sort")?;
    let descending = match args.get(1) {
        Some(Value::Bool(b)) => *b,
        _ => false,
    };
    let mut guard = arr.lock().unwrap();
    guard.sort_by(|a, b| {
        let ord = compare(a, b);
        if descending {
            ord.reverse()
        } else {
            ord
        }
    });
    // 返回数组本身（克隆 Rc，保持引用一致）
    Ok(args[0].clone())
}

/// bi_sort_by_func 用自定义比较函数排序（原地）。
///
/// 比较函数接收两个参数 (a, b)，返回 int：
///   负数 → a 排在 b 前面
///   0   → 相等
///   正数 → a 排在 b 后面
///
/// 用法：sortByFunc(arr, func(a, b) { return a - b })  // 升序
fn bi_sort_by_func(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let arr = bh::as_array(args, 0, "sortByFunc")?;
    bh::require_arg(args, 1, "sortByFunc")?;
    let cmp_fn = args[1].clone();
    // 先克隆快照，逐对调用比较函数，再排序
    // 不能在持锁状态下回调 VM（死锁风险），所以先取快照索引排序
    let snapshot: Vec<Value> = arr.lock().unwrap().clone();
    let n = snapshot.len();
    // 用索引排序避免频繁 clone
    let mut indices: Vec<usize> = (0..n).collect();
    let result = indices.sort_by(|&i, &j| {
        let a = snapshot[i].clone();
        let b = snapshot[j].clone();
        match vm.call_function_value(cmp_fn.clone(), vec![a, b]) {
            Ok(v) => {
                let r = match v {
                    Value::Int(x) => x,
                    Value::Float(f) => f as i64,
                    Value::Byte(b) => b as i64,
                    _ => 0,
                };
                if r < 0 { std::cmp::Ordering::Less }
                else if r > 0 { std::cmp::Ordering::Greater }
                else { std::cmp::Ordering::Equal }
            }
            Err(_) => std::cmp::Ordering::Equal,
        }
    });
    let _ = result;  // sort_by 返回 ()
    // 按排序后的索引重排数组
    let sorted: Vec<Value> = indices.iter().map(|&i| snapshot[i].clone()).collect();
    let mut guard = arr.lock().unwrap();
    *guard = sorted;
    Ok(args[0].clone())
}
fn bi_reverse(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    match args.get(0) {
        Some(Value::Array(_)) => {
            let arr = bh::as_array(args, 0, "reverse")?;
            arr.lock().unwrap().reverse();
            Ok(args[0].clone())
        }
        // 字符串分发到字符串模块实现
        Some(Value::Str(_)) => crate::builtins_str::bi_reverse_str(_vm, args),
        Some(v) => Err(crate::builtins_helpers::err_type(
            "reverse",
            0,
            "array 或 string",
            v.type_code(),
            "reverse 支持数组（原地）和字符串（返回副本）",
        )),
        None => Err(crate::builtins_helpers::err_argc("reverse", 1, args.len())),
    }
}

/// bi_contains 多态判断是否包含。数组按元素相等，字符串按子串。
fn bi_contains(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    match args.get(0) {
        Some(Value::Array(_)) => {
            let arr = bh::as_array(args, 0, "contains")?;
            bh::require_arg(args, 1, "contains")?;
            let target = &args[1];
            let found = arr.lock().unwrap().iter().any(|v| v.equals(target));
            Ok(Value::Bool(found))
        }
        // 字符串分发到字符串模块实现
        Some(Value::Str(_)) => crate::builtins_str::bi_contains_str(_vm, args),
        Some(v) => Err(crate::builtins_helpers::err_type(
            "contains",
            0,
            "array 或 string",
            v.type_code(),
            "contains 支持数组（元素匹配）和字符串（子串匹配）",
        )),
        None => Err(crate::builtins_helpers::err_argc("contains", 1, args.len())),
    }
}

/// bi_index_of 查找元素首次出现的索引；未找到返回 -1。
fn bi_index_of(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let arr = bh::as_array(args, 0, "indexOf")?;
    bh::require_arg(args, 1, "indexOf")?;
    let target = &args[1];
    let pos = arr.lock().unwrap().iter().position(|v| v.equals(target));
    Ok(Value::Int(pos.map(|i| i as i64).unwrap_or(-1)))
}

/// bi_slice 返回子数组切片副本 [start, end)（字符/元素索引）。
///
/// end 省略时取到末尾；负数索引按"距末端"解释。
fn bi_slice(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "slice")?;
    // byteArray 切片：返回 byteArray（类型一致，便于后续就地修改）
    if let Value::ByteArray(b) = &args[0] {
        let guard = b.lock().unwrap();
        let len = guard.len() as i64;
        let mut start = bh::as_int(args, 1, "slice")?;
        let mut end = if args.len() > 2 { bh::as_int(args, 2, "slice")? } else { len };
        if start < 0 { start += len; }
        if end < 0 { end += len; }
        if start < 0 { start = 0; }
        if end > len { end = len; }
        if start >= end {
            return Ok(Value::ByteArray(Arc::new(Mutex::new(Vec::new()))));
        }
        let part: Vec<u8> = guard[(start as usize)..(end as usize)].to_vec();
        return Ok(Value::ByteArray(Arc::new(Mutex::new(part))));
    }
    // 默认：数组切片
    let arr = bh::as_array(args, 0, "slice")?;
    let guard = arr.lock().unwrap();
    let len = guard.len() as i64;
    let mut start = bh::as_int(args, 1, "slice")?;
    let mut end = if args.len() > 2 {
        bh::as_int(args, 2, "slice")?
    } else {
        len
    };
    if start < 0 {
        start += len;
    }
    if end < 0 {
        end += len;
    }
    if start < 0 {
        start = 0;
    }
    if end > len {
        end = len;
    }
    if start >= end {
        return Ok(Value::Array(Arc::new(Mutex::new(Vec::new()))));
    }
    let part: Vec<Value> = guard[(start as usize)..(end as usize)].to_vec();
    Ok(Value::Array(Arc::new(Mutex::new(part))))
}

/// bi_concat 拼接多个数组为一个新数组。
///
/// 所有参数须为数组；返回新数组，原数组不变。
fn bi_concat(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let mut out = Vec::new();
    for i in 0..args.len() {
        let a = bh::as_array(args, i, "concat")?;
        out.extend(a.lock().unwrap().iter().cloned());
    }
    Ok(Value::Array(Arc::new(Mutex::new(out))))
}

/// bi_insert 在指定索引处插入元素（原地）。
///
/// 索引可为负（距末端）；越界则追加到末尾。
fn bi_insert(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let arr = bh::as_array(args, 0, "insert")?;
    let mut idx = bh::as_int(args, 1, "insert")?;
    bh::require_arg(args, 2, "insert")?;
    let val = args[2].clone();
    let mut guard = arr.lock().unwrap();
    let len = guard.len() as i64;
    if idx < 0 {
        idx += len + 1; // 允许在末尾插入：-1 表示追加
    }
    if idx < 0 {
        idx = 0;
    }
    let pos = (idx as usize).min(guard.len());
    guard.insert(pos, val);
    Ok(args[0].clone())
}

/// bi_remove 移除指定索引处的元素并返回它。
fn bi_remove(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let arr = bh::as_array(args, 0, "remove")?;
    let mut idx = bh::as_int(args, 1, "remove")?;
    let mut guard = arr.lock().unwrap();
    let len = guard.len() as i64;
    if idx < 0 {
        idx += len;
    }
    if idx < 0 || idx >= len {
        return Err(crate::value::error_value(format!(
            "remove() 索引越界：{} (数组长度 {}，可能原因：负索引超出范围或索引过大)",
            idx, len,
        )));
    }
    Ok(guard.remove(idx as usize))
}

/// bi_shuffle 原地随机打乱数组（Fisher-Yates 洗牌）。
fn bi_shuffle(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let arr = bh::as_array(args, 0, "shuffle")?;
    let mut guard = arr.lock().unwrap();
    let n = guard.len();
    // Fisher-Yates：从末尾向前，与随机位置交换
    for i in (1..n).rev() {
        let j = (crate::builtins_math::next_rand() as usize) % (i + 1);
        guard.swap(i, j);
    }
    Ok(args[0].clone())
}

/// bi_append_array 批量追加元素到数组末尾（原地修改）。
///
/// 用法：appendArray(arr, items)
///   - items 为数组：将其所有元素逐一追加到 arr 末尾
///   - items 为其他值：作为单个元素追加
///
/// 与 push 的区别：push 仅追加单个元素；appendArray 支持批量展开数组。
/// 返回修改后的数组本身（与 push 行为一致，便于链式调用）。
fn bi_append_array(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let arr = bh::as_array(args, 0, "appendArray")?;
    bh::require_arg(args, 1, "appendArray")?;

    match &args[1] {
        // 数组：展开追加
        Value::Array(src) => {
            let items: Vec<Value> = src.lock().unwrap().clone();
            arr.lock().unwrap().extend(items);
        }
        // 其他值：作为单个元素追加
        other => {
            arr.lock().unwrap().push(other.clone());
        }
    }
    Ok(args[0].clone())
}

/// bi_remove_items 范围移除（原地修改）。
///
/// 用法：removeItems(arr, start, count)
///   - 从 start 索引开始移除 count 个元素
///   - start 可为负数（距末端的偏移，与 slice 一致）
///   - count 超出可移除范围时自动截断到实际可移除数量
///
/// 返回修改后的数组本身。
fn bi_remove_items(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let arr = bh::as_array(args, 0, "removeItems")?;
    let mut start = bh::as_int(args, 1, "removeItems")?;
    let count = bh::as_int(args, 2, "removeItems")?;

    if count < 0 {
        return Err(crate::value::error_value(format!(
            "removeItems() count 不能为负数: {} (可能原因：参数顺序错误，正确顺序 removeItems(arr, start, count))",
            count,
        )));
    }

    let mut guard = arr.lock().unwrap();
    let len = guard.len() as i64;

    // 负索引：从末尾计算
    if start < 0 {
        start += len;
    }
    if start < 0 {
        // 起点仍为负：什么都不移除
        return Ok(args[0].clone());
    }
    if start >= len {
        // 起点越界：什么都不移除
        return Ok(args[0].clone());
    }

    // 计算实际可移除范围，防止 drain 越界
    let start_usize = start as usize;
    let available = (len - start) as usize;
    let remove_count = (count as usize).min(available);

    if remove_count > 0 {
        guard.drain(start_usize..(start_usize + remove_count));
    }

    Ok(args[0].clone())
}
