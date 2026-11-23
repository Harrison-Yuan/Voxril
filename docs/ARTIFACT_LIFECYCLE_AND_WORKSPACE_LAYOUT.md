# 训练产物生命周期与工作区布局设计

状态：待评审（不含代码改动）
基线数据：2026-11-22 在 `~/Desktop/my_voice_training` 与本机仓库实测
范围：训练管线产物、推理/混音产物、工作区目录布局、清理策略、批次标识
非目标：模型音质、训练超参、实时直播链路、Rust 构建缓存优化

---

## 1. 现状事实基线

### 1.1 管线与产物

Rust 只做编排，产物名与读写关系全部在 Python 侧：

| 步骤 | 执行体 | 产出 | 下游消费者（代码位置） |
|------|--------|------|--------------------|
| 预处理 | `infer/modules/train/preprocess_optimized.py` | `0_gt_wavs/`、`1_16k_wavs/` | 见下两行 + `train.py` |
| 提取 F0 | `infer/modules/train/extract/extract_f0_print.py:154` | `2a_f0/`、`2b-f0nsf/` | `train.rs:266-306` 组 filelist → `train.py` |
| 提取特征 | `infer/modules/train/extract_feature_print.py:53-54` | `3_feature768/` | `train.py`、`train.rs:527` 建索引 |
| 训练 | `infer/modules/train/train.py` | `G_*.pth`、`D_*.pth`、`model.pth`、`filelist.txt`、`config.json`、`events.out.tfevents.*`、`eval/`、`train.log` | `model.pth` → 推理；`G_/D_` → 仅"继续训练"；`0_gt_wavs` 在 `train.py:518/531` 供判别器特征匹配损失 |
| 建索引 | `train.rs:522-586` 内联 faiss | `checkpoints/added_*.index` + `.vectors.bin` + `.ivf.bin` + `.meta.txt` | 推理 index 检索（`infer.rs:865-876`） |

### 1.2 体积基线（harrison 单批次实测）

| 产物 | 体积 | 分类 |
|------|------|------|
| `D_2333333.pth` | 857M | 一次性（仅继续训练需要） |
| `G_2333333.pth` | 438M | 一次性（仅继续训练需要） |
| `0_gt_wavs/` | 158M | 训练期必需，训练后仅"重训"需要 |
| `3_feature768/` | 150M | 训练 + 建索引需要 |
| `1_16k_wavs/` | 63M | 提取阶段后无人再读 |
| `model.pth` | 53M | 永久资产 |
| index 家族 | 293M | 永久资产 |
| `events.out.tfevents.*` | 13M | 纯观测 |
| `2a_f0`+`2b-f0nsf` | 2.8M | 训练期必需 |
| `eval/` `*.log` `filelist.txt` `config.json` | <0.5M | 观测/元数据 |

单批次峰值 **≈ 2.0G**，稳态资产只需 **346M**（`model.pth` + index 家族），可回收率 **83%**。
回收量分两种口径：真正零代价（`1_16k_wavs` + `events` + `eval`）≈ **76M/批次**；若允许放弃该批次的"继续训练"能力，把 `G_`+`D_` 一并回收，则再省 **1.28G/批次** —— 后者是主要收益来源，因此必须作为显式产品决策（见 §11 问题 1）。

工作区实测：总 8.1G（清理前），`experiments/` 5.2G、`separated/` 1.2G、`mixed/` 783M、`converted/` 438M、两个源 wav 475M；数据卷可用空间仅 6.3G。

### 1.3 命名规则现状

| 产物 | 规则 | 代码 | 问题 |
|------|------|------|------|
| 转换人声 | `{源文件stem}_{model路径末段}_key{±N}_{YYYYmmdd_HHMMSS}.wav` | `infer.rs:186-195` | `model_stem` 取的是 `harrison/v2_40k` 的**末段** `v2_40k`，不含声音名，不含模型代次 → 任何 40k 实验的产物同名前缀 |
| 混音成品 | `merged_{vocal_stem 去掉 vocal_}_{混音时刻}.wav` | `infer.rs:509-513` | 实际形如 `merged_枪火_v2_40k_key0_20261121_170940_20261121_182451.wav`：双时间戳、无作者、无批次 |
| 索引 | `added_IVF{n}_Flat_nprobe_1_{exp目录名}_v2.index` | `train.rs:554` | 不含训练批次 |
| 权重 | 固定 `model.pth`，每轮覆盖 | `train.py` | 重训即毁上一版 |
| 中间产物 | 固定目录名，重跑先删后写 | `train.rs:126-131`、`172-177` | 完全没有批次概念 |

