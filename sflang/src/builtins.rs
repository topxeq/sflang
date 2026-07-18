//! builtins.rs — 内置函数
//!
//! 设计要点：
//!   - 提供丰富的内置函数，覆盖常见编程任务
//!   - 错误信息包含可能原因（AI 友好）
//!   - 类型校验给出清晰提示

use std::sync::{Arc, Mutex};

use crate::function::BuiltinDoc;
use crate::value::Value;
use crate::vm::VM;

// ---- 核心内置函数文档（help 系统第一批）----

/// DOC_PRINTLN println 的文档。
static DOC_PRINTLN: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "println(...) -> undefined",
    summary: "打印参数（空格分隔）并换行，输出到标准输出。",
    params: &[("...", "任意类型，自动转为字符串（多个用空格分隔）")],
    returns: "undefined（无返回值）",
    examples: &[
        "println(\"hello\")              → hello",
        "println(\"a\", 1, true)         → a 1 true",
        "println(1 + 2)                 → 3",
    ],
    errors: &[],
};

/// DOC_LEN len 的文档。
static DOC_LEN: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "len(x) -> int",
    summary: "返回容器或字符串的长度/元素数。",
    params: &[
        ("x", "string（字符数）/ array / object / map / bytes（字节数）"),
    ],
    returns: "int 长度值",
    examples: &[
        "len(\"hello\")        → 5",
        "len([1,2,3])         → 3",
        "len({\"a\":1,\"b\":2}) → 2",
    ],
    errors: &[
        "对不可计长度的类型（如 int）会返回错误",
    ],
};

/// DOC_TYPECODE typeCode 的文档。
static DOC_TYPECODE: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "typeCode(x) -> int",
    summary: "返回值的固定类型编码（数字），用于快速类型判断。",
    params: &[("x", "任意值")],
    returns: "int 类型编码：0=undefined 1=int 2=float 3=bool 4=string 5=bytes 6=array 7=object 8=function 9=builtin 10=error 13=bigInt 14=bigFloat 15=datetime 16=file 17=byte 18=map 19=stringBuilder",
    examples: &[
        "typeCode(42)         → 1",
        "typeCode(\"hi\")      → 4",
        "typeCode(undefined)  → 0",
    ],
    errors: &[],
};

/// DOC_TYPENAME typeName 的文档。
static DOC_TYPENAME: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "typeName(x) -> string",
    summary: "返回值的类型名字符串（如 \"int\"、\"string\"、\"array\"）。",
    params: &[("x", "任意值")],
    returns: "string 类型名",
    examples: &[
        "typeName(42)         → int",
        "typeName(\"hi\")      → string",
        "typeName([1,2])      → array",
    ],
    errors: &[],
};

/// DOC_HELP help 自身的文档。
static DOC_HELP: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "help([name]) -> string",
    summary: "查阅内置函数文档。无参列出全部分类，有参返回函数详情或分类函数列表。",
    params: &[
        ("name", "可选。函数名（如 \"regFind\"）或分类名（如 \"regex\"）"),
    ],
    returns: "string 多行文档或分类列表",
    examples: &[
        "help()             → 列出所有内置函数分类",
        "help(\"regFind\")    → regFind 的签名/参数/示例/常见错误",
        "help(\"regex\")      → 列出 regex 分类下所有函数",
    ],
    errors: &[
        "函数名拼写错误时返回错误；help() 会自动给出相似函数建议",
    ],
};

static DOC_RANGE: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "range(start, end[, step]) -> array<int>",
    summary: "生成整数数组（含 start 不含 end）。step 默认 1，可为负数。",
    params: &[
        ("start", "起始值（含）"),
        ("end", "结束值（不含）"),
        ("step", "可选。步长，默认 1；负数用于递减"),
    ],
    returns: "array<int> 整数数组",
    examples: &[
        "range(1, 5)          → [1, 2, 3, 4]",
        "range(0, 10, 2)      → [0, 2, 4, 6, 8]",
        "range(5, 0, -1)      → [5, 4, 3, 2, 1]",
    ],
    errors: &[],
};

static DOC_KEYS: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "keys(obj) -> array<string>",
    summary: "返回 object/map 的所有键（无序）。",
    params: &[("obj", "object 或 map")],
    returns: "array<string> 键名数组",
    examples: &[
        "keys({\"a\":1, \"b\":2})  → [\"a\", \"b\"]（顺序不保证）",
    ],
    errors: &["对非 object/map 类型会返回错误"],
};

static DOC_VALUES: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "values(obj) -> array",
    summary: "返回 object/map 的所有值（无序）。",
    params: &[("obj", "object 或 map")],
    returns: "array 值数组",
    examples: &["values({\"a\":1, \"b\":2})  → [1, 2]"],
    errors: &[],
};

static DOC_PUSH: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "push(arr, val) -> int",
    summary: "向数组末尾添加元素，返回新长度。",
    params: &[
        ("arr", "目标数组（原地修改）"),
        ("val", "要添加的值"),
    ],
    returns: "int 添加后的数组长度",
    examples: &[
        "var a = [1, 2]; push(a, 3)  → 3; a 变为 [1, 2, 3]",
    ],
    errors: &["第一个参数应为 array"],
};

static DOC_SPRINTF: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "sprintf(fmt, args...) -> string",
    summary: "格式化字符串（Go 风格占位符：%v %d %s %f %t %x %%）。",
    params: &[
        ("fmt", "格式字符串，含 %v(通用) %d(整数) %s(字符串) %f(浮点) %t(布尔) %x(十六进制) %%(字面%)"),
        ("args", "对应占位符的参数（可变）"),
    ],
    returns: "string 格式化后的字符串",
    examples: &[
        "sprintf(\"%s=%d\", \"count\", 42)   → \"count=42\"",
        "sprintf(\"%.2f\", 3.14159)         → \"3.14\"",
    ],
    errors: &["占位符数量与参数数量不匹配会返回错误"],
};

static DOC_SLEEP: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "sleep(seconds) -> undefined",
    summary: "休眠指定秒数（支持小数，如 0.5）。",
    params: &[("seconds", "休眠时长（秒，float 或 int）")],
    returns: "undefined",
    examples: &[
        "sleep(1)     // 休眠 1 秒",
        "sleep(0.5)   // 休眠 500 毫秒",
    ],
    errors: &[],
};

static DOC_ERROR: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "error(msg) -> error",
    summary: "创建错误值（用于返回错误，不抛异常）。",
    params: &[("msg", "错误信息字符串")],
    returns: "error 错误值",
    examples: &[
        "func divide(a, b) { if b == 0 { return error(\"除零\") }; return a / b }",
    ],
    errors: &[],
};

static DOC_IS_ERROR: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "isError(v) -> bool",
    summary: "判断值是否为 error 类型。",
    params: &[("v", "任意值")],
    returns: "bool：是 error 返回 true",
    examples: &[
        "isError(error(\"x\"))   → true",
        "isError(42)             → false",
    ],
    errors: &[],
};

static DOC_DEFAULT_VAL: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "defaultVal(x, d) -> value",
    summary: "宽松兜底：x 为 falsy 时返回 d，否则返回 x（等价于 x || d）。",
    params: &[
        ("x", "原值"),
        ("d", "兜底值"),
    ],
    returns: "x 为 truthy 时返回 x，否则返回 d",
    examples: &[
        "defaultVal(undefined, 99)  → 99",
        "defaultVal(0, 99)          → 99（0 也是 falsy）",
        "defaultVal(\"hi\", \"x\")     → \"hi\"",
    ],
    errors: &["若只需对 undefined 兜底（不触发 0/\"\"），用 defaultUndef 或 ?? 运算符"],
};

static DOC_DEFAULT_UNDEF: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "defaultUndef(x, d) -> value",
    summary: "严格空合并：仅当 x 为 undefined 时返回 d，否则返回 x（等价于 x ?? d）。",
    params: &[
        ("x", "原值"),
        ("d", "兜底值"),
    ],
    returns: "x 非 undefined 时返回 x，否则返回 d",
    examples: &[
        "defaultUndef(undefined, 99)  → 99",
        "defaultUndef(0, 99)          → 0（0 不触发兜底）",
        "defaultUndef(\"\", 99)         → \"\"（空串不触发兜底）",
    ],
    errors: &[],
};

static DOC_ASSERT: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "assert(cond[, msg]) -> undefined",
    summary: "断言：cond 为 false 时抛异常。",
    params: &[
        ("cond", "条件（truthy 通过，falsy 抛异常）"),
        ("msg", "可选。失败时的错误信息"),
    ],
    returns: "undefined（通过时）",
    examples: &[
        "assert(x > 0)",
        "assert(x > 0, \"x 必须为正数\")",
    ],
    errors: &["断言失败时抛异常（不是返回 error 值）"],
};

static DOC_UUID: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "uuid() -> string",
    summary: "生成随机 UUID 字符串（v4 格式，36 字符含连字符）。",
    params: &[],
    returns: "string UUID，如 \"550e8400-e29b-41d4-a716-446655440000\"",
    examples: &["var id = uuid()   // 每次不同"],
    errors: &[],
};

static DOC_DEEP_CLONE: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "deepClone(v) -> value",
    summary: "深拷贝值（递归克隆 array/object/map 及其嵌套结构）。",
    params: &[("v", "要拷贝的值")],
    returns: "深拷贝后的新值（与原值不共享引用）",
    examples: &[
        "var a = [1, [2, 3]]",
        "var b = deepClone(a)    // 修改 b 不影响 a",
    ],
    errors: &[],
};

static DOC_COMPILE: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "compile(src) -> code|error",
    summary: "编译源码字符串为 Code 对象（可被 runCode 执行）。编译错误以 error 值返回（不抛出）。",
    params: &[("src", "Sflang 源码字符串")],
    returns: "code 编译后的代码对象；失败返回 error 值（用 isErr 检查）",
    examples: &[
        "var c = compile(\"return 1+2\")",
        "if isErr(c) { println(c) } else { println(runCode(c)) }",
    ],
    errors: &["语法错误返回 error 值（不是抛出异常）"],
};

static DOC_RUN_CODE: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "runCode(code) -> value|error",
    summary: "执行 compile() 返回的 Code 对象。运行错误以 error 值返回（不抛出）。",
    params: &[("code", "compile() 的返回值")],
    returns: "执行结果；运行出错返回 error 值（用 isErr 检查）",
    examples: &[
        "var c = compile(\"return fib(10)\")",
        "var r = runCode(c)",
        "if isErr(r) { println(\"错误:\", r) } else { println(\"结果:\", r) }",
    ],
    errors: &["参数不是 code 对象返回 error；运行时错误也返回 error 值"],
};

// ---- 打印/输出函数 ----

static DOC_PRINT: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "print(...) -> undefined",
    summary: "打印参数（空格分隔）不换行，输出到标准输出。",
    params: &[("...", "任意类型，自动转为字符串（多个用空格分隔）")],
    returns: "undefined（无返回值）",
    examples: &[
        "print(\"hello\")              // 输出 hello（不换行）",
        "print(\"a\", 1, true)         // 输出 a 1 true",
    ],
    errors: &[],
};

static DOC_PLN: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "pln(...) -> undefined",
    summary: "println 的简写别名：打印参数（空格分隔）并换行。",
    params: &[("...", "任意类型，自动转为字符串")],
    returns: "undefined",
    examples: &["pln(\"hello\")  // 输出 hello 并换行"],
    errors: &[],
};

static DOC_PR: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "pr(...) -> undefined",
    summary: "print 的简写别名：打印参数（空格分隔）不换行。",
    params: &[("...", "任意类型，自动转为字符串")],
    returns: "undefined",
    examples: &["pr(\"hello\")  // 输出 hello（不换行）"],
    errors: &[],
};

static DOC_PRINTF: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "printf(fmt, args...) -> undefined",
    summary: "按格式字符串打印（不换行），Go 风格占位符（%v %d %s %f %t %x %%）。",
    params: &[
        ("fmt", "格式字符串，含 %v %d %s %f %t %x %%"),
        ("args", "对应占位符的参数（可变）"),
    ],
    returns: "undefined",
    examples: &[
        "printf(\"%s=%d\\n\", \"count\", 42)   // 输出 count=42 并换行",
        "printf(\"%.2f\", 3.14159)            // 输出 3.14",
    ],
    errors: &["占位符数量与参数数量不匹配会返回错误"],
};

static DOC_PRF: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "prf(fmt, args...) -> undefined",
    summary: "printf 的简写别名：按格式字符串打印（不换行）。",
    params: &[
        ("fmt", "格式字符串"),
        ("args", "对应占位符的参数（可变）"),
    ],
    returns: "undefined",
    examples: &["prf(\"%d+%d=%d\\n\", 1, 2, 3)  // 输出 1+2=3 并换行"],
    errors: &[],
};

static DOC_PRINTFLN: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "printfln(fmt, args...) -> undefined",
    summary: "格式化打印并换行（语义 = printf + \"\\n\"）。",
    params: &[
        ("fmt", "格式字符串"),
        ("args", "对应占位符的参数（可变）"),
    ],
    returns: "undefined",
    examples: &["printfln(\"%s=%d\", \"count\", 42)  // 输出 count=42 并换行"],
    errors: &[],
};

static DOC_PL: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "pl(fmt, args...) -> undefined",
    summary: "printfln 的简写别名：格式化打印并换行。",
    params: &[
        ("fmt", "格式字符串"),
        ("args", "对应占位符的参数（可变）"),
    ],
    returns: "undefined",
    examples: &["pl(\"hello %d\", 42)  // 输出 hello 42 并换行"],
    errors: &[],
};

static DOC_FPR: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "fpr(fmt, args...) -> undefined",
    summary: "printf 的别名：按格式字符串打印（不换行）。",
    params: &[
        ("fmt", "格式字符串"),
        ("args", "对应占位符的参数（可变）"),
    ],
    returns: "undefined",
    examples: &["fpr(\"x=%d\", 1)  // 输出 x=1（不换行）"],
    errors: &[],
};

static DOC_SPR: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "spr(fmt, args...) -> string",
    summary: "sprintf 的简写别名：格式化字符串并返回（不打印）。",
    params: &[
        ("fmt", "格式字符串"),
        ("args", "对应占位符的参数（可变）"),
    ],
    returns: "string 格式化后的字符串",
    examples: &["var s = spr(\"%s=%d\", \"count\", 42)  // s == \"count=42\""],
    errors: &[],
};

static DOC_PLT: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "plt(...) -> undefined",
    summary: "打印每个参数的类型与值，格式 (类型名)值，每行一个（调试用）。",
    params: &[("...", "任意类型，逐个输出类型与值")],
    returns: "undefined",
    examples: &[
        "plt(42, \"hi\")",
        "// 输出：",
        "// (int) 42",
        "// (string) \"hi\"",
    ],
    errors: &[],
};

