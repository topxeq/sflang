# Sflang 语言参考

Sflang 是一种轻量级、快速的脚本语言，用 Rust 实现，采用字节码 VM 架构。

---

## 1. 快速开始

```bash
sf                       # 启动 REPL
sf script.sf             # 执行脚本
sf -e "println(\"hi\")"  # 执行代码
sf -h                    # 帮助
```

---

## 2. 注释

```sflang
// 行注释（双斜杠）

/*
   块注释（支持嵌套）
   /* 嵌套注释 */
*/
```

注意：不支持 `#` 注释。

---

## 3. 变量与赋值

```sflang
var x            // 声明，值为 undefined
var y = 42       // 声明并赋值
x = "hello"      // 赋值（动态类型，可改变类型）
```

复合赋值：`+= -= *= /= %= ??= &= |= ^= <<= >>=`

自增自减：`++x` `x++` `--x` `x--`（前缀返回新值，后缀返回旧值）

---

## 4. 数据类型（19 种）

| 类型 | 说明 | 示例 |
|------|------|------|
| `undefined` | 空值（无 nil） | `undefined` |
| `int` | 64 位有符号整数 | `42` `0xFF` `0b1010` `1_000_000` |
| `float` | 64 位浮点 | `3.14` `2.5e10` |
| `bool` | 布尔 | `true` `false` |
| `byte` | 0-255 字节（mod 256 环绕） | `byte(65)` |
| `string` | UTF-8 字符串（不可变） | `"hi"` `` `raw` `` `"""多行"""` |
| `bytes` | 不可变字节序列 | `bytes("ab")` |
| `byteArray` | 可变字节序列 | `byteArray(10)` |
| `array` | 动态数组 | `[1, 2, 3]` |
| `object` | HashMap + 原型链（OOP 载体） | `{name: "A"}` |
| `map` | 有序映射（插入序，纯数据） | `map{"k": "v"}` |
| `function` | 用户函数 / 闭包 | `func(x) { return x }` |
| `builtin` | 内置函数 | `println` |
| `error` | 错误值 | `throw("err")` |
| `native` | 宿主嵌入值 | — |
| `bigInt` | 任意精度整数 | `bigInt("999...999")` |
| `bigFloat` | 任意精度十进制浮点 | `bigFloat("0.1")` |
| `datetime` | 日期时间 | `datetime(2024, 1, 1)` |
| `file` | 文件句柄 | `openFile("a.txt", "r")` |
| `stringBuilder` | 高效字符串构建器 | `newStringBuilder()` |

### 通用类型判断

Sflang 提供 `isType` / `isTypeCode` 作为统一的类型判断入口，取代零散的 `isInt`/`isString` 等谓词：

```sflang
isType(42, "int")           // true（大小写不敏感）
isType("hi", "string")      // true
isType([1,2], "array")      // true
isType(newRing(3), "ring")  // true（Native 细分类型）

isTypeCode(42, 1)           // true（1 = int 的 TypeCode）
isTypeCode("hi", 4)         // true（4 = string 的 TypeCode）

// typeCode() 和 typeName() 获取类型信息
typeCode(42)                // 1
typeName(42)                // "int"
```

### undefined

读取未定义的变量返回 `undefined`（不报错）。map 缺键、无返回值的函数也返回 `undefined`。

```sflang
var a               // undefined
var b = m["missing"] // undefined
isUndefined(a)      // true
a ?? "默认值"        // "默认值"
adjustFloat(0.1 + 0.2)  // 0.3（消除浮点精度误差）
```

### 数值类型互通

- `int` ↔ `float`：自动转换（`1 + 2.5` → `3.5`）
- `int` ↔ `bigInt`：自动（`1 + bigInt(2)` → `int(3)`，大结果保持 bigInt）
- `bigFloat` ↔ `int/bigInt`：自动
- `int(v)` 支持 int/float/bool/byte/string/bigInt（bigInt 超 i64 范围报错）
- `float(v)` 支持 int/float/bool/byte/string/bigInt/bigFloat
- `byte + byte` → `byte`（mod 256 环绕：`byte(255) + byte(1)` → `byte(0)`）
- `byte + int` → `int`（byte 提升）

