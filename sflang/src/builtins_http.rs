//! builtins_http.rs — HTTP 服务器内置函数
//!
//! 提供 Sflang 脚本级 HTTP 服务器能力。分两个层次：
//!
//! 1. 脚本级嵌入 API（与 Charlang mux()/setHandler 对应）：
//!    ```ignore
//!    server := httpServer("--port=8080")
//!    serverSetHandler(server, "/hello", func(req, resp) {
//!        return "Hello!"          // 返回字符串 → 作为响应体
//!    })
//!    serverStart(server)
//!    ```
//!
//! 2. CLI 应用服务器（`sf -server`）：通过 run_server_cli 启动，
//!    将 URL 路径映射到 .sf 脚本文件，每请求一个 VM。
//!
//! # 响应规则（纯返回值类型判断）
//!
//! handler 执行完毕后，服务器根据返回值类型决定行为：
//!   - `Str`       → 作为响应体输出（自动 200）
//!   - `Bytes`     → 作为响应体输出（binary）
//!   - `ByteArray` → 作为响应体输出（binary）
//!   - `Error`     → 服务器返回 500 + 错误详情（AI 友好）
//!   - 其他类型    → 不输出（脚本应已通过 writeResp 等自行写响应）
//!
//! 设计要点：
//!   - 不使用 committed 标志或魔法字符串，纯返回值类型判断
//!   - 每请求一个独立 VM（共享 server 全局环境），天然并行
//!   - 纯标准库实现，不依赖第三方 HTTP 库

use std::sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}};

use crate::http_lite::{self, HttpRequest as LiteReq, HttpResponse as LiteResp, HttpHandler,
    SfHttpRequest, SfHttpResponse, SfWebSocket};
use crate::function::BuiltinDoc;
use crate::value::{Value, SfError, error_value};
use crate::vm::VM;

// ===========================================================================
// 内置函数文档（help 系统）
// ===========================================================================

static DOC_HTTP_SERVER: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "httpServer(\"--port=N\", \"--host=H\", \"--verbose\", \"--certDir=D\", \"--adminToken=T\") -> server",
    summary: "创建 HTTP/HTTPS 服务器实例（尚未监听，需配合 serverSetHandler + serverStart）。",
    params: &[
        ("--port", "监听端口，默认 8080"),
        ("--host", "监听地址，默认 0.0.0.0"),
        ("--verbose", "可选开关，打印请求日志"),
        ("--certDir", "可选 TLS 证书目录（含 server.crt + server.key），设置则启用 HTTPS"),
        ("--adminToken", "管理端点令牌，默认 sflang"),
    ],
    returns: "SfHttpServer 对象",
    examples: &[
        "srv := httpServer(\"--port=8080\")  // 默认 0.0.0.0:8080",
        "srv := httpServer(\"--port=443\", \"--certDir=./certs\", \"--verbose\")  // HTTPS",
    ],
    errors: &[
        "serverStart() 加载 TLS 证书失败（可能原因：certDir 下缺少 server.crt 或 server.key；文件格式非 PEM）",
    ],
};

static DOC_SERVER_SET_HANDLER: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "serverSetHandler(server, path, handler) -> undefined",
    summary: "注册路由处理器，路径可含 :param 参数（如 /api/users/:id），自动注入 routeParamsG。",
    params: &[
        ("server", "httpServer 返回的对象"),
        ("path", "路径模式，以 / 结尾为前缀匹配，否则精确匹配；支持 :param"),
        ("handler", "func(req, resp) 处理函数"),
    ],
    returns: "undefined",
    examples: &[
        "serverSetHandler(srv, \"/hello\", func(req, resp) { return \"Hello!\" })  // 精确匹配",
        "serverSetHandler(srv, \"/api/users/:id\", func(req, resp) { return getGlobal(\"routeParamsG\")[\"id\"] })  // 参数路由",
    ],
    errors: &[
        "serverSetHandler() 第 2 个参数应为 string (路径)",
        "serverSetHandler() 第 3 个参数应为 function（可能原因：未传入 func 或函数名拼写错误）",
    ],
};

static DOC_SERVER_SET_STATIC: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "serverSetStatic(server, dirPath) -> undefined",
    summary: "设置静态文件根目录，未命中路由的请求会从该目录读取文件。",
    params: &[
        ("server", "httpServer 返回的对象"),
        ("dirPath", "静态文件根目录路径"),
    ],
    returns: "undefined",
    examples: &["serverSetStatic(srv, \"./public\")  // ./public 下文件可被直接访问"],
    errors: &["serverSetStatic() 第 2 个参数应为 string (目录路径)"],
};

static DOC_SERVER_START: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "serverStart(server [, \"--thread\"]) -> undefined",
    summary: "启动服务器，默认阻塞当前线程，加 --thread/--go 在后台线程运行。",
    params: &[
        ("server", "httpServer 返回的对象"),
        ("--thread/--go", "可选开关，后台线程运行而非阻塞"),
    ],
    returns: "undefined（阻塞模式下服务器停止后返回）",
    examples: &[
        "serverStart(srv)  // 阻塞当前线程",
        "serverStart(srv, \"--thread\")  // 后台运行，主线程可继续",
    ],
    errors: &[
        "serverStart() 服务器已在运行",
        "serverStart() 加载 TLS 证书失败（可能原因：certDir 下缺少 server.crt 或 server.key；文件格式非 PEM）",
    ],
};

static DOC_SERVER_STOP: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "serverStop(server) -> undefined",
    summary: "停止服务器，设置停止信号使 accept 循环退出。",
    params: &[("server", "httpServer 返回的对象")],
    returns: "undefined",
    examples: &["serverStop(srv)  // 停止监听"],
    errors: &["serverStop() 参数不是 HTTP 服务器对象（可能原因：传入了错误类型）"],
};

static DOC_SERVER_ADDR: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "serverAddr(server) -> string",
    summary: "返回服务器的监听地址（host:port）。",
    params: &[("server", "httpServer 返回的对象")],
    returns: "string，如 \"0.0.0.0:8080\"",
    examples: &["addr := serverAddr(srv)  // \"0.0.0.0:8080\""],
    errors: &["serverAddr() 参数不是 HTTP 服务器对象"],
};

static DOC_GET_REQ_METHOD: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "getReqMethod(req) -> string",
    summary: "返回请求方法（GET/POST/PUT/DELETE 等）。",
    params: &[("req", "handler 中的请求对象")],
    returns: "string 方法名（大写）",
    examples: &["m := getReqMethod(req)  // 如 \"GET\""],
    errors: &[
        "getReqMethod() 需要至少 1 个参数",
        "getReqMethod() 参数不是请求对象",
    ],
};

static DOC_GET_REQ_PATH: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "getReqPath(req) -> string",
    summary: "返回请求路径（不含 query）。",
    params: &[("req", "handler 中的请求对象")],
    returns: "string 路径，如 \"/api/users\"",
    examples: &["p := getReqPath(req)  // \"/api/users\""],
    errors: &["getReqPath() 参数不是请求对象"],
};

static DOC_GET_REQ_URI: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "getReqUri(req) -> string",
    summary: "返回完整 URI（含路径和 query）。",
    params: &[("req", "handler 中的请求对象")],
    returns: "string 完整 URI",
    examples: &["u := getReqUri(req)  // \"/api/list?page=1\""],
    errors: &["getReqUri() 参数不是请求对象"],
};

static DOC_GET_REQ_QUERY: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "getReqQuery(req) -> string",
    summary: "返回查询串（不含前导 ?）。",
    params: &[("req", "handler 中的请求对象")],
    returns: "string query 串",
    examples: &["q := getReqQuery(req)  // \"page=1&size=10\""],
    errors: &["getReqQuery() 参数不是请求对象"],
};

static DOC_GET_REQ_HEADER: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "getReqHeader(req, name) -> string|undefined",
    summary: "返回指定请求头的值，不存在时返回 undefined。",
    params: &[
        ("req", "handler 中的请求对象"),
        ("name", "header 名（大小写不敏感）"),
    ],
    returns: "string 值，不存在时 undefined",
    examples: &["ct := getReqHeader(req, \"Content-Type\")  // \"application/json\" 或 undefined"],
    errors: &[
        "getReqHeader() 第 2 个参数应为 string (header 名称)",
        "getReqHeader() 参数不是请求对象",
    ],
};

static DOC_GET_REQ_HEADERS: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "getReqHeaders(req) -> map",
    summary: "返回所有请求头（Map 对象，键为 header 名）。",
    params: &[("req", "handler 中的请求对象")],
    returns: "Map{headerName: value}",
    examples: &["hs := getReqHeaders(req)  // {\"Content-Type\": \"application/json\", ...}"],
    errors: &["getReqHeaders() 参数不是请求对象"],
};

static DOC_GET_REQ_BODY: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "getReqBody(req) -> string",
    summary: "返回请求体（UTF-8 lossy 转字符串）。",
    params: &[("req", "handler 中的请求对象")],
    returns: "string 请求体",
    examples: &["body := getReqBody(req)  // 表单或 JSON 文本"],
    errors: &["getReqBody() 参数不是请求对象"],
};

static DOC_GET_REQ_BODY_BYTES: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "getReqBodyBytes(req) -> bytes",
    summary: "返回请求体（原始字节，适合二进制内容）。",
    params: &[("req", "handler 中的请求对象")],
    returns: "bytes 请求体",
    examples: &["raw := getReqBodyBytes(req)  // 二进制数据"],
    errors: &["getReqBodyBytes() 参数不是请求对象"],
};

static DOC_GET_REQ_PARAM: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "getReqParam(req, key) -> string|undefined",
    summary: "返回指定查询参数的值，不存在时返回 undefined。",
    params: &[
        ("req", "handler 中的请求对象"),
        ("key", "查询参数名"),
    ],
    returns: "string 值，不存在时 undefined",
    examples: &["page := getReqParam(req, \"page\")  // \"1\" 或 undefined"],
    errors: &[
        "getReqParam() 第 2 个参数应为 string (参数名)",
        "getReqParam() 参数不是请求对象",
    ],
};

static DOC_GET_REQ_PARAMS: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "getReqParams(req) -> map",
    summary: "返回所有查询参数（Map 对象）。",
    params: &[("req", "handler 中的请求对象")],
    returns: "Map{key: value}",
    examples: &["ps := getReqParams(req)  // {\"page\": \"1\", \"size\": \"10\"}"],
    errors: &["getReqParams() 参数不是请求对象"],
};

static DOC_WRITE_RESP: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "writeResp(resp, content) -> undefined",
    summary: "写入响应体（string 或任意会转字符串的值），自动设为响应内容。",
    params: &[
        ("resp", "handler 中的响应对象"),
        ("content", "响应内容（string 或其他会转字符串的类型）"),
    ],
    returns: "undefined",
    examples: &["writeResp(resp, \"<h1>Hi</h1>\")"],
    errors: &["writeResp() 参数不是响应对象"],
};

static DOC_WRITE_RESP_BYTES: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "writeRespBytes(resp, data) -> undefined",
    summary: "写入二进制响应体（bytes/byteArray/string）。",
    params: &[
        ("resp", "handler 中的响应对象"),
        ("data", "bytes / byteArray / string"),
    ],
    returns: "undefined",
    examples: &["writeRespBytes(resp, fileBytes)"],
    errors: &["writeRespBytes() 第 2 个参数应为 bytes/byteArray/string"],
};

static DOC_WRITE_RESP_HEADER: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "writeRespHeader(resp, status) -> undefined",
    summary: "设置响应状态码（兼容 Charlang 函数名，等价于 setRespStatus）。",
    params: &[
        ("resp", "handler 中的响应对象"),
        ("status", "HTTP 状态码（int/float）"),
    ],
    returns: "undefined",
    examples: &["writeRespHeader(resp, 200)"],
    errors: &["writeRespHeader() 第 2 个参数应为 int (状态码)"],
};

static DOC_SET_RESP_HEADER: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "setRespHeader(resp, key, value) -> undefined",
    summary: "设置响应头。",
    params: &[
        ("resp", "handler 中的响应对象"),
        ("key", "header 名"),
        ("value", "header 值（string 或可转字符串的值）"),
    ],
    returns: "undefined",
    examples: &["setRespHeader(resp, \"Content-Type\", \"application/json\")"],
    errors: &["setRespHeader() 第 2 个参数应为 string (header 名)"],
};

static DOC_SET_RESP_STATUS: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "setRespStatus(resp, status) -> undefined",
    summary: "设置响应状态码。",
    params: &[
        ("resp", "handler 中的响应对象"),
        ("status", "HTTP 状态码（int/float）"),
    ],
    returns: "undefined",
    examples: &["setRespStatus(resp, 404)"],
    errors: &["setRespStatus() 第 2 个参数应为 int (状态码)"],
};

static DOC_SET_RESP_CONTENT_TYPE: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "setRespContentType(resp, contentType) -> undefined",
    summary: "设置 Content-Type 响应头（setRespHeader 的便捷封装）。",
    params: &[
        ("resp", "handler 中的响应对象"),
        ("contentType", "MIME 类型，如 application/json"),
    ],
    returns: "undefined",
    examples: &["setRespContentType(resp, \"application/json; charset=utf-8\")"],
    errors: &["setRespContentType() 第 2 个参数应为 string"],
};

static DOC_SERVE_FILE: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "serveFile(resp, filePath) -> undefined",
    summary: "将本地文件作为响应输出，自动按扩展名设置 Content-Type（未设置时）。",
    params: &[
        ("resp", "handler 中的响应对象"),
        ("filePath", "本地文件路径"),
    ],
    returns: "undefined（失败时返回 error 对象）",
    examples: &["serveFile(resp, \"/var/www/logo.png\")  // 自动设 Content-Type 为 image/png"],
    errors: &[
        "serveFile() 读取文件 'xxx' 失败（可能原因：文件不存在或权限不足）",
        "serveFile() 第 2 个参数应为 string (文件路径)",
    ],
};

static DOC_REDIRECT_RESP: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "redirectResp(resp, url [, code]) -> undefined",
    summary: "设置重定向响应，默认状态码 302。",
    params: &[
        ("resp", "handler 中的响应对象"),
        ("url", "目标 URL"),
        ("code", "可选状态码，默认 302（int/float）"),
    ],
    returns: "undefined",
    examples: &[
        "redirectResp(resp, \"https://example.com\")  // 302 跳转",
        "redirectResp(resp, \"/login\", 301)  // 永久重定向",
    ],
    errors: &["redirectResp() 第 2 个参数应为 string (URL)"],
};

static DOC_GEN_JSON_RESP: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "genJsonResp(resp, status, msg) -> undefined",
    summary: "生成标准 JSON 响应 {\"status\":..., \"msg\":...}，自动设 Content-Type 为 application/json。",
    params: &[
        ("resp", "handler 中的响应对象"),
        ("status", "状态字段值（string/int/bool，undefined 时为 null）"),
        ("msg", "消息字段（undefined 时为 null）"),
    ],
    returns: "undefined",
    examples: &["genJsonResp(resp, \"ok\", \"created\")  // {\"status\": \"ok\", \"msg\": \"created\"}"],
    errors: &[],
};

static DOC_GET_REQ_COOKIE: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "getReqCookie(req, name) -> string|undefined",
    summary: "从请求 Cookie 头中取指定名称的值，不存在返回 undefined。",
    params: &[
        ("req", "handler 中的请求对象"),
        ("name", "cookie 名称"),
    ],
    returns: "string 值，不存在时 undefined",
    examples: &["sid := getReqCookie(req, \"sessionId\")"],
    errors: &[
        "getReqCookie() 需要 2 个参数 (req, name)",
        "getReqCookie() 第 2 个参数应为 string (cookie 名)",
    ],
};

static DOC_GET_REQ_COOKIES: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "getReqCookies(req) -> map",
    summary: "返回所有 Cookie（Map 对象）。",
    params: &[("req", "handler 中的请求对象")],
    returns: "Map{cookieName: value}",
    examples: &["cks := getReqCookies(req)  // {\"sessionId\": \"abc\", \"theme\": \"dark\"}"],
    errors: &["getReqCookies() 需要 1 个参数 (req)"],
};

static DOC_SET_RESP_COOKIE: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "setRespCookie(resp, name, value [, --path=, --domain=, --maxAge=, --expires=, --httpOnly, --secure, --sameSite=]) -> undefined",
    summary: "设置响应的 Set-Cookie 头，支持 Path/Domain/MaxAge/Expires/HttpOnly/Secure/SameSite 属性。",
    params: &[
        ("resp", "handler 中的响应对象"),
        ("name", "cookie 名称"),
        ("value", "cookie 值"),
        ("--path", "可选 Path 属性"),
        ("--domain", "可选 Domain 属性"),
        ("--maxAge", "可选 Max-Age 秒数"),
        ("--expires", "可选 Expires（RFC1123 日期）"),
        ("--httpOnly", "可选开关，加 HttpOnly 标志"),
        ("--secure", "可选开关，加 Secure 标志"),
        ("--sameSite", "可选 Strict/Lax/None"),
    ],
    returns: "undefined",
    examples: &[
        "setRespCookie(resp, \"token\", \"abc\", \"--path=/\", \"--maxAge=3600\", \"--httpOnly\", \"--secure\", \"--sameSite=strict\")",
    ],
    errors: &[
        "setRespCookie() 至少需要 3 个参数 (resp, name, value)",
        "setRespCookie() 第 2 个参数应为 string (cookie 名)",
    ],
};

static DOC_SET_CORS_HEADERS: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "setCorsHeaders(resp, origin) -> undefined",
    summary: "设置全套 CORS 响应头（Allow-Origin/Methods/Headers/Credentials/Max-Age）。",
    params: &[
        ("resp", "handler 中的响应对象"),
        ("origin", "允许的来源，\"*\" 表示所有"),
    ],
    returns: "undefined",
    examples: &[
        "setCorsHeaders(resp, \"https://example.com\")  // 单一来源",
        "setCorsHeaders(resp, \"*\")  // 允许所有来源",
    ],
    errors: &[
        "setCorsHeaders() 需要 2 个参数 (resp, origin)",
        "setCorsHeaders() 第 2 个参数应为 string (origin)",
    ],
};

static DOC_WEB_SOCKET: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "webSocket(url) -> ws",
    summary: "以客户端模式连接到远程 WebSocket 服务器（ws://host:port/path）。",
    params: &[("url", "ws:// 协议的 URL")],
    returns: "WebSocket 对象（服务端模式暂不支持在 handler 内升级）",
    examples: &["ws := webSocket(\"ws://echo.example.com:9001/ws\")  // 连接 echo 服务器"],
    errors: &[
        "webSocket() URL 解析失败（可能原因：URL 格式应为 ws://host:port/path）",
        "webSocket() TCP 连接失败（可能原因：服务器未启动、网络不通、防火墙拦截）",
        "webSocket() 握手失败（可能原因：服务器不支持 WebSocket、路径错误）",
    ],
};

static DOC_WS_READ_MSG: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "wsReadMsg(ws) -> [type, data]",
    summary: "读取一条 WebSocket 消息，返回 [type, data]，type 为 1=文本/2=二进制/0=关闭。",
    params: &[("ws", "webSocket 返回的对象")],
    returns: "array [type(int), data(string|bytes)]，关闭帧返回 [0, \"closed\"]",
    examples: &["msg := wsReadMsg(ws)  // [1, \"hello\"] 或 [2, bytes]"],
    errors: &[
        "wsReadMsg() 读取失败（可能原因：连接已关闭、网络中断）",
        "参数应为 webSocket 对象（可能原因：未先调用 webSocket() 建立连接）",
    ],
};

