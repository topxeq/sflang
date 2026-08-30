# sf (Sflang) 发布流程规范

> 适用范围：向 **GitHub Release** 与 **仙缘渡（magicdo.top）** 发布 sf 各平台版本。
> 参考：xxssh 项目的同名流程（`D:\aiprjs\xxssh\publish-*.ps1`、`repo/.github/workflows/release.yml`）。

---

## 0. 概览：两个发布目的地

| 目的地 | 产物 | 触发方式 |
|---|---|---|
| GitHub Release（github.com/topxeq/sflang） | 四平台二进制（公开下载） | 推送 `v*` 标签，全自动 |
| 仙缘渡 magicdo.top 产品页 | 二进制 + 图标 + 文档 + 安装入口 | 本地运行 `publish-*.ps1`，半自动 |

仙缘渡发布脚本不从本地取 Linux/macOS 二进制时，可以直接取 GitHub Release 的产物（`gh release download vX.Y.Z`）。

---

## 1. 发布新版本的完整步骤

以发布 `vX.Y.Z` 为例（当前版本见根目录 `Cargo.toml` 的 `[workspace.package]` version）：

### 1.1 准备（本地）

1. **改版本号**：`Cargo.toml` 中 `version = "X.Y.Z"`（workspace 级，一处改全生效）。
2. **本地构建验证**（提交前自测）：
   ```bash
   cargo build --release          # Windows x64（含 GUI feature）
   cargo test --release           # 全套测试必须绿（CI 不跑测试，见 §3 注意事项）
   ```
3. **提交并推送** main 分支。

### 1.2 GitHub 构建（全自动）

```bash
git tag vX.Y.Z
git push origin vX.Y.Z
```

- 工作流 `.github/workflows/release.yml` 触发，5 个 job 并行，产物自动挂到 Release：

| Job | Runner | 特性 | 产物 |
|---|---|---|---|
| windows | windows-latest | 默认（含 GUI） | `sf.exe` |
| linux | ubuntu + musl-tools | `--no-default-features`，musl 全静态，strip | `sf` |
| linux-arm64 | ubuntu-24.04-arm（原生 ARM 档） | 同上 | `sf-arm64` |
| macos | macos-latest（x64+arm64 双编译，lipo 合并） | `--no-default-features` | `sf-universal` |

- 工作流自带两道防线：tag 版本必须与 Cargo.toml 一致；每个平台产物跑 `--version` 冒烟。
- 首次构建约 15~20 分钟；`rust-cache` 生效后更快。

### 1.3 发布到仙缘渡（本地半自动）

```powershell
cd D:\aiprjs\sflang\repo
.\publish-0.1.0.ps1 -DryRun    # 先检查：配置/图标/各平台二进制存在性、大小、sha256
.\publish-0.1.0.ps1            # 正式发布
```

脚本依次完成：登录（token 24h）→ 产品元数据（存在则 PUT，不存在则 POST）→ 上传图标（≤500KB）并 setIcon → 上传文档（按 slug 增/改）→ 逐平台 `addVersion` + 上传二进制 + 回写 sha256/isLatest → 推首页 featured → `cleanup-latest.py` 清旧版本 → 公开 API 验证。

> **注意**：发新版本前把脚本里的 `$VERSION` 和文件名里的版本号更新（如 `publish-0.1.1.ps1`），二进制路径指向本地实际文件（默认 `repo\target\release\sf.exe` 与 `result\sf-*`）。

### 1.4 验证清单

```bash
# 产品页与版本
curl -s https://magicdo.top/api/products?id=sf | grep isLatest
# 四个下载端点（文件名来自各平台的 filename 字段，必须带扩展名）
curl -sI https://magicdo.top/downloads/sf/sf.exe
curl -sI https://magicdo.top/downloads/sf/sf-linux-amd64.bin
curl -sI https://magicdo.top/downloads/sf/sf-linux-arm64.bin
curl -sI https://magicdo.top/downloads/sf/sf-macos-universal.bin
# 一键安装端点
curl -fsSL https://magicdo.top/install/sf.sh | head -5
```