// ---- 数组/容器操作 ----

static DOC_POP: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "pop(arr) -> value",
    summary: "弹出数组末尾元素并返回（原地修改）。",
    params: &[("arr", "目标数组（原地修改）")],
    returns: "数组末尾元素",
    examples: &[
        "var a = [1, 2, 3]; var x = pop(a)  // x == 3; a 变为 [1, 2]",
    ],
    errors: &[
        "空数组弹出返回错误",
        "第一个参数应为 array",
    ],
};

static DOC_ENTRIES: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "entries(obj) -> array<[key, value]>",
    summary: "返回 object/map 的键值对数组（每对为 [key, value]），过滤掉函数成员。",
    params: &[("obj", "object 或 map")],
    returns: "array，元素为 [key, value] 二元数组",
    examples: &[
        "entries({\"a\":1, \"b\":2})  // [[\"a\", 1], [\"b\", 2]]（顺序不保证）",
    ],
    errors: &["参数应为 object 或 map"],
};

static DOC_DATA_KEYS: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "dataKeys(obj) -> array<string>",
    summary: "返回 object/map 的非函数键名（过滤掉方法）。",
    params: &[("obj", "object 或 map")],
    returns: "array<string> 键名数组",
    examples: &["dataKeys({\"a\":1, \"b\":2})  // [\"a\", \"b\"]（顺序不保证）"],
    errors: &["参数应为 object 或 map"],
};

static DOC_DATA_VALUES: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "dataValues(obj) -> array",
    summary: "返回 object/map 的非函数值（过滤掉方法）。",
    params: &[("obj", "object 或 map")],
    returns: "array 值数组",
    examples: &["dataValues({\"a\":1, \"b\":2})  // [1, 2]"],
    errors: &["参数应为 object 或 map"],
};

static DOC_HAS_KEY: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "hasKey(obj, key) -> bool",
    summary: "判断 object/map 是否包含某键。",
    params: &[
        ("obj", "object 或 map"),
        ("key", "键名字符串"),
    ],
    returns: "bool：存在返回 true",
    examples: &[
        "hasKey({\"a\":1}, \"a\")  // true",
        "hasKey({\"a\":1}, \"b\")  // false",
    ],
    errors: &["第一个参数应为 object 或 map"],
};

static DOC_CLEAR: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "clear(c) -> undefined",
    summary: "清空容器内容（不释放内存）。支持 stringBuilder/array/byteArray/map/ring。",
    params: &[("c", "stringBuilder/array/byteArray/map/ring 之一")],
    returns: "undefined",
    examples: &[
        "var sb = newStringBuilder(\"hi\"); clear(sb)  // sb 内容清空",
        "var a = [1,2]; clear(a)                      // a 变为 []",
    ],
    errors: &["参数类型不支持"],
};

static DOC_RESET: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "reset(c) -> undefined",
    summary: "清空容器并释放内存（对 stringBuilder 效果最明显）。支持 stringBuilder/array/byteArray。",
    params: &[("c", "stringBuilder/array/byteArray 之一")],
    returns: "undefined",
    examples: &["var sb = newStringBuilder(\"hi\"); reset(sb)  // sb 清空并释放内存"],
    errors: &["参数类型不支持"],
};

static DOC_NEW_MAP: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "newMap() -> map",
    summary: "创建空有序 Map（纯数据容器，按插入顺序遍历）。",
    params: &[],
    returns: "map 空有序映射",
    examples: &[
        "var m = newMap(); setMember(m, \"k\", 1)",
    ],
    errors: &[],
};

static DOC_NEW_OBJECT: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "newObject(proto) -> object",
    summary: "创建以 proto 为原型的空 object（暴露原型链，用于方法共享）。",
    params: &[("proto", "原型对象（方法挂在其上）")],
    returns: "object 以 proto 为原型的新对象",
    examples: &[
        "var proto = { greet: func() { return \"hi\" } }",
        "var o = newObject(proto)",
        "o.greet()  // \"hi\"（继承自原型）",
    ],
    errors: &["第一个参数应为 object"],
};

static DOC_NEW_STRING_BUILDER: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "newStringBuilder([initial]) -> stringBuilder",
    summary: "创建 StringBuilder（高效字符串构建器），可选初始内容。",
    params: &[("initial", "可选。初始字符串内容，默认为空")],
    returns: "stringBuilder 对象，可用 writeStr/len/toStr/clear/reset 操作",
    examples: &[
        "var sb = newStringBuilder()",
        "var sb2 = newStringBuilder(\"init\")",
    ],
    errors: &[],
};

static DOC_NEW_REF: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "newRef(value) -> ref",
    summary: "创建引用容器，包装一个初始值。引用是独立可变容器，便于函数传参后修改。",
    params: &[("value", "初始值")],
    returns: "ref 引用容器（用 getValueByRef/setValueByRef 读写）",
    examples: &[
        "var r = newRef(10)",
        "getValueByRef(r)  // 10",
        "setValueByRef(r, 20)",
    ],
    errors: &[],
};

static DOC_GET_VALUE_BY_REF: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "getValueByRef(ref) -> value",
    summary: "读取引用容器内的值。",
    params: &[("ref", "newRef 创建的引用对象")],
    returns: "引用内当前值",
    examples: &["getValueByRef(newRef(5))  // 5"],
    errors: &["参数不是引用对象（需用 newRef 创建）"],
};

static DOC_SET_VALUE_BY_REF: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "setValueByRef(ref, newValue) -> undefined",
    summary: "设置引用容器内的值（原地修改）。",
    params: &[
        ("ref", "newRef 创建的引用对象"),
        ("newValue", "新值"),
    ],
    returns: "undefined",
    examples: &[
        "var r = newRef(1); setValueByRef(r, 99); getValueByRef(r)  // 99",
    ],
    errors: &["第一个参数不是引用对象"],
};

static DOC_BYTE: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "byte(n) -> byte",
    summary: "构造 byte 值（0-255）。超出范围报错。",
    params: &[("n", "0-255 的整数")],
    returns: "byte 字节值",
    examples: &["byte(65)  // Byte(65)"],
    errors: &["值超出 0-255 范围"],
};

static DOC_BYTES_XOR: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "bytesXor(data, key) -> bytes",
    summary: "批量 XOR：data 的每个字节与 key 的对应字节异或（key 循环复用），返回新 bytes。",
    params: &[
        ("data", "bytes/byteArray/string：被异或的数据"),
        ("key", "bytes/byteArray/string/int(byte)：异或密钥"),
    ],
    returns: "bytes 异或后的新字节串（不可变）",
    examples: &[
        "bytesXor(\"abc\", \"x\")  // 与单字节密钥异或的新 bytes",
    ],
    errors: &["key 不能为空"],
};

static DOC_BYTES_XOR_IN_PLACE: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "bytesXorInPlace(byteArray, key) -> byteArray",
    summary: "原地 XOR（直接修改 byteArray，不创建新对象）。返回原 byteArray。",
    params: &[
        ("byteArray", "可变的 byteArray 容器（原地修改）"),
        ("key", "bytes/byteArray/string/int(byte)：异或密钥"),
    ],
    returns: "传入的 byteArray（已就地修改）",
    examples: &["var b = byteArray(\"abc\"); bytesXorInPlace(b, \"x\")"],
    errors: &[
        "key 不能为空",
        "第一个参数须为 byteArray",
    ],
};

// ---- 类型转换 ----

static DOC_STRING: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "string([x]) -> string",
    summary: "转字符串。无参返回空串；有参返回参数的字符串表示。",
    params: &[("x", "可选。要转换的值，省略时返回空串")],
    returns: "string 字符串表示",
    examples: &[
        "string(42)       // \"42\"",
        "string(3.14)     // \"3.14\"",
        "string()         // \"\"",
    ],
    errors: &[],
};

static DOC_INT: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "int(x) -> int",
    summary: "转整数。支持 int/float/bool/byte/string/bigInt（字符串需为有效整数）。",
    params: &[("x", "要转换的值（int/float/bool/byte/string/bigInt）")],
    returns: "int 整数值",
    examples: &[
        "int(3.9)        // 3（截断）",
        "int(true)       // 1",
        "int(\"42\")      // 42",
    ],
    errors: &[
        "字符串不是有效整数返回错误",
        "BigInt 超出 i64 范围返回错误",
    ],
};

static DOC_FLOAT: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "float(x) -> float",
    summary: "转浮点。支持 int/float/bool/byte/string/bigInt/bigFloat。",
    params: &[("x", "要转换的值")],
    returns: "float 浮点值",
    examples: &[
        "float(3)        // 3.0",
        "float(\"3.14\")   // 3.14",
    ],
    errors: &[
        "字符串无法解析为浮点返回错误",
        "BigInt 超出 i64 范围返回错误",
    ],
};

static DOC_TO_STR: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "toStr([x]) -> string",
    summary: "string 的别名：转字符串。无参返回空串。",
    params: &[("x", "可选。要转换的值")],
    returns: "string 字符串表示",
    examples: &["toStr(42)  // \"42\""],
    errors: &[],
};

static DOC_TO_INT: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "toInt(x) -> int",
    summary: "int 的别名：转整数。",
    params: &[("x", "要转换的值")],
    returns: "int 整数值",
    examples: &["toInt(\"42\")  // 42"],
    errors: &["字符串不是有效整数返回错误"],
};

static DOC_TO_FLOAT: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "toFloat(x) -> float",
    summary: "float 的别名：转浮点。",
    params: &[("x", "要转换的值")],
    returns: "float 浮点值",
    examples: &["toFloat(\"3.14\")  // 3.14"],
    errors: &["字符串无法解析返回错误"],
};

static DOC_ADJUST_FLOAT: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "adjustFloat(x[, prec]) -> float",
    summary: "消除浮点计算精度误差，按指定精度四舍五入（默认 10 位）。",
    params: &[
        ("x", "浮点数"),
        ("prec", "可选。小数位数，默认 10"),
    ],
    returns: "float 调整后的浮点数",
    examples: &[
        "adjustFloat(0.1 + 0.2)       // 0.3（消除精度误差）",
        "adjustFloat(3.14159, 2)      // 3.14",
    ],
    errors: &["解析失败返回错误"],
};

// ---- 类型判断 ----

static DOC_IS_UNDEFINED: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "isUndefined([v]) -> bool",
    summary: "判断是否为 undefined。缺参时返回 true（便于链式判空 isUndefined(m[\"k\"])）。",
    params: &[("v", "可选。任意值，省略时视为 undefined")],
    returns: "bool：是 undefined 或缺参返回 true",
    examples: &[
        "isUndefined(undefined)  // true",
        "isUndefined(42)         // false",
        "isUndefined()           // true（缺参）",
    ],
    errors: &[],
};

static DOC_IS_TYPE: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "isType(v, typeName) -> bool",
    summary: "按类型名（大小写不敏感）判断值类型。支持基础类型与 native 细分（如 ring/regex）。",
    params: &[
        ("v", "任意值"),
        ("typeName", "类型名字符串（与 typeName 返回一致）"),
    ],
    returns: "bool：类型匹配返回 true",
    examples: &[
        "isType(42, \"int\")        // true",
        "isType(\"hi\", \"string\")   // true",
    ],
    errors: &[],
};

static DOC_IS_TYPE_CODE: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "isTypeCode(v, code) -> bool",
    summary: "按类型数字编码判断值类型（编码见 typeCode，0-19）。",
    params: &[
        ("v", "任意值"),
        ("code", "类型编码整数（详见 typeCode）"),
    ],
    returns: "bool：编码匹配返回 true",
    examples: &[
        "isTypeCode(42, 1)        // true（1 = int）",
        "isTypeCode(\"hi\", 4)      // true（4 = string）",
    ],
    errors: &[],
};

// ---- 错误处理 ----

static DOC_IS_ERR: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "isErr(v) -> bool",
    summary: "判断是否为错误样值（Error 对象或 \"TXERROR:\" 开头的字符串）。",
    params: &[("v", "任意值")],
    returns: "bool：是错误样值返回 true",
    examples: &[
        "isErr(error(\"x\"))          // true",
        "isErr(\"TXERROR:boom\")      // true",
        "isErr(42)                   // false",
    ],
    errors: &[],
};

static DOC_IS_ERR_X: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "isErrX(v) -> bool",
    summary: "isErr 的 Charlang 兼容别名。",
    params: &[("v", "任意值")],
    returns: "bool：是错误样值返回 true",
    examples: &["isErrX(error(\"x\"))  // true"],
    errors: &[],
};

static DOC_IS_ERR_STR: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "isErrStr(v) -> bool",
    summary: "判断是否为 \"TXERROR:\" 开头的字符串（仅识别字符串形式错误）。",
    params: &[("v", "任意值")],
    returns: "bool：是 TXERROR 字符串返回 true",
    examples: &[
        "isErrStr(\"TXERROR:boom\")  // true",
        "isErrStr(error(\"x\"))       // false（Error 对象不是字符串）",
    ],
    errors: &[],
};

static DOC_GET_ERR_STR: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "getErrStr(v) -> string",
    summary: "提取错误信息字符串：Error 取 message，TXERROR 字符串去前缀，其他取 to_str。",
    params: &[("v", "任意值")],
    returns: "string 错误信息或字符串表示",
    examples: &[
        "getErrStr(error(\"bad\"))        // \"bad\"",
        "getErrStr(\"TXERROR:boom\")      // \"boom\"",
    ],
    errors: &[],
};

static DOC_GET_ERR_STR_X: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "getErrStrX(v) -> string",
    summary: "getErrStr 的 Charlang 兼容别名。",
    params: &[("v", "任意值")],
    returns: "string 错误信息字符串",
    examples: &["getErrStrX(\"TXERROR:x\")  // \"x\""],
    errors: &[],
};

static DOC_ERR_STRF: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "errStrf(fmt, args...) -> string",
    summary: "格式化生成 \"TXERROR:\" 前缀的错误字符串（创建字符串形式错误的便捷方式）。",
    params: &[
        ("fmt", "格式字符串（同 sprintf）"),
        ("args", "对应占位符的参数（可变）"),
    ],
    returns: "string \"TXERROR:\" + 格式化结果",
    examples: &[
        "errStrf(\"%s 失败\", \"加载\")  // \"TXERROR:加载 失败\"",
    ],
    errors: &[],
};

static DOC_ERRF: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "errf(fmt, args...) -> string",
    summary: "errStrf 的 Charlang 兼容别名：格式化生成 TXERROR 错误字符串。",
    params: &[
        ("fmt", "格式字符串"),
        ("args", "对应占位符的参数（可变）"),
    ],
    returns: "string \"TXERROR:\" + 格式化结果",
    examples: &["errf(\"boom\")  // \"TXERROR:boom\""],
    errors: &[],
};