static DOC_WS_READ_TEXT: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "wsReadText(ws) -> string|undefined",
    summary: "读取下一条文本消息（跳过 Ping/Pong/Binary），连接关闭返回 undefined。",
    params: &[("ws", "webSocket 返回的对象")],
    returns: "string 文本消息，关闭时 undefined",
    examples: &["t := wsReadText(ws)  // 读取一行文本"],
    errors: &[
        "wsReadText() 读取失败（可能原因：连接已关闭）",
        "参数应为 webSocket 对象",
    ],
};

static DOC_WS_READ_BIN: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "wsReadBin(ws) -> bytes|undefined",
    summary: "读取下一条二进制消息（跳过 Ping/Pong/Text），连接关闭返回 undefined。",
    params: &[("ws", "webSocket 返回的对象")],
    returns: "bytes 二进制消息，关闭时 undefined",
    examples: &["b := wsReadBin(ws)"],
    errors: &[
        "wsReadBin() 读取失败（可能原因：连接已关闭）",
        "参数应为 webSocket 对象",
    ],
};

static DOC_WS_WRITE_TEXT: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "wsWriteText(ws, text) -> undefined",
    summary: "发送一条文本消息。",
    params: &[
        ("ws", "webSocket 返回的对象"),
        ("text", "要发送的文本（string 或可转字符串的值）"),
    ],
    returns: "undefined",
    examples: &["wsWriteText(ws, \"hello\")"],
    errors: &[
        "wsWriteText() 发送失败（可能原因：连接已关闭）",
        "参数应为 webSocket 对象",
    ],
};

static DOC_WS_WRITE_BIN: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "wsWriteBin(ws, data) -> undefined",
    summary: "发送一条二进制消息。",
    params: &[
        ("ws", "webSocket 返回的对象"),
        ("data", "bytes / byteArray / string"),
    ],
    returns: "undefined",
    examples: &["wsWriteBin(ws, fileBytes)"],
    errors: &[
        "wsWriteBin() 第 2 个参数应为 bytes/byteArray/string",
        "wsWriteBin() 发送失败（可能原因：连接已关闭）",
    ],
};

static DOC_WS_WRITE_MSG: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "wsWriteMsg(ws, type, data) -> undefined",
    summary: "发送一条消息（手动指定类型），type=1 文本、2 二进制。",
    params: &[
        ("ws", "webSocket 返回的对象"),
        ("type", "1=文本，2=二进制"),
        ("data", "文本或二进制数据"),
    ],
    returns: "undefined",
    examples: &["wsWriteMsg(ws, 1, \"ping\")  // 发文本"],
    errors: &[
        "wsWriteMsg() 第 2 个参数应为 int (类型: 1=文本, 2=二进制)",
        "wsWriteMsg() 类型码 N 无效 (1=文本, 2=二进制)",
        "wsWriteMsg() 二进制消息需要 bytes/string",
    ],
};

static DOC_WS_CLOSE: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "wsClose(ws) -> undefined",
    summary: "关闭 WebSocket 连接（发送 Close 帧并关闭底层流）。",
    params: &[("ws", "webSocket 返回的对象")],
    returns: "undefined",
    examples: &["wsClose(ws)"],
    errors: &["参数应为 webSocket 对象"],
};

static DOC_WS_LOCAL_ADDR: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "wsLocalAddr(ws) -> undefined",
    summary: "返回本地地址（当前实现固定返回 undefined，仅做接口兼容）。",
    params: &[("ws", "webSocket 返回的对象")],
    returns: "undefined",
    examples: &["addr := wsLocalAddr(ws)  // 当前恒为 undefined"],
    errors: &["参数应为 webSocket 对象"],
};

static DOC_PARSE_REQ_FORM: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "parseReqForm(req) -> map",
    summary: "解析表单请求体（application/x-www-form-urlencoded 或 multipart/form-data）。",
    params: &[("req", "handler 中的请求对象")],
    returns: "Map{字段名: value}，文件字段为 object {fileName, size, content(bytes)}",
    examples: &[
        "form := parseReqForm(req)  // 普通字段为 string，文件字段为 {fileName, size, content}",
    ],
    errors: &[
        "parseReqForm() 参数不是请求对象",
        "parseReqForm() multipart 表单缺少 boundary",
    ],
};

static DOC_SAVE_FILE_UPLOADS: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "saveFileUploads(req, destDir [, \"--fieldName=name\"]) -> map",
    summary: "将 multipart 表单中的文件字段保存到磁盘，返回字段名 -> {fileName, savedPath, size}。",
    params: &[
        ("req", "handler 中的请求对象，必须为 multipart/form-data"),
        ("destDir", "目标目录（不存在则自动创建）"),
        ("--fieldName", "可选只保存指定字段名"),
    ],
    returns: "Map{字段名: {fileName, savedPath, size}}（非文件字段和空文件名被跳过）",
    examples: &["saved := saveFileUploads(req, \"/tmp/uploads\", \"--fieldName=avatar\")  // 只保存 avatar 字段"],
    errors: &[
        "saveFileUploads() 要求 multipart/form-data 请求（可能原因：未设置 enctype 或未使用 multipart 表单）",
        "saveFileUploads() multipart 表单缺少 boundary",
        "saveFileUploads() 创建目录失败",
        "saveFileUploads() 写入文件失败（可能原因：磁盘空间不足或权限不足）",
    ],
};

static DOC_GET_WEB: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "getWeb(url [, \"--timeout=N\", \"Header: value\"]) -> string",
    summary: "发送 HTTP GET，返回响应体字符串。",
    params: &[
        ("url", "目标 URL"),
        ("--timeout", "可选超时秒数，默认 30"),
        ("header", "可选附加 header，如 \"Content-Type: json\"；支持多行或多参数"),
    ],
    returns: "string 响应体（失败时返回 error 对象）",
    examples: &[
        "body := getWeb(\"https://example.com\")  // 简单 GET",
        "body := getWeb(\"https://api.x.com\", \"--timeout=10\", \"Authorization: Bearer xxx\")  // 带超时和 header",
    ],
    errors: &[
        "getWeb() 第 1 个参数应为 string (URL)",
        "getWeb() 请求失败（可能原因：URL 格式错误、网络不通、DNS 解析失败、服务器超时）",
    ],
};

static DOC_GET_WEB_BYTES: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "getWebBytes(url [, \"--timeout=N\", \"Header: value\"]) -> bytes",
    summary: "发送 HTTP GET，返回响应体原始字节（适合二进制内容如图片、压缩包）。",
    params: &[
        ("url", "目标 URL"),
        ("--timeout", "可选超时秒数，默认 30"),
        ("header", "可选附加 header"),
    ],
    returns: "bytes 响应体（失败时返回 error 对象）",
    examples: &["img := getWebBytes(\"https://example.com/logo.png\")"],
    errors: &[
        "getWebBytes() 第 1 个参数应为 string (URL)",
        "getWebBytes() 请求失败（可能原因：URL 格式错误、网络不通、服务器超时）",
    ],
};

static DOC_GET_WEB_BYTES_WITH_HEADERS: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "getWebBytesWithHeaders(url, headers [, \"--timeout=N\"]) -> bytes",
    summary: "带 header 字典的 HTTP GET，返回响应体字节（用于需要鉴权等场景）。",
    params: &[
        ("url", "目标 URL"),
        ("headers", "Map/Object{key: value}，如 {\"Authorization\": \"Bearer xxx\"}"),
        ("--timeout", "可选超时秒数，默认 30"),
    ],
    returns: "bytes 响应体（失败时返回 error 对象）",
    examples: &[
        "data := getWebBytesWithHeaders(\"https://api.x.com/data\", {\"Authorization\": \"Bearer xxx\", \"Accept\": \"application/json\"})",
    ],
    errors: &[
        "getWebBytesWithHeaders() 第 1 个参数应为 string (URL)（可能原因：参数顺序错误）",
        "headers 参数应为 map/object（可能原因：参数类型错误或顺序错误）",
        "getWebBytesWithHeaders() 请求失败（可能原因：URL 格式错误、网络不通、header 格式错误）",
    ],
};

static DOC_POST_WEB: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "postWeb(url, body, contentType [, \"--timeout=N\", \"Header: value\"]) -> string",
    summary: "发送 HTTP POST，返回响应体字符串。",
    params: &[
        ("url", "目标 URL"),
        ("body", "请求体（string/bytes/byteArray 或可转字符串的值）"),
        ("contentType", "Content-Type，如 application/json"),
        ("--timeout", "可选超时秒数，默认 30"),
        ("header", "可选附加 header"),
    ],
    returns: "string 响应体（失败时返回 error 对象）",
    examples: &[
        "resp := postWeb(\"https://api.x.com\", jsonEncode({a:1}), \"application/json\")",
    ],
    errors: &[
        "postWeb() 需要至少 3 个参数 (URL, body, contentType)",
        "postWeb() 第 3 个参数应为 string (Content-Type)",
        "postWeb() 请求失败（可能原因：URL 格式错误、网络不通、服务器拒绝、Content-Type 不匹配）",
    ],
};

static DOC_DOWNLOAD_FILE: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "downloadFile(url, savePath [, \"--timeout=N\", \"Header: value\"]) -> int",
    summary: "下载远程文件到本地路径，成功返回写入的字节数。",
    params: &[
        ("url", "目标 URL"),
        ("savePath", "本地保存路径"),
        ("--timeout", "可选超时秒数，默认 30"),
        ("header", "可选附加 header"),
    ],
    returns: "int 写入字节数（失败时返回 error 对象）",
    examples: &["size := downloadFile(\"https://example.com/big.zip\", \"/tmp/big.zip\", \"--timeout=120\")"],
    errors: &[
        "downloadFile() 需要至少 2 个参数 (URL, savePath)",
        "downloadFile() 服务器返回错误状态（可能原因：文件不存在、权限不足）",
        "downloadFile() 写入文件 'xxx' 失败（可能原因：目录不存在、权限不足）",
        "downloadFile() 下载失败（可能原因：URL 格式错误、网络不通、服务器超时）",
    ],
};

static DOC_URL_EXISTS: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "urlExists(url) -> bool",
    summary: "检查 URL 是否可访问（状态码 < 400 视为存在，超时 10 秒）。",
    params: &[("url", "目标 URL")],
    returns: "bool：true 表示状态码 < 400，请求失败返回 false",
    examples: &["if urlExists(\"https://example.com\") { ... }"],
    errors: &["urlExists() 第 1 个参数应为 string (URL)"],
};

static DOC_PARSE_URL: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "parseUrl(url) -> object",
    summary: "解析 URL，返回 {scheme, host, port, path, query, fragment} 对象。",
    params: &[("url", "URL 字符串")],
    returns: "object{scheme, host, port(int), path, query, fragment}（缺失部分为空串，端口为 0）",
    examples: &[
        "p := parseUrl(\"https://a.com:8443/api/x?q=1#top\")  // {scheme:\"https\", host:\"a.com\", port:8443, path:\"/api/x\", query:\"q=1\", fragment:\"top\"}",
    ],
    errors: &["parseUrl() 第 1 个参数应为 string (URL)"],
};

static DOC_GET_MIME_TYPE: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "getMimeType(filenameOrExt) -> string",
    summary: "按文件扩展名返回 MIME 类型，未知扩展名返回 application/octet-stream。",
    params: &[("filenameOrExt", "文件名或扩展名（自动取最后一个 . 之后并转小写）")],
    returns: "string MIME 类型",
    examples: &[
        "getMimeType(\"a.png\")  // \"image/png\"",
        "getMimeType(\".JSON\")  // \"application/json\"",
    ],
    errors: &["getMimeType() 第 1 个参数应为 string (文件名)"],
};

static DOC_PARSE_QUERY: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "parseQuery(query) -> map",
    summary: "解析 query string（如 a=1&b=2&c=hello%20world）为 Map，支持 + 和 %XX 解码。",
    params: &[("query", "query string（不含前导 ?）")],
    returns: "Map{key: value}",
    examples: &["m := parseQuery(\"a=1&b=2&c=hello%20world\")  // {a:\"1\", b:\"2\", c:\"hello world\"}"],
    errors: &["parseQuery() 第 1 个参数应为 string (query string)（可能原因：参数顺序错误）"],
};

static DOC_JOIN_URL_PATH: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "joinUrlPath(base, path) -> string",
    summary: "拼接 URL 路径，自动避免重复或缺失斜杠。",
    params: &[
        ("base", "基础 URL/路径"),
        ("path", "要追加的路径"),
    ],
    returns: "string 拼接后的路径",
    examples: &[
        "joinUrlPath(\"http://example.com/\", \"/api/v1\")  // \"http://example.com/api/v1\"",
        "joinUrlPath(\"http://example.com\", \"api/v1\")  // \"http://example.com/api/v1\"",
        "joinUrlPath(\"base/\", \"sub/\")  // \"base/sub/\"",
    ],
    errors: &[
        "joinUrlPath() 需要 2 个参数 (base, path)",
        "joinUrlPath() 第 1/2 个参数应为 string",
    ],
};

static DOC_PREPARE_MULTIPART_FIELD_FROM_BYTES: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "prepareMultiPartFieldFromBytes(fieldName, fileName, data, contentType) -> bytes",
    summary: "构建 multipart/form-data 的一个字段字节块（使用固定 boundary）。调用方需用相同 boundary 拼接多字段并设置 Content-Type。",
    params: &[
        ("fieldName", "表单字段名"),
        ("fileName", "上传文件名"),
        ("data", "字段内容 bytes/byteArray/string"),
        ("contentType", "该字段的 Content-Type"),
    ],
    returns: "bytes 该字段的完整 multipart 字节（含分隔符和头）",
    examples: &[
        "b := prepareMultiPartFieldFromBytes(\"file\", \"a.txt\", fileBytes, \"text/plain\")",
    ],
    errors: &[
        "prepareMultiPartFieldFromBytes() 需要 4 个参数 (fieldName, fileName, bytes, contentType)",
        "prepareMultiPartFieldFromBytes() 第 1 个参数应为 string (字段名)（可能原因：参数顺序错误）",
        "prepareMultiPartFieldFromBytes() 第 3 个参数应为 bytes/byteArray/string",
    ],
};

static DOC_FLUSH_RESP: BuiltinDoc = BuiltinDoc {
    category: "http",
    signature: "flushResp(resp) -> undefined",
    summary: "刷新 HTTP 响应缓冲区（当前实现为 no-op，因响应在 handler 返回后才写出；保留用于 API 兼容）。",
    params: &[("resp", "handler 中的响应对象")],
    returns: "undefined",
    examples: &["flushResp(resp)  // 当前为空操作"],
    errors: &[],
};

/// register 注册所有 HTTP 相关内置函数。
pub fn register(vm: &mut VM) {
    // 服务器管理
    vm.register_builtin_doc("httpServer", bi_http_server, &DOC_HTTP_SERVER);
    vm.register_builtin_doc("serverSetHandler", bi_server_set_handler, &DOC_SERVER_SET_HANDLER);
    vm.register_builtin_doc("serverSetStatic", bi_server_set_static, &DOC_SERVER_SET_STATIC);
    vm.register_builtin_doc("serverStart", bi_server_start, &DOC_SERVER_START);
    vm.register_builtin_doc("serverStop", bi_server_stop, &DOC_SERVER_STOP);
    vm.register_builtin_doc("serverAddr", bi_server_addr, &DOC_SERVER_ADDR);

    // 请求操作
    vm.register_builtin_doc("getReqMethod", bi_get_req_method, &DOC_GET_REQ_METHOD);
    vm.register_builtin_doc("getReqPath", bi_get_req_path, &DOC_GET_REQ_PATH);
    vm.register_builtin_doc("getReqUri", bi_get_req_uri, &DOC_GET_REQ_URI);
    vm.register_builtin_doc("getReqQuery", bi_get_req_query, &DOC_GET_REQ_QUERY);
    vm.register_builtin_doc("getReqHeader", bi_get_req_header, &DOC_GET_REQ_HEADER);
    vm.register_builtin_doc("getReqHeaders", bi_get_req_headers, &DOC_GET_REQ_HEADERS);
    vm.register_builtin_doc("getReqBody", bi_get_req_body, &DOC_GET_REQ_BODY);
    vm.register_builtin_doc("getReqBodyBytes", bi_get_req_body_bytes, &DOC_GET_REQ_BODY_BYTES);
    vm.register_builtin_doc("getReqParam", bi_get_req_param, &DOC_GET_REQ_PARAM);
    vm.register_builtin_doc("getReqParams", bi_get_req_params, &DOC_GET_REQ_PARAMS);

    // 响应操作
    vm.register_builtin_doc("writeResp", bi_write_resp, &DOC_WRITE_RESP);
    vm.register_builtin_doc("writeRespBytes", bi_write_resp_bytes, &DOC_WRITE_RESP_BYTES);
    vm.register_builtin_doc("writeRespHeader", bi_write_resp_header, &DOC_WRITE_RESP_HEADER);
    vm.register_builtin_doc("setRespHeader", bi_set_resp_header, &DOC_SET_RESP_HEADER);
    vm.register_builtin_doc("setRespStatus", bi_set_resp_status, &DOC_SET_RESP_STATUS);
    vm.register_builtin_doc("setRespContentType", bi_set_resp_content_type, &DOC_SET_RESP_CONTENT_TYPE);
    vm.register_builtin_doc("serveFile", bi_serve_file, &DOC_SERVE_FILE);
    vm.register_builtin_doc("redirectResp", bi_redirect_resp, &DOC_REDIRECT_RESP);
    vm.register_builtin_doc("genJsonResp", bi_gen_json_resp, &DOC_GEN_JSON_RESP);

    // 表单解析
    vm.register_builtin_doc("parseReqForm", bi_parse_req_form, &DOC_PARSE_REQ_FORM);
    vm.register_builtin_doc("saveFileUploads", bi_save_file_uploads, &DOC_SAVE_FILE_UPLOADS);

    // Cookie
    vm.register_builtin_doc("getReqCookie", bi_get_req_cookie, &DOC_GET_REQ_COOKIE);
    vm.register_builtin_doc("getReqCookies", bi_get_req_cookies, &DOC_GET_REQ_COOKIES);
    vm.register_builtin_doc("setRespCookie", bi_set_resp_cookie, &DOC_SET_RESP_COOKIE);

    // CORS
    vm.register_builtin_doc("setCorsHeaders", bi_set_cors_headers, &DOC_SET_CORS_HEADERS);

    // WebSocket
    vm.register_builtin_doc("webSocket", bi_web_socket, &DOC_WEB_SOCKET);
    vm.register_builtin_doc("wsReadMsg", bi_ws_read_msg, &DOC_WS_READ_MSG);
    vm.register_builtin_doc("wsReadText", bi_ws_read_text, &DOC_WS_READ_TEXT);
    vm.register_builtin_doc("wsReadBin", bi_ws_read_bin, &DOC_WS_READ_BIN);
    vm.register_builtin_doc("wsWriteText", bi_ws_write_text, &DOC_WS_WRITE_TEXT);
    vm.register_builtin_doc("wsWriteBin", bi_ws_write_bin, &DOC_WS_WRITE_BIN);
    vm.register_builtin_doc("wsWriteMsg", bi_ws_write_msg, &DOC_WS_WRITE_MSG);
    vm.register_builtin_doc("wsClose", bi_ws_close, &DOC_WS_CLOSE);
    vm.register_builtin_doc("wsLocalAddr", bi_ws_local_addr, &DOC_WS_LOCAL_ADDR);

    // HTTP 客户端
    vm.register_builtin_doc("getWeb", bi_get_web, &DOC_GET_WEB);
    vm.register_builtin_doc("getWebBytes", bi_get_web_bytes, &DOC_GET_WEB_BYTES);
    vm.register_builtin_doc("getWebBytesWithHeaders", bi_get_web_bytes_with_headers, &DOC_GET_WEB_BYTES_WITH_HEADERS);
    vm.register_builtin_doc("postWeb", bi_post_web, &DOC_POST_WEB);
    vm.register_builtin_doc("downloadFile", bi_download_file, &DOC_DOWNLOAD_FILE);
    vm.register_builtin_doc("urlExists", bi_url_exists, &DOC_URL_EXISTS);

    // URL 与 MIME 工具
    vm.register_builtin_doc("parseUrl", bi_parse_url, &DOC_PARSE_URL);
    vm.register_builtin_doc("getMimeType", bi_get_mime_type, &DOC_GET_MIME_TYPE);
    vm.register_builtin_doc("parseQuery", bi_parse_query, &DOC_PARSE_QUERY);
    vm.register_builtin_doc("joinUrlPath", bi_join_url_path, &DOC_JOIN_URL_PATH);

    // Multipart 构建
    vm.register_builtin_doc("prepareMultiPartFieldFromBytes", bi_prepare_multipart_field_from_bytes, &DOC_PREPARE_MULTIPART_FIELD_FROM_BYTES);

    // 响应刷新
    vm.register_builtin_doc("flushResp", bi_flush_resp, &DOC_FLUSH_RESP);
}

