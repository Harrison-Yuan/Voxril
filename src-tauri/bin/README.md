# 平台二进制目录

按「平台-架构」分目录存放随包分发的可执行文件，避免不同平台互相覆盖：

```
src-tauri/bin/
├── ffmpeg/              # 当前 macOS Apple Silicon 使用的 ffmpeg（Mach-O arm64）
├── macos-x64/           # 预留：Intel macOS（x86_64）ffmpeg
└── windows-x64/         # 预留：Windows x64 ffmpeg.exe
```

说明：

- 正式支持平台为 **macOS (Apple Silicon)**；`macos-x64/`、`windows-x64/` 为扩展预留位。
- 新增平台时：
  1. 把对应二进制放进平台目录（Windows 文件名必须是 `ffmpeg.exe`）
  2. 在 `src-tauri/tauri.<platform>.conf.json` 的 `bundle.resources` 里声明该目录
  3. 代码侧通过 `src-tauri/src/platform/mod.rs::ffmpeg_filename()` 取文件名，无需改业务逻辑
- `.gitignore` 不再忽略 `ffmpeg.*`，平台二进制可以直接提交；单文件请控制在 100MB 以内。