真实验证（推荐每次发版做一次）：在 wl 服务器执行 `curl -fsSL https://magicdo.top/install/sf.sh | bash` 后跑 `sf -v` 与一段脚本。

---

## 2. 文件清单（本目录）

| 文件 | 用途 |
|---|---|
| `sf.conf.json` | 产品配置：名称/简介(markdown)/标签/平台清单/一键安装脚本/文档。发布脚本的唯一内容来源 |
| `sf-icon.png` | 产品图标（≤500KB，4.2KB） |
| `publish-0.1.0.ps1` | 仙缘渡发布脚本（支持 `-DryRun`）。**含 UTF-8 BOM，勿用无 BOM 编辑器重存**（见 §4） |
| `cleanup-latest.py` | 清理仙缘渡旧版本（先 unset isLatest 再 DELETE，每平台只留当前版） |
| `.github/workflows/release.yml` | GitHub 全平台构建工作流 |
| `result/sf-*`（仓库外，`D:\aiprjs\sflang\result`） | 本地构建产物副本，发布脚本从这里取 Linux/macOS 二进制 |

## 3. 注意事项与已踩过的坑

1. **PS1 必须带 UTF-8 BOM**：PowerShell 5.1 对无 BOM 文件按 ANSI 解析，中文注释的 UTF-8 字节会吞掉字符串引号导致语法错误。保存时务必保持 BOM。
2. **CI 不跑 `cargo test`**：linux job 的测试在 CI 环境会挂起（部分用例依赖本机环境，如剪贴板/GTK/交互输入）。测试职责在开发机；CI 专职出二进制。如要恢复，先定位并隔离环境依赖用例。
3. **非 Windows 平台一律 `--no-default-features`**（关 GUI）：GUI 依赖 webkit2gtk 等系统库，仅 Windows 桌面目标保留。
4. **仙缘渡上传要求文件名带扩展名**：macOS/Linux 产物命名统一 `.bin` 后缀（`sf-macos-universal.bin`、`sf-linux-arm64.bin`）。
5. **产品已存在时必须 PUT**：POST 会报"产品 ID 已存在"。发布脚本已自动处理（先查再定 POST/PUT）。
6. **installScripts 与 versions 必须一致**：不要配置没有对应二进制的安装入口（曾出现 macOS 安装按钮无下载支撑，被真实用户环境暴露）。加新平台的正确顺序：先上传 version，再加安装入口。
7. **凭据**：管理员账号密码内置于发布脚本与 cleanup-latest.py（私有工作区约定）；仓库为公开仓库，**这些文件不应包含真实凭据对外**——若仓库转公开，先把凭据改为环境变量读取。

## 4. 新增平台的步骤（示例：未来加 FreeBSD/iOS）

1. `release.yml` 增加对应 job（交叉编译优先用 `cargo-zigbuild`；macOS 之外的原生 runner 优先）。
2. 本地取回产物，验证 `file` 格式与 `--version`。
3. 仙缘渡 `addVersion`（platform 名即产品页标识，filename 带 `.bin`）+ 上传 + `updateVersion`。
4. `sf.conf.json` 补 platforms 与 installScripts 条目，PUT 更新。
5. 同步 `publish-*.ps1` 的 `$PLATFORMS` 映射，DryRun 验证。

## 5. 本地构建（CI 之外的备用途径）

```bash
# Windows（含 GUI）
cargo build --release
# Linux x64 / ARM64（musl 静态，zig 交叉，需 cargo install cargo-zigbuild）
cargo zigbuild --release --target x86_64-unknown-linux-musl --no-default-features
# macOS：本机无法链接，走 CI 或在真机构建（参照 xxssh 的 mac-build 流程）
```

产物副本约定：`repo/target/release/sf.exe` 与 `D:\aiprjs\sflang\result\sf-*`（发布脚本默认读取路径）。