### string 索引与切片

```sflang
"ABC"[0]              // 65（Unicode 码点，按字符）
"中文"[0]              // 20013
"Hello"[1:3]          // "el"（按字符切片，不切断多字节）
"Hello"[:2]           // "He"
"Hello"[3:]           // "lo"
```

---

## 5. 运算符

优先级从低到高：

```
= += -= *= /= %= ??= &= |= ^= <<= >>=
?:
??
||  &&
|   ^   &
==  !=
<   <=  >   >=
<<  >>
+   -
*   /   %
-   !   ~   ++   --
.   []  ()
```

### 特殊运算符

| 运算符 | 说明 |
|--------|------|
| `??` | 空合并：仅 undefined 触发（0/""/false 不触发） |
| `?:` | 三元：`cond ? a : b`（右结合） |
| `[]` | 切片：`a[1:3]` `a[:2]` `a[3:]` |
| `??=` | 空合并赋值：仅 undefined 时赋值 |

---

## 6. 字符串

### 字面量

```sflang
"双引号\n转义"        // 单行，支持转义
"Hello, ${name}!"    // 单行，支持 ${expr} 插值
`Raw 反引号`          // 多行，不转义（所见即所得，不插值）
"""
三引号多行
支持转义 \t \\
也支持 ${expr} 插值
"""
```

### 字符串插值 `${expr}`

在双引号字符串和多行字符串中，`${expr}` 会求值表达式并自动转为字符串嵌入：

```sflang
var name = "World"
var count = 42
pln("Hello, ${name}!")              // Hello, World!
pln("count = ${count + 1}")         // count = 43
pln("user: ${user.name}")           // 成员访问
pln("len=${len(arr)}")              // 函数调用

// 转义字面 ${}：用 \$
pln("literal: \${x}")               // literal: ${x}

// 反引号 raw string 不插值
pln(`${name}`)                       // ${name}
```

### 字符串拼接 `+`

`+` 支持字符串与任意类型拼接（非字符串侧自动调用 to_str）：

```sflang
pln("count: " + 42)                  // count: 42
pln("ok=" + true)                    // ok=true
pln(100 + "元")                      // 100元
pln("a=" + 1 + ", b=" + 2)          // a=1, b=2

// 注意：int + int 仍是数值加法，不转字符串
pln(1 + 2)                           // 3（数值）
```

### 字符串函数（str 前缀）

| 函数 | 说明 |
|------|------|
| `strToUpper(s)` / `strToLower(s)` | 大小写 |
| `strTrim(s)` | 去空白（也接受 undefined → 空串） |
| `strTrimPrefix(s, prefix)` | 去头部子串 |
| `strTrimSuffix(s, suffix)` | 去尾部子串 |
| `strTrimLeft(s, cutset)` | 去左侧字符集 |
| `strTrimRight(s, cutset)` | 去右侧字符集 |
| `strFind(s, sub)` | 查找子串（返回字符索引，-1=未找到） |
| `strReplace(s, old, new, ...)` | 替换（支持多对） |
| `strSplit(s, sep)` / `strJoin(arr, sep)` | 分割/拼接 |
| `strSub(s, start, end)` | 子串（按字符） |
| `strSubBytes(s, start, end)` | 子串（按字节） |
| `strRepeat(s, n)` | 重复 |
| `strCount(s, sub)` | 统计子串出现次数 |
| `strPad(s, len, fill, right)` | 填充到指定长度 |
| `strSplitN(s, sep, n)` | 限制分割段数 |
| `strReplaceN(s, old, new, n)` | 限制替换次数 |
| `strSplitLines(s)` | 按行分割 |
| `strQuote(s)` / `strUnquote(s)` | 加引号/去引号 |
| `strLimit(s, maxLen, suffix)` | 截断带省略号 |
| `strStartsWith(s, prefix)` / `strEndsWith(s, suffix)` | 前缀/后缀判断 |

跨类型函数（不加 str 前缀）：`contains` / `reverse` / `trim`

### 字节级访问