结论：现有文件名**无法回答"这个成品是哪一版模型产出的"**，用户只能靠手工建 `_backup_20261121_pre_silence_fix` 这类目录自救。

### 1.4 工作区布局现状

```
my_voice_training/
  *.mp3 x20                用户素材（不可再生）
  harrison/song.wav  288M   训练素材   ┐ 与 experiments/ 同级
  shine/shine.wav    187M   训练素材   ┘ 且 shine/ 与 experiments/shine/ 是两个语义
  trainsets/         0B     空遗留目录
  separated/         1.2G   可再生（UVR 产物 + 训练集来源）
  converted/         438M   可再生（推理中间人声 + .stft.npy 旁路缓存）
  mixed/             783M   成品
  experiments/       5.2G   资产 + 可再生中间产物混在一层
  uvr_models/        34M    bs_roformer ckpt（仓库 assets/uvr_models 里只有 yaml，无副本）
  rvc.db             36K    一套 schema（app_settings/configs/history/workflow）
仓库根/rvc.db        40K    另一套 schema（experiment_configs/training_history/workflow_status）
```

### 1.5 现有清理机制与盲区

`cleanup.rs` 启动时做 7 类安全清理：`__pycache__`、`.DS_Store`、npm 缓存、egg-info、7 天前轮转日志、孤立 `.f0.json`、孤立 IVF 缓存。
盲区：上述任何一项都不覆盖 1.2 里的 933M 一次性产物；`deep_clean()` 清的是 `src-tauri/target`（13G）和 `web/dist`，与训练无关，且**前端零入口**。

`file.rs` 提供两个删除命令：

- `clear_training_artifacts`（216-321）：全清 —— 连 `model.pth`、index、`0_gt_wavs`…`3_feature768` 一起删，无中间档
- `delete_training_result`（148-213）：走 `logs/` + `models/` + `data/` 旧布局（`DATA_DIR`/`MODEL_DIR` 常量在 7-8 行），当前布局下**必然空转**，前端也无调用点

### 1.6 已定位缺陷清单

| 编号 | 事实 | 位置 |
|------|------|------|
| D1 | 重训前静默全清，旧模型不可恢复，逼出手工备份习惯 | `web/src/pages/Train/index.tsx:696-700` + `file.rs:216-321` |
| D2 | 步骤完成态靠日志关键字（`end preprocess`/`all-feature-done`），不看产物是否存在 → 手工删了中间产物仍显示"已完成" | `Train/index.tsx:71-212` |
| D3 | 产物展示卡永不渲染：`experimentOptions.includes(qualifiedExpDir)`，前者是 `harrison` 后者是 `harrison/v2_40k` | `Train/index.tsx:950` |
| D4 | 该卡确认文案称"预处理数据将被保留"，实际按钮调全清 | `Train/index.tsx:976` vs `985` |
| D5 | 删旧 index 时 `${expPath}/${f}` 二次拼接（`f` 已是绝对路径），失败被 `catch {}` 吞掉 → 孤儿缓存长期堆积 | `Train/index.tsx:704-708` + `utils.rs:36` |
| D6 | `.stft.npy`（实测 157M）落在 `converted/`，删 wav 不连带删，`cleanup_stale_ivf_cache` 只管 index 的 sidecar | `live.rs:147`、`cleanup.rs:236-285` |
| D7 | 管线代码存在 4 份副本且已漂移：`target/check/debug/_up_/infer/.../train.py` md5 与源文件不一致 | `infer/modules/train/train.py` 及 `src-tauri/target/{debug,release}`、`target/check/debug` |
| D8 | `converted`/`mixed`/`separated` 三处产物在 UI 上无列表、无删除入口，只有本次会话的单条结果 | `Infer/index.tsx:178-241` |
| D9 | UVR 说明文案写 `vocal.wav`/`instrument.wav`，实际为 `vocal_{name}.flac`/`instrument_{name}.*` | `UVR/index.tsx:165-168` vs `vocal_sep_mps.py:116-149` |
| D10 | 两套 rvc.db schema 并存；前端两套 trainApi（`web/src/api/train.ts` 零引用）、`useTrainWorkflow`/`computeNextAction` 未挂载 | `database.rs`、`api/train.ts`、`hooks/useTrainWorkflow.ts` |
| D11 | 已注册但前端零调用的命令：`deep_clean_cache`、`delete_training_result`、`build_index_cache`、`check_preprocess_status` | `main.rs`、`workspace.rs:68-74` |
| D12 | 训练页出现两个"步骤 2"；训练页/模型页中文硬编码不走 i18n | `TrainModelStep.tsx:69`、`FeatureExtractionSteps.tsx:53` |
| D13 | 续训静默失效：`-pg` 读 `["model"]` 键，而 `model.pth` 存 `["weight"]` 键，加载失败被 `except` 吞掉只打 warning，实际从头训练 | `train.py:242-268` vs `process_ckpt.py:16-50` |

