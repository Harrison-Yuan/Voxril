# 参与贡献 Voxril

感谢你愿意花时间改进 Voxril。这份文档说明「怎么改」「怎么验证」「怎么提交」，以及什么样的 PR 最容易被合并。

## 我们最想要什么样的贡献

| 优先级 | 类型 | 例子 |
|---|---|---|
| 🟢 非常欢迎 | 明确的 Bug 修复 | 崩溃、爆音、路径错误、UI 状态错乱 |
| 🟢 非常欢迎 | 交互与体验改进 | 减少点击、文案更清楚、错误提示更可操作 |
| 🟢 非常欢迎 | 文档与教程 | README、截图、新手排错、英文翻译 |
| 🟢 非常欢迎 | 平台适配 | Windows 运行时适配（构建配置已预留） |
| 🟡 需要先讨论 | 模型/算法/音质改动 | 换模型、改推理参数默认值、改训练超参 |
| 🔴 不接受 | 与项目定位无关的改动 | 大规模重构、换框架、加商业化 SDK |

算法类改动请**先开 Issue 说明**动机、预期收益和验证方式，达成一致再写代码 —— 这类改动会直接影响音质与性能，无法只凭「感觉更好」合并。

## 开发环境

前置条件：

- macOS 14+ / Apple Silicon（Intel 与 Windows 暂不支持）
- Xcode Command Line Tools、Rust 工具链
- Node 20+、pnpm 10+
- Python 3.10+（Homebrew 安装）

```bash
git clone https://github.com/Harrison-Yuan/Voxril.git
cd Voxril

python3 -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt

pnpm install
cargo install tauri-cli --version "^2"   # 已经装了可跳过

pnpm dev    # Vite(1420) + Tauri 窗口
```

### 三种开发模式

| 你要改什么 | 怎么跑 | 说明 |
|---|---|---|
| 只改前端界面 | `pnpm -C web dev` | 浏览器打开 `http://localhost:1420`，界面可以正常渲染；需要 Tauri 命令的页面（模型列表、转换等）会没有数据，属于正常现象 |
| 完整应用 | `pnpm dev` | 需要 `.venv` 与基础模型；首次启动会下载约 1 GB 模型 |
| 只改 Python 侧 | 在 `.venv` 中直接运行 `infer/`、`tools/` 下的脚本 | 适合调试分离、特征提取、训练 |

后端命令都放在 `src-tauri/src/commands/`，实时音频在 `src-tauri/src/audio_engine/`，页面在 `web/src/pages/`。

## 提交前自检

请在本地至少跑一遍与改动相关的检查：

```bash
# 前端（类型检查 + 构建）
pnpm -C web run build

# Rust（只检查，不出包）
cargo check --manifest-path src-tauri/Cargo.toml

# Python（语法级检查，秒级完成）
python -m compileall -q infer tools configs

# 依赖与资源是否齐全
python tools/verify_build_deps.py --quick
```

改了界面就附一张截图；改了音频链路请说明你用什么素材、听感差异在哪里。

## 提交规范

- 使用 [Conventional Commits](https://www.conventionalcommits.org/)：`feat:` / `fix:` / `docs:` / `refactor:` / `perf:` / `chore:`
- 一个 PR 只做一件事，避免把格式化、重命名和功能改动混在一起
- 不要提交：`.venv/`、模型权重（`.pth`、`.index`、`.onnx`）、测试音频、`node_modules/`、`src-tauri/target/`
- 不要提交未经本地运行的、明显由 AI 生成的大段代码；如果需要 AI 辅助，请自己读一遍并验证

## PR 流程

1. **先开 Issue**（较大的功能或算法改动必须先讨论），说明问题、方案、影响面
2. Fork 仓库 → 从 `main` 切分支 → 提交 → 发起 PR，勾选模板里的检查项
3. PR 描述里写清：改了什么、为什么改、怎么验证、有什么副作用
4. 维护者会尽量在 **7 天内**给出第一次回复；如果需要更多信息，会直接说清楚缺什么
5. 合并后会在 Release Notes 里致谢

### 贡献授权

项目采用 PolyForm Noncommercial 1.0.0（非商业免费，商业使用需授权）。提交 PR 即表示你同意：

- 你的贡献按项目当前许可分发；
- 维护者可以为商业授权目的对你的贡献进行再许可（relicense）。

如果你所在的组织对此有限制，请先联系维护者。

## 第一次贡献从哪里开始

- 看 [`docs/contributor-tasks.md`](docs/contributor-tasks.md) 里的任务清单，或在 Issue 列表筛选 `good first issue`
- 最容易上手的方向：界面文案与交互、i18n 英文化、README 教程与截图、错误提示、Windows 适配
- 不确定怎么改，可以开 Issue 只描述现象，附上截图或日志，也是一种贡献

---

# Contributing (English summary)

Thanks for helping improve Voxril.

- **Most wanted:** bug fixes, UI/UX polish, docs and tutorials, translations, Windows support.
- **Discuss first:** changes to models, algorithms or audio-quality defaults — open an issue before writing code.
- **Dev setup:** macOS 14+ on Apple Silicon, Python 3.10+, Node 20+, pnpm 10+, Rust. `pnpm install`, then `pnpm dev`. For UI-only work, `pnpm -C web dev` renders the interface in a browser (Tauri commands will have no data).
- **Before opening a PR:** `pnpm -C web run build`, `cargo check --manifest-path src-tauri/Cargo.toml`, `python -m compileall -q infer tools configs`.
- **PR rules:** one change per PR, Conventional Commits, include a screenshot for UI changes and a short verification note for audio changes. Never commit `.venv/`, model weights or test audio.
- **Review:** expect a first response within about 7 days.
- **Licensing:** the project is PolyForm Noncommercial 1.0.0. By submitting a PR you agree your contribution is distributed under the project license and may be relicensed by the maintainer for commercial licensing.

Good starting points are listed in [`docs/contributor-tasks.md`](docs/contributor-tasks.md).