static DOC_ERR_TO_EMPTY: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "errToEmpty(v) -> value",
    summary: "若 v 是错误样值则转为空字符串，否则原样返回（安全处理可能错误值）。",
    params: &[("v", "任意值")],
    returns: "错误时返回空字符串，非错误时返回原值",
    examples: &[
        "errToEmpty(error(\"x\"))   // \"\"",
        "errToEmpty(\"hi\")          // \"hi\"",
    ],
    errors: &[],
};

static DOC_CHECK_ERR: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "checkErr(v[, \"-format=...\"]) -> value",
    summary: "若 v 是错误样值则打印错误并退出进程（退出码 1）；非错误原样返回。",
    params: &[
        ("v", "要检查的值"),
        ("-format=", "可选。自定义输出格式，默认 \"Error: %v\\n\""),
    ],
    returns: "v 非错误时原样返回",
    examples: &[
        "var r = checkErr(loadFile(\"x.txt\"))  // 加载失败则退出",
        "checkErr(r, \"-format=fatal: %v\\n\")",
    ],
    errors: &["v 为错误时直接 exit(1)"],
};

static DOC_CHECK_ERR_X: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "checkErrX(v[, \"-format=...\"]) -> value",
    summary: "checkErr 的 Charlang 兼容别名。",
    params: &[
        ("v", "要检查的值"),
        ("-format=", "可选。自定义输出格式"),
    ],
    returns: "v 非错误时原样返回",
    examples: &["checkErrX(error(\"x\"))  // 打印并退出"],
    errors: &["v 为错误时直接 exit(1)"],
};

static DOC_TRIM_ERR: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "trimErr(v[, cutset...]) -> string|value",
    summary: "若 v 是错误样值则原样返回（不丢失错误），否则去空白。可指定 cutset 字符集。",
    params: &[
        ("v", "要处理的值"),
        ("cutset", "可选。要去除的字符（多个字符串参数）"),
    ],
    returns: "错误样值原样返回；否则返回去空白后的字符串",
    examples: &[
        "trimErr(\"  hi  \")            // \"hi\"",
        "trimErr(error(\"x\"))          // 原样返回错误",
        "trimErr(\"##hi##\", \"#\")       // \"hi\"",
    ],
    errors: &[],
};

static DOC_UNDEF_TO_EMPTY: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "undefToEmpty(v) -> string",
    summary: "将 undefined（或缺参）转为空字符串，其余值转为 to_str（对标 Charlang nilToEmpty）。",
    params: &[("v", "任意值")],
    returns: "undefined 返回空串，其余返回 to_str",
    examples: &[
        "undefToEmpty(undefined)  // \"\"",
        "undefToEmpty(42)          // \"42\"",
    ],
    errors: &[],
};

static DOC_EXPLAIN_UNDEF: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "explainUndef(name) -> string",
    summary: "返回某名字为何为 undefined 的诊断字符串（含是否预定义、相似名字提示，AI 友好）。",
    params: &[("name", "变量名字符串")],
    returns: "string 多行诊断信息",
    examples: &[
        "explainUndef(\"printl\")",
        "// 提示：未定义，相似名字：println",
    ],
    errors: &[],
};

// ---- 数组高阶函数 ----

static DOC_FILTER: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "filter(arr, fn) -> array",
    summary: "用谓词函数过滤数组，返回新数组（仅保留 fn(x) 为 truthy 的元素）。",
    params: &[
        ("arr", "原数组（不修改）"),
        ("fn", "谓词函数 fn(x) -> truthy/falsy"),
    ],
    returns: "array 过滤后的新数组",
    examples: &[
        "filter([1,2,3,4], func(x) { return x > 2 })  // [3, 4]",
    ],
    errors: &["第一个参数应为 array"],
};

static DOC_MAP_FN: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "map(arr, fn) -> array",
    summary: "用函数映射数组每个元素，返回新数组 [fn(a[0]), fn(a[1]), ...]。",
    params: &[
        ("arr", "原数组（不修改）"),
        ("fn", "映射函数 fn(x) -> y"),
    ],
    returns: "array 映射后的新数组",
    examples: &[
        "map([1,2,3], func(x) { return x * 2 })  // [2, 4, 6]",
    ],
    errors: &["第一个参数应为 array"],
};

static DOC_FIND: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "find(arr, fn) -> value|undefined",
    summary: "查找数组中第一个满足谓词的元素，返回该元素；无则 undefined。",
    params: &[
        ("arr", "原数组"),
        ("fn", "谓词函数 fn(x) -> truthy/falsy"),
    ],
    returns: "第一个匹配元素，无匹配返回 undefined",
    examples: &[
        "find([1,2,3,4], func(x) { return x > 2 })  // 3",
    ],
    errors: &["第一个参数应为 array"],
};

// ---- 命令行参数 ----

static DOC_GET_PARAM: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "getParam(args, index[, default]) -> value",
    summary: "从参数数组取第 index 个元素，不存在则返回 default（或 undefined）。",
    params: &[
        ("args", "参数数组（如 argsG）"),
        ("index", "位置索引（int）"),
        ("default", "可选。缺省时返回的默认值"),
    ],
    returns: "对应位置参数；不存在返回 default 或 undefined",
    examples: &[
        "getParam(argsG, 0)               // 第一个参数",
        "getParam(argsG, 2, \"fallback\")    // 越界返回 \"fallback\"",
    ],
    errors: &[],
};

static DOC_GET_SWITCH: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "getSwitch(args, key[, default]) -> string",
    summary: "从参数数组按 \"--key=value\" 前缀提取开关值；无匹配返回 default。",
    params: &[
        ("args", "参数数组（如 argsG）"),
        ("key", "前缀字符串，含前缀和等号，如 \"--host=\""),
        ("default", "可选。无匹配时的默认值"),
    ],
    returns: "匹配项的等号右侧值；无匹配返回 default",
    examples: &[
        "getSwitch(argsG, \"--host=\", \"localhost\")  // 匹配 --host=x 返回 x",
    ],
    errors: &[],
};

static DOC_GET_ALL_SWITCHES: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "getAllSwitches(args, key) -> array<string>",
    summary: "从参数数组提取所有匹配 \"--key=value\" 的值（可多个同名），返回数组。",
    params: &[
        ("args", "参数数组（如 argsG）"),
        ("key", "前缀字符串，含前缀和等号"),
    ],
    returns: "array<string> 所有匹配值；无匹配返回空数组",
    examples: &[
        "getAllSwitches(argsG, \"--attach=\")  // [\"file1.pdf\", \"file2.xlsx\"]",
    ],
    errors: &[],
};

static DOC_IF_SWITCH_EXISTS: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "ifSwitchExists(args, key) -> bool",
    summary: "检查参数数组中是否存在某个布尔型开关（无值，如 \"--verbose\"）。",
    params: &[
        ("args", "参数数组（如 argsG）"),
        ("key", "开关名字符串，如 \"--verbose\" 或 \"-v\""),
    ],
    returns: "bool：存在返回 true",
    examples: &[
        "ifSwitchExists(argsG, \"--verbose\")  // true/false",
    ],
    errors: &[],
};

// ---- 系统与杂项 ----

static DOC_SLEEP_MS: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "sleepMs(ms) -> undefined",
    summary: "休眠指定毫秒数（整数）。",
    params: &[("ms", "毫秒数（int）")],
    returns: "undefined",
    examples: &["sleepMs(500)  // 休眠 500 毫秒"],
    errors: &[],
};

static DOC_RANDOM_STR: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "randomStr(n) -> string",
    summary: "生成长度为 n 的随机字母数字字符串（大小写字母 + 数字）。",
    params: &[("n", "长度（int，非负）")],
    returns: "string 随机字母数字串",
    examples: &[
        "randomStr(8)  // 如 \"aB3xK9mN\"",
    ],
    errors: &["长度为负返回错误"],
};

static DOC_PASS: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "pass(...) -> undefined",
    summary: "空操作占位符（no-op），忽略所有参数，返回 undefined。",
    params: &[("...", "任意参数，全部忽略")],
    returns: "undefined",
    examples: &["pass()  // 什么都不做"],
    errors: &[],
};

static DOC_TO_KMG: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "toKMG(n[, decimals]) -> string",
    summary: "将数字转为带单位的易读字符串（K/M/G/T/P，1024 进制），默认 2 位小数。",
    params: &[
        ("n", "数字（int 或 float）"),
        ("decimals", "可选。小数位数，默认 2"),
    ],
    returns: "string 带 K/M/G/T/P 单位的字符串",
    examples: &[
        "toKMG(1536)        // \"1.50K\"",
        "toKMG(1048576)     // \"1.00M\"",
        "toKMG(0, 0)        // \"0\"",
    ],
    errors: &[],
};

// ---- 调试与反射 ----

static DOC_DUMP_VAR: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "dumpVar(v) -> string",
    summary: "转储变量详细信息（类型名、类型码、值摘要），返回多行诊断字符串。",
    params: &[("v", "任意值")],
    returns: "string 多行诊断信息（type/typeCode/value）",
    examples: &[
        "println(dumpVar(42))",
        "// type: int",
        "// typeCode: 1",
        "// value: 42",
    ],
    errors: &[],
};

static DOC_GLOBALS: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "globals() -> array<string>",
    summary: "列出所有全局变量名（用于反射与调试）。",
    params: &[],
    returns: "array<string> 全局变量名数组",
    examples: &["var names = globals()  // 当前所有全局变量名"],
    errors: &[],
};

static DOC_GET_MEMBER: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "getMember(obj, key) -> value",
    summary: "反射式读取 object/map 的成员。Object 沿原型链查找；不存在返回 undefined。",
    params: &[
        ("obj", "object 或 map"),
        ("key", "成员名字符串"),
    ],
    returns: "成员值；不存在返回 undefined",
    examples: &[
        "getMember({\"a\":1}, \"a\")  // 1",
        "getMember(obj, \"method\")  // 沿原型链查找",
    ],
    errors: &["第一个参数应为 object 或 map"],
};

static DOC_SET_MEMBER: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "setMember(obj, key, value) -> undefined",
    summary: "反射式设置 object/map 的成员值（原地修改，仅写入自身，与 obj.key = v 一致）。",
    params: &[
        ("obj", "object 或 map"),
        ("key", "成员名字符串"),
        ("value", "新值"),
    ],
    returns: "undefined",
    examples: &[
        "var o = {}; setMember(o, \"k\", 1)  // o 变为 {\"k\":1}",
    ],
    errors: &["第一个参数应为 object 或 map"],
};

static DOC_CALL_METHOD: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "callMethod(obj, methodName[, args]) -> value",
    summary: "调用对象方法（沿原型链查找 methodName），obj 作为隐式 self 传入。",
    params: &[
        ("obj", "object 实例"),
        ("methodName", "方法名字符串（沿原型链查找）"),
        ("args", "可选。参数数组或单个值，作为方法后续参数"),
    ],
    returns: "方法返回值",
    examples: &[
        "callMethod(o, \"greet\")               // 无参调用",
        "callMethod(o, \"add\", [1, 2])          // 带参调用",
    ],
    errors: &[
        "对象上找不到方法返回错误",
        "第一个参数应为 object",
    ],
};

static DOC_SHOW_TABLE: BuiltinDoc = BuiltinDoc {
    category: "core",
    signature: "showTable(data[, opts]) -> string",
    summary: "将二维数组渲染为对齐的 ASCII 表格字符串（首行默认为表头）。",
    params: &[
        ("data", "array<array>，每行是一维数组"),
        ("opts", "可选。map/object，支持 header(bool,默认true) 和 sep(string,默认\"|\")"),
    ],
    returns: "string 表格字符串（含边框，不直接打印）",
    examples: &[
        "println(showTable([[\"姓名\",\"年龄\"],[\"张三\",20],[\"李四\",25]]))",
        "// +------+----+",
        "// | 姓名 | 年龄 |",
        "// +------+----+",
        "// | 张三 | 20  |",
        "// | 李四 | 25  |",
        "// +------+----+",
    ],
    errors: &["每行必须是一维数组"],
};