| 函数 | 说明 |
|------|------|
| `bytesSlice(s, start, end)` | 按字节切 string → bytes |
| `bytesAt(s, i)` | 取第 i 字节 → byte |
| `lenBytes(s)` | UTF-8 字节数 |

### 码点转换

| 函数 | 说明 |
|------|------|
| `charFromCode(n)` | 码点 → 单字符 string |
| `codeOf(c)` | 单字符 → 码点 |

---

## 7. 数组

```sflang
var a = [1, "two", true]    // 可存任意类型
a[0]                         // 1
a[-1]                        // 最后一个
a[1:3]                       // 切片
push(a, 4)                   // 追加
pop(a)                       // 弹出末尾
len(a)                       // 长度
sort(a) / sort(a, true)      // 排序（可选降序）
sortByFunc(a, func(x, y) { return x - y })  // 自定义排序
reverse(a)                   // 反转
concat(a1, a2)               // 拼接
insert(a, idx, val)          // 插入
remove(a, idx)               // 删除并返回
contains(a, val)             // 是否包含
indexOf(a, val)              // 查找索引
filter(a, func(x) { return x > 0 })  // 过滤
map(a, func(x) { return x * 2 })     // 映射
```

---

## 8. Object 与 Map

### Object（HashMap 无序 + 原型链 + OOP）

```sflang
var obj = {name: "Alice", age: 30}
obj.name                     // "Alice"
obj["email"] = "a@b.com"     // 新增成员
hasKey(obj, "name")          // true
keys(obj) / values(obj)      // 键/值数组
```

### Map（有序，纯数据）

```sflang
var m = map{"first": 1, "second": 2}  // 插入序
var m2 = newMap()
m["key"] = "value"
for k, v in m { }            // 按插入序遍历
isType(m, "map")             // true
```

### Object vs Map

| | Object | Map |
|---|---|---|
| 底层 | HashMap（无序） | Vec（插入序） |
| 原型链 | 有 | 无 |
| 方法 | 可挂（OOP） | 纯数据 |
| 字面量 | `{k: v}` | `map{k: v}` |

### Object 过滤方法

```sflang
entries(obj)       // [["k", v], ...]（过滤 function）
dataKeys(obj)      // 非 function 键
dataValues(obj)    // 非 function 值
```

---

## 9. 面向对象（OOP）

构造函数 + 自动 self 绑定 + 原型链。

```sflang
// obj.method(args) 自动注入 obj 作为隐式首参 self
// 方法定义时声明 self 作为首参
func Counter(startA) {
    var self = {value: startA}
    self.inc = func(self, n) { self.value += n; return self.value }
    self.get = func(self) { return self.value }
    return self
}

var c = Counter(10)
c.inc(5)        // 15 — 不需要手动传 c
c.get()         // 15
```

### 原型链（方法共享）

```sflang
var proto = {distance: func(self) { return self.x * self.x + self.y * self.y }}
func Point(xA, yA) {
    var self = newObject(proto)
    self.x = xA; self.y = yA
    return self
}
```

### 继承（构造函数组合）

```sflang
func Dog(nameA, breedA) {
    var self = Animal(nameA)     // 复用父类
    self.breed = breedA
    self.speak = func(self) { ... }  // 覆盖
    return self
}
```

---

## 10. 函数

```sflang
func add(a, b) { return a + b }

var double = func(n) { return n * 2 }   // 匿名函数

// 默认参数：用 = 默认值 声明，调用时可省略
func greet(name, greeting="你好") {
    return greeting + ", " + name
}
greet("Alice")          // 你好, Alice
greet("Bob", "Hi")      // Hi, Bob

// 默认值可引用前面的参数
func makeRange(start, end=start+10) { ... }

func sum(...nums) {                      // 可变参数（... 标记）
    var t = 0
    for n in nums { t += n }
    return t
}

var arr = [1, 2, 3]
sum(...arr)                              // 展开调用
sum(1, ...arr, 100)                      // 混合展开
```

闭包：捕获外层变量，共享可变状态。

---

## 11. 控制流