// ===========================================================================
// Native 包装类型
// ===========================================================================

/// SfHttpServer HTTP 服务器实例。
///
/// 包装路由表、静态文件目录、停止信号等。
/// 通过 Value::Native(Arc<SfHttpServer>) 暴露给脚本（HttpServer 保持 Native 包装，
/// 因为它是内部管理对象，不需要固定 TypeCode）。
pub struct SfHttpServer {
    /// addr 监听地址（如 "0.0.0.0:8080"）。
    pub addr: String,
    /// routes 路由表：path -> handler 函数值。
    /// 精确匹配优先于前缀匹配（"/api/" 为前缀匹配）。
    pub routes: Mutex<Vec<RouteEntry>>,
    /// static_dir 静态文件根目录（可选）。
    pub static_dir: Mutex<Option<std::path::PathBuf>>,
    /// verbose 是否打印请求日志。
    pub verbose: bool,
    /// running 是否正在运行。
    pub running: AtomicBool,
    /// stop 停止信号（true 时 accept 循环退出）。
    pub stop: Arc<AtomicBool>,
    /// globals 全局变量句柄（与创建者 VM 共享，使 handler 能访问脚本定义）。
    pub globals: Arc<Mutex<std::collections::HashMap<String, Value>>>,
    /// admin_token 管理端点令牌。
    pub admin_token: String,
    /// cert_dir TLS 证书目录（含 server.crt + server.key），空则不启用 HTTPS。
    pub cert_dir: String,
}

/// RouteEntry 一条路由。
#[derive(Clone)]
pub struct RouteEntry {
    /// path 路径模式。以 "/" 结尾表示前缀匹配，否则精确匹配。
    /// 支持 :param 参数提取，如 /api/users/:id
    pub path: String,
    /// handler 处理器（脚本函数值）。
    pub handler: Value,
    /// param_names 路径参数名列表（从 path 中提取的 :param 名）。
    /// 空列表表示无参数（纯精确或前缀匹配）。
    pub param_names: Vec<String>,
    /// segments 路径分段（按 / 拆分），用于参数路由匹配。
    /// None 表示纯精确/前缀匹配，无需分段。
    /// Some(Vec<Option<String>>) 中 None 表示参数位，Some(s) 表示静态段。
    pub segments: Option<Vec<Option<String>>>,
}

// ===========================================================================
// ActiveVMs 注册表（管理端点用）
// ===========================================================================

/// VmInfo 活跃 VM 信息（供 /admin/status 查看）。
struct VmInfo {
    info: String,
    start: std::time::Instant,
}

/// 全局活跃 VM 注册表。
static ACTIVE_VMS: once_cell_placeholder::OnceCell<Mutex<HashMap<u64, VmInfo>>> =
    once_cell_placeholder::OnceCell::new();

/// 简易 OnceCell（避免引入第三方 once_cell）。
mod once_cell_placeholder {
    use std::sync::Mutex;
    pub struct OnceCell<T>(Mutex<Option<T>>);
    impl<T> OnceCell<T> {
        pub const fn new() -> Self { OnceCell(Mutex::new(None)) }
        pub fn get_or_init<F: FnOnce() -> T>(&self, f: F) -> &T {
            let mut guard = self.0.lock().unwrap();
            if guard.is_none() {
                *guard = Some(f());
            }
            // SAFETY: 此时 guard 已有值，且 OnceCell 活跃期间不会清空
            unsafe { &*(guard.as_ref().unwrap() as *const T) }
        }
    }
}

use std::collections::HashMap;

/// active_vms 获取全局 ActiveVMs 表的引用。
fn active_vms() -> &'static Mutex<HashMap<u64, VmInfo>> {
    ACTIVE_VMS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 全局 VM ID 计数器。
static VM_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

// ===========================================================================
// 开关式参数解析工具
// ===========================================================================

/// get_switch 从参数列表中提取 --key=value 的值。
fn get_switch(args: &[Value], key: &str, default: &str) -> String {
    let p1 = format!("--{}=", key);
    let p2 = format!("-{}=", key);
    for arg in args {
        if let Value::Str(s) = arg {
            if let Some(rest) = s.strip_prefix(&p1).or_else(|| s.strip_prefix(&p2)) {
                return rest.to_string();
            }
        }
    }
    default.to_string()
}

/// has_switch 检查参数列表中是否存在 --key 开关。
fn has_switch(args: &[Value], key: &str) -> bool {
    let p1 = format!("--{}", key);
    let p2 = format!("-{}", key);
    args.iter().any(|arg| {
        if let Value::Str(s) = arg { &**s == p1 || &**s == p2 }
        else { false }
    })
}

// ===========================================================================
// Native 类型识别与提取工具
// ===========================================================================

/// extract_server 从 Value 中提取 SfHttpServer 引用。
///
/// 注意：Native 值存储为 Arc<Arc<SfHttpServer>>（双层 Arc），
/// 以便 downcast_ref::<Arc<SfHttpServer>>() 正确工作（与 concurrency 模块一致）。
fn extract_server<'a>(v: &'a Value) -> Result<&'a Arc<SfHttpServer>, Value> {
    match v {
        Value::Native(n) => {
            n.downcast_ref::<Arc<SfHttpServer>>().ok_or_else(|| {
                error_value(format!(
                    "参数应为 httpServer 对象 (可能原因：传入了其他 native 类型 '{}')",
                    v.type_name_ex()
                ))
            })
        }
        _ => Err(error_value(format!(
            "参数应为 httpServer 对象，得到 {} (可能原因：参数顺序错误)",
            v.type_name()
        ))),
    }
}

/// extract_req 从 Value 中提取 SfHttpRequest 引用。
///
/// Value::HttpReq 直接持有 Arc<SfHttpRequest>，无需 downcast。
fn extract_req<'a>(v: &'a Value) -> Result<&'a Arc<SfHttpRequest>, Value> {
    match v {
        Value::HttpReq(r) => Ok(r),
        _ => Err(error_value(format!(
            "参数应为 httpReq 对象，得到 {} (可能原因：参数顺序错误或未在 server 模式下运行)",
            v.type_name()
        ))),
    }
}

/// extract_resp 从 Value 中提取 SfHttpResponse 引用。
///
/// Value::HttpResp 直接持有 Arc<SfHttpResponse>，无需 downcast。
fn extract_resp<'a>(v: &'a Value) -> Result<&'a Arc<SfHttpResponse>, Value> {
    match v {
        Value::HttpResp(r) => Ok(r),
        _ => Err(error_value(format!(
            "参数应为 httpResp 对象，得到 {} (可能原因：参数顺序错误)",
            v.type_name()
        ))),
    }
}

// ===========================================================================
// 服务器管理内置函数
// ===========================================================================

/// bi_http_server 创建 HTTP 服务器实例。
///
/// 用法：`httpServer("--port=8080", "--host=0.0.0.0", "--verbose")`
/// HTTPS：`httpServer("--port=443", "--certDir=./certs")`
fn bi_http_server(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let port = get_switch(args, "port", "8080");
    let host = get_switch(args, "host", "0.0.0.0");
    let verbose = has_switch(args, "verbose");
    let admin_token = get_switch(args, "adminToken", "sflang");
    let cert_dir = get_switch(args, "certDir", "");

    let addr = format!("{}:{}", host, port);

    let server = Arc::new(SfHttpServer {
        addr,
        routes: Mutex::new(Vec::new()),
        static_dir: Mutex::new(None),
        verbose,
        running: AtomicBool::new(false),
        stop: Arc::new(AtomicBool::new(false)),
        globals: vm.globals_handle(),
        admin_token,
        cert_dir,
    });

    Ok(Value::Native(Arc::new(server)))  // 双层 Arc：外层 Arc<dyn Any>，内层 Arc<SfHttpServer>
}

/// bi_server_set_handler 注册路由处理器。
///
/// 用法：`serverSetHandler(server, "/path", handler)`
/// 或：`serverSetHandler(server, "/api/users/:id", handler)` 支持路径参数
/// handler 为脚本函数 func(req, resp)。
/// 路径中的 :param 部分会自动提取并注入到 routeParamsG 全局变量
fn bi_server_set_handler(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let server = extract_server(&args[0])?;
    let path = match &args[1] {
        Value::Str(s) => s.to_string(),
        v => return Err(error_value(format!(
            "serverSetHandler() 第 2 个参数应为 string (路径)，得到 {}",
            v.type_name()
        ))),
    };
    let handler = args[2].clone();

    if !matches!(handler, Value::Func(_) | Value::Builtin(_)) {
        return Err(error_value(format!(
            "serverSetHandler() 第 3 个参数应为 function，得到 {} (可能原因：未传入 func 或函数名拼写错误)",
            handler.type_name()
        )));
    }

    // 解析路径参数模式
    // 如果路径包含 :param，则提取参数名并构建分段
    let has_params = path.contains(":");
    let (param_names, segments) = if has_params && !path.ends_with('/') {
        let mut names = Vec::new();
        let mut segs: Vec<Option<String>> = Vec::new();
        for seg in path.split('/') {
            if let Some(name) = seg.strip_prefix(':') {
                names.push(name.to_string());
                segs.push(None); // None 标记参数位
            } else {
                segs.push(Some(seg.to_string()));
            }
        }
        (names, Some(segs))
    } else {
        (Vec::new(), None)
    };

    server.routes.lock().unwrap().push(RouteEntry { path, handler, param_names, segments });
    Ok(Value::Undefined)
}

/// bi_server_set_static 设置静态文件根目录。
///
/// 用法：`serverSetStatic(server, "/path/to/dir")`
fn bi_server_set_static(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let server = extract_server(&args[0])?;
    let dir = match &args[1] {
        Value::Str(s) => std::path::PathBuf::from(s.as_ref()),
        v => return Err(error_value(format!(
            "serverSetStatic() 第 2 个参数应为 string (目录路径)，得到 {}",
            v.type_name()
        ))),
    };

    *server.static_dir.lock().unwrap() = Some(dir);
    Ok(Value::Undefined)
}

/// bi_server_start 启动服务器。
///
/// 用法：`serverStart(server, "--thread")`
/// 默认阻塞当前线程；`--thread` 在后台线程运行。
fn bi_server_start(vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let server = extract_server(&args[0])?.clone();
    let in_thread = has_switch(args, "thread") || has_switch(args, "go");

    if server.running.load(Ordering::SeqCst) {
        return Err(error_value("serverStart() 服务器已在运行"));
    }
    server.running.store(true, Ordering::SeqCst);
    server.stop.store(false, Ordering::SeqCst);

    // 提取服务器配置（所有权转移到线程）
    let globals = server.globals.clone();
    let stop = server.stop.clone();
    let verbose = server.verbose;
    let admin_token = server.admin_token.clone();
    let cert_dir = server.cert_dir.clone();
    let routes = server.routes.lock().unwrap().clone();
    let static_dir = server.static_dir.lock().unwrap().clone();
    let addr = server.addr.clone();

    // 如果指定了 certDir，加载 TLS 配置
    let tls_config = if !cert_dir.is_empty() {
        match load_tls_config(&cert_dir) {
            Ok(cfg) => {
                eprintln!("HTTPS enabled, cert from {}", cert_dir);
                Some(cfg)
            }
            Err(e) => {
                server.running.store(false, Ordering::SeqCst);
                return Err(error_value(format!(
                    "serverStart() 加载 TLS 证书失败: {} (可能原因：certDir 下缺少 server.crt 或 server.key；文件格式非 PEM)",
                    e
                )));
            }
        }
    } else {
        None
    };

    if in_thread {
        // 后台线程运行
        std::thread::spawn(move || {
            run_server_blocking(&addr, routes, static_dir, &globals, verbose, &admin_token, &stop, tls_config);
            server.running.store(false, Ordering::SeqCst);
        });

        Ok(Value::Undefined)
    } else {
        // 阻塞当前线程
        let _ = vm;

        run_server_blocking(&addr, routes, static_dir, &globals, verbose, &admin_token, &stop, tls_config);
        server.running.store(false, Ordering::SeqCst);
        Ok(Value::Undefined)
    }
}

/// bi_server_stop 停止服务器。
///
/// 用法：`serverStop(server)`
fn bi_server_stop(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let server = extract_server(&args[0])?;
    server.stop.store(true, Ordering::SeqCst);
    Ok(Value::Undefined)
}

/// bi_server_addr 返回服务器的监听地址。
///
/// 用法：`serverAddr(server)`
fn bi_server_addr(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let server = extract_server(&args[0])?;
    Ok(Value::str(&server.addr))
}

// ===========================================================================
// 请求操作内置函数
// ===========================================================================

/// bi_get_req_method 返回请求方法。
fn bi_get_req_method(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(error_value("getReqMethod() 需要至少 1 个参数"));
    }
    let req = extract_req(&args[0])?;
    let method = req.inner.lock().unwrap().method.clone();
    Ok(Value::str(&method))
}

/// bi_get_req_path 返回请求路径。
fn bi_get_req_path(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let req = extract_req(&args[0])?;
    let path = req.inner.lock().unwrap().path.clone();
    Ok(Value::str(&path))
}

/// bi_get_req_uri 返回完整 URI。
fn bi_get_req_uri(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let req = extract_req(&args[0])?;
    let uri = req.inner.lock().unwrap().uri.clone();
    Ok(Value::str(&uri))
}

/// bi_get_req_query 返回查询串。
fn bi_get_req_query(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let req = extract_req(&args[0])?;
    let query = req.inner.lock().unwrap().query.clone();
    Ok(Value::str(&query))
}

/// bi_get_req_header 返回指定 header 的值。
///
/// 用法：`getReqHeader(req, "Content-Type")`
fn bi_get_req_header(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let req = extract_req(&args[0])?;
    let key = match &args[1] {
        Value::Str(s) => s.to_string(),
        v => return Err(error_value(format!(
            "getReqHeader() 第 2 个参数应为 string (header 名称)，得到 {}",
            v.type_name()
        ))),
    };
    let req = req.inner.lock().unwrap();
    match req.get_header(&key) {
        Some(v) => Ok(Value::str(v)),
        None => Ok(Value::Undefined),
    }
}

/// bi_get_req_headers 返回所有 header（Map 对象）。
fn bi_get_req_headers(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let req = extract_req(&args[0])?;
    let req = req.inner.lock().unwrap();
    let mut map = crate::ord_map::OrdMap::new();
    for (k, v) in &req.headers {
        map.set(k.clone(), Value::str(v));
    }
    Ok(Value::Map(Arc::new(Mutex::new(map))))
}

/// bi_get_req_body 返回请求体（字符串）。
fn bi_get_req_body(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let req = extract_req(&args[0])?;
    let body = req.inner.lock().unwrap().body.clone();
    let s = String::from_utf8_lossy(&body).into_owned();
    Ok(Value::str(&s))
}

/// bi_get_req_body_bytes 返回请求体（Bytes）。
fn bi_get_req_body_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let req = extract_req(&args[0])?;
    let body = req.inner.lock().unwrap().body.clone();
    Ok(Value::Bytes(Arc::new(body)))
}

/// bi_get_req_param 返回指定查询参数的值。
///
/// 用法：`getReqParam(req, "key")`
fn bi_get_req_param(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let req = extract_req(&args[0])?;
    let key = match &args[1] {
        Value::Str(s) => s.to_string(),
        v => return Err(error_value(format!(
            "getReqParam() 第 2 个参数应为 string (参数名)，得到 {}",
            v.type_name()
        ))),
    };
    let req = req.inner.lock().unwrap();
    let params = req.parse_query();
    for (k, v) in &params {
        if k == &key {
            return Ok(Value::str(v));
        }
    }
    Ok(Value::Undefined)
}

/// bi_get_req_params 返回所有查询参数（Map 对象）。
fn bi_get_req_params(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let req = extract_req(&args[0])?;
    let req = req.inner.lock().unwrap();
    let params = req.parse_query();
    let mut map = crate::ord_map::OrdMap::new();
    for (k, v) in &params {
        map.set(k.clone(), Value::str(v));
    }
    Ok(Value::Map(Arc::new(Mutex::new(map))))
}

// ===========================================================================
// 响应操作内置函数
// ===========================================================================

/// bi_write_resp 写入响应体（字符串）。
///
/// 用法：`writeResp(resp, "content")`
fn bi_write_resp(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let resp = extract_resp(&args[0])?;
    let content = match &args[1] {
        Value::Str(s) => s.as_bytes().to_vec(),
        v => v.to_str().into_bytes(),
    };
    resp.inner.lock().unwrap().write_body(&content);
    Ok(Value::Undefined)
}

/// bi_write_resp_bytes 写入响应体（字节）。
///
/// 用法：`writeRespBytes(resp, bytes)`
fn bi_write_resp_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let resp = extract_resp(&args[0])?;
    let content = match &args[1] {
        Value::Bytes(b) => b.as_ref().clone(),
        Value::ByteArray(b) => b.lock().unwrap().clone(),
        Value::Str(s) => s.as_bytes().to_vec(),
        v => return Err(error_value(format!(
            "writeRespBytes() 第 2 个参数应为 bytes/byteArray/string，得到 {}",
            v.type_name()
        ))),
    };
    resp.inner.lock().unwrap().write_body(&content);
    Ok(Value::Undefined)
}