/// register 注册所有内置函数到 VM。
pub fn register(vm: &mut VM) {
    vm.register_builtin_doc("println", bi_println, &DOC_PRINTLN);
    vm.register_builtin_doc("print", bi_print, &DOC_PRINT);
    // 打印函数简称（别名）
    vm.register_builtin_doc("pln", bi_println, &DOC_PLN);
    vm.register_builtin_doc("pr", bi_print, &DOC_PR);
    // 格式化打印（Go 风格占位符 %v %d %s %f %t %x %%）
    vm.register_builtin_doc("printf", bi_printf, &DOC_PRINTF);
    vm.register_builtin_doc("prf", bi_printf, &DOC_PRF);
    vm.register_builtin_doc("printfln", bi_printfln, &DOC_PRINTFLN);
    vm.register_builtin_doc("pl", bi_printfln, &DOC_PL);
    vm.register_builtin_doc("len", bi_len, &DOC_LEN);
    vm.register_builtin_doc("keys", bi_keys, &DOC_KEYS);
    vm.register_builtin_doc("push", bi_push, &DOC_PUSH);
    vm.register_builtin_doc("pop", bi_pop, &DOC_POP);
    vm.register_builtin_doc("typeCode", bi_type_code, &DOC_TYPECODE);
    vm.register_builtin_doc("typeName", bi_type_name, &DOC_TYPENAME);
    vm.register_builtin_doc("string", bi_string, &DOC_STRING);
    vm.register_builtin_doc("int", bi_int, &DOC_INT);
    vm.register_builtin_doc("float", bi_float, &DOC_FLOAT);
    vm.register_builtin_doc("range", bi_range, &DOC_RANGE);
    vm.register_builtin_doc("assert", bi_assert, &DOC_ASSERT);
    vm.register_builtin_doc("sleep", bi_sleep, &DOC_SLEEP);
    vm.register_builtin_doc("sleepMs", bi_sleep_ms, &DOC_SLEEP_MS);
    vm.register_builtin_doc("newStringBuilder", bi_new_string_builder, &DOC_NEW_STRING_BUILDER);
    vm.register_builtin_doc("clear", bi_clear, &DOC_CLEAR);
    vm.register_builtin_doc("reset", bi_reset, &DOC_RESET);
    // ---- 实用函数（对标 charlang 常见编程任务）----
    vm.register_builtin_doc("uuid", bi_uuid, &DOC_UUID);
    vm.register_builtin_doc("randomStr", bi_random_str, &DOC_RANDOM_STR);
    vm.register_builtin_doc("values", bi_values, &DOC_VALUES);
    vm.register_builtin_doc("hasKey", bi_has_key, &DOC_HAS_KEY);
    vm.register_builtin_doc("deepClone", bi_deep_clone, &DOC_DEEP_CLONE);
    vm.register_builtin_doc("newObject", bi_new_object, &DOC_NEW_OBJECT);
    vm.register_builtin_doc("filter", bi_filter, &DOC_FILTER);
    vm.register_builtin_doc("map", bi_map, &DOC_MAP_FN);
    vm.register_builtin_doc("find", bi_find, &DOC_FIND);
    vm.register_builtin_doc("sprintf", bi_sprintf, &DOC_SPRINTF);
    vm.register_builtin_doc("spr", bi_sprintf, &DOC_SPR);
    vm.register_builtin_doc("fpr", bi_printf, &DOC_FPR);
    vm.register_builtin_doc("adjustFloat", bi_adjust_float, &DOC_ADJUST_FLOAT);
    vm.register_builtin_doc("pass", bi_pass, &DOC_PASS);
    vm.register_builtin_doc("plt", bi_plt, &DOC_PLT);
    vm.register_builtin_doc("getParam", bi_get_param, &DOC_GET_PARAM);
    vm.register_builtin_doc("getSwitch", bi_get_switch, &DOC_GET_SWITCH);
    vm.register_builtin_doc("getAllSwitches", bi_get_all_switches, &DOC_GET_ALL_SWITCHES);
    vm.register_builtin_doc("ifSwitchExists", bi_if_switch_exists, &DOC_IF_SWITCH_EXISTS);
    vm.register_builtin_doc("toStr", bi_string, &DOC_TO_STR);
    vm.register_builtin_doc("toInt", bi_int, &DOC_TO_INT);
    vm.register_builtin_doc("toFloat", bi_float, &DOC_TO_FLOAT);
    vm.register_builtin_doc("compile", bi_compile, &DOC_COMPILE);
    vm.register_builtin_doc("runCode", bi_run_code, &DOC_RUN_CODE);
    vm.register_builtin_doc("newRef", bi_new_ref, &DOC_NEW_REF);
    vm.register_builtin_doc("getValueByRef", bi_get_value_by_ref, &DOC_GET_VALUE_BY_REF);
    vm.register_builtin_doc("setValueByRef", bi_set_value_by_ref, &DOC_SET_VALUE_BY_REF);
    // byte 构造与字节操作
    vm.register_builtin_doc("byte", bi_byte, &DOC_BYTE);
    vm.register_builtin_doc("newMap", bi_new_map, &DOC_NEW_MAP);
    vm.register_builtin_doc("entries", bi_entries, &DOC_ENTRIES);
    vm.register_builtin_doc("dataKeys", bi_data_keys, &DOC_DATA_KEYS);
    vm.register_builtin_doc("dataValues", bi_data_values, &DOC_DATA_VALUES);
    vm.register_builtin_doc("bytesXor", bi_bytes_xor, &DOC_BYTES_XOR);
    vm.register_builtin_doc("bytesXorInPlace", bi_bytes_xor_in_place, &DOC_BYTES_XOR_IN_PLACE);
    // 类型判断：isUndefined 保留（特殊语义：缺参返回 true，链式判空）
    vm.register_builtin_doc("isUndefined", bi_is_undefined, &DOC_IS_UNDEFINED);
    // 错误处理：isError/isErr 保留（错误判断，非纯类型判断）
    vm.register_builtin_doc("error", bi_error, &DOC_ERROR);
    vm.register_builtin_doc("isError", bi_is_error, &DOC_IS_ERROR);
    // ---- TXERROR 错误字符串机制（对标 Charlang isErrX/getErrStrX 等）----
    vm.register_builtin_doc("isErr", bi_is_err, &DOC_IS_ERR);
    vm.register_builtin_doc("isErrX", bi_is_err, &DOC_IS_ERR_X);      // Charlang 兼容别名
    vm.register_builtin_doc("isErrStr", bi_is_err_str, &DOC_IS_ERR_STR);
    vm.register_builtin_doc("getErrStr", bi_get_err_str, &DOC_GET_ERR_STR);
    vm.register_builtin_doc("getErrStrX", bi_get_err_str, &DOC_GET_ERR_STR_X);  // Charlang 兼容别名
    vm.register_builtin_doc("errStrf", bi_err_strf, &DOC_ERR_STRF);
    vm.register_builtin_doc("errf", bi_err_strf, &DOC_ERRF);       // Charlang 兼容别名
    vm.register_builtin_doc("errToEmpty", bi_err_to_empty, &DOC_ERR_TO_EMPTY);
    vm.register_builtin_doc("checkErr", bi_check_err, &DOC_CHECK_ERR);
    vm.register_builtin_doc("checkErrX", bi_check_err, &DOC_CHECK_ERR_X); // Charlang 兼容别名
    vm.register_builtin_doc("trimErr", bi_trim_err, &DOC_TRIM_ERR);
    // ---- undefined 配套内置函数（对标 Charlang 的 nilToEmpty 等）----
    vm.register_builtin_doc("undefToEmpty", bi_undef_to_empty, &DOC_UNDEF_TO_EMPTY);
    // 注：原 "default" 改名为 "defaultVal"，因 "default" 已成为 switch 关键字。
    // 语义不变（truthy 兜底，等价于 x || d 运算符）。
    vm.register_builtin_doc("defaultVal", bi_default, &DOC_DEFAULT_VAL);
    vm.register_builtin_doc("defaultUndef", bi_default_undef, &DOC_DEFAULT_UNDEF);
    vm.register_builtin_doc("explainUndef", bi_explain_undef, &DOC_EXPLAIN_UNDEF);
    // ---- 通用类型判断（取代零散的 isXxx 谓词）----
    vm.register_builtin_doc("isType", bi_is_type, &DOC_IS_TYPE);
    vm.register_builtin_doc("isTypeCode", bi_is_type_code, &DOC_IS_TYPE_CODE);
    // ---- 调试与反射 ----
    vm.register_builtin_doc("dumpVar", bi_dump_var, &DOC_DUMP_VAR);
    vm.register_builtin_doc("globals", bi_globals, &DOC_GLOBALS);
    // ---- 成员反射 ----
    vm.register_builtin_doc("getMember", bi_get_member, &DOC_GET_MEMBER);
    vm.register_builtin_doc("setMember", bi_set_member, &DOC_SET_MEMBER);
    vm.register_builtin_doc("callMethod", bi_call_method, &DOC_CALL_METHOD);
    // ---- 格式化辅助 ----
    vm.register_builtin_doc("toKMG", bi_to_kmg, &DOC_TO_KMG);
    vm.register_builtin_doc("showTable", bi_show_table, &DOC_SHOW_TABLE);
    // ---- Help 系统（AI 友好的文档自省）----
    vm.register_builtin_doc("help", bi_help, &DOC_HELP);
}

/// bi_println 打印并换行。
fn bi_println(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = args.iter().map(|v| v.to_str()).collect::<Vec<_>>().join(" ");
    let out = _vm.output_handle();
    writeln!(out.lock().unwrap(), "{}", s).map_err(|e| crate::value::error_value(e.to_string()))?;
    Ok(Value::Undefined)
}

/// bi_print 打印不换行。
fn bi_print(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = args.iter().map(|v| v.to_str()).collect::<Vec<_>>().join(" ");
    let out = _vm.output_handle();
    write!(out.lock().unwrap(), "{}", s).map_err(|e| crate::value::error_value(e.to_string()))?;
    Ok(Value::Undefined)
}

/// bi_printf 格式化打印（不换行）。
///
/// Go 风格占位符：
///   %v  任意值（用 to_str 表示）
///   %d  整数（int/bigInt，截断小数）
///   %s  字符串
///   %f  浮点
///   %t  布尔（true/false）
///   %x  十六进制（整数）
///   %c  码点 → 单字符
///   %%  字面百分号
/// 宽度/精度：支持 %5d、%-5s、%.2f（对标 Go fmt）。
/// 占位符多于参数：保留原样；参数多于占位符：多余参数忽略。
fn bi_printf(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = sprintf(args)?;
    let out = _vm.output_handle();
    write!(out.lock().unwrap(), "{}", s).map_err(|e| crate::value::error_value(e.to_string()))?;
    Ok(Value::Undefined)
}

/// bi_printfln 格式化打印并换行。语义 = printf + "\n"。
fn bi_printfln(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = sprintf(args)?;
    let out = _vm.output_handle();
    writeln!(out.lock().unwrap(), "{}", s).map_err(|e| crate::value::error_value(e.to_string()))?;
    Ok(Value::Undefined)
}

/// sprintf 格式化核心：args[0] 为格式串，args[1..] 为占位符实参。
///
/// 解析 %[flags][width][.precision]verb，按 verb 取下一个参数格式化。
/// 未识别 verb 按字面输出。参数耗尽后剩余占位符按字面输出。
fn sprintf(args: &[Value]) -> Result<String, Value> {
    if args.is_empty() {
        return Ok(String::new());
    }
    let fmt = match &args[0] {
        Value::Str(s) => s.to_string(),
        v => v.to_str(),
    };
    let rest = &args[1..];
    let mut out = String::with_capacity(fmt.len() + 8);
    let mut arg_idx = 0usize;
    let bytes = fmt.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            // 正确处理多字节 UTF-8：取完整字符而非单字节
            // 找到当前字符的 UTF-8 边界
            let ch_len = utf8_char_len(bytes[i]);
            let end = (i + ch_len).min(bytes.len());
            if let Ok(s) = std::str::from_utf8(&bytes[i..end]) {
                out.push_str(s);
            } else {
                out.push(bytes[i] as char); // 回退
            }
            i = end;
            continue;
        }
        // 遇到 %，解析格式说明
        i += 1;
        if i >= bytes.len() {
            out.push('%');
            break;
        }
        if bytes[i] == b'%' {
            out.push('%');
            i += 1;
            continue;
        }
        // 解析 flags（- 0 + 空格）
        let mut flags = String::new();
        while i < bytes.len() && matches!(bytes[i], b'-' | b'0' | b'+' | b' ') {
            flags.push(bytes[i] as char);
            i += 1;
        }
        // 解析 width
        let mut width = String::new();
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            width.push(bytes[i] as char);
            i += 1;
        }
        // 解析 .precision
        let mut precision: Option<String> = None;
        if i < bytes.len() && bytes[i] == b'.' {
            i += 1;
            let mut prec = String::new();
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                prec.push(bytes[i] as char);
                i += 1;
            }
            precision = Some(prec);
        }
        // 解析 verb
        if i >= bytes.len() {
            // 格式说明未闭合，按字面输出已解析部分
            out.push('%');
            out.push_str(&flags);
            out.push_str(&width);
            if let Some(p) = precision { out.push('.'); out.push_str(&p); }
            break;
        }
        let verb = bytes[i] as char;
        i += 1;
        // 取参数
        let arg = rest.get(arg_idx);
        if arg.is_none() {
            // 参数耗尽：占位符按字面输出
            out.push('%');
            out.push_str(&flags);
            out.push_str(&width);
            if let Some(p) = precision { out.push('.'); out.push_str(&p); }
            out.push(verb);
            continue;
        }
        arg_idx += 1;
        let arg = arg.unwrap();
        let formatted = format_value(verb, arg, &flags, &width, precision.as_deref())?;
        out.push_str(&formatted);
    }
    Ok(out)
}

/// format_value 按 verb 格式化单个值，应用 width/precision/flags。
fn format_value(verb: char, v: &Value, flags: &str, width: &str, precision: Option<&str>) -> Result<String, Value> {
    let body: String = match verb {
        'v' => v.to_str(),
        's' => v.to_str(),
        'd' => match v {
            Value::Int(x) => x.to_string(),
            Value::BigInt(b) => b.to_string_decimal(),
            Value::Float(f) => (*f as i64).to_string(),
            _ => return Err(crate::value::error_value(format!(
                "printf %d 需要整数，得到 {} (可能原因：类型不匹配)", v.type_name(),
            ))),
        },
        'f' | 'g' | 'e' => match v {
            Value::Float(f) => format_float(verb, *f, precision),
            Value::Int(x) => format_float(verb, *x as f64, precision),
            _ => return Err(crate::value::error_value(format!(
                "printf %f 需要数值，得到 {} (可能原因：类型不匹配)", v.type_name(),
            ))),
        },
        't' => match v {
            Value::Bool(b) => b.to_string(),
            _ => v.is_truthy().to_string(),
        },
        'T' => v.type_name().to_string(),
        'x' => match v {
            Value::Int(x) => format!("{:x}", x),
            Value::BigInt(b) => {
                // 十六进制（绝对值 + 符号）
                let mag: String = b.to_string_decimal().chars().filter(|c| c.is_ascii_digit()).collect();
                let n = mag.parse::<u128>().unwrap_or(0);
                format!("{:x}", n)
            }
            _ => return Err(crate::value::error_value(format!(
                "printf %x 需要整数，得到 {}", v.type_name(),
            ))),
        },
        'c' => match v {
            Value::Int(code) => {
                match char::from_u32(*code as u32) {
                    Some(c) => c.to_string(),
                    None => return Err(crate::value::error_value(format!(
                        "printf %c 码点 {} 无效", code,
                    ))),
                }
            }
            _ => return Err(crate::value::error_value(format!(
                "printf %c 需要 int 码点，得到 {}", v.type_name(),
            ))),
        },
        _ => {
            // 未识别 verb：原样输出（Go 风格 %!verb）
            let prec_str = precision.map(|p| format!(".{}", p)).unwrap_or_default();
            return Ok(format!("%{}{}{}{}", flags, width, prec_str, verb));
        }
    };
    Ok(apply_width(body, flags, width, verb == 's' || verb == 'v'))
}

/// format_float 按精度格式化浮点（%.2f 等）。
fn format_float(verb: char, f: f64, precision: Option<&str>) -> String {
    let prec: usize = precision.and_then(|p| p.parse().ok()).unwrap_or(6);
    match verb {
        'f' => format!("{:.*}", prec, f),
        'g' => format!("{}", f),
        'e' => format!("{:.*e}", prec, f),
        _ => format!("{}", f),
    }
}

/// apply_width 应用宽度与对齐（- 左对齐，否则右对齐，0 填充对数值）。
fn apply_width(body: String, flags: &str, width: &str, is_string_like: bool) -> String {
    let w: usize = match width.parse() { Ok(n) => n, Err(_) => return body };
    if w == 0 || body.chars().count() >= w {
        return body;
    }
    let pad = w - body.chars().count();
    let fill = if flags.contains('0') && !is_string_like { '0' } else { ' ' };
    if flags.contains('-') {
        format!("{}{}", body, " ".repeat(pad))
    } else {
        format!("{}{}", fill.to_string().repeat(pad), body)
    }
}

/// bi_len 返回长度。
fn bi_len(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("len() 需要至少 1 个参数 (可能原因：忘记传参)"));
    }
    let n = match &args[0] {
        Value::Str(s) => s.chars().count() as i64,
        Value::Bytes(b) => b.len() as i64,
        Value::ByteArray(b) => b.lock().unwrap().len() as i64,
        Value::Array(a) => a.lock().unwrap().len() as i64,
        Value::Object(o) => o.lock().unwrap().len() as i64,
        Value::Map(m) => m.lock().unwrap().len() as i64,
        Value::StringBuilder(sb) => sb.lock().unwrap().chars().count() as i64,
        v => return Err(crate::value::error_value(format!("len() 不支持类型 {} (可能原因：参数类型错误)", v.type_name()))),
    };
    Ok(Value::Int(n))
}

