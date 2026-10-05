# Sflang 服务器模式示例

## 示例文件

| 文件 | 说明 | 启动方式 |
|------|------|----------|
| `basic.sf` | HTTP 服务器基础（8 种响应风格） | `sf examples/server/basic.sf` |
| `json_api.sf` | JSON REST API 服务（CRUD + 并发安全） | `sf examples/server/json_api.sf` |
| `concurrent.sf` | 并发计数器（mutex 保护共享状态） | `sf examples/server/concurrent.sf` |
| `http_client.sf` | HTTP 客户端（getWeb/postWeb/downloadFile） | `sf examples/server/http_client.sf` |
| `websocket_client.sf` | WebSocket 客户端（连接/收发/关闭） | `sf examples/server/websocket_client.sf` |
| `scripts/pages/index.sf` | CLI 服务器脚本（动态页面） | `sf -server --port=8080 --msDir=examples/server/scripts` |
| `scripts/pages/api.sf` | CLI 服务器脚本（JSON API） | 同上，访问 `/api.sf` |
| `scripts/pages/form.sf` | CLI 服务器脚本（表单处理） | 同上，访问 `/form.sf` |
| `scripts/pages/demo.sfp` | CLI 服务器脚本（.sfp 动态页面模板） | 同上，访问 `/demo.sfp` |

## 快速开始

### 1. 脚本级 HTTP 服务器

```bash
sf examples/server/basic.sf
# 然后访问 http://127.0.0.1:8080/hello
```

### 2. CLI 应用服务器

```bash
sf -server --port=8080 --dir=examples/server/scripts --verbose
# 然后访问 http://127.0.0.1:8080/index.sf
```

### 3. HTTP 客户端

```bash
sf examples/server/http_client.sf
```

## 响应规则

handler 返回值的类型决定服务器行为：

| 返回类型 | 行为 |
|----------|------|
| `Str` | 作为响应体输出（自动 200） |
| `Bytes` / `ByteArray` | 作为二进制响应体输出 |
| `Error` | 服务器返回 500 + 结构化 JSON 错误 |
| 其他（`undefined`/`int`/`bool`/...） | 不输出（脚本应已通过 `writeResp` 自行写响应） |

## 关键内置函数

### 服务器管理
- `httpServer("--port=8080")` - 创建服务器
- `serverSetHandler(server, path, handler)` - 注册路由
- `serverSetStatic(server, dirPath)` - 设置静态文件目录
- `serverStart(server, "--thread")` - 启动（`--thread` 后台运行）

### 请求
- `getReqMethod(req)` / `getReqPath(req)` / `getReqUri(req)` / `getReqQuery(req)`
- `getReqHeader(req, key)` / `getReqHeaders(req)`
- `getReqBody(req)` / `getReqBodyBytes(req)`
- `getReqParam(req, key)` / `getReqParams(req)`
- `parseReqForm(req)` - 解析表单（urlencoded + multipart）

### 响应
- `writeResp(resp, content)` / `writeRespBytes(resp, bytes)`
- `setRespStatus(resp, code)` / `writeRespHeader(resp, code)`
- `setRespHeader(resp, key, value)` / `setRespContentType(resp, type)`
- `serveFile(resp, path)` / `redirectResp(resp, url, code)`

### HTTP 客户端
- `getWeb(url, ...)` - GET 请求，返回字符串
- `getWebBytes(url, ...)` - GET 请求，返回 Bytes
- `postWeb(url, body, contentType, ...)` - POST 请求
- `downloadFile(url, savePath, ...)` - 下载文件
- `urlExists(url)` - 检查 URL 是否可访问

HTTPS 客户端底层由 ureq 实现（纯 Rust 同步 HTTP 客户端）：
- TLS 证书由系统证书库校验（Windows SChannel / Linux ca-certificates / macOS Keychain），跨平台行为一致
- Agent 连接池按超时秒数缓存复用，重复请求同一主机性能更优
- 自动跟随重定向（最多 10 次）
- 错误信息包含可能原因（DNS 失败、网络不通、TLS 证书验证失败等），便于 AI 定位

### WebSocket
- `webSocket("ws://host:port/path")` - 客户端连接
- `wsReadText(ws)` / `wsReadBin(ws)` / `wsReadMsg(ws)`
- `wsWriteText(ws, text)` / `wsWriteBin(ws, bytes)` / `wsWriteMsg(ws, type, data)`
- `wsClose(ws)`

## .sfp 动态页面

`.sfp` 文件是 HTML 模板，内嵌 `<?sf ... ?>` 代码块（类似 PHP 的 `<?php ?>`）：