```sflang
if x > 0 { }
elif x == 0 { }
else { }

while x < 10 { x++ }

// for 循环的四种形式：
for i := 0; i < 5; i++ { }           // C 风格 for（init; cond; post）
for x < 10 { x++ }                   // Go 风格：单条件（等同 while）
for { break }                        // Go 风格：无限循环
for i in range(5) { }                // for-in
for k, v in obj { }                  // 遍历 object/map
for i, v in arr { }                  // 遍历 array（带索引）

break / continue                     // 支持标签：break label / continue label

// switch：等值匹配，默认不贯穿（命中即跳出，无需 break）
switch day {
    case 1 { println("周一") }
    case 6 { println("周六") }
    case 7 { println("周日") }
    default { println("工作日") }
}

// switch 内的 break 只跳出 switch（不影响外层循环）
// switch 内的 continue 作用于外层循环（switch 不是循环）
```

---

## 12. 异常处理

Sflang 的错误处理遵循"一般返回错误对象为主，必要时才抛异常"的原则。

### 错误值（推荐）

```sflang
// 函数返回错误值（不抛异常）
func divide(a, b) {
    if b == 0 {
        return error("除数不能为零")
    }
    return a / b
}

r := divide(10, 0)
if isError(r) {
    println("出错了:", r)
}
```

### try / catch / finally

```sflang
try {
    // 可能出错的代码
} catch (e) {
    // e 是错误值
    println(e)
} finally {
    // 总是执行
}

defer close(f)        // 延迟执行（函数返回时逆序执行）

throw("错误信息")      // 主动抛出
```

> **何时抛异常**：除零、空指针（undefined.x）、不可恢复的内部错误等。
> **何时返回错误值**：常规的业务错误、参数校验失败、IO 失败等可用 `error()` 返回。

### TXERROR 错误字符串机制

Sflang 同时支持两种错误表示，配套函数统一处理：

| 表示方式 | 说明 | 示例 |
|---------|------|------|
| `error(msg)` | Error 对象（推荐，结构化） | `error("文件不存在")` |
| `"TXERROR:xxx"` | 错误字符串（字符串形式，跨边界） | `"TXERROR:文件不存在"` |

配套函数（同时识别两种形式）：

```sflang
// 创建错误
e1 := error("错误一")              // Error 对象
e2 := errStrf("失败: %v", 404)     // → "TXERROR:失败: 404"

// 判断错误（同时识别两种形式）
isErr(e1)       // true
isErr(e2)       // true
isErr(42)       // false
isErrStr(e2)    // true（仅识别 TXERROR 字符串）

// 提取错误信息
getErrStr(e1)   // "错误一"
getErrStr(e2)   // "失败: 404"

// 安全转换
errToEmpty(e1)  // "" （错误转为空串）
errToEmpty(42)  // 42 （非错误原样返回）

// 错误不丢失的去空白
trimErr(e1)         // 原样返回错误（不静默丢失）
trimErr("  hi  ")   // "hi"

// 检查并退出（错误时打印到 stderr 并 exit(1)）
checkErr(result, "-format=Error: %v\n")
```

> **设计要点**：`isErr` 是统一的错误判断函数，同时识别 Error 对象和 `"TXERROR:"` 开头的字符串，便于在不同场景下灵活使用。
```

---

## 13. 并发

```sflang
// 启动新线程
run func() { println("子线程") }

// Channel（线程间通信）
var ch = newChannel()
run func() { chanSend(ch, 42) }
println(chanRecv(ch))       // 42

