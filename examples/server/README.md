# Sflang 服务器模式示例

## 示例文件

| 文件 | 说明 | 启动方式 |
|------|------|----------|
| `basic.sf` | HTTP 服务器基础（8 种响应风格） | `sf examples/server/basic.sf` |
| `json_api.sf` | JSON REST API 服务（CRUD + 并发安全） | `sf examples/server/json_api.sf` |
| `concurrent.sf` | 并发计数器（mutex 保护共享状态） | `sf examples/server/concurrent.sf` |
| `http_client.sf` | HTTP 客户端（getWeb/postWeb/downloadFile） | `sf examples/server/http_client.sf` |
| `websocket_client.sf` | WebSocket 客户端（连接/收发/关闭） | `sf examples/server/websocket_client.sf` |
| `scripts/index.sf` | CLI 服务器脚本（动态页面） | `sf -server --port=8080 --dir=examples/server/scripts` |
| `scripts/api.sf` | CLI 服务器脚本（JSON API） | 同上，访问 `/api.sf` |
| `scripts/form.sf` | CLI 服务器脚本（表单处理） | 同上，访问 `/form.sf` |
| `scripts/demo.sfp` | CLI 服务器脚本（.sfp 动态页面模板） | 同上，访问 `/demo.sfp` |

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