/// bi_keys 返回所有键。
fn bi_keys(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("keys() 需要至少 1 个参数"));
    }
    let keys: Vec<Value> = match &args[0] {
        Value::Object(o) => o.lock().unwrap().keys().into_iter().map(|k| Value::str(&k)).collect(),
        Value::Map(m) => m.lock().unwrap().keys().into_iter().map(|k| Value::str(&k)).collect(),
        Value::Array(a) => {
            a.lock().unwrap().iter().enumerate().map(|(i, _)| Value::Int(i as i64)).collect()
        }
        Value::Str(s) => s.chars().enumerate().map(|(i, _)| Value::Int(i as i64)).collect(),
        v => return Err(crate::value::error_value(format!("keys() 不支持类型 {}", v.type_name()))),
    };
    Ok(Value::Array(Arc::new(Mutex::new(keys))))
}

/// bi_push 追加元素到数组。
fn bi_push(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.len() < 2 {
        return Err(crate::value::error_value("push() 需要 2 个参数 (array, value)"));
    }
    match &args[0] {
        Value::Array(a) => {
            a.lock().unwrap().push(args[1].clone());
            Ok(args[0].clone())
        }
        v => Err(crate::value::error_value(format!("push() 第一个参数必须是数组，得到 {} (可能原因：参数顺序错误；正确顺序 push(arr, value))", v.type_name()))),
    }
}

/// bi_pop 弹出数组末尾元素。
fn bi_pop(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("pop() 需要 1 个参数"));
    }
    match &args[0] {
        Value::Array(a) => {
            let mut arr = a.lock().unwrap();
            arr.pop().ok_or_else(|| crate::value::error_value("pop() on empty array"))
        }
        v => Err(crate::value::error_value(format!("pop() 不支持类型 {}", v.type_name()))),
    }
}

/// bi_type_code 返回类型编码。
fn bi_type_code(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("typeCode() 需要 1 个参数"));
    }
    Ok(Value::Int(args[0].type_code() as i64))
}

/// bi_type_name 返回类型名。
///
/// 对 Native 类型返回细化的类型名（如 image/canvas/font/ring/channel 等），
/// 其他类型返回基础类型名。
fn bi_type_name(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("typeName() 需要 1 个参数"));
    }
    Ok(Value::str_from(args[0].type_name_ex()))
}

/// bi_string 转字符串。
fn bi_string(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Ok(Value::str(""));
    }
    Ok(Value::str_from(args[0].to_str()))
}

/// bi_int 转整数。
fn bi_int(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("int() 需要 1 个参数"));
    }
    match &args[0] {
        Value::Int(_) => Ok(args[0].clone()),
        Value::Float(f) => Ok(Value::Int(*f as i64)),
        Value::Bool(b) => Ok(Value::Int(if *b { 1 } else { 0 })),
        Value::Byte(b) => Ok(Value::Int(*b as i64)),
        Value::Str(s) => s.parse::<i64>().map(Value::Int).map_err(|_| {
            crate::value::error_value(format!("int() 无法解析 '{}' (可能原因：字符串不是有效整数)", s))
        }),
        Value::BigInt(b) => {
            // BigInt -> Int，超出 i64 范围则报错
            match b.to_i64() {
                Some(v) => Ok(Value::Int(v)),
                None => Err(crate::value::error_value(format!(
                    "int() BigInt 超出 i64 范围: {} (可能原因：数值过大，请保持使用 bigInt 类型)",
                    b
                ))),
            }
        }
        v => Err(crate::value::error_value(format!("int() 不支持类型 {}", v.type_name()))),
    }
}

/// bi_float 转浮点。
fn bi_float(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("float() 需要 1 个参数"));
    }
    match &args[0] {
        Value::Int(i) => Ok(Value::Float(*i as f64)),
        Value::Float(_) => Ok(args[0].clone()),
        Value::Bool(b) => Ok(Value::Float(if *b { 1.0 } else { 0.0 })),
        Value::Byte(b) => Ok(Value::Float(*b as f64)),
        Value::BigInt(b) => {
            match b.to_i64() {
                Some(v) => Ok(Value::Float(v as f64)),
                None => Err(crate::value::error_value(format!(
                    "float() BigInt 超出 i64 范围: {} (可能原因：数值过大，无法精确转为 f64)", b
                ))),
            }
        }
        Value::BigFloat(b) => {
            // bigFloat -> f64，通过字符串中转尽量保留精度
            let s = format!("{}", b);
            Ok(Value::Float(s.parse::<f64>().unwrap_or(0.0)))
        }
        Value::Str(s) => s.parse::<f64>().map(Value::Float).map_err(|_| {
            crate::value::error_value(format!("float() 无法解析 '{}'", s))
        }),
        v => Err(crate::value::error_value(format!("float() 不支持类型 {}", v.type_name()))),
    }
}

/// bi_range 生成范围数组。
fn bi_range(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let (start, end, step) = match args.len() {
        1 => (0, args[0].to_int().ok_or_else(|| crate::value::error_value("range() 参数需为整数"))?, 1i64),
        2 => (
            args[0].to_int().ok_or_else(|| crate::value::error_value("range() 参数需为整数"))?,
            args[1].to_int().ok_or_else(|| crate::value::error_value("range() 参数需为整数"))?,
            1,
        ),
        3 => (
            args[0].to_int().ok_or_else(|| crate::value::error_value("range() 参数需为整数"))?,
            args[1].to_int().ok_or_else(|| crate::value::error_value("range() 参数需为整数"))?,
            args[2].to_int().ok_or_else(|| crate::value::error_value("range() 参数需为整数"))?,
        ),
        _ => return Err(crate::value::error_value("range() 需要 1-3 个参数")),
    };
    if step == 0 {
        return Err(crate::value::error_value("range() step 不能为 0"));
    }
    let mut v = Vec::new();
    if step > 0 {
        let mut i = start;
        while i < end {
            v.push(Value::Int(i));
            i += step;
        }
    } else {
        let mut i = start;
        while i > end {
            v.push(Value::Int(i));
            i += step;
        }
    }
    Ok(Value::Array(Arc::new(Mutex::new(v))))
}

/// bi_assert 断言。
fn bi_assert(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("assert() 需要至少 1 个参数"));
    }
    if !args[0].is_truthy() {
        let msg = if args.len() > 1 {
            args[1].to_str()
        } else {
            format!("assertion failed: value is falsy ({})", args[0].inspect())
        };
        return Err(crate::value::error_value(msg));
    }
    Ok(Value::Undefined)
}

/// bi_sleep 睡眠（毫秒）。
/// bi_sleep 睡眠指定秒数（支持小数）。
///
/// 用法：sleep(1.5) — 睡眠 1.5 秒
fn bi_sleep(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("sleep() 需要 1 个参数 (秒)"));
    }
    let secs = args[0].to_f64().ok_or_else(|| crate::value::error_value("sleep() 参数需为数字"))?;
    let dur = std::time::Duration::from_secs_f64(secs.max(0.0));
    std::thread::sleep(dur);
    Ok(Value::Undefined)
}

/// bi_sleep_ms 睡眠指定毫秒数（整数）。
///
/// 用法：sleepMs(500) — 睡眠 500 毫秒
fn bi_sleep_ms(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(crate::value::error_value("sleepMs() 需要 1 个参数 (毫秒)"));
    }
    let ms = args[0].to_int().ok_or_else(|| crate::value::error_value("sleepMs() 参数需为整数"))?;
    std::thread::sleep(std::time::Duration::from_millis(ms.max(0) as u64));
    Ok(Value::Undefined)
}

// ---- byte 构造 ----

/// bi_byte 构造 byte 值（0-255）。
///
/// byte(65) → Byte(65)。超出 0-255 报错。
fn bi_byte(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let v = bh::as_int(args, 0, "byte")?;
    if v < 0 || v > 255 {
        return Err(crate::value::error_value(format!(
            "byte() 值 {} 超出范围 (0-255; 可能原因：传入了非字节整数)", v,
        )));
    }
    Ok(Value::Byte(v as u8))
}

/// bi_new_map 创建空有序 Map。
fn bi_new_map(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Map(std::sync::Arc::new(std::sync::Mutex::new(crate::ord_map::OrdMap::new()))))
}

/// bi_new_string_builder 创建 StringBuilder（高效字符串构建器）。
///
/// 用法：
///   newStringBuilder()       — 空 builder
///   newStringBuilder("初始")  — 带初始内容
///
/// 通过通用函数操作：writeStr/writeBytes/len/toStr/clear/reset。
fn bi_new_string_builder(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let initial = if args.is_empty() {
        String::new()
    } else {
        args[0].to_str()
    };
    Ok(Value::StringBuilder(std::sync::Arc::new(std::sync::Mutex::new(initial))))
}

/// bi_clear 清空容器内容（不释放内存）。
///
/// 支持：stringBuilder、array、byteArray、map、ring。
/// 用法：clear(sb)
fn bi_clear(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "clear")?;
    match &args[0] {
        Value::StringBuilder(sb) => sb.lock().unwrap().clear(),
        Value::Array(a) => a.lock().unwrap().clear(),
        Value::ByteArray(b) => b.lock().unwrap().clear(),
        Value::Map(m) => m.lock().unwrap().clear(),
        Value::Native(n) => {
            // ring
            if let Some(r) = n.downcast_ref::<std::sync::Arc<std::sync::Mutex<crate::ring::Ring>>>() {
                r.lock().unwrap().clear();
            } else {
                return Err(crate::value::error_value(format!(
                    "clear() 不支持此 native 类型 (可能原因：不是 ring)",
                )));
            }
        }
        other => return Err(crate::value::error_value(format!(
            "clear() 不支持类型 {} (可能原因：参数应为 stringBuilder/array/byteArray/map/ring)", other.type_name(),
        ))),
    }
    Ok(Value::Undefined)
}

/// bi_reset 清空容器并释放内存（对 stringBuilder 效果最明显）。
///
/// 支持：stringBuilder、array、byteArray、map。
/// 用法：reset(sb)
fn bi_reset(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "reset")?;
    match &args[0] {
        Value::StringBuilder(sb) => {
            // 清空并 shrink_to_fit 释放内存
            let mut guard = sb.lock().unwrap();
            guard.clear();
            guard.shrink_to_fit();
        }
        Value::Array(a) => {
            let mut guard = a.lock().unwrap();
            guard.clear();
            guard.shrink_to_fit();
        }
        Value::ByteArray(b) => {
            let mut guard = b.lock().unwrap();
            guard.clear();
            guard.shrink_to_fit();
        }
        other => return Err(crate::value::error_value(format!(
            "reset() 不支持类型 {} (可能原因：参数应为 stringBuilder/array/byteArray)", other.type_name(),
        ))),
    }
    Ok(Value::Undefined)
}

/// bi_entries 返回对象的非函数键值对（过滤方法），每对为 [key, value]。
///
/// 用法：entries(obj) → [["k1", v1], ["k2", v2], ...]
/// 也支持 Map（不过滤，Map 本来就是纯数据）。
fn bi_entries(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "entries")?;
    let pairs: Vec<(String, Value)> = match &args[0] {
        Value::Object(o) => {
            o.lock().unwrap().snapshot().into_iter()
                .filter(|(_, v)| !matches!(v, Value::Func(_) | Value::Builtin(_)))
                .collect()
        }
        Value::Map(m) => m.lock().unwrap().snapshot(),
        _ => return Err(crate::value::error_value(format!(
            "entries() 需要 object 或 map，得到 {}", args[0].type_name(),
        ))),
    };
    let result: Vec<Value> = pairs.into_iter().map(|(k, v)| {
        Value::Array(std::sync::Arc::new(std::sync::Mutex::new(vec![Value::str(&k), v])))
    }).collect();
    Ok(Value::Array(std::sync::Arc::new(std::sync::Mutex::new(result))))
}

/// bi_data_keys 返回对象的非函数键（过滤方法）。
fn bi_data_keys(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "dataKeys")?;
    let keys: Vec<Value> = match &args[0] {
        Value::Object(o) => {
            o.lock().unwrap().snapshot().into_iter()
                .filter(|(_, v)| !matches!(v, Value::Func(_) | Value::Builtin(_)))
                .map(|(k, _)| Value::str(&k))
                .collect()
        }
        Value::Map(m) => m.lock().unwrap().keys().into_iter().map(|k| Value::str(&k)).collect(),
        _ => return Err(crate::value::error_value(format!(
            "dataKeys() 需要 object 或 map，得到 {}", args[0].type_name(),
        ))),
    };
    Ok(Value::Array(std::sync::Arc::new(std::sync::Mutex::new(keys))))
}

/// bi_data_values 返回对象的非函数值（过滤方法）。
fn bi_data_values(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "dataValues")?;
    let vals: Vec<Value> = match &args[0] {
        Value::Object(o) => {
            o.lock().unwrap().snapshot().into_iter()
                .filter(|(_, v)| !matches!(v, Value::Func(_) | Value::Builtin(_)))
                .map(|(_, v)| v)
                .collect()
        }
        Value::Map(m) => m.lock().unwrap().values(),
        _ => return Err(crate::value::error_value(format!(
            "dataValues() 需要 object 或 map，得到 {}", args[0].type_name(),
        ))),
    };
    Ok(Value::Array(std::sync::Arc::new(std::sync::Mutex::new(vals))))
}

/// bi_bytes_xor 批量 XOR：data 的每个字节与 key 的对应字节异或。
///
/// data 可以是 bytes 或 byteArray。key 可以是 bytes/byteArray/int(byte)。
/// 返回新的 bytes（不可变）。适合高效加密/解密。
fn bi_bytes_xor(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "bytesXor")?;
    bh::require_arg(args, 1, "bytesXor")?;
    let data = to_byte_vec(&args[0]).map_err(crate::value::error_value)?;
    let key = to_byte_vec(&args[1]).map_err(crate::value::error_value)?;
    if key.is_empty() {
        return Err(crate::value::error_value("bytesXor() key 不能为空"));
    }
    let result: Vec<u8> = data.iter().enumerate()
        .map(|(i, &b)| b ^ key[i % key.len()])
        .collect();
    Ok(Value::Bytes(std::sync::Arc::new(result)))
}

