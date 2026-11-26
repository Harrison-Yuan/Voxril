# 发版与桌面打包

面向维护者。目标：**打一个 tag，GitHub 自动产出可直接下载安装的 DMG。**

## 触发与产物

| 方式 | 操作 |
|---|---|
| 打 tag（推荐） | `git tag v0.2.1 && git push origin v0.2.1`，同时推 GitHub 镜像 |
| 重打旧 tag | Actions → **Release** → Run workflow → 填 tag（如 `v0.2.0`） |

产物：

- `Voxril_<version>_aarch64.dmg` —— 用户下载即用
- `SHA256SUMS.txt` —— 校验文件

版本号取自 tag（`v0.2.1` → `0.2.1`），不需要改 `tauri.conf.json` / `package.json`，CI 用 `--config` 覆盖。
本地的 `pnpm build` 仍然会走 `bump_version` + 依赖校验，两套流程互不影响。

## 流程做了什么

`.github/workflows/release.yml`：

1. `macos-latest` runner 上装 Node / pnpm / Rust（`aarch64-apple-darwin`）
2. 构建前端（`pnpm -C web run build`）
3. `tauri build --bundles app` 只产出 `.app`（有证书时由 Tauri 自动完成签名）
4. 用 `hdiutil` 手写 DMG —— Tauri 自带的 DMG 脚本依赖 AppleScript 操作 Finder，在无图形会话的 CI 上不可靠
5. 有证书时对 DMG 追加 `notarytool` 公证与 `stapler`
6. 生成 `SHA256SUMS.txt`，与 DMG 一起上传到 Release

> 证书相关变量用 `SIGN_*` 中转，只有配置了 `APPLE_CERTIFICATE` 才会导出成 `APPLE_*` 交给 Tauri。未配置时它们是空字符串，直接透传会让 Tauri 误判为「要签名」并在导入空证书时失败。

## 签名与公证（可选，但强烈建议）

不配置也能发版，只是用户首次打开需要执行：

```bash
xattr -dr com.apple.quarantine /Applications/Voxril.app
```

配置以下 Secrets 后，产物为 **已签名 + 已公证**，用户双击即可打开，也不再出现「已损坏」。

| Secret | 内容 |
|---|---|
| `APPLE_CERTIFICATE` | `Developer ID Application` 证书 `.p12` 的 base64 |
| `APPLE_CERTIFICATE_PASSWORD` | 导出 `.p12` 时设置的密码 |
| `APPLE_SIGNING_IDENTITY` | 例如 `Developer ID Application: Your Name (ABCDE12345)` |
| `APPLE_ID` | Apple 开发者账号邮箱 |
| `APPLE_PASSWORD` | <https://appleid.apple.com> 生成的 **App 专用密码** |
| `APPLE_TEAM_ID` | 10 位 Team ID（开发者后台 Membership 里可查） |

### 准备证书

1. 加入 Apple Developer Program（个人/公司均可）
2. 在开发者后台创建 **Developer ID Application** 证书，下载并安装到钥匙串
3. 钥匙串中「我的证书」→ 右键导出为 `.p12`，设置密码
4. 转 base64：`base64 -i cert.p12 | pbcopy`，粘到 `APPLE_CERTIFICATE`

### 验证签名（本地）

```bash
codesign -dv --verbose=4 /Applications/Voxril.app
spctl -a -vv /Applications/Voxril.app     # 应输出 accepted / Notarized Developer ID
```

## 冷启动体验检查清单

发版前在一台**没装过 Voxril** 的 Mac 上确认：

- [ ] 双击 DMG → 拖入 Applications → 直接打开（已签名时不应有安全提示）
- [ ] 首次启动能看到内置运行时初始化、依赖安装与模型下载进度，不闪退（无需系统 Python）
- [ ] 选择一个工作区后重启，路径仍然保留
- [ ] 模型管理里依赖全部「已就绪」
- [ ] 有一个可用音色时，「一键翻唱」选中歌曲后能跑完三阶段，成品出现在作品列表并能播放

### 签名后的额外检查

内置 Python 运行时位于 `Voxril.app/Contents/Resources/bin/python/`，其中包含 `python3` 可执行文件和大量 `.so` / `.dylib`。启用 Apple 签名后：

- 确认这些 Mach-O 文件已随 App 一起签名，否则公证会失败
- 如 Tauri 未自动签名资源目录，在打包后对整个 App 执行一次 `codesign --deep --force --sign "$APPLE_SIGNING_IDENTITY" Voxril.app`，再提交公证
- 公证实测通过后，把这步写进 `release.yml` 的签名分支

运行时由 `tools/fetch_python_runtime.sh` 在构建前拉取（不入库），版本在脚本顶部的 `PB_TAG` / `PY_VERSION` 固定；升级时同步修改并回归冷启动。

## 常见失败

| 现象 | 原因 |
|---|---|
| `security import` 失败 | `.p12` 不是 base64、或证书密码错 |
| `notarytool` 401 | `APPLE_PASSWORD` 用了账号密码，需要 App 专用密码 |
| 构建报签名相关错误 | 证书已过期，或被 `CSC_IDENTITY_AUTO_DISCOVERY` 干扰（未配置证书时应为 `false`） |
| Release 没有附件 | `permissions: contents: write` 缺失，或 tag 与 `tagName` 不一致 |
| 产物体积异常 | 忘了 `--bundles dmg`，或 `target/` 缓存污染（清 `swatinem/rust-cache`） |

## 后续可选项

- **自动更新**：需要接入 `@tauri-apps/plugin-updater` + 签名密钥（`TAURI_SIGNING_PRIVATE_KEY`），并在 Release 提供 `latest.json`
- **Windows**：`release-windows.yml` 已预留，待运行时适配完成后启用
- **通用二进制**：Intel 版本已放弃，暂无计划