---

## 2. 设计原则

1. **单一判据**：任何文件只属于两类之一 —— "删了能低成本重生成"（可回收）或"不能"（资产）。UI、清理策略、命名全部围绕这一条。
2. **批次不可变**：一次训练 = 一个批次，产出只归档不覆盖；重训永不毁旧版。
3. **自动回收挂在生命周期终点**（训练/索引成功退出），不在启动时按 mtime 猜测 —— 训练进程会实时读这些目录，启动期清理有破坏性风险。
4. **名字即元数据**：文件名/目录名自带 `voice + run_id`，不依赖上下文就能分辨代次。
5. **不改动 Python 训练脚本的输出名**，批次化在 Rust 归档层完成，把风险集中在一处。
6. **兼容优先**：旧工作区、旧布局、旧库记录只读兼容，不做破坏性迁移，除非用户显式点"升级布局"。

---

## 3. 批次（Run）模型

### 3.1 run_id

`run_id = %y%m%d-%H%M`（本地时间，例：`261122-0156`）。
特性：可字典序排序 = 时间序、目录安全、长度固定 11 字符、无时区歧义。
冲突处理：同一分钟重复建批 → 追加 `-2`、`-3`。

### 3.2 run.json（批次清单，新增）

```json
{
  "run_id": "261122-0156",
  "voice": "harrison",
  "sr": "40k", "version": "v2", "if_f0": true,
  "pipeline_version": "<train.rs/preprocess_optimized.py 内容哈希>",
  "dataset": {
    "source": "separated/vocals",
    "fingerprint": "<排序后的 (相对路径, size) 列表哈希>",
    "splits": 371, "total_seconds": 1842.6
  },
  "params": { "epochs": 150, "batch_size": 4, "save_epoch": 10, "f0_method": "rmvpe", "np": 4 },
  "stages": {
    "preprocess": { "status": "done", "manifest_hash": "..." },
    "f0": { "status": "done", "manifest_hash": "..." },
    "feature": { "status": "done", "manifest_hash": "..." },
    "train": { "status": "done", "epoch": 150, "started_at": 0, "finished_at": 0 },
    "index": { "status": "done", "ntotal": 48228, "n_ivf": 1236 }
  },
  "assets": [
    { "kind": "weights", "path": "weights/harrison-261122-0156-e150.pth", "bytes": 55187535 },
    { "kind": "index", "path": "weights/harrison-261122-0156-e150.index", "bytes": 152349259 }
  ],
  "reclaimable": { "bytes": 933000000, "reclaimed_at": null }
}
```

`pipeline_version` 与 `dataset.fingerprint` 用于替代 D2 的日志关键字判定：任一变，相应 stage 立刻降级为"需重跑"，UI 不再给出虚假的"已完成"。
`stages.*.manifest_hash` 为该目录 `(文件名, size)` 列表的哈希 —— 手工删文件会被直接发现（覆盖 D2/D5）。

### 3.3 归档机制（不覆盖、不额外占空间）