/// bi_bytes_xor_in_place 原地 XOR（修改 byteArray，不创建新对象）。
fn bi_bytes_xor_in_place(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "bytesXorInPlace")?;
    bh::require_arg(args, 1, "bytesXorInPlace")?;
    let key = to_byte_vec(&args[1]).map_err(crate::value::error_value)?;
    if key.is_empty() {
        return Err(crate::value::error_value("bytesXorInPlace() key 不能为空"));
    }
    match &args[0] {
        Value::ByteArray(b) => {
            let mut guard = b.lock().map_err(|e| crate::value::error_value(format!("锁异常: {}", e)))?;
            for (i, byte) in guard.iter_mut().enumerate() {
                *byte ^= key[i % key.len()];
            }
            Ok(args[0].clone())
        }
        _ => Err(crate::value::error_value("bytesXorInPlace() 第一个参数须为 byteArray")),
    }
}

/// to_byte_vec 将 Value 转为字节 Vec（bytes/byteArray/string/int）。
fn to_byte_vec(v: &Value) -> Result<Vec<u8>, String> {
    match v {
        Value::Bytes(b) => Ok(b.as_ref().to_vec()),
        Value::ByteArray(b) => Ok(b.lock().unwrap().clone()),
        Value::Str(s) => Ok(s.as_bytes().to_vec()),
        Value::Int(x) => {
            if *x < 0 || *x > 255 { return Err(format!("值 {} 超出字节范围 0-255", x)); }
            Ok(vec![*x as u8])
        }
        Value::Byte(x) => Ok(vec![*x]),
        _ => Err(format!("无法将 {} 转为字节", v.type_name())),
    }
}

/// bi_is_undefined 判断是否为 undefined（含旧称 nil）。
///
/// 缺参时返回 true（便于链式判空：`isUndefined(m["maybe"])`）。
/// 这是特殊保留的类型判断函数（缺参返回 true 的语义不同于 isType）。
fn bi_is_undefined(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Bool(matches!(args.get(0), Some(Value::Undefined) | None)))
}

/// bi_error 创建一个错误值。
///
/// 用法：error(msg) → Error 值
/// 错误值是普通值（不抛出），用于返回错误结果；配合 isError 判断。
/// 这符合 Sflang "一般返回错误对象为主" 的设计原则。
fn bi_error(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let msg = bh::as_str(args, 0, "error")?;
    Ok(crate::value::error_value(msg))
}

/// bi_is_error 判断是否为错误值。
fn bi_is_error(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Bool(matches!(args.get(0), Some(Value::Error(_)))))
}

// ---- TXERROR 错误字符串机制（对标 Charlang） ----
//
// Sflang 同时支持两种错误表示：
//   1. Error 对象（Value::Error）— 推荐方式，结构化
//   2. "TXERROR:xxx" 字符串 — 字符串形式的错误，便于跨边界传递
//
// 配套函数统一处理两种形式：
//   - isErr(v):       判断 v 是否为 Error 对象或 "TXERROR:" 开头的字符串
//   - isErrStr(v):    判断 v 是否为 "TXERROR:" 开头的字符串
//   - getErrStr(v):   提取错误信息字符串（Error 取 message，TXERROR 字符串去前缀）
//   - errStrf(fmt, args...): 格式化生成 "TXERROR:" 前缀的错误字符串
//   - errf(fmt, args...):    同 errStrf（别名）
//   - checkErr(v, ...):      若 v 是错误则打印并退出进程
//   - checkErrX(v, ...):     checkErr 的别名
//   - errToEmpty(v):         若 v 是错误则转为空字符串，否则原样返回
//   - trimErr(v, ...):       若 v 是错误则原样返回，否则去空白（错误不静默丢失）

/// TXERROR 前缀常量。
const TXERROR_PREFIX: &str = "TXERROR:";

/// is_err_value 内部辅助：判断 Value 是否为"错误样"值（Error 对象或 TXERROR 字符串）。
fn is_err_value(v: &Value) -> bool {
    match v {
        Value::Error(_) => true,
        Value::Str(s) => s.starts_with(TXERROR_PREFIX),
        _ => false,
    }
}

/// get_err_str 内部辅助：从错误样值提取错误信息字符串。
/// - Error 对象 → message（若已是 "error: xxx" 形式则去掉 "error: " 前缀）
/// - TXERROR 字符串 → 去掉 "TXERROR:" 前缀后的内容
/// - 非错误 → 值的字符串表示
fn extract_err_str(v: &Value) -> String {
    match v {
        Value::Error(e) => {
            // Error 对象的 message 可能以 "error: " 开头（VM 抛出时），去掉保持一致
            let msg = &e.message;
            if let Some(rest) = msg.strip_prefix("error: ") {
                rest.to_string()
            } else {
                msg.clone()
            }
        }
        Value::Str(s) => {
            // TXERROR:xxx → xxx
            s.strip_prefix(TXERROR_PREFIX).map(|r| r.to_string()).unwrap_or_else(|| s.to_string())
        }
        _ => v.to_str(),
    }
}

/// bi_is_err 判断是否为错误样值（Error 对象或 TXERROR 字符串）。
///
/// 这是统一判断函数，同时识别两种错误形式。
/// 别名：isErrX（与 Charlang 完全一致）。
fn bi_is_err(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Bool(args.get(0).map(is_err_value).unwrap_or(false)))
}

/// bi_is_err_str 判断是否为 TXERROR 字符串。
fn bi_is_err_str(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Bool(matches!(
        args.get(0),
        Some(Value::Str(s)) if s.starts_with(TXERROR_PREFIX)
    )))
}

/// bi_get_err_str 提取错误信息字符串。
///
/// 用法：getErrStr(v) → 字符串
/// - Error 对象 → message（去 "error: " 前缀）
/// - TXERROR 字符串 → 去 "TXERROR:" 前缀
/// - 其他 → to_str
fn bi_get_err_str(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    match args.get(0) {
        Some(v) => Ok(Value::str_from(extract_err_str(v))),
        None => Ok(Value::str_from(String::new())),
    }
}

/// bi_err_strf 格式化生成 TXERROR 错误字符串。
///
/// 用法：errStrf(format, args...) → "TXERROR:" + sprintf(format, args...)
/// 这是创建字符串形式错误的便捷方式。
fn bi_err_strf(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Ok(Value::str_from(TXERROR_PREFIX.to_string()));
    }
    let formatted = sprintf(args)?;
    Ok(Value::str_from(format!("{}{}", TXERROR_PREFIX, formatted)))
}

/// bi_err_to_empty 若 v 是错误样值则转为空字符串，否则原样返回。
///
/// 用于安全地处理可能为错误的值：错误时得到空串，非错误时保留原值。
fn bi_err_to_empty(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    match args.get(0) {
        Some(v) if is_err_value(v) => Ok(Value::str_from(String::new())),
        Some(v) => Ok(v.clone()),
        None => Ok(Value::str_from(String::new())),
    }
}

/// bi_check_err 若 v 是错误样值则打印错误信息并退出进程（退出码 1）。
///
/// 用法：checkErr(v) 或 checkErr(v, "-format=自定义格式 %v\n")
/// 默认格式："Error: %v\n"
/// 非错误时原样返回 v。
///
/// 对标 Charlang checkErrX/checkErr。
fn bi_check_err(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let v = match args.get(0) {
        Some(v) => v,
        None => return Err(crate::value::error_value("checkErr() 至少需要 1 个参数")),
    };
    if is_err_value(v) {
        // 解析可选的 -format= 参数
        let default_fmt = "Error: %v\n";
        let mut fmt = default_fmt.to_string();
        for i in 1..args.len() {
            if let Value::Str(s) = &args[i] {
                if let Some(rest) = s.strip_prefix("-format=") {
                    fmt = rest.to_string();
                }
            }
        }
        let err_msg = extract_err_str(v);
        let formatted = sprintf(&[Value::str_from(fmt), Value::str_from(err_msg)])?;
        // 打印到 stderr 并退出
        eprint!("{}", formatted);
        std::process::exit(1);
    }
    Ok(v.clone())
}

/// bi_trim_err 若 v 是错误样值则原样返回（不静默丢失错误），否则去空白。
///
/// 用法：trimErr(v) 或 trimErr(v, cutset...)
/// 这是对 trim 的安全增强：避免 trim 意外吞掉错误信息。
fn bi_trim_err(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let v = match args.get(0) {
        Some(v) => v,
        None => return Err(crate::value::error_value("trimErr() 至少需要 1 个参数")),
    };
    // 错误样值原样返回（不丢失错误）
    if is_err_value(v) {
        return Ok(v.clone());
    }
    // undefined 转空字符串
    if matches!(v, Value::Undefined) {
        return Ok(Value::str_from(String::new()));
    }
    let s = bh::as_str(args, 0, "trimErr")?;
    // 收集 cutset 字符
    let cutsets: Vec<&str> = args[1..].iter().filter_map(|a| match a {
        Value::Str(s) => Some(&**s),
        _ => None,
    }).collect();
    let trimmed = if cutsets.is_empty() {
        s.trim().to_string()
    } else {
        let chars: Vec<char> = cutsets.iter().flat_map(|c| c.chars()).collect();
        s.trim_matches(|c| chars.contains(&c)).to_string()
    };
    Ok(Value::str_from(trimmed))
}

// ---- undefined 配套内置函数 ----
//
// 设计目标（对标 Charlang 的 nilToEmpty/trim，并为 AI 友好补强）：
//   - undefToEmpty: undefined → 空字符串，其余 → to_str()
//   - default(x, d): x 为 falsy 时返回 d（宽松兜底，含 0/""）
//   - defaultUndef(x, d): 仅 x 为 undefined 时返回 d（严格空合并，0/"" 不触发）
//   - explainUndef(name): 返回某名字为何为 undefined 的诊断字符串（AI 定位用）

/// bi_undef_to_empty 将 undefined 转为空字符串，其余值转为 to_str。
///
/// 用途：把"可能为 undefined"的值安全接入字符串处理。
/// 等价 Charlang 的 nilToEmpty。
fn bi_undef_to_empty(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    match args.get(0) {
        Some(Value::Undefined) | None => Ok(Value::str("")),
        Some(v) => Ok(Value::str_from(v.to_str())),
    }
}

/// bi_default 宽松兜底：x 为 falsy（undefined/0/""/空容器）时返回 d，否则返回 x。
///
/// 注册名为 `defaultVal`（原 `default`，因 `default` 已成为 switch 关键字而改名）。
/// 语义不变：0 和 "" 也会触发兜底（与 Python 的 `or` 一致），等价于 `x || d` 运算符。
/// 若只想对 undefined 兜底，请用 defaultUndef（等价于 `x ?? d`）。
fn bi_default(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let x = args.get(0).cloned().unwrap_or(Value::Undefined);
    let d = args.get(1).cloned().unwrap_or(Value::Undefined);
    if x.is_truthy() {
        Ok(x)
    } else {
        Ok(d)
    }
}

/// bi_default_undef 严格空合并：仅当 x 为 undefined 时返回 d，否则返回 x。
///
/// 对应其他语言的 `??` 运算符：0/""/空数组 都视为有效值，不触发兜底。
fn bi_default_undef(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let x = args.get(0).cloned().unwrap_or(Value::Undefined);
    let d = args.get(1).cloned().unwrap_or(Value::Undefined);
    if matches!(x, Value::Undefined) {
        Ok(d)
    } else {
        Ok(x)
    }
}

/// bi_explain_undef 返回某名字"为何为 undefined"的诊断字符串（AI 友好）。
///
/// 由于本实现读取未定义变量直接返回 undefined（不抛错），脚本难以察觉拼写错误。
/// 此函数让 AI/用户主动诊断：返回包含名字、是否为预定义全局、相似已声明变量等
/// 信息的提示。缺省或非字符串参数时给出通用说明。
fn bi_explain_undef(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let name = match args.get(0) {
        Some(Value::Str(s)) => s.as_ref(),
        _ => return Ok(Value::str("explainUndef 需传入变量名（字符串）。读取未定义变量在 Sflang 中返回 undefined（非错误）。可用 isUndefined(x) 判空，default(x, d) 或 defaultUndef(x, d) 提供默认值。")),
    };
    // 检查该名字是否已绑定（全局或内置）
    let bound = vm.get_global(name).is_some();
    if bound {
        return Ok(Value::str_from(format!(
            "'{}' 当前已绑定（非 undefined）。若仍得到 undefined，请检查是否读到了 map 缺键或函数无返回值的情形。",
            name,
        )));
    }
    // 预定义全局名单（与 VM::new / sf/main 设置的一致）
    let predefined = ["piG", "eG", "argsG", "scriptPathG"];
    let is_predefined = predefined.contains(&name);
    // 收集相似名字：取全局中编辑距离最近的前 3 个（简单实现，避免大改依赖）
    let globals = vm.globals_handle();
    let g = globals.lock().unwrap();
    let mut similar: Vec<(String, usize)> = g.keys()
        .map(|k| (k.clone(), lev(name, k)))
        .filter(|(_, d)| *d <= name.len().max(1) / 2 + 1)
        .collect();
    similar.sort_by_key(|(_, d)| *d);
    let hints: Vec<String> = similar.into_iter().take(3).map(|(k, _)| k).collect();
    let mut msg = format!(
        "'{}' 未定义（读取返回 undefined）。{}",
        name,
        if is_predefined { "它是预定义全局变量，但当前未赋值（如在 REPL 中 argsG/scriptPathG 未设置）。" } else { "可能原因：变量未声明、拼写错误，或为 map 缺键/函数无返回值。" },
    );
    if !hints.is_empty() {
        msg.push_str(&format!(" 作用域内相似名字：{}。", hints.join(", ")));
    }
    msg.push_str(" 可用 isUndefined(x) 判空；default(x, d) / defaultUndef(x, d) 提供默认值。");
    Ok(Value::str_from(msg))
}

// ---- 通用类型判断（取代零散的 isXxx 谓词） ----

/// bi_is_type 通用类型判断：按类型名字符串判断。
///
/// 用法：isType(v, "string") → bool
///
/// 支持的类型名（与 type_name_ex 一致）：
///   基础类型：undefined, int, float, bool, string, bytes, byteArray, array,
///             object, function, builtin, error, native, bigInt, bigFloat,
///             datetime, file, byte, map
///   Native 细分：ring, channel, mutex, rwmutex, waitGroup, semaphore, code, ref, regex
///
/// 这取代零散的 isInt/isString/isArray 等谓词，统一为一个入口。
fn bi_is_type(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "isType")?;
    let type_name = bh::as_str(args, 1, "isType")?;
    // 用 type_name_ex 获取细化类型名，做大小写不敏感比较
    let actual = args[0].type_name_ex();
    let result = actual.eq_ignore_ascii_case(type_name);
    Ok(Value::Bool(result))
}

/// bi_is_type_code 通用类型判断：按类型数字编码判断。
///
/// 用法：isTypeCode(v, 4) → bool   // 4 = string
///
/// 数字编码与 TypeCode 枚举一致（0-18，详见 typeCode(v)）。
/// 对于 Native 细分类型（ring 等），编码均为 11（Native），需用 isType 按名字判断。
fn bi_is_type_code(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "isTypeCode")?;
    let code = bh::as_int(args, 1, "isTypeCode")?;
    let actual = args[0].type_code() as i64;
    Ok(Value::Bool(actual == code))
}