// 同步原语
var mu = newMutex()
lock(mu)
defer close(mu)
// 临界区
```

同步原语：`newMutex`/`lock`/`unlock`/`tryLock`、`newRWMutex`/`rlock`/`runlock`/`wlock`/`wunlock`、`newWaitGroup`/`wgAdd`/`wgDone`/`wgWait`、`newSemaphore`/`semAcquire`/`semRelease`、`newOnce`/`onceDo`

---

## 14. 文件 IO

### 全量读写（小文件）

```sflang
var text = readFile("data.txt")
writeFile("out.txt", "content")
appendFile("log.txt", "new line\n")
var data = readFileBytes("binary.bin")
writeFileBytes("out.bin", data)
readLines("log.txt")           // → ["line1", "line2", ...]
```

### 文件句柄（流式/随机访问）

```sflang
var f = openFile("data.txt", "r")    // mode: r/w/a/r+
defer close(f)
readLine(f)                    // → string 或 undefined(EOF)
readAll(f)                     // → bytes
readN(f, 1024)                 // → bytes
readStr(f)                     // → string（从 file/string/bytes/byteArray 统一读取）
readBytes(f)                   // → bytes
readChars(f, 10)               // → string（按字符读）
writeStr(f, "text")
writeBytes(f, data)
writeLine(f, "line")
seek(f, 0, 0)                  // offset, whence(0=开头/1=当前/2=末尾)
tell(f)                        // 当前位置
```

### 路径与目录

```sflang
joinPath("a", "b", "c.txt")
dirName("/x/y/z.txt")         // "/x/y"
baseName("/x/y/z.txt")        // "z.txt"
fileExt("z.txt")              // ".txt"
absPath("relative/path")
makeDir("newdir") / makeDirAll("a/b/c")
listDir(".")                  // 目录条目列表
fileExists("path")
deleteFile("path")
getCurDir() / getTempDir() / getHomeDir()
```

---

## 15. 打印与格式化

| 函数 | 简称 | 说明 |
|------|------|------|
| `println(...)` | `pln` | 打印+换行，空格分隔 |
| `print(...)` | `pr` | 打印不换行 |
| `printf(fmt, ...)` | `prf` / `fpr` | 格式化打印（不换行） |
| `printfln(fmt, ...)` | `pl` | 格式化打印+换行 |
| `sprintf(fmt, ...)` | `spr` | 格式化返回字符串 |

格式占位符（Go 风格）：`%v %d %s %f %.2f %t %x %c %T %% %5d %-5d %05d`

---

## 16. 正则表达式

基于 Rust 官方 `regex` crate（线性时间，不支持前后向断言）。

```sflang
regMatch("^\\d+$", "12345")          // true
regFind("\\d+", "abc123")            // "123"
regFindAll("\\d+", "a1b22c333")      // ["1", "22", "333"]
regFindFirst("(\\d+)-(\\d+)", "x12-34")  // ["12-34", "12", "34"]
regReplace("\\d+", "a1b2", "#")      // "a#b#"
regSplit(",\\s*", "a, b, c")         // ["a", "b", "c"]
var re = regCompile("\\d+")           // 预编译（多次用提速）
regMatch(re, "999")
```

---

## 17. 编码与哈希

### 编解码

```sflang
base64Encode("Hello")          // "SGVsbG8="
base64Decode("SGVsbG8=")       // bytes("Hello")
urlEncode("a b&c")             // "a%20b%26c"
urlDecode("a%20b")             // "a b"
urlFormEncode("a b")           // "a+b"
urlFormDecode("a+b")           // "a b"
```

### 哈希（自实现，纯标准库）

```sflang
md5Hex("abc")                  // "900150983cd24fb0d6963f7d28e17f72"
sha256Hex("abc")               // "ba7816bf8f01cfea..."
md5("abc")                     // bytes(16)
sha1("abc")                    // bytes(20)
sha256("abc")                  // bytes(32)
```

### 字节序列

```sflang
bytes("text")                  // string → bytes
bytesHex(b)                    // bytes → 十六进制字符串
bytesFromHex("4142")           // 十六进制 → bytes
byteArray(10, 0xFF)            // 创建可变字节序列
byteArrayFromBytes(b)          // bytes → byteArray
bytesXor(data, key)            // 批量 XOR（高效加密）
bytesXorInPlace(ba, key)       // 原地 XOR
copy(dst, src)                 // 批量复制字节
```

### CSV（RFC 4180）

```sflang
rows := readCsv("data.csv")           // 文件 → 二维数组（全字符串）
rows := readCsvFromStr("a,b\n1,2\n")  // 字符串 → 二维数组
writeCsv(data, "out.csv")             // 二维数组 → 文件（自动转义引号/逗号）
```

自动处理引号包裹、`""` 转义、字段内换行/逗号。

### Excel（xlsx）

```sflang
// 写入
wb := excelNew()                           // 创建工作簿
excelWriteSheet(wb, 0, [["name","age"],    // 写到 sheet 0
                        ["Alice", 30]])
