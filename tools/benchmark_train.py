"""MPS 训练性能基准测试脚本"""
import os, sys, time, shutil, subprocess, json

PROJECT = "/Users/harrison/myDocument/Retrieval-based-Voice-Conversion-WebUI"
os.chdir(PROJECT)
sys.path.insert(0, PROJECT)

ENV = os.environ.copy()
ENV["PYTORCH_ENABLE_MPS_FALLBACK"] = "1"
ENV["PYTORCH_MPS_HIGH_WATERMARK_RATIO"] = "0.0"
ENV["PYTHONPATH"] = PROJECT
ENV["OPENBLAS_NUM_THREADS"] = "6"

PYTHON = os.path.join(PROJECT, ".venv/bin/python")
TRAINSET = os.path.join(PROJECT, "dataset/harrison_voice")
EXP_NAME = "benchmark_v1_40k"
EXP_DIR = os.path.join(PROJECT, "logs", EXP_NAME)
SR = "40k"
VERSION = "v1"
F0 = 0
BATCH = 2
TOTAL_EPOCH = 5
SAVE_EPOCH = 5
NP = 3
PRETRAINED_G = os.path.join(PROJECT, "assets/pretrained/G40k.pth")
PRETRAINED_D = os.path.join(PROJECT, "assets/pretrained/D40k.pth")

shutil.rmtree(EXP_DIR, ignore_errors=True)
os.makedirs(EXP_DIR, exist_ok=True)

# Write config.json for training
with open("configs/v1/40k.json") as f:
    config_json = json.load(f)
config_json["train"]["batch_size"] = BATCH
config_json["train"]["epochs"] = TOTAL_EPOCH
config_json["train"]["fp16_run"] = True
config_json["data"]["training_files"] = "filelist.txt"
with open(os.path.join(EXP_DIR, "config.json"), "w") as f:
    json.dump(config_json, f, indent=2)

results = {"start": time.time(), "steps": {}}

def run_step(name, args_list):
    print(f"\n{'─'*60}")
    print(f"[{name}] {' '.join(args_list[:5])}...")
    t0 = time.time()
    r = subprocess.run(
        [PYTHON] + args_list,
        cwd=PROJECT, env=ENV,
        capture_output=True, text=True, timeout=7200
    )
    t1 = time.time()
    elapsed = t1 - t0
    ok = "✅" if r.returncode == 0 else "❌"
    print(f"[{name}] {ok} {elapsed:.1f}s")
    if r.returncode != 0:
        print(f"STDERR: {r.stderr[-600:]}")
        if r.stdout.strip():
            print(f"STDOUT: {r.stdout[-400:]}")
    results["steps"][name] = elapsed
    return r.returncode == 0

# Step 1: Preprocess
if not run_step("1-预处理", [
    "infer/modules/train/preprocess.py",
    TRAINSET, "40000", str(NP), EXP_DIR, "False", "3.0"
]):
    print("预处理失败,终止")
    sys.exit(1)

# Step 2: F0 (fcpe)
if not run_step("2-F0提取(fcpe)", [
    "infer/modules/train/extract/extract_f0_print.py",
    EXP_DIR, str(NP), "fcpe"
]):
    print("F0提取失败,终止")
    sys.exit(1)

# Step 3: Feature (HuBERT MPS)
if not run_step("3-特征提取(MPS)", [
    "infer/modules/train/extract_feature_print.py",
    "mps", "1", "0", EXP_DIR, VERSION, "false"
]):
    print("特征提取失败,终止")
    sys.exit(1)

# Step 4: Training - 先写 filelist.txt
import glob as g
feature_dir = "3_feature256" if VERSION == "v1" else "3_feature768"
f0_dir = "2a_f0"
f0nsf_dir = "2b-f0nsf"
wavs = [os.path.basename(w) for w in g.glob(f"{EXP_DIR}/0_gt_wavs/*.wav")]
feats = set(os.path.splitext(os.path.basename(f))[0] for f in g.glob(f"{EXP_DIR}/{feature_dir}/*.npy"))
names = sorted(set(os.path.splitext(w)[0] for w in wavs) & feats)
lines = []
for name in names:
    if F0:
        line = f"{EXP_DIR}/0_gt_wavs/{name}.wav|{EXP_DIR}/{feature_dir}/{name}.npy|{EXP_DIR}/{f0_dir}/{name}.wav.npy|{EXP_DIR}/{f0nsf_dir}/{name}.wav.npy|0"
    else:
        line = f"{EXP_DIR}/0_gt_wavs/{name}.wav|{EXP_DIR}/{feature_dir}/{name}.npy|0"
    lines.append(line)
with open(os.path.join(EXP_DIR, "filelist.txt"), "w") as f:
    f.write("\n".join(lines))
print(f"filelist: {len(lines)} entries")

if not run_step("4-模型训练", [
    "infer/modules/train/train.py",
    "-e", EXP_NAME,
    "-sr", SR,
    "-f0", str(F0),
    "-bs", str(BATCH),
    "-g", "0",
    "-te", str(TOTAL_EPOCH),
    "-se", str(SAVE_EPOCH),
    "-pg", PRETRAINED_G,
    "-pd", PRETRAINED_D,
    "-l", "0",
    "-c", "1",
    "-sw", "0",
    "-v", VERSION
]):
    print("训练失败")

# Summary
total = time.time() - results["start"]
results["total"] = total
print(f"\n{'='*60}")
print(f"=== MPS 训练基准测试结果 ===")
for name, sec in results["steps"].items():
    print(f"  {name:25s}: {sec:8.1f}s  ({sec/60:5.1f}min)")
print(f"  {'─'*25}  {'─'*8}  {'─'*5}")
print(f"  {'总耗时':25s}: {total:8.1f}s  ({total/60:5.1f}min)")
print(f"{'='*60}")

train_log = os.path.join(EXP_DIR, "train.log")
if os.path.exists(train_log):
    print(f"\n=== train.log 片段 ===")
    with open(train_log) as f:
        lines = f.readlines()
        for l in lines[:5]:
            print(l.strip())
        if len(lines) > 5:
            print(f"... ({len(lines)} lines total)")
        for l in lines[-5:]:
            print(l.strip())