训练+索引成功退出时，Rust 侧执行：

1. 写 `experiments/<voice>/runs/<run_id>.json`
2. 把 `v2_40k/model.pth` 与 `checkpoints/added_*.index` **硬链接**进 `experiments/<voice>/weights/<voice>-<run_id>-e<epoch>.<ext>`
   - APFS 同卷硬链接零额外空间；跨卷或失败时回落为复制并在 UI 提示
   - 因是独立目录链接，`clear_training_artifacts` 清工作位时不会伤到历史批次
3. 立即执行一次性产物回收（第 4 节）

推理侧无需新逻辑：`infer.rs:157-178` 已支持 `"<相对目录>/<文件>.pth"` 形式的模型名，并把 `index_root` 解析到该文件所在目录 —— 归档 pth 与归档 index 同放 `weights/`，`find_experiment_index` 自然命中同批索引。

### 3.4 命名规范

| 产物 | 规范 | 示例 |
|------|------|------|
| 归档权重 | `<voice>-<run_id>-e<epoch>.pth` | `harrison-261122-0156-e150.pth` |
| 归档索引 | `<voice>-<run_id>-e<epoch>.index`（sidecar 同前缀） | 同上 |
| 转换人声 | `<song>_<voice>_<run_id>_key<±N>_<HHmm>.wav` | `枪火_harrison_261122-0156_key0_0930.wav` |
| 混音成品 | `merged_<song>_<voice>_<run_id>_key<±N>_<HHmm>.wav` | `merged_枪火_harrison_261122-0156_key0_0955.wav` |
| 分离产物 | `vocal_<song>.<ext>` / `inst_<song>.<ext>`（保持，Python 侧已稳定） | `vocal_枪火.flac` |

`<song>` 取输入文件名去扩展、去 `vocal_`/`instrument_` 角色前缀（`infer.rs::song_stem`），按 `[^\w\u4e00-\u9fff-]` → `_` 清洗并截断 40 字符。
混音名继承其 vocal 输入的批次串，但**必须以 `merged_` 开头**：类型标记放在前缀而不是 `_mix` 后缀，翻唱成品在文件列表里一眼就能和人声干音分开，也不会因为源文件叫 `vocal_枪火` 就退化成 `vocal_枪火_...`。认不出命名时退化为 `merged_<歌名>_<HHmm>`，不再叠加双时间戳（覆盖 1.3 表第 2 行）。
历史 `_mix<HHmm>` 后缀名仍被 `parse_output_name` 识别为 mixed，不做批量改名。

### 3.5 兼容旧数据

| 旧形态 | 读取策略 |
|--------|----------|
| `v2_40k/model.pth` 无 `weights/` 归档 | 视为"未归档批次"，`run_id` 取 `model.pth` 的 mtime 格式化串，UI 标"旧版 · 未归档" |
| `experiments/<voice>/<非 v*_ 目录>/`（如 `_backup_20261121_pre_silence_fix`） | 现有 `list_models` 会误判为 version 目录；改为白名单 `^v\d_(32\|40\|48)k$`，其余目录只出现在"归档/待清理"列表 |
| `added_*.index` 旧名 | 正则提取 `IVF\d+` 作为 label 补充，不改文件 |
| 旧 `rvc.db` 双 schema | 以 `data_dir` 下的为准；仓库根那份标为遗留、只读、不再写入（D10） |

---

## 4. 产物生命周期与自动回收

### 4.1 回收分级

| 级别 | 内容 | 触发 | 代价 |
|------|------|------|------|
| R1 零代价 | `1_16k_wavs/`、`events.out.tfevents.*`、`eval/`、`converted/*.stft.npy`（无宿主 wav 时）、轮转日志、`.DS_Store` | 批次完成即执行 | 无（`1_16k_wavs` 重跑预处理即可再生；观测类无人读取） |
| R1' 功能代价 | `D_*.pth`(857M)、`G_*.pth`(438M) | 默认延迟 7 天、批次未锁定且用户确认后才删 | 该批次不再支持"继续训练"——`G_`/`D_` 必须成对在场，`model.pth` 无法作为 `-pg/-pd` 续训入口；音质验证只能靠推理成品 |
| R2 重算代价 | `0_gt_wavs/`、`2a_f0/`、`2b-f0nsf/` | 用户点"回收训练集"或磁盘水位低于阈值 | 需重跑预处理（分钟级） |
| R3 不可再生 | `separated/`、`inputs` 素材、`weights/` 归档 | 永不自动删，仅 UI 明示体积并允许手删 | 重跑 UVR（小时级） |
| 资产 | `model.pth`/归档权重、`*.index`、`run.json`、`mixed/` 成品 | 永不自动删 | — |