excelNewSheet(wb, "MySheet")                // 新建 sheet
excelWriteSheet(wb, "MySheet", data)       // 按名称写入
excelSaveAs(wb, "out.xlsx")                // 保存

// 读取
rows := excelReadSheet("data.xlsx")        // 默认第一个 sheet → 二维数组
rows := excelReadSheet("data.xlsx", 1)     // 按索引
rows := excelReadSheet("data.xlsx", "My")  // 按名称
all := excelReadAll("data.xlsx")           // 所有 sheets → map{名: 二维数组}
```

读取自动保留类型（int/float/string/bool）。写入时 int/float/bool 按原生类型写入。

### Word (docx)

```sflang
// 提取段落文本
paragraphs := docxToStrs("report.docx")     // → ["第一段", "第二段", ...]

// 模板替换（bytes 进 bytes 出）
template := readFileBytes("template.docx")
filled := docxReplace(template, ["{name}", "张三", "{date}", "2026-07-07"])
writeFileBytes("output.docx", filled)

// 提取占位符
placeholders := docxGetPlaceholders(template) // → ["{name}", "{date}"]
```

docx 本质是 ZIP 包，内部 `word/document.xml` 存放正文。Sflang 通过 zip crate 解压，
用字符串操作处理 `<w:t>` 标签文本，自动解码 XML 实体（`&amp;` → `&`）。

### SQLite 数据库

4 个核心函数（对标 Charlang，API 设计为通用多数据库形式，当前实现 SQLite）：

```sflang
db := dbConnect("sqlite3", ":memory:")        // SQLite 内存数据库
db := dbConnect("sqlite3", "data.db")          // SQLite 文件数据库
db := dbConnect("mysql", "mysql://user:pass@localhost:3306/dbname")  // MySQL

dbExec(db, "CREATE TABLE test (id INTEGER, name TEXT)")
dbExec(db, "INSERT INTO test VALUES (?, ?)", 1, "Alice")  // 参数绑定，返回影响行数

rows := dbQuery(db, "SELECT * FROM test WHERE id > ?", 0)
// rows 是 array of map：[{"id": 1, "name": "Alice"}, ...]

dbClose(db)
```

支持数据库：
- **sqlite3**（rusqlite bundled，零配置）：`:memory:` 或文件路径
- **mysql**（纯 Rust，连接池）：`mysql://user:pass@host:port/db`
- **postgres**（同步驱动）：`postgresql://user:pass@host:5432/db`
- **mssql**（纯 Rust TDS，tokio 桥接）：`mssql://user:pass@host:port/db`
- **oracle**（纯 Rust TNS，tokio 桥接）：`oracle://user:pass@host:port/service`

类型映射：INTEGER → int，REAL/FLOAT → float，TEXT/VARCHAR → string，NULL → undefined。
`?` 占位符参数绑定（PostgreSQL 的 `$1`、MSSQL 的 `@P1` 格式自动转换），支持 int/float/string/bool/null/bytes。

---

## 18. JSON

```sflang
jsonEncode({name: "Alice", age: 30})    // '{"name":"Alice","age":30}'
jsonDecode('{"a": 1, "b": 2}')          // → map{"a": 1, "b": 2}（有序 Map）
```

JSON 对象解码为有序 Map（保持键的原始顺序）。

---

## 19. datetime

```sflang
var now = nowDT()                       // 当前时间
var dt = datetime(2024, 6, 15, 14, 30)  // 构造
dt.year / dt.month / dt.day             // 字段访问
dt.hour / dt.minute / dt.second
dt.weekday                              // 0=周日
dtFormat(dt, "2006-01-02 15:04:05")    // 格式化（Go 风格）
dtAddDays(dt, 10)                       // 加天
dtAddSeconds(dt, 3600)                  // 加秒
dtToMillis(dt)                          // → Unix 毫秒
datetimeFromMillis(1704067200000)       // 毫秒 → datetime
datetimeParse("2024-12-25", "2006-01-02")  // 解析
```