/// bi_write_resp_header 写入状态码（兼容 Charlang 风格的函数名）。
///
/// 用法：`writeRespHeader(resp, 200)`
/// 等价于 setRespStatus。
fn bi_write_resp_header(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let resp = extract_resp(&args[0])?;
    let status = match &args[1] {
        Value::Int(i) => *i as u16,
        Value::Float(f) => *f as u16,
        v => return Err(error_value(format!(
            "writeRespHeader() 第 2 个参数应为 int (状态码)，得到 {}",
            v.type_name()
        ))),
    };
    resp.inner.lock().unwrap().status = status;
    Ok(Value::Undefined)
}

/// bi_set_resp_header 设置响应头。
///
/// 用法：`setRespHeader(resp, "Content-Type", "application/json")`
fn bi_set_resp_header(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let resp = extract_resp(&args[0])?;
    let key = match &args[1] {
        Value::Str(s) => s.to_string(),
        v => return Err(error_value(format!(
            "setRespHeader() 第 2 个参数应为 string (header 名)，得到 {}",
            v.type_name()
        ))),
    };
    let value = match &args[2] {
        Value::Str(s) => s.to_string(),
        v => v.to_str(),
    };
    resp.inner.lock().unwrap().set_header(key, value);
    Ok(Value::Undefined)
}

/// bi_set_resp_status 设置响应状态码。
///
/// 用法：`setRespStatus(resp, 404)`
fn bi_set_resp_status(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let resp = extract_resp(&args[0])?;
    let status = match &args[1] {
        Value::Int(i) => *i as u16,
        Value::Float(f) => *f as u16,
        v => return Err(error_value(format!(
            "setRespStatus() 第 2 个参数应为 int (状态码)，得到 {}",
            v.type_name()
        ))),
    };
    resp.inner.lock().unwrap().status = status;
    Ok(Value::Undefined)
}

/// bi_set_resp_content_type 设置 Content-Type 响应头。
///
/// 用法：`setRespContentType(resp, "application/json")`
fn bi_set_resp_content_type(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let resp = extract_resp(&args[0])?;
    let ct = match &args[1] {
        Value::Str(s) => s.to_string(),
        v => return Err(error_value(format!(
            "setRespContentType() 第 2 个参数应为 string，得到 {}",
            v.type_name()
        ))),
    };
    resp.inner.lock().unwrap().set_header("Content-Type".to_string(), ct);
    Ok(Value::Undefined)
}

/// bi_serve_file 将文件内容作为响应。
///
/// 用法：`serveFile(resp, "/path/to/file")`
fn bi_serve_file(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let resp = extract_resp(&args[0])?;
    let path = match &args[1] {
        Value::Str(s) => s.to_string(),
        v => return Err(error_value(format!(
            "serveFile() 第 2 个参数应为 string (文件路径)，得到 {}",
            v.type_name()
        ))),
    };

    match std::fs::read(&path) {
        Ok(data) => {
            let mut r = resp.inner.lock().unwrap();
            r.write_body(&data);
            // 自动设置 Content-Type
            let mime = http_lite::guess_mime_type(&path);
            if r.content_type().is_none() {
                r.set_header("Content-Type".to_string(), mime.to_string());
            }
            Ok(Value::Undefined)
        }
        Err(e) => Ok(error_value(format!(
            "serveFile() 读取文件 '{}' 失败: {} (可能原因：文件不存在或权限不足)",
            path, e
        ))),
    }
}

/// bi_redirect_resp 设置重定向响应。
///
/// 用法：`redirectResp(resp, "https://example.com", 302)` 或 `redirectResp(resp, url)` (默认 302)
fn bi_redirect_resp(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let resp = extract_resp(&args[0])?;
    let url = match &args[1] {
        Value::Str(s) => s.to_string(),
        v => return Err(error_value(format!(
            "redirectResp() 第 2 个参数应为 string (URL)，得到 {}",
            v.type_name()
        ))),
    };
    let code = match args.get(2) {
        Some(Value::Int(i)) => *i as u16,
        Some(Value::Float(f)) => *f as u16,
        _ => 302,
    };

    let mut r = resp.inner.lock().unwrap();
    r.status = code;
    r.set_header("Location".to_string(), url);
    Ok(Value::Undefined)
}

/// bi_gen_json_resp 生成 JSON 响应。
///
/// 用法：`genJsonResp(resp, status, message)` → {"status": status, "msg": message}
fn bi_gen_json_resp(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let resp = extract_resp(&args[0])?;
    let status_val = args.get(1).cloned().unwrap_or(Value::Undefined);
    let msg_val = args.get(2).cloned().unwrap_or(Value::Undefined);

    // 手动构建简单 JSON（避免调用 jsonEncode 的循环依赖）
    let status_str = match &status_val {
        Value::Str(s) => format!("\"{}\"", s),
        Value::Int(i) => i.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Undefined => "null".to_string(),
        v => format!("\"{}\"", v.inspect()),
    };
    let msg_str = match &msg_val {
        Value::Str(s) => format!("\"{}\"", s.replace('"', "\\\"")),
        Value::Undefined => "null".to_string(),
        v => format!("\"{}\"", v.inspect()),
    };

    let json = format!("{{\"status\": {}, \"msg\": {}}}", status_str, msg_str);

    let mut r = resp.inner.lock().unwrap();
    r.set_header("Content-Type".to_string(), "application/json; charset=utf-8".to_string());
    r.write_body(json.as_bytes());
    Ok(Value::Undefined)
}

// ===========================================================================
// Cookie 支持
// ===========================================================================

/// bi_get_req_cookie 获取请求中的指定 Cookie 值。
///
/// 用法：`getReqCookie(req, "sessionId")`
/// 返回 undefined 表示该 Cookie 不存在
fn bi_get_req_cookie(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.len() < 2 {
        return Err(error_value("getReqCookie() 需要 2 个参数 (req, name)"));
    }
    let req = extract_req(&args[0])?;
    let name = match &args[1] {
        Value::Str(s) => s.to_string(),
        v => return Err(error_value(format!(
            "getReqCookie() 第 2 个参数应为 string (cookie 名)，得到 {}",
            v.type_name()
        ))),
    };
    let req = req.inner.lock().unwrap();
    let cookies = parse_cookie_header(req.get_header("cookie").unwrap_or(""));
    match cookies.into_iter().find(|(k, _)| k == &name) {
        Some((_, v)) => Ok(Value::str(&v)),
        None => Ok(Value::Undefined),
    }
}

/// bi_get_req_cookies 获取所有 Cookie（Map 对象）。
///
/// 用法：`getReqCookies(req)` -> {"name1": "value1", "name2": "value2"}
fn bi_get_req_cookies(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(error_value("getReqCookies() 需要 1 个参数 (req)"));
    }
    let req = extract_req(&args[0])?;
    let req = req.inner.lock().unwrap();
    let cookies = parse_cookie_header(req.get_header("cookie").unwrap_or(""));
    let mut map = crate::ord_map::OrdMap::new();
    for (k, v) in cookies {
        map.set(k, Value::str(&v));
    }
    Ok(Value::Map(Arc::new(Mutex::new(map))))
}

/// bi_set_resp_cookie 设置响应的 Set-Cookie 头。
///
/// 用法：`setRespCookie(resp, name, value, "--path=/", "--maxAge=3600", "--httpOnly", "--secure", "--sameSite=strict")`
/// 可选开关：--path, --domain, --maxAge(秒), --expires(RFC1123日期), --httpOnly, --secure, --sameSite=strict|lax|none
fn bi_set_resp_cookie(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.len() < 3 {
        return Err(error_value("setRespCookie() 至少需要 3 个参数 (resp, name, value)"));
    }
    let resp = extract_resp(&args[0])?;
    let name = match &args[1] {
        Value::Str(s) => s.to_string(),
        v => return Err(error_value(format!(
            "setRespCookie() 第 2 个参数应为 string (cookie 名)，得到 {}",
            v.type_name()
        ))),
    };
    let value = match &args[2] {
        Value::Str(s) => s.to_string(),
        v => v.to_str(),
    };

    let mut cookie = format!("{}={}", name, value);

    // 解析可选开关参数
    for i in 3..args.len() {
        let opt = match &args[i] {
            Value::Str(s) => s.to_string(),
            v => v.to_str(),
        };
        if let Some(val) = opt.strip_prefix("--path=") {
            cookie.push_str(&format!("; Path={}", val));
        } else if let Some(val) = opt.strip_prefix("--domain=") {
            cookie.push_str(&format!("; Domain={}", val));
        } else if let Some(val) = opt.strip_prefix("--maxAge=") {
            let secs: i64 = val.parse().unwrap_or(0);
            cookie.push_str(&format!("; Max-Age={}", secs));
        } else if let Some(val) = opt.strip_prefix("--expires=") {
            cookie.push_str(&format!("; Expires={}", val));
        } else if let Some(val) = opt.strip_prefix("--sameSite=") {
            cookie.push_str(&format!("; SameSite={}", val));
        } else if opt == "--httpOnly" {
            cookie.push_str("; HttpOnly");
        } else if opt == "--secure" {
            cookie.push_str("; Secure");
        }
    }

    resp.inner.lock().unwrap().set_header("Set-Cookie".to_string(), cookie);
    Ok(Value::Undefined)
}

/// parse_cookie_header 解析 Cookie 请求头。
///
/// Cookie 头格式：`name1=value1; name2=value2`
/// 返回有序键值对列表
fn parse_cookie_header(header: &str) -> Vec<(String, String)> {
    let mut result = Vec::new();
    for pair in header.split(';') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        if let Some(pos) = pair.find('=') {
            let name = pair[..pos].trim().to_string();
            let value = pair[pos + 1..].trim().to_string();
            result.push((name, value));
        } else {
            // 没有 = 的 cookie 值，跳过
        }
    }
    result
}

// ===========================================================================
// CORS 支持
// ===========================================================================

/// bi_set_cors_headers 设置 CORS 响应头。
///
/// 用法：`setCorsHeaders(resp, "https://example.com")`
/// 或：`setCorsHeaders(resp, "*")` 允许所有来源
/// 自动设置：Access-Control-Allow-Origin, Allow-Methods, Allow-Headers, Allow-Credentials
fn bi_set_cors_headers(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.len() < 2 {
        return Err(error_value("setCorsHeaders() 需要 2 个参数 (resp, origin)"));
    }
    let resp = extract_resp(&args[0])?;
    let origin = match &args[1] {
        Value::Str(s) => s.to_string(),
        v => return Err(error_value(format!(
            "setCorsHeaders() 第 2 个参数应为 string (origin)，得到 {}",
            v.type_name()
        ))),
    };

    let mut r = resp.inner.lock().unwrap();
    r.set_header("Access-Control-Allow-Origin".to_string(), origin);
    r.set_header("Access-Control-Allow-Methods".to_string(), "GET, POST, PUT, DELETE, OPTIONS, PATCH".to_string());
    r.set_header("Access-Control-Allow-Headers".to_string(), "Content-Type, Authorization, X-Requested-With".to_string());
    r.set_header("Access-Control-Allow-Credentials".to_string(), "true".to_string());
    r.set_header("Access-Control-Max-Age".to_string(), "86400".to_string());
    Ok(Value::Undefined)
}

// ===========================================================================
// TLS 支持
// ===========================================================================

/// TlsConfig TLS 配置（rustls）。
type TlsConfig = Arc<rustls::ServerConfig>;

/// load_tls_config 从指定目录加载 PEM 格式的证书和私钥，构造 rustls ServerConfig。
///
/// 期望目录下有 `server.crt`（证书链）和 `server.key`（私钥）。
fn load_tls_config(cert_dir: &str) -> Result<TlsConfig, String> {
    use std::io::BufReader;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};

    let cert_path = std::path::Path::new(cert_dir).join("server.crt");
    let key_path = std::path::Path::new(cert_dir).join("server.key");

    let cert_file = std::fs::File::open(&cert_path)
        .map_err(|e| format!("打开证书文件 {} 失败: {}", cert_path.display(), e))?;
    let cert_reader = &mut BufReader::new(cert_file);
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(cert_reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("解析证书 PEM 失败: {}", e))?;
    if certs.is_empty() {
        return Err("证书文件中未找到任何证书".to_string());
    }

    let key_file = std::fs::File::open(&key_path)
        .map_err(|e| format!("打开私钥文件 {} 失败: {}", key_path.display(), e))?;
    let key_reader = &mut BufReader::new(key_file);
    let key = rustls_pemfile::private_key(key_reader)
        .map_err(|e| format!("解析私钥 PEM 失败: {}", e))?
        .ok_or_else(|| "私钥文件中未找到任何私钥".to_string())?;

    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, PrivateKeyDer::try_from(key).map_err(|e| format!("私钥格式转换失败: {}", e))?)
        .map_err(|e| format!("构建 TLS 配置失败: {}", e))?;

    Ok(Arc::new(config))
}

// ===========================================================================
// 服务器执行核心
// ===========================================================================