`G_`/`D_` 成对才有续训意义，且不能用 `model.pth` 替代：`savee()` 写出的 `model.pth` 用 `weight` 键且是 fp16（`process_ckpt.py:16-50`），而续训加载读的是 `["model"]` 键（`train.py:253/261/279`）——键不匹配会被外层 `except` 捕获、只打一行 warning 后**静默 from scratch**。所以 R1' 只能"两个都留"或"两个都删"，UI 上必须把这个取舍讲成一句人话：**回收后这一批只能用来转换，不能再接着练**。批次提供"锁定"开关，锁定批次跳过 R1'。

### 4.2 触发时机

- 主路径：`train.py` 进程退出且末轮 epoch == 目标 epoch → 归档 → R1
- 索引完成（`train_index` 成功）→ 归档 index → 允许回收 `3_feature768/`（当前它只在 UI 显示 index 就绪后才有二次利用价值）
- 兜底：启动时只清理"无任何 `run.json` 引用且 mtime > 30 天"的孤儿，绝不清当前批
- 互斥：`state.active_heavy_task == "train"` 期间禁止任何回收（复用现有令牌 `train.rs:434`）

理由见 D1 与本轮实操：训练进行到 epoch 142 时，`0_gt_wavs`/`3_feature768` 正被 dataloader 读取。

### 4.3 命令语义重构

| 命令 | 新语义 | 删除范围 |
|------|--------|----------|
| `reclaim_run`（新增） | 回收该批一次性产物 | R1（可选 R2） |
| `reset_experiment`（由 `clear_training_artifacts` 改名收窄） | 重训前重置工作位 | 工作位全部产物，**不含** `weights/`、`run.json` |
| `purge_run`（新增） | 删除某历史批次 | 指定 `weights/<run_id>` + 该批 `run.json`，需二次确认 |
| `delete_experiment` | 删除整个声音 | 现有 `delete_model` 已实现（`infer.rs:578-597`），补 UI 体积预览 |

`delete_training_result` 直接删除（D11，旧布局空转）。

### 4.4 保护规则

1. 任何被 `outputs/` 中成品引用的批次不得进入 R2/R3（凭产物文件名里的 `run_id` 反查）
2. 磁盘水位阈值：可用 < 2G 时，在训练启动前提示并建议执行 R1/R2，而不是静默删除
3. 回收前先在 `run.json.reclaimable` 记录待删清单与体积，回收后写入 `reclaimed_at`，UI 可展示"本批次已为你省 933M"
4. 所有删除走"移动到 `workspace/.trash/<run_id>/`"+ 7 天过期，替代现在的直接 `remove_dir_all`（当前 `clear_training_artifacts` 不可撤销，而 D4 又给它配了一句错误的说明）

### 4.5 预期收益

| 场景 | 现在 | 之后 |
|------|------|------|
| 单批次稳态占用 | 2.0G | 346M（回收 R1+R1'，-83%）/ 1.63G（保留续训能力，仅 R1） |
| 3 个批次并行留存 | 6.0G | 1.04G + 按需归档 |
| 重训后找回上一版 | 手工建 `_backup_*` 目录（1.5G 全量副本） | `weights/` 硬链接，0 额外空间 |
| 孤儿缓存（如 104M `.ivf.bin`、157M `.stft.npy`） | 永久堆积 | 批次终点即清 |

---

## 5. 工作区目录布局

### 5.1 目标布局