---

## 20. Ring 环形缓冲区

通用固定容量环形缓冲区（对标 Charlang/tkc 的 AnyQueue + StringRing + ByteQueue，用一个通用类型替代三者）。可存储任意 Value。

```sflang
// 创建（cap > 0 固定容量，cap <= 0 无限制，缺省 10）
r := newRing(3)

ringPush(r, 10)         // 尾部追加，超容量淘汰头部
ringPush(r, 20)
ringPush(r, 30)
ringPush(r, 40)          // 10 被淘汰 → [20, 30, 40]

ringGet(r)               // 头部元素（不删除）→ 20
ringGet(r, -1)           // 尾部元素 → 40
ringGet(r, 1)            // 指定位置 → 30

ringPick(r)              // 取出头部（删除）→ 20
ringPop(r)               // 取出尾部（删除）→ 40

ringInsert(r, 0, 99)     // 在位置 0 插入
ringSet(r, 0, 100)       // 修改位置 0 的值
ringRemove(r, 0)         // 删除位置 0

ringSize(r)              // 当前元素数
ringToList(r)            // 转为数组
ringClear(r)             // 清空
```

典型用途：滑动窗口、日志缓冲、实时数据采样。

---

## 21. StringBuilder 高效字符串构建器

大量字符串拼接时用 StringBuilder 避免 O(n²) 的重复分配。通过通用函数操作，无专属函数。

```sflang
sb := newStringBuilder()           // 创建（可选初始内容）
writeStr(sb, "hello")              // 追加字符串（返回 sb，支持链式）
writeStr(sb, 42)                   // 追加任意值（自动 toStr）
writeBytes(sb, bytes([0x41, 0x42])) // 追加字节序列
toStr(sb)                          // 获取最终字符串
len(sb)                            // 当前字符数
clear(sb)                          // 清空（不释放内存）
reset(sb)                          // 清空并释放内存
```

`writeStr` 对 StringBuilder 接受任意值（用 `toStr` 转换）；对 file 严格要求 string 参数。
`clear` / `reset` 也支持 array、byteArray、map、ring。

---

## 22. 系统与环境

```sflang
getEnv("HOME")                // 环境变量
setEnv("MY_VAR", "value")
osName()                      // "windows" / "linux" / "macos"
osArch()                      // "amd64" / "arm64"
random()                      // [0, 1) 浮点随机
randInt(1, 100)               // [1, 100] 整数随机
randomStr(16)                 // 随机字母数字串
uuid()                        // UUID v4
sleep(1.5)                    // 秒（支持小数）
sleepMs(500)                  // 毫秒（整数）
```

---

## 21. import（脚本加载）

```sflang
import "lib.sf"               // 加载并执行，顶层定义合并到当前全局
```

- 相对路径基于当前脚本目录
- 幂等：同一路径只执行一次
- 循环检测：A import B import A → 报错

---

## 22. 预定义全局变量

| 变量 | 说明 |
|------|------|
| `piG` | 圆周率 π |
| `eG` | 自然对数底 e |
| `argsG` | 命令行参数数组 |
| `scriptPathG` | 脚本路径 |

读取未定义的全局返回 `undefined`（不报错）。

---

## 23. Help 系统（内置函数自省）

Sflang 提供运行时文档查询，无需查阅外部文档即可了解每个内置函数。

### help() 内置函数

```sflang
// 无参：列出所有内置函数分类
help()                  // → 多行字符串，按分类列出全部 ~680 个函数

// 有参：查看函数详情
help("regFind")         // → regFind 的签名、参数、返回值、示例、常见错误

// 有参：查看分类下所有函数
help("regex")           // → regex 分类下所有函数 + 简介

// 拼写容错：自动给出相似建议
help("regfind")         // → 提示 regFind / regFindAll 等相似函数
```

### sf --list-builtins 命令

```bash
sf --list-builtins          # 列出所有内置函数（按分类）
sf --list-builtins regex    # 筛选 regex 分类（带简介）
sf --list-builtins math     # 筛选 math 分类
```

### 已有文档的分类

