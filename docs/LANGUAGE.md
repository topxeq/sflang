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
`Raw 反引号`          // 多行，不转义（所见即所得）
"""
三引号多行
支持转义 \t \\
"""
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
isMap(m)                     // true
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

break / continue
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

## 20. 系统与环境

```sflang
getEnv("HOME")                // 环境变量
setEnv("MY_VAR", "value")
osName()                      // "windows" / "linux" / "macos"
osArch()                      // "amd64" / "arm64"
random()                      // [0, 1) 浮点随机
randInt(1, 100)               // [1, 100] 整数随机
randomStr(16)                 // 随机字母数字串
uuid()                        // UUID v4
sleep(1000)                   // 毫秒
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