```
workspace/
  inputs/                   用户素材：原始歌曲、训练源音频（永不自动清理）
  work/                     全部可再生数据
    datasets/<voice>/       原 separated 语义：vocal_<song> / inst_<song> / *.f0.json / *.lrc
    runs/<voice>/<run_id>/  原 experiments/<voice>/v2_40k 的工作位（0_gt…3_feature、G_/D_、日志）
    cache/                  UVR 模型、IVF 缓存等派生缓存
  outputs/
    draft/                  converted 人声 + mixed 初版混音（允许按保留期回收）
    final/                  用户点"定稿"的成品（永不自动清理）
  models/<voice>/           资产：weights/*.pth + *.index + runs/*.json
```

层级语义一次讲清：`inputs` 进、`work` 算、`outputs` 出、`models` 留。用户判断"能不能删"只需问"这目录在哪一层"。

### 5.2 迁移映射

| 现路径 | 新路径 | 方式 |
|--------|--------|------|
| `*.mp3`（根目录 20 首） | `inputs/raw/` | 移动 |
| `harrison/song.wav`、`shine/shine.wav` | `inputs/training/<voice>/` | 移动（同时消灭 `shine/` vs `experiments/shine/` 撞名） |
| `separated/{vocals,instrumental}` | `work/datasets/<voice>/`（或按 song 平铺） | 移动 |
| `experiments/<voice>/v2_40k/{0_gt_wavs…train.log}` | `work/runs/<voice>/<run_id>/` | 按现有 mtime 推断 run_id 后移动 |
| `experiments/<voice>/v2_40k/model.pth`、`checkpoints/*.index` | `models/<voice>/weights/` | 硬链接归档 + 原文件移动 |
| `converted/` | `outputs/draft/` | 移动 |
| `mixed/` | `outputs/draft/`（与 draft 合并，靠文件名区分 vocal/merged 类型） | 移动 |
| `uvr_models/` | `work/cache/uvr/` | 移动 |
| `trainsets/`（空） | 删除 | — |
| `rvc.db`（workspace 与仓库两份） | `workspace/state.db` 单份，仓库那份停止写入 | 迁移 + 兼容读 |

### 5.3 迁移策略

- 一次性向导（设置页"升级工作区结构"）：预览 → 校验可用空间 → 同卷 `rename`（瞬时）→ 失败即整体回滚
- 迁移前写 `workspace/layout.json { version: 2, migrated_at, mapping[] }`；读侧按该文件决定用哪套路径，缺失时走旧布局，保证不迁也能用（原则 6）
- 不做双向同步：迁移即终态，旧路径只读 30 天后由用户确认删除
- 硬链接前提是同卷；跨卷（用户把 workspace 指到外置盘）时 `models/` 退化为复制并明确告知体积代价

### 5.4 兼容读取矩阵（代码落点）

| 常量/逻辑 | 现状 | 目标 |
|-----------|------|------|
| `EXPERIMENTS_DIR`（`train.rs:8`、`infer.rs:10`、`events.rs:8`、`file.rs:220` 四处重复） | 各自硬编码 `"experiments"` | 收敛到单一 `WorkspaceLayout` 模块，按 `layout.json` 解析 |
| `workspace.join("converted")`（`infer.rs:195`）、`workspace.join("mixed")`（516-518） | 字面量散落 | `layout.output_dir(kind)` |
| `separated/{vocals,instrumental}`（`live.rs:57-58`、`cleanup.rs:190`、`useInferState.ts:104/118`、`UVR/index.tsx:60/84-85`） | 前端拼路径 + 后端拼路径两套 | 后端统一返回，前端只消费 |
| `DATA_DIR="data"` / `MODEL_DIR="models"`（`file.rs:7-8`） | 旧布局残留 | 随 `delete_training_result` 一起删除 |

---

## 6. UI 与信息架构

### 6.1 训练页

- 步骤状态改为 `run.json.stages` + 指纹比对（修 D2/D4）：产物被人删或管线版本变化 → 显示"需重跑"，而不是"已完成"
- 修 D3：卡片可达性条件用 `list_experiments` 返回的**限定名**（`harrison/v2_40k`），同时把 `list_experiments` 返回结构从"单层目录名字符串"改为 `{voice, run_id, sr, has_model, has_index, bytes}` 数组
- 修 D12：步骤编号统一为 1 预处理 / 2 特征 / 3 训练 / 4 索引，并把"下一步"由 `computeNextAction` 真正驱动（现为零使用死代码 D10）
- 每步下方显示"本步产出体积"，批次卡汇总显示"当前批次可回收 / 已自动回收"体积