| 分类 | 函数数 | 说明 |
|------|--------|------|
| core | 18 | println/len/typeCode/typeName/help/range/keys/values/push/sprintf/sleep/error/isError/defaultVal/defaultUndef/assert/uuid/deepClone |
| concurrency | 22 | channel/mutex/rwlock/waitGroup/semaphore/once 全套 |
| file | 16 | readFile/writeFile/openFile/readLine/fileExists/makeDir/getFileList ... |
| array | 13 | sort/sortByFunc/reverse/contains/indexOf/slice/concat/insert/remove/shuffle |
| system | 13 | getEnv/setEnv/osName/getCurDir/joinPath/listDir/systemCmd/exit ... |
| bytes | 12 | byteArray/bytes/strFromBytes/copy/bytesHex/bytesFromHex/hexEncode ... |
| containers | 12 | stack（push/pop/peek/len/clear）+ queue（同） |
| string | 12 | strToUpper/strSplit/strReplace/strSub/strTrim/strJoin/strFind ... |
| ring | 11 | 环形缓冲（push/pop/get/set/insert/remove/size/clear/toList） |
| hash | 10 | md5/sha1/sha256/hmacSha256/getOtpCode/checkOtpCode ... |
| encode | 10 | base64/url/html 编解码全套 |
| datetime | 10 | now/datetime/dtFormat/dtAddDays/runTicker/formatTime ... |
| math | 10 | abs/floor/ceil/round/sqrt/pow/min/max/random/randInt |
| bigint | 7 | bigInt/bigFloat/toBigInt/toBigFloat/isBigInt/isBigFloat/bigFloatDiv |
| regex | 6 | regMatch/regFind/regFindAll/regReplace/regSplit/regCompile |
| crypto | 6 | aesEncrypt/aesDecrypt（+str 变体）/ genJwtToken/parseJwtToken（HS256+RS256） |
| csv | 5 | readCsv/writeCsv（+别名和字符串版） |
| json | 4 | jsonEncode/jsonDecode（+别名 toJson/fromJson） |
| xml | 3 | fromXml/xmlGetNodeStr/formatXml |
| test | 3 | testByText/testByContains/testByReg |
| clipboard | 2 | getClipText/setClipText |
| pinyin | 2 | toPinYin/toPinYinInitial |
| template | 2 | renderMarkdown/replaceHtmlByMap |

共 **212 个函数** 有详细文档（24 个分类）。其余函数（image/http/db/s3/ssh/ftp/xlsx 等专用模块）暂无详细文档，但 help() 会显示"暂无文档"并提示分类。文档将持续补充。

---

## 24. 已知限制与设计取舍

以下为 Sflang 在性能优先原则下的有意设计取舍，非 bug：

### 块级作用域的 slot 不回收
- 块内声明的变量在块结束后"名字不可见"（块外引用得到 undefined），但其 slot 不会被回收复用。
- 这是 slot 数组模型的固有特征（编译期固定索引换取 O(1) 访问），与 Lua 等高性能脚本语言一致。
- 代价：深嵌套块中 `frame.locals` 数组可能略大于实际需要（多几个 `undefined`），可忽略。
- 闭包捕获的块内变量在块结束后仍可通过闭包正确访问（slot 不回收保证了这一点）。

### 循环引用不自动回收
- Sflang 无 GC（垃圾回收），引用类型用 Rust 的 `Arc` 引用计数管理内存。
- 循环引用（A 引用 B，B 引用 A）会导致内存泄漏（Arc 通病）。
- 脚本场景循环引用罕见；如需打破循环，可将其中一个引用设为 `undefined`。
- 优势：零 GC 暂停，执行时间可预测，适合性能敏感场景。

### VM 中的 `unreachable!()` 兜底
- VM 字节码分发的算术/比较分支有少量 `unreachable!()` 兜底（约 9 处）。
- 这些是字节码不变式的保护：理论上编译器不会产出不匹配的 opcode+操作数组合。
- 若因编译器 bug 触发，会 panic 并给出明确位置，便于定位。
- 这是性能优先的取舍（避免每条指令都做运行时类型检查）。
