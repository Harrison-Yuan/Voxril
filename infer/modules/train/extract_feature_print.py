import os
import sys
import traceback

os.environ["PYTORCH_ENABLE_MPS_FALLBACK"] = "1"
# NOTE: Do NOT set PYTORCH_MPS_HIGH_WATERMARK_RATIO here — Apple Silicon uses
# Unified Memory with default_low_watermark_ratio_unified = 1.4 (MPSAllocator.h)
# which is incompatible with the dedicated-GPU-style HIGH_WATERMARK_RATIO check.

device = sys.argv[1]
n_part = int(sys.argv[2])
i_part = int(sys.argv[3])
if len(sys.argv) == 6:
    exp_dir = sys.argv[4]
    is_half = sys.argv[5].lower() == "true"
else:
    i_gpu = sys.argv[4]
    exp_dir = sys.argv[5]
    os.environ["CUDA_VISIBLE_DEVICES"] = str(i_gpu)
    is_half = sys.argv[6].lower() == "true"

import numpy as np
import soundfile as sf
import torch
import torch.nn.functional as F

from infer.lib.hubert_mps import MPSHubertFeatureExtractor

if "privateuseone" not in device:
    device = "cpu"
    if torch.cuda.is_available():
        device = "cuda"
else:
    import torch_directml
    device = torch_directml.device(torch_directml.default_device())

f = open("%s/extract_f0_feature.log" % exp_dir, "a+")


def printt(strr):
    print(strr)
    f.write("%s\n" % strr)
    f.flush()


printt(" ".join(sys.argv))
model_path = os.environ.get(
    "HUBERT_MODEL_PATH",
    "assets/hubert/vec-768-layer-12.onnx",
)

printt("exp_dir: " + exp_dir)
wavPath = "%s/1_16k_wavs" % exp_dir
outPath = "%s/3_feature768" % exp_dir
os.makedirs(outPath, exist_ok=True)


def readwave(wav_path, normalize=False):
    wav, sr = sf.read(wav_path)
    assert sr == 16000
    feats = torch.from_numpy(wav).float()
    if feats.dim() == 2:
        feats = feats.mean(-1)
    assert feats.dim() == 1, feats.dim()
    if normalize:
        with torch.no_grad():
            feats = F.layer_norm(feats, feats.shape)
    feats = feats.view(1, -1)
    return feats


printt("load model(s) from {}".format(model_path))
if os.access(model_path, os.F_OK) == False:
    printt(
        "Error: Extracting is shut down because %s does not exist, you may download it from https://huggingface.co/lj1995/VoiceConversionWebUI/tree/main"
        % model_path
    )
    exit(0)

model = MPSHubertFeatureExtractor(ckpt_path=model_path, device=device)
printt("move model to %s" % device)
model.eval()

todo = sorted(list(os.listdir(wavPath)))[i_part::n_part]
total = len(todo)
n = max(1, total // 10)
if total == 0:
    printt("no-feature-todo")
else:
    printt("all-feature-%s" % total)
    for idx, file in enumerate(todo):
        try:
            if file.endswith(".wav"):
                wav_path = "%s/%s" % (wavPath, file)
                out_path = "%s/%s" % (outPath, file.replace("wav", "npy"))

                if os.path.exists(out_path):
                    continue

                feats = readwave(wav_path, normalize=False)
                padding_mask = torch.BoolTensor(feats.shape).fill_(False)
                inputs = {
                    "source": feats.to(device),
                    "padding_mask": padding_mask.to(device),
                    "output_layer": 12,
                }
                with torch.no_grad():
                    logits = model.extract_features(**inputs)
                    feats = logits[0]

                feats = feats.squeeze(0).float().cpu().numpy()
                if np.isnan(feats).sum() == 0:
                    np.save(out_path, feats, allow_pickle=False)
                else:
                    printt("%s-contains nan" % file)
                percent = int((idx + 1) * 100 / total)
                # 输出百分比进度，前端可解析 [PROGRESS:feature:XX%]
                printt("[PROGRESS:feature:%s%%]" % percent)
                if idx % n == 0:
                    printt("now-%s,all-%s,%s,%s" % (total, idx, file, feats.shape))
        except:
            printt(traceback.format_exc())
    printt("all-feature-done")