/// utf8_char_len 根据 UTF-8 首字节返回字符长度。
fn utf8_char_len(b: u8) -> usize {
    if b < 0x80 { 1 }
    else if b < 0xC0 { 1 }
    else if b < 0xE0 { 2 }
    else if b < 0xF0 { 3 }
    else { 4 }
}

/// lev 计算两字符串的 Levenshtein 编辑距离（用于相似名字提示）。
fn lev(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());
    if m == 0 { return n; }
    if n == 0 { return m; }
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut cur = vec![0usize; n + 1];
    for i in 1..=m {
        cur[0] = i;
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[n]
}

// ---- 实用函数 ----

/// bi_uuid 生成 UUID v4 字符串（如 "550e8400-e29b-41d4-a716-446655440000"）。
///
/// 用随机数填充（randInt 已有的 xorshift），版本位设为 4，变体位设为 RFC 4122。
fn bi_uuid(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEED: AtomicU64 = AtomicU64::new(0x1234_5678_9ABC_DEF0);
    let next = || {
        let mut s = SEED.load(Ordering::Relaxed);
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        SEED.store(s, Ordering::Relaxed);
        s
    };
    // 生成 16 字节
    let mut bytes = [0u8; 16];
    for chunk in bytes.chunks_mut(8) {
        let n = next();
        for (i, b) in chunk.iter_mut().enumerate() {
            *b = (n >> (i * 8)) as u8;
        }
    }
    // 版本位（byte 6 高 4 位 = 4），变体位（byte 8 高 2 位 = 10）
    bytes[6] = (bytes[6] & 0x0F) | 0x40;
    bytes[8] = (bytes[8] & 0x3F) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    Ok(Value::str_from(format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32],
    )))
}

/// bi_random_str 生成长度为 n 的随机字母数字字符串。
fn bi_random_str(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let n = bh::as_int(args, 0, "randomStr")?;
    if n < 0 {
        return Err(crate::value::error_value("randomStr() 长度不能为负"));
    }
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut out = String::with_capacity(n as usize);
    for _ in 0..n {
        let r = crate::builtins_math::next_rand() as usize % CHARS.len();
        out.push(CHARS[r] as char);
    }
    Ok(Value::str_from(out))
}

/// bi_values 返回 object 的所有值（array）。
fn bi_values(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "values")?;
    match &args[0] {
        Value::Object(o) => {
            let vals: Vec<Value> = o.lock().unwrap().snapshot().into_iter().map(|(_, v)| v).collect();
            Ok(Value::Array(std::sync::Arc::new(std::sync::Mutex::new(vals))))
        }
        Value::Array(a) => Ok(Value::Array(a.clone())), // 数组的 values 即自身
        _ => Err(crate::value::error_value(format!(
            "values() 需要 object 或 array，得到 {}", args[0].type_name(),
        ))),
    }
}

/// bi_has_key 判断 object 是否包含某键。
fn bi_has_key(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let key = bh::as_str(args, 1, "hasKey")?;
    match &args[0] {
        Value::Object(o) => Ok(Value::Bool(o.lock().unwrap().has(key))),
        Value::Map(m) => Ok(Value::Bool(m.lock().unwrap().has(key))),
        v => Err(crate::value::error_value(format!(
            "hasKey() 第 1 个参数应为 object 或 map，得到 {}", v.type_name(),
        ))),
    }
}

/// bi_deep_clone 深拷贝值（递归复制 array/object/byteArray）。
fn bi_deep_clone(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "deepClone")?;
    Ok(deep_clone_value(&args[0]))
}

/// bi_new_object 创建以 proto 为原型的空 object（暴露原型链到脚本层，用于方法共享）。
fn bi_new_object(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let proto = bh::as_object(args, 0, "newObject")?;
    Ok(Value::Object(crate::object_map::new_map_with_proto(proto.clone())))
}

/// deep_clone_value 递归克隆（内部辅助）。
fn deep_clone_value(v: &Value) -> Value {
    match v {
        Value::Array(a) => {
            let cloned: Vec<Value> = a.lock().unwrap().iter().map(deep_clone_value).collect();
            Value::Array(std::sync::Arc::new(std::sync::Mutex::new(cloned)))
        }
        Value::Object(o) => {
            let snap = o.lock().unwrap().snapshot();
            let mut new_map = crate::object_map::Map::new();
            for (k, val) in snap {
                new_map.set(k, deep_clone_value(&val));
            }
            Value::Object(std::sync::Arc::new(std::sync::Mutex::new(new_map)))
        }
        Value::ByteArray(b) => {
            let cloned = b.lock().unwrap().clone();
            Value::ByteArray(std::sync::Arc::new(std::sync::Mutex::new(cloned)))
        }
        other => other.clone(), // 不可变值（int/string/bytes 等）直接 clone
    }
}

/// bi_filter 用谓词函数过滤数组，返回新数组。
///
/// filter(arr, fn) → 仅保留 fn(x) 为 truthy 的元素。
fn bi_filter(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let arr = bh::as_array(args, 0, "filter")?;
    bh::require_arg(args, 1, "filter")?;
    let pred = args[1].clone();
    let snap = arr.lock().unwrap().clone();
    let mut result = Vec::new();
    for item in snap {
        let keep = vm.call_function_value(pred.clone(), vec![item.clone()])?;
        if keep.is_truthy() {
            result.push(item);
        }
    }
    Ok(Value::Array(std::sync::Arc::new(std::sync::Mutex::new(result))))
}

/// bi_map 用函数映射数组的每个元素，返回新数组。
///
/// map(arr, fn) → [fn(a[0]), fn(a[1]), ...]
fn bi_map(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let arr = bh::as_array(args, 0, "map")?;
    bh::require_arg(args, 1, "map")?;
    let f = args[1].clone();
    let snap = arr.lock().unwrap().clone();
    let mut result = Vec::with_capacity(snap.len());
    for item in snap {
        let mapped = vm.call_function_value(f.clone(), vec![item])?;
        result.push(mapped);
    }
    Ok(Value::Array(std::sync::Arc::new(std::sync::Mutex::new(result))))
}

/// bi_find 查找数组中第一个满足条件的元素，返回该元素或 undefined。
///
/// find(arr, fn) → 第一个 fn(x) 为真的元素，无则 undefined
fn bi_find(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let arr = bh::as_array(args, 0, "find")?;
    bh::require_arg(args, 1, "find")?;
    let pred = args[1].clone();
    let snap = arr.lock().unwrap().clone();
    for item in snap {
        let matched = vm.call_function_value(pred.clone(), vec![item.clone()])?;
        if matched.is_truthy() {
            return Ok(item);
        }
    }
    Ok(Value::Undefined)
}

/// bi_sprintf 格式化字符串（同 printf 的格式，但返回字符串而非打印）。
fn bi_sprintf(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let s = sprintf(args)?;
    Ok(Value::str_from(s))
}

/// bi_adjust_float 消除浮点计算精度误差。
fn bi_adjust_float(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let x = bh::as_float(args, 0, "adjustFloat")?;
    let prec = if args.len() > 1 {
        bh::as_int(args, 1, "adjustFloat")? as usize
    } else {
        10
    };
    let formatted = format!("{:.*}", prec, x);
    let result = formatted.parse::<f64>().map_err(|_| crate::value::error_value(
        format!("adjustFloat() 解析失败: {}", formatted),
    ))?;
    Ok(Value::Float(result))
}

/// bi_pass 空操作占位符（对标 Charlang pass()）。
fn bi_pass(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Undefined)
}

/// bi_plt 打印类型+值（对标 Charlang plt）。
/// 输出格式：(类型名)值
fn bi_plt(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let out = _vm.output_handle();
    for v in args {
        writeln!(out.lock().unwrap(), "({}) {}", v.type_name(), v.inspect())
            .map_err(|e| crate::value::error_value(e.to_string()))?;
    }
    Ok(Value::Undefined)
}

/// bi_get_param 从 argsG 中取第 idx 个参数，不存在则返回默认值。
/// 用法：getParam(argsG, index) 或 getParam(argsG, index, default)
fn bi_get_param(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    if args.is_empty() {
        return Ok(Value::Undefined);
    }
    let arr = match &args[0] {
        Value::Array(a) => a,
        _ => return Ok(args.get(2).cloned().unwrap_or(Value::Undefined)),
    };
    let idx = if args.len() > 1 { bh::as_int(args, 1, "getParam")? as usize } else { 0 };
    let guard = arr.lock().unwrap();
    Ok(guard.get(idx).cloned().unwrap_or_else(|| {
        args.get(2).cloned().unwrap_or(Value::Undefined)
    }))
}

/// bi_get_switch 从参数数组中按 --key=value 或 -key=value 格式提取开关值。
///
/// 用法：
///   getSwitch(argsG, "--host=", "localhost")  → 匹配 --host=xxx 返回 xxx
///   getSwitch(argsG, "-port=", "22")          → 匹配 -port=xxx 返回 xxx
///
/// key 参数应包含前缀（- 或 --）和等号（=）。
/// 如果找不到匹配，返回 default。
fn bi_get_switch(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let arr = match args.get(0) {
        Some(Value::Array(a)) => a.lock().unwrap().clone(),
        _ => return Ok(args.get(2).cloned().unwrap_or(Value::Undefined)),
    };
    let key = args.get(1).map(|v| v.to_str()).unwrap_or_default();
    let default = args.get(2).cloned().unwrap_or(Value::Undefined);

    for arg in &arr {
        let s = arg.to_str();
        if s.starts_with(&key) {
            let val = &s[key.len()..];
            return Ok(Value::str(val));
        }
    }
    Ok(default)
}

/// bi_get_all_switches 从参数数组中提取所有匹配 --key=value 的值（可多个同名）。
///
/// 用法：getAllSwitches(argsG, "--attach=") → ["file1.pdf", "file2.xlsx"]
/// 返回所有匹配值的数组。无匹配时返回空数组。
fn bi_get_all_switches(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use std::sync::{Arc, Mutex};
    let arr = match args.get(0) {
        Some(Value::Array(a)) => a.lock().unwrap().clone(),
        _ => return Ok(Value::Array(Arc::new(Mutex::new(Vec::new())))),
    };
    let key = args.get(1).map(|v| v.to_str()).unwrap_or_default();

    let mut results: Vec<Value> = Vec::new();
    for arg in &arr {
        let s = arg.to_str();
        if s.starts_with(&key) {
            results.push(Value::str_from(s[key.len()..].to_string()));
        }
    }
    Ok(Value::Array(Arc::new(Mutex::new(results))))
}

/// bi_if_switch_exists 检查参数数组中是否存在某个开关（布尔型，无值）。
///
/// 用法：ifSwitchExists(argsG, "--verbose")  → true/false
///       ifSwitchExists(argsG, "-v")         → true/false
fn bi_if_switch_exists(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let arr = match args.get(0) {
        Some(Value::Array(a)) => a.lock().unwrap().clone(),
        _ => return Ok(Value::Bool(false)),
    };
    let key = args.get(1).map(|v| v.to_str()).unwrap_or_default();
    Ok(Value::Bool(arr.iter().any(|arg| arg.to_str() == key)))
}
fn bi_compile(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let src = bh::as_str(args, 0, "compile")?;
    let tokens = match crate::lexer::tokenize(src, "<compile>") {
        Ok(t) => t,
        Err(e) => return Ok(crate::value::error_value(format!("compile() 词法错误: {}", e))),
    };
    let prog = match crate::parser::parse_program(tokens, "<compile>") {
        Ok(p) => p,
        Err(e) => return Ok(crate::value::error_value(format!("compile() 语法错误: {}", e))),
    };
    let code = match crate::compiler::compile(&prog) {
        Ok(c) => c,
        Err(e) => return Ok(crate::value::error_value(format!("compile() 编译错误: {}", e))),
    };
    Ok(Value::Native(std::sync::Arc::new(std::sync::Arc::new(code))))
}

/// bi_run_code 执行编译后的 Code 对象，返回结果。
/// 运行错误以 error 值返回（不抛出），与 compile() 行为一致。
fn bi_run_code(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use std::sync::Arc;
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "runCode")?;
    let code_arc: Arc<crate::opcode::Code> = match &args[0] {
        Value::Native(n) => match n.downcast_ref::<Arc<crate::opcode::Code>>() {
            Some(c) => c.clone(),
            None => return Ok(crate::value::error_value("runCode() 参数不是编译后的代码对象")),
        },
        _ => return Ok(crate::value::error_value("runCode() 参数应为 compile() 的返回值")),
    };
    // vm.run 返回 Result；错误转为 error 值返回（不抛出）
    match vm.run(code_arc) {
        Ok(v) => Ok(v),
        Err(e) => Ok(e),
    }
}

/// bi_new_ref 创建引用容器，包装一个初始值。
///
/// 用法：newRef(value) → 返回引用对象
/// 引用是独立可变容器，函数传参后可修改容器内的值。
fn bi_new_ref(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "newRef")?;
    Ok(Value::Native(std::sync::Arc::new(std::sync::Arc::new(
        std::sync::Mutex::new(args[0].clone()),
    ))))
}

/// bi_get_value_by_ref 读取引用容器内的值。
///
/// 用法：getValueByRef(ref) → 返回引用内的值
fn bi_get_value_by_ref(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "getValueByRef")?;
    match &args[0] {
        Value::Native(n) => {
            if let Some(cell) = n.downcast_ref::<std::sync::Arc<std::sync::Mutex<Value>>>() {
                Ok(cell.lock().unwrap().clone())
            } else {
                Err(crate::value::error_value("getValueByRef() 参数不是引用对象（用 newRef 创建）"))
            }
        }
        v => Err(crate::value::error_value(format!(
            "getValueByRef() 参数应为引用，得到 {}", v.type_name(),
        ))),
    }
}

/// bi_set_value_by_ref 设置引用容器内的值。
///
/// 用法：setValueByRef(ref, newValue) → 返回 undefined
fn bi_set_value_by_ref(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "setValueByRef")?;
    bh::require_arg(args, 1, "setValueByRef")?;
    match &args[0] {
        Value::Native(n) => {
            if let Some(cell) = n.downcast_ref::<std::sync::Arc<std::sync::Mutex<Value>>>() {
                *cell.lock().unwrap() = args[1].clone();
                Ok(Value::Undefined)
            } else {
                Err(crate::value::error_value("setValueByRef() 第一个参数不是引用对象（用 newRef 创建）"))
            }
        }
        v => Err(crate::value::error_value(format!(
            "setValueByRef() 第一个参数应为引用，得到 {}", v.type_name(),
        ))),
    }
}

