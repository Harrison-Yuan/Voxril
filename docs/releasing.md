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

1. `macos-14`（Apple Silicon）上装 Node / pnpm / Rust（`aarch64-apple-darwin`）
2. 构建前端（`pnpm -C web run build`）
3. 若配置了证书，导入临时钥匙串并签名
4. 用官方 `tauri-apps/tauri-action` 构建 DMG、创建 Release、上传产物
5. 生成 `SHA256SUMS.txt` 并追加到同一个 Release

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
| `KEYCHAIN_PASSWORD` | 任意随机字符串，仅用于 CI 临时钥匙串 |
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
- [ ] 首次启动能看到 Python 环境与模型下载进度，不闪退
- [ ] 选择一个工作区后重启，路径仍然保留
- [ ] 模型管理里依赖全部「已就绪」
- [ ] 能完成一次「分离 → 转换 → 合成」，`mixed/` 下出现文件

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
