## 这个 PR 做了什么

<!-- 一两句话说明改动内容和目的 -->

## 为什么要改

<!-- 关联 Issue，例如 Closes #12；说明原来的问题是什么 -->

## 改动范围

- [ ] 前端界面（`web/`）
- [ ] Rust 后端（`src-tauri/`）
- [ ] Python 引擎（`infer/`、`tools/`）
- [ ] 文档 / 截图 / 翻译
- [ ] 构建、CI 或依赖

## 怎么验证的

<!-- 写清你实际跑了什么、看到什么结果。改了音频链路请说明素材与听感差异 -->

```bash
pnpm -C web run build
cargo check --manifest-path src-tauri/Cargo.toml
python -m compileall -q infer tools configs
```

## 截图 / 录屏（界面改动必填）

<!-- 直接拖进来 -->

## 检查项

- [ ] 只做了这一件事，没有混入无关的格式化或重命名
- [ ] 本地跑过上面与改动相关的检查
- [ ] 没有提交 `.venv/`、模型权重、测试音频、`node_modules/`、`src-tauri/target/`
- [ ] 提交信息遵循 Conventional Commits（`feat:` / `fix:` / `docs:` / …）
- [ ] 我同意我的贡献按项目当前许可（PolyForm Noncommercial 1.0.0）分发，并允许维护者为商业授权目的再许可