/// bi_to_kmg 将数字转为带单位的易读字符串（K/M/G/T）。
///
/// 用法：toKMG(n) 或 toKMG(n, decimals)
/// 默认保留 2 位小数。1024 进制（KB = 1024 bytes）。
/// 例：toKMG(1536) → "1.50K"，toKMG(1048576) → "1.00M"
fn bi_to_kmg(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let n = bh::as_float(args, 0, "toKMG")?;
    let decimals = match args.get(1) {
        Some(Value::Int(d)) => *d as usize,
        _ => 2,
    };
    let units = ["", "K", "M", "G", "T", "P"];
    let mut size = n.abs();
    let mut idx = 0;
    while size >= 1024.0 && idx < units.len() - 1 {
        size /= 1024.0;
        idx += 1;
    }
    Ok(Value::str_from(format!("{:.*}{}", decimals, size, units[idx])))
}

/// bi_dump_var 转储变量详细信息，返回多行诊断字符串。
///
/// 输出包含：类型名、类型码、值摘要。
/// 用于调试与 AI 定位问题。
fn bi_dump_var(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    bh::require_arg(args, 0, "dumpVar")?;
    let v = &args[0];
    let info = format!(
        "type: {}\ntypeCode: {}\nvalue: {}",
        v.type_name_ex(),
        v.type_code() as u32,
        v.inspect(),
    );
    Ok(Value::str_from(info))
}

/// bi_globals 列出所有全局变量名，返回 array<string>。
///
/// 用于反射与调试。
fn bi_globals(vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    let g = vm.globals_handle();
    let guard = g.lock().unwrap();
    let names: Vec<Value> = guard.keys().map(|k| Value::str_from(k.clone())).collect();
    Ok(Value::Array(std::sync::Arc::new(std::sync::Mutex::new(names))))
}

/// bi_show_table 将二维数组渲染为对齐的 ASCII 表格字符串。
///
/// 用法：
///   showTable(data)            — data 第一行作为表头
///   showTable(data, opts)      — opts 为 map，支持 header(默认true)、sep(默认"|")
///
/// data: Array of Array，每行元素会转为字符串显示
/// 返回：表格字符串（不直接打印，调用方可用 println 输出）
///
/// 例：
///   showTable([["姓名","年龄"],["张三",20],["李四",25]])
///   →
///   +------+----+
///   | 姓名 | 年龄 |
///   +------+----+
///   | 张三 | 20  |
///   | 李四 | 25  |
///   +------+----+
fn bi_show_table(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let arr = bh::as_array(args, 0, "showTable")?;
    let snapshot = arr.lock().unwrap().clone();

    if snapshot.is_empty() {
        return Ok(Value::str_from("(empty table)".to_string()));
    }

    // 解析可选第二参数 opts（map/object），支持 header 和 sep
    let mut header = true;
    let mut sep = "|".to_string();
    if args.len() > 1 {
        match &args[1] {
            Value::Map(m) => {
                let g = m.lock().unwrap();
                if let Some(Value::Bool(b)) = g.get("header") {
                    header = b;
                }
                if let Some(Value::Str(s)) = g.get("sep") {
                    sep = s.to_string();
                }
            }
            Value::Object(o) => {
                let g = o.lock().unwrap();
                if let Some(Value::Bool(b)) = g.data.get("header") {
                    header = *b;
                }
                if let Some(Value::Str(s)) = g.data.get("sep") {
                    sep = s.to_string();
                }
            }
            _ => {}
        }
    }

    // 把每行转为 Vec<String>，校验每行是数组
    let mut rows: Vec<Vec<String>> = Vec::with_capacity(snapshot.len());
    for (i, row) in snapshot.iter().enumerate() {
        match row {
            Value::Array(r) => {
                let cells: Vec<String> = r.lock().unwrap().iter().map(|v| v.to_str()).collect();
                rows.push(cells);
            }
            v => {
                return Err(crate::value::error_value(format!(
                    "showTable() 第 {} 行不是数组 (得到 {})，可能原因：每行必须是一维数组",
                    i, v.type_name(),
                )));
            }
        }
    }

    // 计算每列最大宽度
    let n_cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if n_cols == 0 {
        return Ok(Value::str_from("(empty table)".to_string()));
    }
    let mut widths = vec![0usize; n_cols];
    for r in &rows {
        for (i, c) in r.iter().enumerate() {
            let w = c.chars().count();
            if w > widths[i] {
                widths[i] = w;
            }
        }
    }

    // 渲染：边框 + 数据行
    let pad = |s: &str, w: usize| -> String {
        let len = s.chars().count();
        if len >= w {
            s.to_string()
        } else {
            format!("{}{}", s, " ".repeat(w - len))
        }
    };

    let border = {
        let mut b = String::from("+");
        for &w in &widths {
            b.push_str(&"-".repeat(w + 2));
            b.push('+');
        }
        b
    };

    let mut out = String::new();
    out.push_str(&border);
    out.push('\n');

    for (idx, r) in rows.iter().enumerate() {
        if header && idx == 0 {
            // 表头行
            out.push_str(&sep);
            for (i, &w) in widths.iter().enumerate() {
                let cell = r.get(i).map(|s| s.as_str()).unwrap_or("");
                out.push(' ');
                out.push_str(&pad(cell, w));
                out.push(' ');
                out.push_str(&sep);
            }
            out.push('\n');
            out.push_str(&border);
            out.push('\n');
        } else {
            out.push_str(&sep);
            for (i, &w) in widths.iter().enumerate() {
                let cell = r.get(i).map(|s| s.as_str()).unwrap_or("");
                out.push(' ');
                out.push_str(&pad(cell, w));
                out.push(' ');
                out.push_str(&sep);
            }
            out.push('\n');
        }
    }
    out.push_str(&border);

    Ok(Value::str_from(out))
}

// ---- 成员反射函数 ----
//
// 设计要点：
//   - getMember: 反射式读取 Object/Map 的成员值（字符串 key）
//   - setMember: 反射式设置 Object/Map 的成员值（修改原对象）
//   - callMethod: 调用 Object 上的方法（沿原型链查找）
//   - 与 obj.key / obj.key = v / obj.method(args) 的区别：
//     内置函数接收动态 key 字符串，便于反射式编程

/// bi_get_member 获取对象/Map 的成员值。
///
/// 用法：getMember(obj, key) → value 或 undefined
///
/// obj 为 Object 或 Map，key 为字符串。
/// Object 沿原型链查找；Map 仅查自身（Map 本就是纯数据容器）。
/// 不存在时返回 undefined（不报错），便于链式判空。
fn bi_get_member(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let key = bh::as_str(args, 1, "getMember")?;
    match &args[0] {
        Value::Object(o) => {
            // 沿原型链查找
            Ok(o.lock().unwrap().get_proto(key).unwrap_or(Value::Undefined))
        }
        Value::Map(m) => {
            // Map 不支持原型链，仅查自身
            Ok(m.lock().unwrap().get(key).unwrap_or(Value::Undefined))
        }
        v => Err(crate::value::error_value(format!(
            "getMember() 第 1 个参数应为 object 或 map，得到 {} (可能原因：参数顺序错误，正确顺序 getMember(obj, key))",
            v.type_name(),
        ))),
    }
}

/// bi_set_member 设置对象/Map 的成员值（原地修改）。
///
/// 用法：setMember(obj, key, value) → undefined
///
/// obj 为 Object 或 Map，key 为字符串。
/// 仅写入自身（不沿原型链），与 obj.key = v 语义一致。
fn bi_set_member(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let key = bh::as_str(args, 1, "setMember")?;
    bh::require_arg(args, 2, "setMember")?;
    let val = args[2].clone();

    match &args[0] {
        Value::Object(o) => {
            o.lock().unwrap().set(key.to_string(), val);
            Ok(Value::Undefined)
        }
        Value::Map(m) => {
            m.lock().unwrap().set(key.to_string(), val);
            Ok(Value::Undefined)
        }
        v => Err(crate::value::error_value(format!(
            "setMember() 第 1 个参数应为 object 或 map，得到 {} (可能原因：参数顺序错误，正确顺序 setMember(obj, key, value))",
            v.type_name(),
        ))),
    }
}

/// bi_call_method 调用对象的方法（沿原型链查找）。
///
/// 用法：
///   callMethod(obj, methodName)              — 无参数调用
///   callMethod(obj, methodName, argsArray)    — 带参数调用
///
/// 先在 Object 上沿原型链查找 methodName，找到则调用。
/// 调用时 obj 作为隐式 self（第一个参数）传入，args 数组中的元素作为后续参数。
/// 如果对象没有该方法，返回错误。
///
/// 与 obj.method(args) 的区别：methodName 为动态字符串，便于反射式调用。
fn bi_call_method(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use crate::builtins_helpers as bh;
    let obj = match args.get(0) {
        Some(v) => v.clone(),
        None => return Err(crate::value::error_value(
            "callMethod() 需要至少 2 个参数 (可能原因：参数缺失)",
        )),
    };
    let method_name = bh::as_str(args, 1, "callMethod")?;

    // 收集调用参数（obj 作为第一个 self 参数）
    let mut call_args: Vec<Value> = vec![obj.clone()];
    if let Some(args_val) = args.get(2) {
        match args_val {
            Value::Array(a) => {
                let items: Vec<Value> = a.lock().unwrap().clone();
                call_args.extend(items);
            }
            other => {
                // 非数组的单个值作为单个参数
                call_args.push(other.clone());
            }
        }
    }

    // 在 Object 上沿原型链查找方法
    let method = match &obj {
        Value::Object(o) => {
            match o.lock().unwrap().get_proto(method_name) {
                Some(v) => v,
                None => return Err(crate::value::error_value(format!(
                    "callMethod() 对象上找不到方法 '{}' (可能原因：方法名拼写错误或未在原型链上定义)",
                    method_name,
                ))),
            }
        }
        v => return Err(crate::value::error_value(format!(
            "callMethod() 第 1 个参数应为 object，得到 {} (可能原因：参数顺序错误，正确顺序 callMethod(obj, methodName, args?))",
            v.type_name(),
        ))),
    };

    // 调用方法（self 作为第一个参数）
    vm.call_function_value(method, call_args)
}

/// bi_help Help 系统：查阅内置函数文档与分类列表。
///
/// 三种调用形式：
///   help()          → 按分类列出所有内置函数（多行字符串）
///   help("funcName")→ 该函数的完整文档（签名/参数/返回/示例/常见错误）
///   help("category")→ 该分类下所有函数列表（如 help("regex")）
///
/// 设计目标：让 AI 和人类能自省内置函数，无需查阅外部文档。
fn bi_help(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    // 无参：按分类列出所有函数
    if args.is_empty() {
        let cats = vm.builtin_categories();
        let mut out = String::new();
        out.push_str(&format!("Sflang 内置函数（共 {} 个，按分类列出）：\n", vm.builtin_names().len()));
        out.push_str("用 help(\"函数名\") 查看详细文档，如 help(\"regFind\")。\n\n");
        for (cat, names) in &cats {
            out.push_str(&format!("== {}（{}）==\n", cat, names.len()));
            // 每行最多 6 个函数名，避免过长
            for chunk in names.chunks(6) {
                out.push_str("  ");
                out.push_str(&chunk.join(", "));
                out.push('\n');
            }
        }
        return Ok(Value::str_from(out));
    }

    // 有参：查询函数或分类
    let key = match args.get(0) {
        Some(v) => v.to_str(),
        None => return Ok(Value::str("")),
    };

    // 1. 先尝试作为函数名查询文档
    if let Some(doc) = vm.builtin_doc(&key) {
        return Ok(Value::str_from(format_builtin_doc(&key, doc)));
    }
    // 2. 函数存在但无文档
    if vm.builtin_exists(&key) {
        let mut out = format!("{}\n（该函数暂无详细文档）\n\n", key);
        out.push_str("提示：可用 help(\"分类\") 查看同类函数，例如 help(\"regex\")。\n");
        out.push_str("     常见分类：string, regex, array, math, file, json, encode, datetime, system。\n");
        return Ok(Value::str_from(out));
    }
    // 3. 尝试作为分类名
    let cats = vm.builtin_categories();
    let lower_key = key.to_lowercase();
    if let Some((_, names)) = cats.iter().find(|(c, _)| c.to_lowercase() == lower_key) {
        let mut out = format!("== {}（{} 个函数）==\n", key, names.len());
        for name in names {
            out.push_str("  ");
            out.push_str(name);
            // 若该函数有文档，标注简介首行
            if let Some(doc) = vm.builtin_doc(name) {
                out.push_str(" — ");
                out.push_str(doc.summary);
            }
            out.push('\n');
        }
        return Ok(Value::str_from(out));
    }
    // 4. 模糊匹配：找名字包含 key 的函数
    let all = vm.builtin_names();
    let matches: Vec<&&str> = all.iter().filter(|n| n.to_lowercase().contains(&lower_key)).collect();
    if !matches.is_empty() {
        let mut out = format!("未找到函数或分类 '{}'，但找到以下相似的：\n", key);
        for m in matches.iter().take(20) {
            out.push_str("  ");
            out.push_str(m);
            out.push('\n');
        }
        if matches.len() > 20 {
            out.push_str(&format!("  ...（共 {} 个匹配）\n", matches.len()));
        }
        return Ok(Value::str_from(out));
    }

    Err(crate::value::error_value(format!(
        "help() 未找到函数或分类 '{}' (可能原因：名称拼写错误；用 help() 无参查看全部分类)",
        key
    )))
}

/// format_builtin_doc 将 BuiltinDoc 格式化为人类与 AI 友好的多行文档字符串。
fn format_builtin_doc(name: &str, doc: &crate::function::BuiltinDoc) -> String {
    let mut out = String::new();
    out.push_str(doc.signature);
    out.push_str("\n分类: ");
    out.push_str(doc.category);
    out.push_str("\n\n");
    out.push_str(doc.summary);
    out.push_str("\n\n");
    if !doc.params.is_empty() {
        out.push_str("参数:\n");
        for (pname, pdesc) in doc.params {
            out.push_str("  ");
            out.push_str(pname);
            out.push_str(" - ");
            out.push_str(pdesc);
            out.push('\n');
        }
        out.push('\n');
    }
    out.push_str("返回: ");
    out.push_str(doc.returns);
    out.push('\n');
    if !doc.examples.is_empty() {
        out.push_str("\n示例:\n");
        for ex in doc.examples {
            out.push_str("  ");
            out.push_str(ex);
            out.push('\n');
        }
    }
    if !doc.errors.is_empty() {
        out.push_str("\n常见错误:\n");
        for e in doc.errors {
            out.push_str("  - ");
            out.push_str(e);
            out.push('\n');
        }
    }
    // 去掉末尾多余换行
    let _ = name; // name 已在 signature 中体现
    if out.ends_with('\n') {
        out.pop();
    }
    out
}