```html
<html><body>
<h1>当前时间: <?sf return toStr(now()) ?></h1>
<ul>
<?sf
result := ""
for i in range(1, 4) {
    result = result + "<li>第 " + toStr(i) + " 项</li>"
}
return result
?>
</ul>
</body></html>
```

- 代码块外的文本原样输出
- 代码块执行后返回值插入到 HTML 中
- 多个代码块共享同一个执行环境（变量互通）
- 单个代码块出错时内联显示错误，不中断页面渲染
- `runModeG` 设为 `"sfp"`

## .sfAllow 文件机制

在文件所在目录放置 `.sfAllow` 文件，允许服务非白名单扩展名的文件：

```text
# .sfAllow 文件格式（每行一个 glob 模式，# 开头为注释）
*.csv
data-?.bin
secret.dat
```

- 匹配的文件以 `Content-Disposition: attachment` 强制下载方式服务
- 不匹配的返回 404
- 仅检查文件所在目录（不递归向上查找）

## CLI 应用服务器（sf -server）

把 URL 路径映射到 .sf 脚本文件，每请求一个 VM，类似 PHP/Charlang 的部署模型。

```bash
sf -server --port=8080 --msDir=<脚本根目录> --webDir=<静态根目录>
```

| 参数 | 说明 |
|------|------|
| `--port` | HTTP 监听端口（默认 80） |
| `--sslPort` | HTTPS 监听端口（默认 443；配了 --certDir 才启用） |
| `--host` | 监听地址（默认 0.0.0.0） |
| `--msDir` | **脚本根目录**（唯一脚本来源）：URL 镜像文件路径，子目录名任意、层级任意，页面脚本与接口脚本可同目录混放，服务端不做任何分类 |
| `--webDir` | 静态文件根目录（白名单扩展名；目录自动回落 index.html） |
| `--certDir` | TLS 证书目录（server.crt + server.key，纯 Rust rustls 实现） |
| `--verbose` | 打印请求日志 |
| `--adminToken` | /admin/status、/admin/kill 管理端点令牌（仅限本机访问） |
| `--dir` | **已废弃**：仅为兼容旧命令保留，等价于 --msDir |

路由规则（对齐 Charlang 的最大灵活度）：

```
URL /foo/bar                → <msDir>/foo/bar
  目录                       → index.sf → index.sfp →（web 目录 index.html）
  .sf / .sfp                → 执行 / 渲染
  无扩展名                   → 追加 .sf、.sfp 再试（/api/products → api/products.sf）
脚本树内非脚本文件            → 一律私有（不服务、不放行）
静态文件                     → 全部来自 --webDir（白名单 + 目录回落 index.html + .sfAllow）
无匹配                       → 404
```

目录布局示例（组织方式完全由你定，服务端只认"路径即 URL"）：

```
msdir/
  index.sf            # / → index.sf
  product.sf          # /product.sf
  api/                # 分组纯为整洁：/api/products → api/products.sf
    products.sf
    admin/
      auth.sf         # /api/admin/auth
  lib/                # 共享库（import 相对脚本自身目录解析）
  data/               # 数据文件放树内任意处都安全（脚本树非脚本文件外部拿不到）
```

要点：

- **脚本树内只有 `.sf`/`.sfp` 会被响应**，其他文件（数据、配置、笔记）外部 URL 一律
  拿不到——数据文件可安全放在树内任意位置。
- 静态文件（css/js/图片/下载包）统一放 `--webDir`。
- import 相对路径基于脚本自身目录解析（如 api/admin/x.sf 里 `import "../../lib/security.sf"`）。

### 脚本可用的请求上下文全局变量

| 变量 | 说明 |
|------|------|
| `requestG` / `responseG` | 请求/响应对象（配合 getReqHeader、writeResp、setRespHeader 等） |
| `paraMapG` | URL 查询参数 Map（键值已做百分号解码），`paraMapG["id"]` 取值 |
| `reqMethodG` / `reqPathG` / `reqUriG` | 请求方法 / 路径 / 完整 URI |
| `inputG` | 请求体文本（等价 getReqBody(requestG)） |
| `basePathG` / `msRootG` | 脚本根目录（--msDir） |
| `scriptPathG` | 当前脚本完整路径 |
| `webRootG` | 静态 Web 根目录（--webDir） |
| `runModeG` | 运行模式（"sfserver" / "sfp"） |

### 脚本响应规则

- `return "字符串"` → 作为响应体输出；未显式设置 Content-Type 时默认 `text/html; charset=utf-8`
- `return bytes` → 二进制响应（默认 `application/octet-stream`）
- `return undefined`（或非字符串值）→ 不追加输出，适用于已用 `writeResp` 自行写响应的场景
- 脚本内异常 → 500 + AI 友好 JSON（含 possibleCauses）
- import 相对路径基于脚本自身目录解析