/// run_server_blocking 阻塞运行 HTTP/HTTPS 服务器。
///
/// 在指定地址监听，每个连接在新线程中处理。
/// 路由匹配 -> 执行 handler -> 根据返回值类型生成响应。
///
/// # 参数
/// - `addr`: 监听地址
/// - `routes`: 路由表
/// - `static_dir`: 静态文件根目录
/// - `globals`: 共享全局变量
/// - `verbose`: 是否打印日志
/// - `admin_token`: 管理端点令牌
/// - `stop`: 停止信号
/// - `tls_config`: TLS 配置（None 则纯 HTTP）
fn run_server_blocking(
    addr: &str,
    routes: Vec<RouteEntry>,
    static_dir: Option<std::path::PathBuf>,
    globals: &Arc<Mutex<std::collections::HashMap<String, Value>>>,
    verbose: bool,
    admin_token: &str,
    stop: &Arc<AtomicBool>,
    tls_config: Option<TlsConfig>,
) {
    let listener = match std::net::TcpListener::bind(addr) {
        Ok(l) => {
            let proto = if tls_config.is_some() { "HTTPS" } else { "HTTP" };
            eprintln!("Sflang {} server listening on {}", proto, addr);
            l
        }
        Err(e) => {
            eprintln!("bind {} failed: {}", addr, e);
            return;
        }
    };

    // 设置非阻塞模式以便轮询 stop 信号
    let _ = listener.set_nonblocking(true);

    // 用 Arc 共享 handler 数据
    let handler = Arc::new(ServerHandler {
        routes,
        static_dir,
        globals: globals.clone(),
        verbose,
        admin_token: admin_token.to_string(),
        tls_config: tls_config.clone(),
    });

    loop {
        if stop.load(Ordering::SeqCst) {
            eprintln!("server stopping...");
            break;
        }

        match listener.accept() {
            Ok((stream, _peer)) => {
                let h = handler.clone();
                std::thread::spawn(move || {
                    handle_connection_thread(stream, &h);
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => {
                eprintln!("accept error: {}", e);
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
}

/// ServerHandler 服务器请求处理器（实现 HttpHandler trait）。
struct ServerHandler {
    routes: Vec<RouteEntry>,
    static_dir: Option<std::path::PathBuf>,
    globals: Arc<Mutex<std::collections::HashMap<String, Value>>>,
    verbose: bool,
    admin_token: String,
    /// tls_config TLS 配置（None 则纯 HTTP）。
    tls_config: Option<TlsConfig>,
}

impl HttpHandler for ServerHandler {
    fn handle(&self, req: LiteReq) -> LiteResp {
        // 管理端点
        if req.path == "/admin/status" {
            return self.handle_admin_status(&req);
        }
        if req.path == "/admin/kill" {
            return self.handle_admin_kill(&req);
        }

        // 路由匹配
        if let Some((handler_val, route_params)) = self.match_route(&req.path) {
            return self.execute_handler(handler_val, req, route_params);
        }

        // 静态文件
        if let Some(ref dir) = self.static_dir {
            return self.serve_static(dir, &req);
        }

        // 无匹配
        let mut resp = LiteResp::new();
        resp.status = 404;
        resp.set_header("Content-Type".to_string(), "text/plain; charset=utf-8".to_string());
        resp.write_body(format!("404 Not Found: {}", req.path).as_bytes());
        resp
    }
}

impl ServerHandler {
    /// match_route 匹配路由，返回 (handler, 路径参数)。
    ///
    /// 匹配优先级：
    ///   1. 精确匹配（无参数）
    ///   2. 参数路由匹配（/api/users/:id）
    ///   3. 前缀匹配（"/api/" 匹配 "/api/xxx"）
    fn match_route(&self, req_path: &str) -> Option<(Value, Vec<(String, String)>)> {
        // 精确匹配
        for entry in &self.routes {
            if entry.segments.is_none() && entry.path == req_path {
                return Some((entry.handler.clone(), Vec::new()));
            }
        }
        // 参数路由匹配
        let req_segs: Vec<&str> = req_path.split('/').collect();
        for entry in &self.routes {
            if let Some(ref pattern) = entry.segments {
                if let Some(params) = match_param_route(pattern, &entry.param_names, &req_segs) {
                    return Some((entry.handler.clone(), params));
                }
            }
        }
        // 前缀匹配（"/api/" 匹配 "/api/xxx"）
        for entry in &self.routes {
            if entry.segments.is_none() && entry.path.ends_with('/') && req_path.starts_with(entry.path.as_str()) {
                return Some((entry.handler.clone(), Vec::new()));
            }
        }
        None
    }

    /// execute_handler 在新 VM 中执行 handler 函数。
    ///
    /// 注入 requestG/responseG 等全局变量，根据返回值类型生成响应。
    fn execute_handler(&self, handler_val: Value, req: LiteReq, route_params: Vec<(String, String)>) -> LiteResp {
        let sf_req = Arc::new(SfHttpRequest::new(req));
        let resp = Arc::new(SfHttpResponse::new());

        // 创建 Sflang 实例（自带完整 VM + builtins），共享全局环境
        let mut sf = crate::api::Sflang::new();
        sf.vm_mut().set_globals_handle(self.globals.clone());

        // 注入请求上下文全局变量
        {
            let mut g = self.globals.lock().unwrap();
            g.insert("requestG".to_string(), Value::HttpReq(sf_req.clone()));
            g.insert("responseG".to_string(), Value::HttpResp(resp.clone()));

            let req_guard = sf_req.inner.lock().unwrap();
            g.insert("reqUriG".to_string(), Value::str(&req_guard.uri));
            g.insert("reqPathG".to_string(), Value::str(&req_guard.path));
            g.insert("reqMethodG".to_string(), Value::str(&req_guard.method));
            g.insert("inputG".to_string(),
                Value::str(&String::from_utf8_lossy(&req_guard.body)));
            g.insert("runModeG".to_string(), Value::str("sfserver"));

            // 路由参数：将 :param 提取的值注入 routeParamsG
            let mut params_map = crate::ord_map::OrdMap::new();
            for (k, v) in &route_params {
                params_map.set(k.clone(), Value::str(v));
            }
            g.insert("routeParamsG".to_string(), Value::Map(Arc::new(Mutex::new(params_map))));
        }

        // 注册到 ActiveVMs
        let vm_id = VM_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
        let info_str = {
            let r = sf_req.inner.lock().unwrap();
            format!("{} {}", r.method, r.uri)
        };
        active_vms().lock().unwrap().insert(vm_id, VmInfo {
            info: info_str,
            start: std::time::Instant::now(),
        });

        // 调用 handler 函数：通过 call_function_value 执行
        let result = sf.vm_mut().call_function_value(handler_val, vec![
            Value::HttpReq(sf_req.clone()),
            Value::HttpResp(resp.clone()),
        ]);

        // 从 ActiveVMs 移除
        active_vms().lock().unwrap().remove(&vm_id);

        // 恢复全局变量（清理 requestG 等临时变量）
        {
            let mut g = self.globals.lock().unwrap();
            g.remove("requestG");
            g.remove("responseG");
            g.remove("reqUriG");
            g.remove("reqPathG");
            g.remove("reqMethodG");
            g.remove("inputG");
            g.remove("runModeG");
            g.remove("routeParamsG");
        }

        // 根据返回值类型生成响应
        let mut resp_guard = resp.inner.lock().unwrap();

        match result {
            Ok(ret) => {
                match ret {
                    Value::Str(s) => {
                        // 字符串 → 作为响应体输出
                        resp_guard.write_body(s.as_bytes());
                    }
                    Value::Bytes(b) => {
                        // Bytes → 作为响应体输出
                        resp_guard.write_body(&b);
                    }
                    Value::ByteArray(b) => {
                        // ByteArray → 作为响应体输出
                        resp_guard.write_body(&b.lock().unwrap());
                    }
                    Value::Error(e) => {
                        // Error → 500 + 错误详情（AI 友好）
                        resp_guard.status = 500;
                        resp_guard.set_header(
                            "Content-Type".to_string(),
                            "application/json; charset=utf-8".to_string(),
                        );
                        let error_json = format_error_json(&e);
                        resp_guard.write_body(error_json.as_bytes());
                    }
                    _ => {
                        // 其他类型（undefined/int/bool/...）→ 不输出
                        // 脚本应已通过 writeResp 自行写响应
                    }
                }
            }
            Err(err_val) => {
                // handler 执行抛出异常（未被 try-catch）
                resp_guard.status = 500;
                resp_guard.set_header(
                    "Content-Type".to_string(),
                    "application/json; charset=utf-8".to_string(),
                );
                let error_json = match &err_val {
                    Value::Error(e) => format_error_json(e),
                    _ => format!(
                        r#"{{"error": "{}"}}"#,
                        err_val.inspect().replace('"', "\\\"").replace('\\', "\\\\")
                    ),
                };
                resp_guard.write_body(error_json.as_bytes());
            }
        }

        let result_resp = resp_guard.clone();
        drop(resp_guard);
        result_resp
    }

    /// serve_static 提供静态文件服务。
    fn serve_static(&self, root: &std::path::Path, req: &LiteReq) -> LiteResp {
        // 安全：规范化路径，防止目录穿越
        let rel_path = req.path.trim_start_matches('/');
        let full_path = root.join(rel_path);

        // 检查路径是否在 root 内（防止 .. 穿越）
        let canonical = match full_path.canonicalize() {
            Ok(p) => p,
            Err(_) => {
                let mut r = LiteResp::new();
                r.status = 404;
                r.set_header("Content-Type".to_string(), "text/plain; charset=utf-8".to_string());
                r.write_body(b"404 Not Found");
                return r;
            }
        };

        let root_canonical = match root.canonicalize() {
            Ok(p) => p,
            Err(_) => {
                let mut r = LiteResp::new();
                r.status = 500;
                r.write_body(b"500 Internal Server Error");
                return r;
            }
        };

        if !canonical.starts_with(&root_canonical) {
            let mut r = LiteResp::new();
            r.status = 403;
            r.write_body(b"403 Forbidden");
            return r;
        }

        // 目录 → 尝试 index.html
        let target = if canonical.is_dir() {
            canonical.join("index.html")
        } else {
            canonical
        };

        // 检查扩展名白名单
        if !http_lite::is_web_ext(&target.to_string_lossy()) && target.is_file() {
            let mut r = LiteResp::new();
            r.status = 403;
            r.write_body(b"403 Forbidden: file type not allowed");
            return r;
        }

        match std::fs::read(&target) {
            Ok(data) => {
                let mut r = LiteResp::new();
                let mime = http_lite::guess_mime_type(&target.to_string_lossy());
                r.set_header("Content-Type".to_string(), mime.to_string());
                r.write_body(&data);
                r
            }
            Err(_) => {
                let mut r = LiteResp::new();
                r.status = 404;
                r.set_header("Content-Type".to_string(), "text/plain; charset=utf-8".to_string());
                r.write_body(b"404 Not Found");
                r
            }
        }
    }

    /// handle_admin_status 处理 /admin/status 请求。
    fn handle_admin_status(&self, req: &LiteReq) -> LiteResp {
        if !self.is_admin(req) {
            return self.forbidden();
        }

        let vms = active_vms().lock().unwrap();
        let vm_list: Vec<String> = vms.iter().map(|(id, info)| {
            let duration = info.start.elapsed().as_millis();
            format!(r#"{{"id": {}, "info": "{}", "duration_ms": {}}}"#,
                id, info.info, duration)
        }).collect();

        let json = format!(r#"{{"count": {}, "vms": [{}]}}"#, vms.len(), vm_list.join(", "));

        let mut r = LiteResp::new();
        r.set_header("Content-Type".to_string(), "application/json; charset=utf-8".to_string());
        r.write_body(json.as_bytes());
        r
    }

    /// handle_admin_kill 处理 /admin/kill 请求。
    ///
    /// 目前仅返回信息（VM 中断需要更深层的 VM 改造，留作后续增强）。
    fn handle_admin_kill(&self, req: &LiteReq) -> LiteResp {
        if !self.is_admin(req) {
            return self.forbidden();
        }

        // 解析 id 参数
        let id: Option<u64> = req.parse_query().iter()
            .find(|(k, _)| k == "id")
            .and_then(|(_, v)| v.parse().ok());

        let mut r = LiteResp::new();
        r.set_header("Content-Type".to_string(), "application/json; charset=utf-8".to_string());

        match id {
            Some(id) => {
                // 从 ActiveVMs 移除（标记为已完成）
                let existed = active_vms().lock().unwrap().remove(&id).is_some();
                let msg = if existed {
                    format!(r#"{{"killed": true, "id": {}}}"#, id)
                } else {
                    format!(r#"{{"killed": false, "id": {}, "msg": "vm not found"}}"#, id)
                };
                r.write_body(msg.as_bytes());
            }
            None => {
                r.status = 400;
                r.write_body(br#"{"error": "missing id parameter"}"#);
            }
        }
        r
    }

    /// is_admin 检查是否为管理请求（localhost + token）。
    fn is_admin(&self, req: &LiteReq) -> bool {
        // 检查来源 IP（仅允许 localhost）
        let is_local = req.remote_addr.starts_with("127.0.0.1")
            || req.remote_addr.starts_with("::1")
            || req.remote_addr.starts_with("[::1]")
            || req.remote_addr.is_empty();

        if !is_local {
            return false;
        }

        // 检查 token（query 参数或 header）
        let token_from_query = req.parse_query().iter()
            .find(|(k, _)| k == "token")
            .map(|(_, v)| v.clone());

        let token_from_header = req.get_header("x-admin-token").map(|s| s.to_string());

        let token = token_from_query.or(token_from_header);
        token.as_deref() == Some(&self.admin_token)
    }

    /// forbidden 返回 403 响应。
    fn forbidden(&self) -> LiteResp {
        let mut r = LiteResp::new();
        r.status = 403;
        r.set_header("Content-Type".to_string(), "text/plain; charset=utf-8".to_string());
        r.write_body(b"403 Forbidden");
        r
    }
}

/// match_param_route 匹配参数路由模式。
///
/// pattern 是路由分段，None 表示参数位，Some(s) 表示静态段。
/// param_names 是参数名列表（按出现顺序对应参数位）。
/// req_segs 是请求路径的分段。
/// 返回 Some(params) 如果匹配成功，否则 None。
fn match_param_route(pattern: &[Option<String>], param_names: &[String], req_segs: &[&str]) -> Option<Vec<(String, String)>> {
    if pattern.len() != req_segs.len() {
        return None;
    }
    let mut params = Vec::new();
    let mut param_idx = 0;
    for (p_seg, r_seg) in pattern.iter().zip(req_segs.iter()) {
        match p_seg {
            None => {
                // 参数位：提取值
                if r_seg.is_empty() {
                    return None; // 参数值不能为空
                }
                if param_idx < param_names.len() {
                    params.push((param_names[param_idx].clone(), r_seg.to_string()));
                    param_idx += 1;
                }
            }
            Some(static_seg) => {
                // 静态段必须精确匹配
                if static_seg != r_seg {
                    return None;
                }
            }
        }
    }
    Some(params)
}

/// handle_connection_thread 在线程中处理一个 TCP 连接。
///
/// 支持 keep-alive，循环读取请求直到对端关闭。
fn handle_connection_thread(stream: std::net::TcpStream, handler: &ServerHandler) {
    // 如果启用了 TLS，用 rustls 包装连接
    if let Some(ref tls_config) = handler.tls_config {
        let conn = rustls::ServerConnection::new(tls_config.clone());
        match conn {
            Ok(server_conn) => {
                let tls_stream = rustls::StreamOwned::new(server_conn, stream);
                handle_connection_impl(tls_stream, handler);
            }
            Err(e) => {
                if handler.verbose {
                    eprintln!("TLS handshake init failed: {}", e);
                }
            }
        }
    } else {
        handle_connection_impl(stream, handler);
    }
}

/// handle_connection_impl 泛型处理一个连接（支持 TcpStream 和 TLS Stream）。
///
/// 支持 keep-alive，循环读取请求直到对端关闭或出错。
fn handle_connection_impl<S: std::io::Read + std::io::Write>(stream: S, handler: &ServerHandler) {
    use std::io::BufReader;

    let peer = "unknown".to_string(); // TLS 包装后无法直接获取 peer_addr
    let mut reader = BufReader::new(stream);
    // 使用 reader 内部引用来写入（BufReader 不支持 Write，需要 try_clone 方式）
    // 对于 TLS 流，无法 clone，所以使用 by_ref 方式分别读写
    // 实际上 BufReader 内部的 stream 可以通过 get_mut 获取
    let mut requests_left = 100;

    loop {
        if requests_left == 0 {
            break;
        }
        requests_left -= 1;

        // 设置读取超时（仅对 TcpStream 有效，TLS 流忽略）
        // reader.get_mut() 返回底层流的可变引用

        let mut req = match http_lite::parse_request(&mut reader) {
            Ok(r) => r,
            Err(http_lite::HttpError::Io(ref e)) if e.kind() == std::io::ErrorKind::TimedOut => {
                break;
            }
            Err(http_lite::HttpError::Io(ref e)) if e.kind() == std::io::ErrorKind::ConnectionReset => {
                break;
            }
            Err(http_lite::HttpError::Io(ref e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(http_lite::HttpError::Parse(msg)) => {
                if msg == "connection closed" {
                    break;
                }
                if handler.verbose {
                    eprintln!("parse error: {}", msg);
                }
                let mut resp = LiteResp::new();
                resp.status = 400;
                resp.set_header("Content-Type".to_string(), "text/plain; charset=utf-8".to_string());
                resp.write_body(format!("Bad Request: {}", msg).as_bytes());
                let _ = http_lite::write_response(reader.get_mut(), &resp);
                break;
            }
            Err(e) => {
                if handler.verbose {
                    eprintln!("error: {}", e);
                }
                break;
            }
        };

        req.remote_addr = peer.clone();

        if handler.verbose {
            eprintln!("{} {} {}", req.method, req.uri, req.remote_addr);
        }

        let resp = handler.handle(req);

        let should_close = resp.headers.iter().any(|(k, v)| {
            k.eq_ignore_ascii_case("connection") && v.eq_ignore_ascii_case("close")
        });

        let _ = http_lite::write_response(reader.get_mut(), &resp);

        if should_close {
            break;
        }
    }
}

/// format_error_json 将 SfError 格式化为 AI 友好的 JSON 错误响应。
///
/// 包含 message、stack、possibleCauses 提示。
fn format_error_json(e: &SfError) -> String {
    let msg = e.message.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
    let stack_items: Vec<String> = e.stack.iter().map(|s| {
        format!(r#""{}""#, s.replace('\\', "\\\\").replace('"', "\\\""))
    }).collect();

    // AI 友好提示：常见错误原因
    let possible_causes = vec![
        "变量拼写错误",
        "函数名拼写错误",
        "参数顺序错误",
        "类型不匹配",
        "未定义的全局变量（检查 runModeG 是否为 sfserver）",
    ];
    let causes_items: Vec<String> = possible_causes.iter().map(|s| {
        format!(r#""{}""#, s)
    }).collect();

    format!(
        r#"{{"error": "{}", "stack": [{}], "possibleCauses": [{}]}}"#,
        msg,
        stack_items.join(", "),
        causes_items.join(", ")
    )
}

// ===========================================================================
// CLI 应用服务器入口（sf -server）
// ===========================================================================

/// run_server_cli CLI 应用服务器入口。
///
/// 由 sf 主程序在检测到 -server 参数时调用。
/// 启动文件路由模式的 HTTP 服务器：URL 路径映射到 .sf 脚本文件。
///
/// # 参数
/// - `args`: 命令行参数（含 -server 本身）
///
/// # 路由规则
/// 1. 路径对应目录 → 找 index.sf
/// 2. 路径对应 .sf 文件 → 编译执行
/// 3. 路径对应白名单扩展名文件 → 静态服务
/// 4. 追加 .sf 再试
/// 5. 无匹配 → 404
pub fn run_server_cli(args: &[String]) -> i32 {
    let port = get_switch_str(args, "port", "8080");
    let host = get_switch_str(args, "host", "0.0.0.0");
    let base_dir = get_switch_str(args, "dir", ".");
    let web_dir = get_switch_str(args, "webDir", &base_dir);
    let admin_token = get_switch_str(args, "adminToken", "sflang");
    let verbose = has_switch_str(args, "verbose");
    let cert_dir = get_switch_str(args, "certDir", "");

    let addr = format!("{}:{}", host, port);

    let base_path = std::path::PathBuf::from(&base_dir);
    let web_path = std::path::PathBuf::from(&web_dir);

    // 加载 TLS 配置（如果指定了 certDir）
    let tls_config = if !cert_dir.is_empty() {
        match load_tls_config(&cert_dir) {
            Ok(cfg) => {
                eprintln!("HTTPS enabled, cert from {}", cert_dir);
                Some(cfg)
            }
            Err(e) => {
                eprintln!("加载 TLS 证书失败: {}", e);
                eprintln!("可能原因：certDir 下缺少 server.crt 或 server.key");
                return 1;
            }
        }
    } else {
        None
    };

    let proto = if tls_config.is_some() { "HTTPS" } else { "HTTP" };
    eprintln!("Sflang CLI {} server starting on {}", proto, addr);
    eprintln!("  script dir: {}", base_dir);
    eprintln!("  web dir: {}", web_dir);

    let listener = match std::net::TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("bind {} failed: {}", addr, e);
            eprintln!("可能原因：端口被占用、权限不足");
            return 1;
        }
    };

    let _ = listener.set_nonblocking(true);

    let stop = Arc::new(AtomicBool::new(false));

    // 设置 Ctrl+C 处理
    let stop_clone = stop.clone();
    let _ = ctrlc_set_handler(stop_clone);

    loop {
        if stop.load(Ordering::SeqCst) {
            eprintln!("\nserver stopping...");
            break;
        }

        match listener.accept() {
            Ok((stream, _peer)) => {
                let base = base_path.clone();
                let web = web_path.clone();
                let token = admin_token.clone();
                let verb = verbose;
                let tls = tls_config.clone();
                std::thread::spawn(move || {
                    handle_cli_connection(stream, &base, &web, &token, verb, tls);
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => {
                eprintln!("accept error: {}", e);
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }

    0
}

/// handle_cli_connection 处理 CLI 服务器的一个连接。
fn handle_cli_connection(
    stream: std::net::TcpStream,
    base_dir: &std::path::Path,
    web_dir: &std::path::Path,
    admin_token: &str,
    verbose: bool,
    tls_config: Option<TlsConfig>,
) {
    // 如果启用了 TLS，用 rustls 包装连接
    if let Some(ref tls_cfg) = tls_config {
        match rustls::ServerConnection::new(tls_cfg.clone()) {
            Ok(server_conn) => {
                let tls_stream = rustls::StreamOwned::new(server_conn, stream);
                handle_cli_connection_impl(tls_stream, base_dir, web_dir, admin_token, verbose);
            }
            Err(e) => {
                if verbose {
                    eprintln!("TLS handshake init failed: {}", e);
                }
            }
        }
    } else {
        handle_cli_connection_impl(stream, base_dir, web_dir, admin_token, verbose);
    }
}

/// handle_cli_connection_impl 泛型处理 CLI 服务器的一个连接。
fn handle_cli_connection_impl<S: std::io::Read + std::io::Write>(
    stream: S,
    base_dir: &std::path::Path,
    web_dir: &std::path::Path,
    admin_token: &str,
    verbose: bool,
) {
    use std::io::BufReader;

    let peer = "unknown".to_string();

    let mut reader = BufReader::new(stream);

    let mut requests_left = 100;

    loop {
        if requests_left == 0 {
            break;
        }
        requests_left -= 1;

        let mut req = match http_lite::parse_request(&mut reader) {
            Ok(r) => r,
            Err(http_lite::HttpError::Io(ref e)) if e.kind() == std::io::ErrorKind::TimedOut => {
                break;
            }
            Err(http_lite::HttpError::Io(ref e)) if e.kind() == std::io::ErrorKind::ConnectionReset => {
                break;
            }
            Err(http_lite::HttpError::Io(ref e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(http_lite::HttpError::Parse(msg)) => {
                if msg == "connection closed" {
                    break;
                }
                if verbose {
                    eprintln!("parse error from {}: {}", peer, msg);
                }
                let mut resp = LiteResp::new();
                resp.status = 400;
                resp.set_header("Content-Type".to_string(), "text/plain; charset=utf-8".to_string());
                resp.write_body(format!("Bad Request: {}", msg).as_bytes());
                let _ = http_lite::write_response(reader.get_mut(), &resp);
                break;
            }
            Err(e) => {
                if verbose {
                    eprintln!("error from {}: {}", peer, e);
                }
                break;
            }
        };

        req.remote_addr = peer.clone();

        if verbose {
            eprintln!("{} {} {}", req.method, req.uri, req.remote_addr);
        }

        // 管理端点
        if req.path == "/admin/status" || req.path == "/admin/kill" {
            let resp = handle_cli_admin(&req, admin_token);
            let _ = http_lite::write_response(reader.get_mut(), &resp);
            continue;
        }

        // 文件路由
        let resp = route_and_execute(&req, base_dir, web_dir);

        let should_close = resp.headers.iter().any(|(k, v)| {
            k.eq_ignore_ascii_case("connection") && v.eq_ignore_ascii_case("close")
        });

        let _ = http_lite::write_response(reader.get_mut(), &resp);

        if should_close {
            break;
        }
    }
}

/// route_and_execute CLI 服务器的文件路由与脚本执行。
///
/// 路由规则：
/// 1. 目录 -> 找 index.sf -> index.sfp -> index.html
/// 2. .sf 文件 -> 编译执行
/// 3. .sfp 文件 -> 动态页面渲染（HTML + 内嵌 <?sf ... ?> 代码块）
/// 4. 白名单扩展名 -> 静态服务
/// 5. 非白名单扩展名 -> 检查 .sfAllow 文件（glob 白名单）
/// 6. 追加 .sf 再试
/// 7. web 目录查找静态文件（同样支持 .sfAllow）
/// 8. 404
fn route_and_execute(req: &LiteReq, base_dir: &std::path::Path, web_dir: &std::path::Path) -> LiteResp {
    let rel_path = req.path.trim_start_matches('/');
    let rel_path = if rel_path.is_empty() { "." } else { rel_path };

    // 1. 在脚本目录查找
    let script_target = base_dir.join(rel_path);

    // 目录 -> index.sf -> index.sfp -> index.html
    if script_target.is_dir() {
        let index_sf = script_target.join("index.sf");
        if index_sf.is_file() {
            return execute_script_file(&index_sf, req, base_dir);
        }
        let index_sfp = script_target.join("index.sfp");
        if index_sfp.is_file() {
            return execute_sfp_file(&index_sfp, req, base_dir);
        }
        // 尝试静态 web 目录的 index.html
        let web_target = web_dir.join(rel_path).join("index.html");
        if web_target.is_file() {
            return serve_static_file(&web_target);
        }
    }

    // 已存在的文件
    if script_target.is_file() {
        let ext = script_target.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        match ext.as_str() {
            // .sf 文件 -> 执行脚本
            "sf" => return execute_script_file(&script_target, req, base_dir),
            // .sfp 文件 -> 动态页面渲染
            "sfp" => return execute_sfp_file(&script_target, req, base_dir),
            _ => {}
        }
        // 白名单扩展名 -> 静态服务
        if http_lite::is_web_ext(&script_target.to_string_lossy()) {
            return serve_static_file(&script_target);
        }
        // 非白名单扩展名 -> 检查 .sfAllow
        if let Some(resp) = check_sf_allow_and_serve(&script_target, req) {
            return resp;
        }
    }

    // 追加 .sf 再试
    let with_sf = script_target.with_extension("sf");
    if with_sf.is_file() {
        return execute_script_file(&with_sf, req, base_dir);
    }

    // 追加 .sfp 再试
    let with_sfp = script_target.with_extension("sfp");
    if with_sfp.is_file() {
        return execute_sfp_file(&with_sfp, req, base_dir);
    }

    // 在 web 目录查找静态文件
    let web_target = web_dir.join(rel_path);
    if web_target.is_file() {
        if http_lite::is_web_ext(&web_target.to_string_lossy()) {
            return serve_static_file(&web_target);
        }
        // web 目录也支持 .sfAllow
        if let Some(resp) = check_sf_allow_and_serve(&web_target, req) {
            return resp;
        }
    }

    // 404
    let mut r = LiteResp::new();
    r.status = 404;
    r.set_header("Content-Type".to_string(), "text/plain; charset=utf-8".to_string());
    r.write_body(format!("404 Not Found: {}", req.path).as_bytes());
    r
}

/// execute_script_file 执行一个 .sf 脚本文件来处理请求。
///
/// 注入请求上下文全局变量，执行脚本，根据返回值类型生成响应。
fn execute_script_file(script_path: &std::path::Path, req: &LiteReq, base_dir: &std::path::Path) -> LiteResp {
    let src = match std::fs::read_to_string(script_path) {
        Ok(s) => s,
        Err(e) => {
            let mut r = LiteResp::new();
            r.status = 500;
            r.set_header("Content-Type".to_string(), "application/json; charset=utf-8".to_string());
            let msg = format!("read script failed: {}", e);
            r.write_body(format!(r#"{{"error": "{}"}}"#, msg.replace('"', "\\\"")).as_bytes());
            return r;
        }
    };

    let mut sf = crate::api::Sflang::new();
    sf.set_output(std::io::sink());

    // 注入请求上下文全局变量
    let req_val = Arc::new(SfHttpRequest::new(req.clone()));
    let resp_val = Arc::new(SfHttpResponse::new());

    sf.set_global("requestG", Value::HttpReq(req_val.clone()));
    sf.set_global("responseG", Value::HttpResp(resp_val.clone()));
    sf.set_global("reqUriG", Value::str(&req.uri));
    sf.set_global("reqPathG", Value::str(&req.path));
    sf.set_global("reqMethodG", Value::str(&req.method));
    sf.set_global("inputG", Value::str(&String::from_utf8_lossy(&req.body)));
    sf.set_global("basePathG", Value::str(&base_dir.to_string_lossy()));
    sf.set_global("scriptPathG", Value::str(&script_path.to_string_lossy()));
    sf.set_global("runModeG", Value::str("sfserver"));

    // 注册到 ActiveVMs
    let vm_id = VM_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
    active_vms().lock().unwrap().insert(vm_id, VmInfo {
        info: format!("{} {} (file: {})", req.method, req.uri, script_path.display()),
        start: std::time::Instant::now(),
    });

    let result = sf.run_string(&src);

    active_vms().lock().unwrap().remove(&vm_id);

    let mut resp_guard = resp_val.inner.lock().unwrap();

    match result {
        Ok(ret) => {
            match ret {
                Value::Str(s) => {
                    resp_guard.write_body(s.as_bytes());
                }
                Value::Bytes(b) => {
                    resp_guard.write_body(&b);
                }
                Value::ByteArray(b) => {
                    resp_guard.write_body(&b.lock().unwrap());
                }
                Value::Error(e) => {
                    resp_guard.status = 500;
                    resp_guard.set_header(
                        "Content-Type".to_string(),
                        "application/json; charset=utf-8".to_string(),
                    );
                    resp_guard.write_body(format_error_json(&e).as_bytes());
                }
                _ => {
                    // 其他类型 → 不输出（脚本应已通过 writeResp 自行写响应）
                }
            }
        }
        Err(err_val) => {
            resp_guard.status = 500;
            resp_guard.set_header(
                "Content-Type".to_string(),
                "application/json; charset=utf-8".to_string(),
            );
            let error_json = match &err_val {
                Value::Error(e) => format_error_json(e),
                _ => format!(
                    r#"{{"error": "{}"}}"#,
                    err_val.inspect().replace('"', "\\\"").replace('\\', "\\\\")
                ),
            };
            resp_guard.write_body(error_json.as_bytes());
        }
    }

    resp_guard.clone()
}

/// serve_static_file 服务一个静态文件。
fn serve_static_file(path: &std::path::Path) -> LiteResp {
    match std::fs::read(path) {
        Ok(data) => {
            let mut r = LiteResp::new();
            let mime = http_lite::guess_mime_type(&path.to_string_lossy());
            r.set_header("Content-Type".to_string(), mime.to_string());
            r.write_body(&data);
            r
        }
        Err(_) => {
            let mut r = LiteResp::new();
            r.status = 404;
            r.set_header("Content-Type".to_string(), "text/plain; charset=utf-8".to_string());
            r.write_body(b"404 Not Found");
            r
        }
    }
}

/// execute_sfp_file 渲染一个 .sfp 动态页面文件。
///
/// .sfp 文件是 HTML 模板，其中内嵌 `<?sf ... ?>` 代码块。
/// 代码块外的文本原样输出，代码块被执行后其返回值插入到输出中。
/// 多个代码块共享同一个 Sflang 实例（状态互通）。
///
/// 类似 PHP 的 `<?php ... ?>` 机制。
///
/// # 全局变量注入
/// 与 execute_script_file 相同：requestG/responseG/reqPathG/reqMethodG 等。
/// runModeG 设为 "sfp"。
///
/// # 错误处理
/// 单个代码块出错时，错误信息内联显示为 `[块序号] 错误信息`，不中断页面渲染。
fn execute_sfp_file(sfp_path: &std::path::Path, req: &LiteReq, base_dir: &std::path::Path) -> LiteResp {
    // 读取 .sfp 文件内容
    let template = match std::fs::read_to_string(sfp_path) {
        Ok(s) => s,
        Err(e) => {
            let mut r = LiteResp::new();
            r.status = 500;
            r.set_header("Content-Type".to_string(), "text/plain; charset=utf-8".to_string());
            r.write_body(format!("读取 .sfp 文件失败: {}", e).as_bytes());
            return r;
        }
    };

    // 创建 Sflang 实例，注入请求上下文
    let mut sf = crate::api::Sflang::new();
    sf.set_output(std::io::sink());

    let req_val = Arc::new(SfHttpRequest::new(req.clone()));
    let resp_val = Arc::new(SfHttpResponse::new());

    sf.set_global("requestG", Value::HttpReq(req_val.clone()));
    sf.set_global("responseG", Value::HttpResp(resp_val.clone()));
    sf.set_global("reqUriG", Value::str(&req.uri));
    sf.set_global("reqPathG", Value::str(&req.path));
    sf.set_global("reqMethodG", Value::str(&req.method));
    sf.set_global("inputG", Value::str(&String::from_utf8_lossy(&req.body)));
    sf.set_global("basePathG", Value::str(&base_dir.to_string_lossy()));
    sf.set_global("scriptPathG", Value::str(&sfp_path.to_string_lossy()));
    sf.set_global("runModeG", Value::str("sfp"));

    // 用正则分割模板：<?sf ... ?> 代码块 vs 静态文本
    // (?s) = dotall（. 匹配换行）；非贪婪匹配支持多个代码块
    let re = regex::Regex::new(r"(?s)<\?sf(.*?)\?>").unwrap();

    let mut output = String::new();
    let mut block_count = 0u32;

    // 遍历所有匹配，输出代码块之间的静态文本和代码块结果
    let mut last_end = 0;
    for mat in re.find_iter(&template) {
        // 输出代码块之前的静态文本
        output.push_str(&template[last_end..mat.start()]);

        // 提取代码块内容（去掉 <?sf 和 ?>）
        let code = &mat.as_str()[4..mat.as_str().len() - 2]; // "<?sf" = 4 chars, "?>" = 2 chars

        block_count += 1;

        // 执行代码块
        match sf.run_string(code) {
            Ok(result) => {
                // 根据返回值类型决定输出
                match result {
                    Value::Str(s) => output.push_str(&s),
                    Value::Int(i) => output.push_str(&i.to_string()),
                    Value::Float(f) => output.push_str(&f.to_string()),
                    Value::Bool(b) => output.push_str(&b.to_string()),
                    Value::Undefined => {} // undefined -> 不输出
                    Value::Error(e) => {
                        // 错误内联显示
                        output.push_str(&format!("[{} 错误] {}", block_count, e.message));
                    }
                    other => output.push_str(&other.to_str()),
                }
            }
            Err(err_val) => {
                // 执行异常：内联显示错误
                let msg = match &err_val {
                    Value::Error(e) => e.message.clone(),
                    other => other.inspect(),
                };
                output.push_str(&format!("[{} 错误] {}", block_count, msg));
            }
        }

        last_end = mat.end();
    }

    // 输出最后一个代码块之后的静态文本
    output.push_str(&template[last_end..]);

    // 构建响应
    let mut r = resp_val.inner.lock().unwrap().clone();

    // 如果脚本通过 writeResp 手动写了响应，使用已写入的内容
    // 否则用拼接的模板输出作为响应体
    if r.body.is_empty() {
        r.set_header("Content-Type".to_string(), "text/html; charset=utf-8".to_string());
        r.write_body(output.as_bytes());
    }

    r
}

/// check_sf_allow_and_serve 检查 .sfAllow 文件，决定是否服务非白名单扩展名的文件。
///
/// 在文件所在目录下查找 `.sfAllow` 文件，每行一个 glob 模式（# 开头为注释）。
/// 如果某个模式匹配文件名，则以 attachment 方式服务该文件（强制下载）。
/// 不匹配则返回 None（调用方进一步处理，通常 404）。
///
/// # .sfAllow 文件格式
/// ```text
/// # 允许下载 CSV 文件
/// *.csv
/// # 允许特定文件
/// data-?.bin
/// secret.dat
/// ```
fn check_sf_allow_and_serve(file_path: &std::path::Path, _req: &LiteReq) -> Option<LiteResp> {
    let dir = file_path.parent()?;
    let base_name = file_path.file_name()?.to_str()?;
    let allow_file = dir.join(".sfAllow");

    let allow_content = std::fs::read_to_string(&allow_file).ok()?;

    // 逐行检查 glob 匹配
    for line in allow_content.lines() {
        let line = line.trim();
        // 跳过空行和注释
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // glob 匹配文件名
        if glob_match(line, base_name) {
            // 匹配成功：以 attachment 方式服务文件
            return Some(serve_file_as_attachment(file_path));
        }
    }

    None
}

/// serve_file_as_attachment 以附件下载方式服务文件。
///
/// 设置 Content-Disposition: attachment 强制浏览器下载。
fn serve_file_as_attachment(path: &std::path::Path) -> LiteResp {
    match std::fs::read(path) {
        Ok(data) => {
            let mut r = LiteResp::new();
            let mime = http_lite::guess_mime_type(&path.to_string_lossy());
            r.set_header("Content-Type".to_string(), mime.to_string());
            // 强制下载
            let filename = path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("download");
            r.set_header(
                "Content-Disposition".to_string(),
                format!("attachment; filename=\"{}\"", filename),
            );
            r.write_body(&data);
            r
        }
        Err(_) => {
            let mut r = LiteResp::new();
            r.status = 404;
            r.write_body(b"404 Not Found");
            r
        }
    }
}

/// glob_match 简单的 shell glob 匹配（支持 * 和 ?）。
///
/// 不使用第三方库，手写实现。支持：
/// - `*` 匹配任意数量字符（不含路径分隔符）
/// - `?` 匹配单个字符
/// - 其他字符精确匹配
fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    glob_match_inner(&p, 0, &t, 0)
}

/// glob_match_inner glob 匹配递归实现。
fn glob_match_inner(p: &[char], pi: usize, t: &[char], ti: usize) -> bool {
    if pi == p.len() {
        return ti == t.len();
    }
    match p[pi] {
        '*' => {
            // * 匹配 0 个或多个字符
            // 尝试匹配 0 个字符（跳过 *）或匹配 1 个字符（消耗 text 一个字符后继续）
            if glob_match_inner(p, pi + 1, t, ti) {
                return true;
            }
            if ti < t.len() && glob_match_inner(p, pi, t, ti + 1) {
                return true;
            }
            false
        }
        '?' => {
            // ? 匹配恰好 1 个字符
            ti < t.len() && glob_match_inner(p, pi + 1, t, ti + 1)
        }
        c => {
            // 精确匹配
            ti < t.len() && t[ti] == c && glob_match_inner(p, pi + 1, t, ti + 1)
        }
    }
}

/// handle_cli_admin 处理 CLI 服务器的管理端点。
fn handle_cli_admin(req: &LiteReq, admin_token: &str) -> LiteResp {
    // 检查权限
    let is_local = req.remote_addr.starts_with("127.0.0.1")
        || req.remote_addr.starts_with("::1")
        || req.remote_addr.starts_with("[::1]")
        || req.remote_addr.is_empty();

    if !is_local {
        let mut r = LiteResp::new();
        r.status = 403;
        r.write_body(b"403 Forbidden");
        return r;
    }

    let token_from_query = req.parse_query().iter()
        .find(|(k, _)| k == "token")
        .map(|(_, v)| v.clone());
    let token_from_header = req.get_header("x-admin-token").map(|s| s.to_string());
    let token = token_from_query.or(token_from_header);

    if token.as_deref() != Some(admin_token) {
        let mut r = LiteResp::new();
        r.status = 403;
        r.write_body(b"403 Forbidden: invalid token");
        return r;
    }

    if req.path == "/admin/status" {
        let vms = active_vms().lock().unwrap();
        let vm_list: Vec<String> = vms.iter().map(|(id, info)| {
            let duration = info.start.elapsed().as_millis();
            format!(r#"{{"id": {}, "info": "{}", "duration_ms": {}}}"#,
                id, info.info, duration)
        }).collect();

        let json = format!(r#"{{"count": {}, "vms": [{}]}}"#, vms.len(), vm_list.join(", "));
        let mut r = LiteResp::new();
        r.set_header("Content-Type".to_string(), "application/json; charset=utf-8".to_string());
        r.write_body(json.as_bytes());
        return r;
    }

    // /admin/kill
    let id: Option<u64> = req.parse_query().iter()
        .find(|(k, _)| k == "id")
        .and_then(|(_, v)| v.parse().ok());

    let mut r = LiteResp::new();
    r.set_header("Content-Type".to_string(), "application/json; charset=utf-8".to_string());

    match id {
        Some(id) => {
            let existed = active_vms().lock().unwrap().remove(&id).is_some();
            let msg = if existed {
                format!(r#"{{"killed": true, "id": {}}}"#, id)
            } else {
                format!(r#"{{"killed": false, "id": {}, "msg": "vm not found"}}"#, id)
            };
            r.write_body(msg.as_bytes());
        }
        None => {
            r.status = 400;
            r.write_body(br#"{"error": "missing id parameter"}"#);
        }
    }
    r
}

// ===========================================================================
// 字符串参数解析工具（CLI 用）
// ===========================================================================

/// get_switch_str 从字符串参数列表中提取 --key=value。
fn get_switch_str(args: &[String], key: &str, default: &str) -> String {
    let p1 = format!("--{}=", key);
    let p2 = format!("-{}=", key);
    for arg in args {
        if let Some(rest) = arg.strip_prefix(&p1).or_else(|| arg.strip_prefix(&p2)) {
            return rest.to_string();
        }
    }
    default.to_string()
}

/// has_switch_str 检查字符串参数列表中是否存在 --key。
fn has_switch_str(args: &[String], key: &str) -> bool {
    let p1 = format!("--{}", key);
    let p2 = format!("-{}", key);
    args.iter().any(|arg| arg == &p1 || arg == &p2)
}

/// ctrlc_set_handler 设置 Ctrl+C 处理器（跨平台）。
fn ctrlc_set_handler(stop: Arc<AtomicBool>) -> std::io::Result<()> {
    // 使用平台特定的方式设置 Ctrl+C
    // 简化实现：启动一个监听线程读取 Ctrl+C 信号
    // 完整实现需要 OS 特定 API，此处用简化方案
    #[cfg(unix)]
    {
        // Unix: 使用 signal-hook 会引入第三方库，这里用 SIGINT 的 fallback
        // 实际上 set_nonblocking + 轮询已经能处理大多数情况
        // 用户可以通过 serverStop 或直接终止进程
        let _ = stop;
    }
    #[cfg(windows)]
    {
        let _ = stop;
    }
    Ok(())
}

// ===========================================================================
// WebSocket 内置函数
// ===========================================================================

/// extract_ws 从 Value 中提取 SfWebSocket 引用。
fn extract_ws<'a>(v: &'a Value) -> Result<&'a Arc<SfWebSocket>, Value> {
    match v {
        Value::WebSocket(ws) => Ok(ws),
        _ => Err(error_value(format!(
            "参数应为 webSocket 对象，得到 {} (可能原因：未先调用 webSocket() 建立连接)",
            v.type_name()
        ))),
    }
}

/// bi_web_socket 创建 WebSocket 连接。
///
/// 服务端模式：`webSocket(req, resp)` -- 从 HTTP 请求升级为 WebSocket
/// 客户端模式：`webSocket("ws://host:port/path")` -- 连接到远程 WebSocket 服务器
fn bi_web_socket(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Err(error_value("webSocket() 需要至少 1 个参数"));
    }

    // 客户端模式：参数为 URL 字符串
    if let Value::Str(url) = &args[0] {
        // 手动解析 URL 并建立 TCP 连接，再用 tungstenite::client() 升级
        // 避免 connect() 对 MaybeTlsStream 的 TLS feature 依赖
        let parsed = http_lite::parse_ws_url(url);
        match parsed {
            Ok((host, port, path)) => {
                let addr = format!("{}:{}", host, port);
                match std::net::TcpStream::connect(&addr) {
                    Ok(stream) => {
                        // 构造 WebSocket 握手请求
                        let key = tungstenite::handshake::client::generate_key();
                        let full_uri = format!("ws://{}:{}{}", host, port, path);
                        let request = tungstenite::handshake::client::Request::builder()
                            .method("GET")
                            .uri(&full_uri)
                            .header("Host", &host)
                            .header("Connection", "Upgrade")
                            .header("Upgrade", "websocket")
                            .header("Sec-WebSocket-Version", "13")
                            .header("Sec-WebSocket-Key", &key)
                            .body(())
                            .map_err(|e| error_value(format!(
                                "webSocket() 构造握手请求失败: {}", e
                            )))?;
                        match tungstenite::client(request, stream) {
                            Ok((ws, _response)) => {
                                let ws_arc = Arc::new(SfWebSocket::new(ws));
                                Ok(Value::WebSocket(ws_arc))
                            }
                            Err(e) => Err(error_value(format!(
                                "webSocket() 握手失败: {} (可能原因：服务器不支持 WebSocket、路径错误)",
                                e
                            ))),
                        }
                    }
                    Err(e) => Err(error_value(format!(
                        "webSocket() TCP 连接失败: {} (可能原因：服务器未启动、网络不通、防火墙拦截)",
                        e
                    ))),
                }
            }
            Err(e) => Err(error_value(format!(
                "webSocket() URL 解析失败: {} (可能原因：URL 格式应为 ws://host:port/path)",
                e
            ))),
        }
    } else {
        // 服务端模式：参数为 (req, resp)
        // 从 HTTP 请求中提取原始 TcpStream 进行升级
        // 注意：当前架构中 req/resp 是包装对象，无法直接获取底层 TcpStream
        // WebSocket 服务端升级需要在连接处理层面进行，而非在 handler 内部
        // 此处提供基于 tungstenite accept 的简化实现
        Err(error_value(
            "webSocket() 服务端模式暂不支持在 handler 内升级。请使用 wsReadMsg/wsWriteMsg 操作已建立的 WebSocket 连接。"
        ))
    }
}

/// bi_ws_read_msg 读取一条 WebSocket 消息。
///
/// 返回 [type, data]，type 为 1=文本 2=二进制，data 为内容。
fn bi_ws_read_msg(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use tungstenite::Message;
    let ws = extract_ws(&args[0])?;
    let mut guard = ws.inner.lock().unwrap();
    match guard.read() {
        Ok(msg) => {
            match msg {
                Message::Text(text) => {
                    let arr = vec![
                        Value::Int(1),
                        Value::str(&text),
                    ];
                    Ok(Value::Array(Arc::new(Mutex::new(arr))))
                }
                Message::Binary(bin) => {
                    let arr = vec![
                        Value::Int(2),
                        Value::Bytes(Arc::new(bin.to_vec())),
                    ];
                    Ok(Value::Array(Arc::new(Mutex::new(arr))))
                }
                Message::Ping(_) | Message::Pong(_) => {
                    // Ping/Pong 自动处理，继续读下一条
                    drop(guard);
                    bi_ws_read_msg(_vm, args)
                }
                Message::Close(_) => {
                    Ok(Value::Array(Arc::new(Mutex::new(vec![
                        Value::Int(0),
                        Value::str("closed"),
                    ]))))
                }
                Message::Frame(_) => {
                    Ok(Value::Array(Arc::new(Mutex::new(vec![
                        Value::Int(0),
                        Value::str("frame"),
                    ]))))
                }
            }
        }
        Err(e) => Err(error_value(format!(
            "wsReadMsg() 读取失败: {} (可能原因：连接已关闭、网络中断)",
            e
        ))),
    }
}

/// bi_ws_read_text 读取一条文本消息。
fn bi_ws_read_text(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use tungstenite::Message;
    let ws = extract_ws(&args[0])?;
    let mut guard = ws.inner.lock().unwrap();
    loop {
        match guard.read() {
            Ok(Message::Text(text)) => {
                return Ok(Value::str(&text));
            }
            Ok(Message::Close(_)) => {
                return Ok(Value::Undefined);
            }
            Ok(Message::Ping(_) | Message::Pong(_)) => continue,
            Ok(_) => continue,
            Err(e) => {
                return Err(error_value(format!(
                    "wsReadText() 读取失败: {} (可能原因：连接已关闭)",
                    e
                )));
            }
        }
    }
}

/// bi_ws_read_bin 读取一条二进制消息。
fn bi_ws_read_bin(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use tungstenite::Message;
    let ws = extract_ws(&args[0])?;
    let mut guard = ws.inner.lock().unwrap();
    loop {
        match guard.read() {
            Ok(Message::Binary(bin)) => {
                return Ok(Value::Bytes(Arc::new(bin.to_vec())));
            }
            Ok(Message::Close(_)) => {
                return Ok(Value::Undefined);
            }
            Ok(Message::Ping(_) | Message::Pong(_)) => continue,
            Ok(_) => continue,
            Err(e) => {
                return Err(error_value(format!(
                    "wsReadBin() 读取失败: {} (可能原因：连接已关闭)",
                    e
                )));
            }
        }
    }
}

/// bi_ws_write_text 发送文本消息。
///
/// 用法：`wsWriteText(ws, "hello")`
fn bi_ws_write_text(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use tungstenite::Message;
    let ws = extract_ws(&args[0])?;
    let text = match &args[1] {
        Value::Str(s) => s.to_string(),
        v => v.to_str(),
    };
    let mut guard = ws.inner.lock().unwrap();
    guard.send(Message::Text(text.into()))
        .map_err(|e| error_value(format!(
            "wsWriteText() 发送失败: {} (可能原因：连接已关闭)",
            e
        )))?;
    Ok(Value::Undefined)
}

/// bi_ws_write_bin 发送二进制消息。
///
/// 用法：`wsWriteBin(ws, bytes)`
fn bi_ws_write_bin(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use tungstenite::Message;
    let ws = extract_ws(&args[0])?;
    let data = match &args[1] {
        Value::Bytes(b) => b.to_vec(),
        Value::ByteArray(b) => b.lock().unwrap().clone(),
        Value::Str(s) => s.as_bytes().to_vec(),
        v => return Err(error_value(format!(
            "wsWriteBin() 第 2 个参数应为 bytes/byteArray/string，得到 {}",
            v.type_name()
        ))),
    };
    let mut guard = ws.inner.lock().unwrap();
    guard.send(Message::Binary(data.into()))
        .map_err(|e| error_value(format!(
            "wsWriteBin() 发送失败: {} (可能原因：连接已关闭)",
            e
        )))?;
    Ok(Value::Undefined)
}

/// bi_ws_write_msg 发送消息（指定类型）。
///
/// 用法：`wsWriteMsg(ws, type, data)` type=1 文本, type=2 二进制
fn bi_ws_write_msg(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use tungstenite::Message;
    let ws = extract_ws(&args[0])?;
    let msg_type = match &args[1] {
        Value::Int(i) => *i,
        v => return Err(error_value(format!(
            "wsWriteMsg() 第 2 个参数应为 int (类型: 1=文本, 2=二进制)，得到 {}",
            v.type_name()
        ))),
    };

    let mut guard = ws.inner.lock().unwrap();
    match msg_type {
        1 => {
            let text = match &args[2] {
                Value::Str(s) => s.to_string(),
                v => v.to_str(),
            };
            guard.send(Message::Text(text.into()))
                .map_err(|e| error_value(format!("wsWriteMsg() 发送失败: {}", e)))?;
        }
        2 => {
            let data = match &args[2] {
                Value::Bytes(b) => b.to_vec(),
                Value::ByteArray(b) => b.lock().unwrap().clone(),
                Value::Str(s) => s.as_bytes().to_vec(),
                v => return Err(error_value(format!(
                    "wsWriteMsg() 二进制消息需要 bytes/string，得到 {}", v.type_name()
                ))),
            };
            guard.send(Message::Binary(data.into()))
                .map_err(|e| error_value(format!("wsWriteMsg() 发送失败: {}", e)))?;
        }
        _ => return Err(error_value(format!(
            "wsWriteMsg() 类型码 {} 无效 (1=文本, 2=二进制)", msg_type
        ))),
    }
    Ok(Value::Undefined)
}

/// bi_ws_close 关闭 WebSocket 连接。
fn bi_ws_close(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    use tungstenite::Message;
    let ws = extract_ws(&args[0])?;
    let mut guard = ws.inner.lock().unwrap();
    let _ = guard.send(Message::Close(None));
    let _ = guard.close(None);
    Ok(Value::Undefined)
}

/// bi_ws_local_addr 返回本地地址（如果可获取）。
fn bi_ws_local_addr(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let _ws = extract_ws(&args[0])?;
    // tungstenite 的 WebSocket 内部是 MaybeTlsStream<TcpStream>，
    // 获取 local_addr 需要访问底层 TcpStream，此处简化为返回 undefined
    Ok(Value::Undefined)
}

// ===========================================================================
// Multipart 表单解析
// ===========================================================================

/// bi_parse_req_form 解析请求体中的表单数据。
///
/// 支持 application/x-www-form-urlencoded 和 multipart/form-data。
/// 返回 Map 对象：普通字段为 string，文件字段为 object {fileName, size, content(bytes)}。
///
/// 用法：`parseReqForm(req)`
fn bi_parse_req_form(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let req = extract_req(&args[0])?;
    let req_guard = req.inner.lock().unwrap();

    let content_type = req_guard.get_header("content-type").unwrap_or("").to_string();
    let body = &req_guard.body;

    let mut map = crate::ord_map::OrdMap::new();

    if content_type.starts_with("application/x-www-form-urlencoded") {
        // URL 编码表单
        let body_str = String::from_utf8_lossy(body);
        let pairs = http_lite::url_decode_pairs(&body_str);
        for (k, v) in pairs {
            map.set(k, Value::str(&v));
        }
    } else if content_type.starts_with("multipart/form-data") {
        // multipart 表单
        // 提取 boundary
        let boundary = content_type.split("boundary=")
            .nth(1)
            .map(|b| b.trim().to_string())
            .unwrap_or_default();

        if boundary.is_empty() {
            return Ok(error_value("parseReqForm() multipart 表单缺少 boundary"));
        }

        let parts = parse_multipart(body, &boundary);
        for part in parts {
            if part.is_file {
                // 文件字段：构建 object
                let file_obj = crate::object_map::new_map();
                {
                    let mut obj = file_obj.lock().unwrap();
                    obj.set("fileName".to_string(), Value::str(&part.filename));
                    obj.set("size".to_string(), Value::Int(part.data.len() as i64));
                    obj.set("content".to_string(), Value::Bytes(Arc::new(part.data)));
                }
                map.set(part.name, Value::Object(file_obj));
            } else {
                // 普通字段
                let text = String::from_utf8_lossy(&part.data).into_owned();
                map.set(part.name, Value::str(&text));
            }
        }
    } else {
        // 尝试当作 URL 编码解析
        let body_str = String::from_utf8_lossy(body);
        if !body_str.is_empty() {
            let pairs = http_lite::url_decode_pairs(&body_str);
            for (k, v) in pairs {
                map.set(k, Value::str(&v));
            }
        }
    }

    Ok(Value::Map(Arc::new(Mutex::new(map))))
}

/// bi_save_file_upload 保存 multipart 表单中的文件字段到磁盘。
///
/// 用法：`saveFileUploads(req, destDir)`
/// 可选：`saveFileUploads(req, destDir, "--fieldName=avatar")` 只保存指定字段
/// 返回 Map：字段名 -> object {fileName, savedPath, size}
/// 非文件字段和空文件名会被跳过。destDir 不存在时自动创建。
fn bi_save_file_uploads(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let req = extract_req(&args[0])?;
    let dest_dir = match &args[1] {
        Value::Str(s) => s.to_string(),
        v => return Err(error_value(format!(
            "saveFileUploads() 第 2 个参数应为 string (目标目录)，得到 {}",
            v.type_name()
        ))),
    };
    let only_field = get_switch(args, "fieldName", "");

    let req_guard = req.inner.lock().unwrap();
    let content_type = req_guard.get_header("content-type").unwrap_or("").to_string();
    let body = req_guard.body.clone();
    drop(req_guard);

    if !content_type.starts_with("multipart/form-data") {
        return Ok(error_value("saveFileUploads() 要求 multipart/form-data 请求 (可能原因：未设置 enctype 或未使用 multipart 表单)"));
    }

    let boundary = content_type.split("boundary=")
        .nth(1)
        .map(|b| b.trim().to_string())
        .unwrap_or_default();
    if boundary.is_empty() {
        return Ok(error_value("saveFileUploads() multipart 表单缺少 boundary"));
    }

    let dest_path = std::path::Path::new(&dest_dir);
    if !dest_path.exists() {
        if let Err(e) = std::fs::create_dir_all(dest_path) {
            return Ok(error_value(format!("saveFileUploads() 创建目录失败: {} (目录: {})", e, dest_dir)));
        }
    }

    let parts = parse_multipart(&body, &boundary);
    let mut result = crate::ord_map::OrdMap::new();

    for part in parts {
        if !part.is_file || part.filename.is_empty() {
            continue;
        }
        if !only_field.is_empty() && part.name != only_field {
            continue;
        }

        let saved_path = dest_path.join(&part.filename);
        match std::fs::write(&saved_path, &part.data) {
            Ok(_) => {
                let file_obj = crate::object_map::new_map();
                {
                    let mut obj = file_obj.lock().unwrap();
                    obj.set("fileName".to_string(), Value::str(&part.filename));
                    obj.set("savedPath".to_string(), Value::str(saved_path.to_string_lossy().as_ref()));
                    obj.set("size".to_string(), Value::Int(part.data.len() as i64));
                }
                result.set(part.name, Value::Object(file_obj));
            }
            Err(e) => {
                return Ok(error_value(format!(
                    "saveFileUploads() 写入文件失败: {} (文件: {}, 可能原因：磁盘空间不足或权限不足)",
                    e, part.filename
                )));
            }
        }
    }

    Ok(Value::Map(Arc::new(Mutex::new(result))))
}

/// MultipartPart multipart 表单的一个部分。
struct MultipartPart {
    /// name 字段名。
    name: String,
    /// filename 文件名（仅文件字段有）。
    filename: String,
    /// is_file 是否为文件字段。
    is_file: bool,
    /// data 字段数据。
    data: Vec<u8>,
}

/// parse_multipart 解析 multipart/form-data 请求体。
///
/// 按 boundary 分割，解析每个部分的 headers 和数据。
fn parse_multipart(body: &[u8], boundary: &str) -> Vec<MultipartPart> {
    let mut parts = Vec::new();
    let delimiter = format!("--{}", boundary);

    // 按 boundary 分割
    let mut segments: Vec<&[u8]> = Vec::new();
    let mut start = 0;
    while start < body.len() {
        if let Some(pos) = find_subslice(&body[start..], delimiter.as_bytes()) {
            if start > 0 {
                segments.push(&body[start..start + pos]);
            }
            start = start + pos + delimiter.len();
        } else {
            break;
        }
    }

    // 跳过第一个空段和最后的结束标记
    for seg in segments {
        // 跳过结尾的 -- （结束标记）
        if seg.starts_with(b"--") {
            continue;
        }
        // 去掉开头的 \r\n
        let seg = if seg.starts_with(b"\r\n") { &seg[2..] } else { seg };
        // 去掉结尾的 \r\n
        let seg = if seg.ends_with(b"\r\n") { &seg[..seg.len() - 2] } else { seg };

        // 找到 headers 与 body 的分隔（空行 \r\n\r\n）
        if let Some(header_end) = find_subslice(seg, b"\r\n\r\n") {
            let header_bytes = &seg[..header_end];
            let data = &seg[header_end + 4..];

            // 解析 Content-Disposition
            let header_str = String::from_utf8_lossy(header_bytes);
            let mut name = String::new();
            let mut filename = String::new();

            for line in header_str.lines() {
                if line.to_lowercase().starts_with("content-disposition:") {
                    // 提取 name="..." 和 filename="..."
                    if let Some(n) = extract_quoted_value(line, "name") {
                        name = n;
                    }
                    if let Some(f) = extract_quoted_value(line, "filename") {
                        filename = f;
                    }
                }
            }

            if !name.is_empty() {
                parts.push(MultipartPart {
                    name,
                    filename: filename.clone(),
                    is_file: !filename.is_empty(),
                    data: data.to_vec(),
                });
            }
        }
    }

    parts
}

/// find_subslice 在 haystack 中查找 needle 的位置。
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len())
        .position(|window| window == needle)
}

/// extract_quoted_value 从字符串中提取 key="value" 中的 value。
fn extract_quoted_value(s: &str, key: &str) -> Option<String> {
    let pattern = format!("{}=\"", key);
    if let Some(start) = s.find(&pattern) {
        let value_start = start + pattern.len();
        if let Some(end) = s[value_start..].find('"') {
            return Some(s[value_start..value_start + end].to_string());
        }
    }
    None
}

// ===========================================================================
// HTTP 客户端内置函数
// ===========================================================================

/// parse_headers_from_args 从参数列表中提取 headers。
///
/// 支持两种格式：
/// - 单个字符串参数，每行一个 header（`"Content-Type: text/plain\nAccept: json"`）
/// - 多个字符串参数，每个为一个 header（`"Content-Type: text/plain", "Accept: json"`）
fn parse_headers_from_args(args: &[Value], start_idx: usize) -> Vec<String> {
    let mut headers = Vec::new();
    for i in start_idx..args.len() {
        if let Value::Str(s) = &args[i] {
            // 检查是否是 --timeout= 等开关
            if s.starts_with("--") || s.starts_with("-timeout=") || s.starts_with("-headers=") {
                continue;
            }
            // 多行 header 支持
            for line in s.lines() {
                let line = line.trim();
                if !line.is_empty() && line.contains(':') {
                    headers.push(line.to_string());
                }
            }
        }
    }
    headers
}

/// get_timeout_from_args 从参数列表中提取 --timeout= 值。
fn get_timeout_from_args(args: &[Value], start_idx: usize) -> u64 {
    for i in start_idx..args.len() {
        if let Value::Str(s) = &args[i] {
            if let Some(rest) = s.strip_prefix("--timeout=").or_else(|| s.strip_prefix("-timeout=")) {
                return rest.parse().unwrap_or(30);
            }
        }
    }
    30
}

/// bi_get_web 发送 HTTP GET 请求，返回响应体字符串。
///
/// 用法：
///   getWeb(url)                          -- 简单 GET
///   getWeb(url, "--timeout=30")          -- 带超时
///   getWeb(url, "Content-Type: json")    -- 带自定义 header
fn bi_get_web(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let url = match args.get(0) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(error_value(format!(
            "getWeb() 第 1 个参数应为 string (URL)，得到 {}", v.type_name()
        ))),
        None => return Err(error_value("getWeb() 需要至少 1 个参数 (URL)")),
    };

    let headers = parse_headers_from_args(args, 1);
    let timeout = get_timeout_from_args(args, 1);

    match http_lite::http_get(&url, &headers, timeout) {
        Ok(resp) => {
            let text = String::from_utf8_lossy(&resp.body).into_owned();
            Ok(Value::str(&text))
        }
        Err(e) => Ok(error_value(format!(
            "getWeb() 请求失败: {} (可能原因：URL 格式错误、网络不通、DNS 解析失败、服务器超时)",
            e
        ))),
    }
}

/// bi_get_web_bytes 发送 HTTP GET 请求，返回响应体字节。
///
/// 用法：`getWebBytes(url, "--timeout=30")`
fn bi_get_web_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let url = match args.get(0) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(error_value(format!(
            "getWebBytes() 第 1 个参数应为 string (URL)，得到 {}", v.type_name()
        ))),
        None => return Err(error_value("getWebBytes() 需要至少 1 个参数 (URL)")),
    };

    let headers = parse_headers_from_args(args, 1);
    let timeout = get_timeout_from_args(args, 1);

    match http_lite::http_get(&url, &headers, timeout) {
        Ok(resp) => Ok(Value::Bytes(Arc::new(resp.body))),
        Err(e) => Ok(error_value(format!(
            "getWebBytes() 请求失败: {} (可能原因：URL 格式错误、网络不通、服务器超时)",
            e
        ))),
    }
}

/// bi_post_web 发送 HTTP POST 请求，返回响应体字符串。
///
/// 用法：
///   postWeb(url, body, contentType)               -- 基本用法
///   postWeb(url, body, contentType, "--timeout=30") -- 带超时
///   postWeb(url, body, contentType, "Accept: json") -- 带额外 header
fn bi_post_web(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let url = match args.get(0) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(error_value(format!(
            "postWeb() 第 1 个参数应为 string (URL)，得到 {}", v.type_name()
        ))),
        None => return Err(error_value("postWeb() 需要至少 3 个参数 (URL, body, contentType)")),
    };

    let body = match args.get(1) {
        Some(Value::Str(s)) => s.as_bytes().to_vec(),
        Some(Value::Bytes(b)) => b.to_vec(),
        Some(Value::ByteArray(b)) => b.lock().unwrap().clone(),
        Some(v) => v.to_str().into_bytes(),
        None => return Err(error_value("postWeb() 需要至少 3 个参数 (URL, body, contentType)")),
    };

    let content_type = match args.get(2) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(error_value(format!(
            "postWeb() 第 3 个参数应为 string (Content-Type)，得到 {}", v.type_name()
        ))),
        None => return Err(error_value("postWeb() 需要至少 3 个参数 (URL, body, contentType)")),
    };

    let headers = parse_headers_from_args(args, 3);
    let timeout = get_timeout_from_args(args, 3);

    match http_lite::http_post(&url, &body, &content_type, &headers, timeout) {
        Ok(resp) => {
            let text = String::from_utf8_lossy(&resp.body).into_owned();
            Ok(Value::str(&text))
        }
        Err(e) => Ok(error_value(format!(
            "postWeb() 请求失败: {} (可能原因：URL 格式错误、网络不通、服务器拒绝、Content-Type 不匹配)",
            e
        ))),
    }
}

/// bi_download_file 下载文件到本地。
///
/// 用法：`downloadFile(url, savePath, "--timeout=60")`
/// 返回成功时为文件大小（int），失败为 Error。
fn bi_download_file(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let url = match args.get(0) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(error_value(format!(
            "downloadFile() 第 1 个参数应为 string (URL)，得到 {}", v.type_name()
        ))),
        None => return Err(error_value("downloadFile() 需要至少 2 个参数 (URL, savePath)")),
    };

    let save_path = match args.get(1) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(error_value(format!(
            "downloadFile() 第 2 个参数应为 string (保存路径)，得到 {}", v.type_name()
        ))),
        None => return Err(error_value("downloadFile() 需要至少 2 个参数 (URL, savePath)")),
    };

    let headers = parse_headers_from_args(args, 2);
    let timeout = get_timeout_from_args(args, 2);

    match http_lite::http_get(&url, &headers, timeout) {
        Ok(resp) => {
            if resp.status >= 400 {
                return Ok(error_value(format!(
                    "downloadFile() 服务器返回错误状态: {} (可能原因：文件不存在、权限不足)",
                    resp.status
                )));
            }
            match std::fs::write(&save_path, &resp.body) {
                Ok(_) => Ok(Value::Int(resp.body.len() as i64)),
                Err(e) => Ok(error_value(format!(
                    "downloadFile() 写入文件 '{}' 失败: {} (可能原因：目录不存在、权限不足)",
                    save_path, e
                ))),
            }
        }
        Err(e) => Ok(error_value(format!(
            "downloadFile() 下载失败: {} (可能原因：URL 格式错误、网络不通、服务器超时)",
            e
        ))),
    }
}

/// bi_url_exists 检查 URL 是否可访问。
///
/// 用法：`urlExists(url)` -> 返回 bool
fn bi_url_exists(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let url = match args.get(0) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(error_value(format!(
            "urlExists() 第 1 个参数应为 string (URL)，得到 {}", v.type_name()
        ))),
        None => return Err(error_value("urlExists() 需要 1 个参数 (URL)")),
    };

    match http_lite::http_get(&url, &[], 10) {
        Ok(resp) => Ok(Value::Bool(resp.status < 400)),
        Err(_) => Ok(Value::Bool(false)),
    }
}

/// extract_headers_from_map 从 Map/Object 值中提取 header 键值对。
///
/// 支持两种格式：
/// - Value::Map (OrdMap) — 有序映射
/// - Value::Object (object_map::Map) — 对象映射
/// 返回 Vec<String>，每个元素为 "Key: Value" 格式（与 http_lite::http_get 的 headers 参数格式一致）
fn extract_headers_from_map(v: &Value) -> Result<Vec<String>, Value> {
    let mut headers = Vec::new();
    match v {
        Value::Map(m) => {
            let m = m.lock().unwrap();
            for (k, val) in &m.entries {
                let val_str = match val {
                    Value::Str(s) => s.to_string(),
                    other => other.to_str(),
                };
                headers.push(format!("{}: {}", k, val_str));
            }
        }
        Value::Object(o) => {
            let o = o.lock().unwrap();
            for (k, val) in o.data.iter() {
                let val_str = match val {
                    Value::Str(s) => s.to_string(),
                    other => other.to_str(),
                };
                headers.push(format!("{}: {}", k, val_str));
            }
        }
        Value::Undefined => {
            // 无 headers，返回空列表
        }
        _ => return Err(error_value(format!(
            "headers 参数应为 map/object，得到 {} (可能原因：参数类型错误或顺序错误)",
            v.type_name()
        ))),
    }
    Ok(headers)
}

/// bi_get_web_bytes_with_headers 带 header 的 HTTP GET，返回响应体字节。
///
/// 用法：`getWebBytesWithHeaders(url, headers)`
/// headers 为 Map/Object{key: value}，如 {"Authorization": "Bearer xxx", "Accept": "application/json"}
/// 也支持可选的 --timeout=N 参数
fn bi_get_web_bytes_with_headers(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let url = match args.get(0) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(error_value(format!(
            "getWebBytesWithHeaders() 第 1 个参数应为 string (URL)，得到 {} (可能原因：参数顺序错误)",
            v.type_name()
        ))),
        None => return Err(error_value("getWebBytesWithHeaders() 需要至少 2 个参数 (url, headers)")),
    };

    let headers = match args.get(1) {
        Some(hv) => extract_headers_from_map(hv)?,
        None => Vec::new(),
    };

    let timeout = get_timeout_from_args(args, 2);

    match http_lite::http_get(&url, &headers, timeout) {
        Ok(resp) => Ok(Value::Bytes(Arc::new(resp.body))),
        Err(e) => Ok(error_value(format!(
            "getWebBytesWithHeaders() 请求失败: {} (可能原因：URL 格式错误、网络不通、DNS 解析失败、服务器超时、header 格式错误)",
            e
        ))),
    }
}

// ===========================================================================
// URL 解析与 MIME 类型
// ===========================================================================

/// bi_parse_url 解析 URL，返回 map。
///
/// 返回字段：scheme, host, port, path, query, fragment
/// 不含某部分则为空字符串。端口未显式给出时为 0。
fn bi_parse_url(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let url = match args.get(0) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(error_value(format!(
            "parseUrl() 第 1 个参数应为 string (URL)，得到 {}", v.type_name()
        ))),
        None => return Err(error_value("parseUrl() 需要 1 个参数 (URL)")),
    };

    let mut scheme = String::new();
    let mut host = String::new();
    let mut port: i64 = 0;
    let path: String;
    let mut query = String::new();
    let mut fragment = String::new();

    let rest = if let Some(pos) = url.find("://") {
        scheme = url[..pos].to_string();
        &url[pos + 3..]
    } else {
        &url[..]
    };

    // 分离 fragment
    let rest = if let Some(pos) = rest.find('#') {
        fragment = rest[pos + 1..].to_string();
        &rest[..pos]
    } else {
        rest
    };

    // 分离 query
    let rest = if let Some(pos) = rest.find('?') {
        query = rest[pos + 1..].to_string();
        &rest[..pos]
    } else {
        rest
    };

    // 分离 path：从第一个 / 开始
    let (authority, path_part) = if let Some(pos) = rest.find('/') {
        (&rest[..pos], &rest[pos..])
    } else {
        (rest, "")
    };
    path = path_part.to_string();

    // 从 authority 分离 host:port
    if authority.starts_with('[') {
        // IPv6: [::1]:port
        if let Some(end) = authority.find(']') {
            host = authority[1..end].to_string();
            if let Some(rest) = authority[end + 1..].strip_prefix(':') {
                port = rest.parse().unwrap_or(0);
            }
        }
    } else if let Some(pos) = authority.rfind(':') {
        host = authority[..pos].to_string();
        port = authority[pos + 1..].parse().unwrap_or(0);
    } else {
        host = authority.to_string();
    }

    let obj = crate::object_map::new_map();
    {
        let mut m = obj.lock().unwrap();
        m.set("scheme".to_string(), Value::str_from(scheme));
        m.set("host".to_string(), Value::str_from(host));
        m.set("port".to_string(), Value::Int(port));
        m.set("path".to_string(), Value::str_from(path));
        m.set("query".to_string(), Value::str_from(query));
        m.set("fragment".to_string(), Value::str_from(fragment));
    }
    Ok(Value::Object(obj))
}

/// bi_get_mime_type 根据文件扩展名获取 MIME 类型。
///
/// 用法：getMimeType(filename) 或 getMimeType(ext)
/// 未知的扩展名返回 "application/octet-stream"。
fn bi_get_mime_type(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let filename = match args.get(0) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(error_value(format!(
            "getMimeType() 第 1 个参数应为 string (文件名)，得到 {}", v.type_name()
        ))),
        None => return Err(error_value("getMimeType() 需要 1 个参数 (文件名)")),
    };

    // 提取扩展名（最后一个 . 之后，转小写）
    let ext = filename.rsplit('.').next().unwrap_or("").to_lowercase();
    let mime = mime_type_for_ext(&ext);
    Ok(Value::str_from(mime.to_string()))
}

/// mime_type_for_ext 根据扩展名返回 MIME 类型字符串。
fn mime_type_for_ext(ext: &str) -> &'static str {
    match ext {
        // 文本
        "txt" | "text" => "text/plain",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "csv" => "text/csv",
        "md" => "text/markdown",
        // 代码
        "js" | "mjs" => "text/javascript",
        "json" => "application/json",
        "xml" => "application/xml",
        "yaml" | "yml" => "application/yaml",
        // 图片
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "bmp" => "image/bmp",
        "tiff" | "tif" => "image/tiff",
        // 音频
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        "m4a" => "audio/mp4",
        // 视频
        "mp4" => "video/mp4",
        "avi" => "video/x-msvideo",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        // 文档
        "pdf" => "application/pdf",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        // 压缩
        "zip" => "application/zip",
        "gz" | "gzip" => "application/gzip",
        "tar" => "application/x-tar",
        "rar" => "application/vnd.rar",
        "7z" => "application/x-7z-compressed",
        // 字体
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        // 其他
        "bin" | "exe" | "dll" | "so" | "dylib" => "application/octet-stream",
        "wasm" => "application/wasm",
        "sf" => "text/plain",
        _ => "application/octet-stream",
    }
}

// ===========================================================================
// Query 解析与 URL 路径拼接
// ===========================================================================

/// bi_parse_query 解析 URL query string 为 Map。
///
/// 用法：`parseQuery("a=1&b=2&c=hello%20world")`
/// 返回 OrdMap：{a:"1", b:"2", c:"hello world"}
/// 支持 + 和 %XX 百分号编码解码
fn bi_parse_query(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let query = match args.get(0) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(error_value(format!(
            "parseQuery() 第 1 个参数应为 string (query string)，得到 {} (可能原因：参数顺序错误)",
            v.type_name()
        ))),
        None => return Err(error_value("parseQuery() 需要 1 个参数 (query string)")),
    };

    let pairs = http_lite::url_decode_pairs(&query);
    let mut map = crate::ord_map::OrdMap::new();
    for (k, v) in pairs {
        map.set(k, Value::str(&v));
    }
    Ok(Value::Map(Arc::new(Mutex::new(map))))
}

/// bi_join_url_path 拼接 URL 路径，避免重复斜杠。
///
/// 用法：`joinUrlPath("http://example.com", "/api/v1")` → "http://example.com/api/v1"
/// `joinUrlPath("http://example.com/", "api/v1")` → "http://example.com/api/v1"
/// `joinUrlPath("http://example.com/", "/api/v1")` → "http://example.com/api/v1"
/// `joinUrlPath("base/", "sub/")` → "base/sub/"
fn bi_join_url_path(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let base = match args.get(0) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(error_value(format!(
            "joinUrlPath() 第 1 个参数应为 string (base URL)，得到 {}", v.type_name()
        ))),
        None => return Err(error_value("joinUrlPath() 需要 2 个参数 (base, path)")),
    };
    let path = match args.get(1) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(error_value(format!(
            "joinUrlPath() 第 2 个参数应为 string (path)，得到 {}", v.type_name()
        ))),
        None => return Err(error_value("joinUrlPath() 需要 2 个参数 (base, path)")),
    };

    let result = join_url_path_impl(&base, &path);
    Ok(Value::str(&result))
}

/// join_url_path_impl 拼接 URL 路径的内部实现。
///
/// 规则：
/// - base 末尾的 / 和 path 开头的 / 合并为单个 /
/// - 如果 path 为空，返回 base
/// - 如果 base 为空，返回 path
fn join_url_path_impl(base: &str, path: &str) -> String {
    if path.is_empty() {
        return base.to_string();
    }
    if base.is_empty() {
        return path.to_string();
    }
    let base_has_slash = base.ends_with('/');
    let path_has_slash = path.starts_with('/');
    if base_has_slash && path_has_slash {
        // 两边都有 /，去掉 path 开头的 /
        format!("{}{}", base, &path[1..])
    } else if !base_has_slash && !path_has_slash {
        // 两边都没有 /，补一个
        format!("{}/{}", base, path)
    } else {
        // 一边有一边没有，直接拼接
        format!("{}{}", base, path)
    }
}

// ===========================================================================
// Multipart 构建
// ===========================================================================

/// MULTIPART_BOUNDARY multipart 表单的固定 boundary 分隔符。
///
/// 用于 prepareMultiPartFieldFromBytes 构建的字段，
/// 调用方在设置 Content-Type 时应使用相同的 boundary：
/// "multipart/form-data; boundary=----SflangBoundary7MA4YWxkTrZu0gW"
const MULTIPART_BOUNDARY: &str = "----SflangBoundary7MA4YWxkTrZu0gW";

/// bi_prepare_multipart_field_from_bytes 构建 multipart/form-data 的一个字段字节。
///
/// 用法：`prepareMultiPartFieldFromBytes(fieldName, fileName, bytes, contentType)`
/// 返回 Bytes，格式为：
/// ```text
/// --boundary\r\n
/// Content-Disposition: form-data; name="fieldName"; filename="fileName"\r\n
/// Content-Type: contentType\r\n
/// \r\n
/// <bytes>\r\n
/// ```
/// 调用方可将多个字段的 Bytes 拼接后，加上结尾的 `--boundary--\r\n`，
/// 并设置 Content-Type 为 `multipart/form-data; boundary=----SflangBoundary7MA4YWxkTrZu0gW`
fn bi_prepare_multipart_field_from_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let field_name = match args.get(0) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(error_value(format!(
            "prepareMultiPartFieldFromBytes() 第 1 个参数应为 string (字段名)，得到 {} (可能原因：参数顺序错误)",
            v.type_name()
        ))),
        None => return Err(error_value("prepareMultiPartFieldFromBytes() 需要 4 个参数 (fieldName, fileName, bytes, contentType)")),
    };

    let file_name = match args.get(1) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(error_value(format!(
            "prepareMultiPartFieldFromBytes() 第 2 个参数应为 string (文件名)，得到 {}", v.type_name()
        ))),
        None => return Err(error_value("prepareMultiPartFieldFromBytes() 需要 4 个参数 (fieldName, fileName, bytes, contentType)")),
    };

    let data = match args.get(2) {
        Some(Value::Bytes(b)) => b.to_vec(),
        Some(Value::ByteArray(b)) => b.lock().unwrap().clone(),
        Some(Value::Str(s)) => s.as_bytes().to_vec(),
        Some(v) => return Err(error_value(format!(
            "prepareMultiPartFieldFromBytes() 第 3 个参数应为 bytes/byteArray/string，得到 {} (可能原因：参数类型错误)",
            v.type_name()
        ))),
        None => return Err(error_value("prepareMultiPartFieldFromBytes() 需要 4 个参数 (fieldName, fileName, bytes, contentType)")),
    };

    let content_type = match args.get(3) {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => return Err(error_value(format!(
            "prepareMultiPartFieldFromBytes() 第 4 个参数应为 string (Content-Type)，得到 {}", v.type_name()
        ))),
        None => return Err(error_value("prepareMultiPartFieldFromBytes() 需要 4 个参数 (fieldName, fileName, bytes, contentType)")),
    };

    // 构建 multipart 字段字节
    let mut result = Vec::new();
    result.extend_from_slice(format!("--{}\r\n", MULTIPART_BOUNDARY).as_bytes());
    result.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\n", field_name, file_name).as_bytes()
    );
    result.extend_from_slice(format!("Content-Type: {}\r\n", content_type).as_bytes());
    result.extend_from_slice(b"\r\n");
    result.extend_from_slice(&data);
    result.extend_from_slice(b"\r\n");

    Ok(Value::Bytes(Arc::new(result)))
}

// ===========================================================================
// 响应刷新
// ===========================================================================

/// bi_flush_resp 刷新 HTTP 响应缓冲区。
///
/// 用法：`flushResp(resp)`
///
/// 当前架构下，writeResp/writeRespBytes 直接将数据写入 HttpResponse.body（内存 Vec<u8>），
/// 实际的网络写入和 flush 发生在请求处理结束后的 write_response 中。
/// 因此 flushResp 为空操作（no-op），返回 undefined。
/// 保留此函数是为了 API 兼容性，便于将来如果改为流式写入时可以使用。
fn bi_flush_resp(_vm: &mut VM, _args: &[Value]) -> Result<Value, Value> {
    Ok(Value::Undefined)
}