### 6.2 批次列表（新增，训练页与模型页共用）

按 `voice` 分组的表格：`批次 | 日期 | epoch | 数据集指纹 | 权重 | 索引 | 占用 | 操作`。
操作：加载为当前工作位 / 删除批次（`purge_run`，带体积预览与 .trash 提示）。批次权重与索引在训练/建索引完成时已自动硬链接归档到 `models/<voice>/`，无需手工"定稿"；音频成品的定稿在成品库（§6.3）。
"当前模型"用指针显示（取该声音最新批次），重训不再产生"上一版消失"。

### 6.3 成品库

新增页面（或 Infer 页下方面板）解决 D8：
- 按批次折叠列出 `outputs/draft` 的转换/混音结果：名称、时长、模型批次、升降调、体积
- 每条支持播放、定稿（移到 `final/`）、删除（连同 sidecar 一起）
- `converted` 与 `mixed` 的关联由文件名里的 `run_id` + `song` 直接推断，不再靠双时间戳肉眼比对

### 6.4 清理中心（设置页）

- `deep_clean` 入口补上（D11）
- 分区展示："可立即回收（R1）/ 重算可再生（R2、R3）/ 资产"，各带实测体积
- 单一"释放空间"动作 + 明细展开；执行前展示将删清单与总体积
- `.stft.npy`、IVF sidecar 之类派生缓存改为按宿主文件存在性自动清（修 D6/D5）

---

## 7. 数据模型变更

`data_dir` 下 `rvc.db` 新增/调整：

```
runs(run_id TEXT PRIMARY KEY, voice TEXT, sr TEXT, if_f0 INT,
     created_at INT, finished_at INT, epochs INT,
     dataset_fingerprint TEXT, pipeline_version TEXT,
     stages_json TEXT, assets_json TEXT, bytes_total INT,
     reclaimed_at INT NULL, note TEXT)

outputs(id INTEGER PRIMARY KEY, kind TEXT,        -- converted | mixed | final
        path TEXT, song TEXT, voice TEXT, run_id TEXT,
        transpose INT, created_at INT, bytes INT, starred INT DEFAULT 0)
```

`configs` 表保留但增加 `run_id` 列（旧行为 NULL → 读作"未归档"）。
`workflow` 表逐步被 `runs.stages_json` 取代，过渡期两处同写、UI 只读 `runs`。

---

## 8. 代码改动清单（按文件，供排期）

| 文件 | 改动 |
|------|------|
| `src-tauri/src/workspace_layout.rs`（新增） | `WorkspaceLayout`：路径解析、`layout.json` 读写、v1/v2 兼容 |
| `src-tauri/src/run_manifest.rs`（新增） | `run.json` 读写、fingerprint/manifest_hash 计算、run_id 生成 |
| `src-tauri/src/commands/train.rs` | 归档 + 硬链接 + R1 回收；`train_index` 后允许回收 `3_feature768`；`EXPERIMENTS_DIR` 改用 layout；索引命名加批次；启动前校验 `-pg/-pd` 权重键名，续训必须成对使用同批 `G_`/`D_`（修 D13） |
| `src-tauri/src/commands/infer.rs` | 输出文件名规范（186-195、509-513）；`list_models` 增列 `weights/` 归档 + version 目录白名单（823-863）；删除产物时连带 sidecar |
| `src-tauri/src/commands/file.rs` | 拆 `reclaim_run`/`reset_experiment`/`purge_run`；删 `delete_training_result` 与 `DATA_DIR/MODEL_DIR`；修 D5 类路径拼接；删除改走 `.trash` |
| `src-tauri/src/cleanup.rs` | 新增 `.stft.npy` 宿主校验；`deep_clean` 移除 `src-tauri/target` 或改为显式参数；接入 R1 |
| `src-tauri/src/database.rs` | `runs` / `outputs` 表 + 迁移 |
| `src-tauri/src/main.rs` | 新命令注册；启动清理改按 `layout.json` |
| `web/src/pages/Train/*` | 状态判定改读 `run.json`；修 D3/D4/D12；批次列表卡；体积显示 |
| `web/src/pages/Models/index.tsx` | 改读 `list_models` 新结构，展开声音展示批次明细、删除批次 |
| `web/src/pages/Infer/*` | 成品库面板（D8） |
| `web/src/pages/UVR/index.tsx` | 修 D9 文案；目录改由后端返回 |
| `web/src/pages/Settings/index.tsx` | 清理中心 + `deep_clean` 入口 + 布局升级向导 |
| `web/src/api/train.ts` | 删除（零引用重复实现，D10） |

---

## 9. 分阶段实施

| 阶段 | 内容 | 验收 |
|------|------|------|
| P0 | R1 自动回收 + `reclaim/reset` 拆分 + 修 D3/D4/D5/D6/D13 + `.trash` 兜底 | 新批次完成后占用 ≤ 1.7G（R1' 待 §11 问题 1 定案后再定 500M 目标）；重训不再产生手工备份需求；孤儿缓存 0 残留；训练中途无任何删除动作；键名不匹配的续训入口直接报错而非静默 from scratch |
| P1 | `run_id` + `run.json` + 硬链接归档 + 命名规范 + 批次列表 UI | 同声音多批次共存且名字可辨；推理能直接选历史批次并命中同批 index；指纹变更后 UI 正确显示"需重跑" |
| P2 | 工作区三分 + 迁移向导 + `WorkspaceLayout` 收敛 | 旧 workspace 一键迁移可回滚；新路径全覆盖；仓库根 `rvc.db` 不再写入 |
| P3 | 成品库（outputs/draft→final）+ 清理中心 + 死代码清理（D7/D10/D11/D12） | 用户无需理解目录即可删除/回收；`pipeline_version` 漂移检测生效 |

依赖：P1 依赖 P0 的 reclaim 语义；P2 依赖 P1 的 `run.json`；P3 可与 P2 并行。

---

## 10. 风险与回滚

| 风险 | 缓解 |
|------|------|
| 硬链接失败（跨卷/网络盘/旧 FS） | 回落复制 + UI 明示额外体积；复制失败则跳过归档并保留工作位，绝不删源 |
| 回收误删"继续训练"所需权重 | `G_*.pth` 默认保留 7 天；批次可"锁定"跳过 R1/R2 |
| 迁移中断导致数据半新半旧 | 逐项 rename + 失败即回滚；迁移前校验同卷与空间；`.trash` 与 `layout.json` 保留映射 |
| 新命名让旧产物列表不兼容 | 读取侧正则匹配两代格式，写入侧只用新格式 |
| `list_models` 结构变更冲击前端 | 新增 `list_runs` 命令并行提供，旧命令返回值保持可解析 |
| 训练中途删除（历史事故点） | 回收仅在 `train.py` 退出后触发，`active_heavy_task` 期间全局禁用删除路径 |

---

## 11. 待决策问题

1. 批次保留策略：永久保留所有历史批次权重（占空间但零风险），还是默认保留 N 批（如 3）+ 更老批次转"仅 model.pth，无继续训练能力"？
2. 输出保留策略：`outputs/draft` 是否需要自动过期（如 30 天未定稿自动清理），还是永久保留由用户手删？
3. `separated/`（1.2G，重跑 UVR 为小时级）算 R3 可再生，还是提升为"准资产"完全不自动碰？
4. 是否把 UVR 模型权重（`uvr_models/*.ckpt`）统一收敛到应用 `data_dir/assets`，消除仓库/数据目录/workspace 三处分散？
5. 工作区路径冲突：迁移后 `models/` 目录名与旧 `file.rs` 的 `MODEL_DIR` 语义撞名，是否需要换名（如 `voices/`）以免读代码时误解？
6. `deep_clean` 是否应继续包含 `src-tauri/target`（13G，重编代价大）？若保留，需要单独分区并明确标注"开发者选项"。
7. 训练素材（`song.wav` 288M）与原始 mp3 高度重复，是否在迁移时只保留 mp3 并在 `run.json` 记录其派生关系？
