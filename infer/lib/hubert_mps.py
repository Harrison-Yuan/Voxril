import os
import numpy as np
import torch
import torch.nn as nn
import onnxruntime as ort


class ONNXFeatureExtractor(nn.Module):
    """
    ContentVec ONNX feature extractor (vec-768-layer-12.onnx).
    Input: raw audio waveform [1, T]
    Output: unit12 — layer-12 features [1, T', 768]
    """

    def __init__(self, ckpt_path=None, device="cpu"):
        if ckpt_path is None:
            ckpt_path = os.environ.get(
                "HUBERT_MODEL_PATH",
                "assets/hubert/vec-768-layer-12.onnx",
            )
        if not os.path.exists(ckpt_path):
            raise FileNotFoundError(
                f"ContentVec ONNX 模型未找到: {ckpt_path}\n"
                "请在模型管理页面下载 'ContentVec' 基础模型后重试。"
            )
        super().__init__()

        providers = []
        available = ort.get_available_providers()

        # CoreML EP is unstable on macOS 26.x (Tahoe beta): causes memory corruption
        # ("Context leak detected, CoreAnalytics returned false") that corrupts ONNX
        # output buffers, leading to SIGSEGV in downstream PyTorch upsample ops.
        # CoreML EP also only covers 524/874 nodes (60%), so speedup is minimal.
        # Use CPU-only to ensure stability.
        import platform
        _mac_ver = platform.mac_ver()[0]  # e.g. '15.4.1' or '26.3.1'
        _mac_major = int(_mac_ver.split('.')[0]) if _mac_ver else 0
        _coreml_safe = _mac_major < 26  # disable on macOS 26+ (Tahoe)

        if _coreml_safe and "CoreMLExecutionProvider" in available:
            providers.append("CoreMLExecutionProvider")
        providers.append("CPUExecutionProvider")

        sess_options = ort.SessionOptions()
        sess_options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
        # Force single-threaded sequential execution: on macOS 26 (Tahoe beta) the ONNX
        # Runtime WorkerLoop thread pool invokes PyTorch ATen ops (TensorIteratorBase)
        # which conflicts with PyTorch's kmp/OpenMP, causing SIGSEGV at 0x580.
        # ORT_SEQUENTIAL + intra_op_num_threads=1 bypasses WorkerLoop entirely.
        sess_options.execution_mode = ort.ExecutionMode.ORT_SEQUENTIAL
        sess_options.intra_op_num_threads = 1
        sess_options.inter_op_num_threads = 1

        self.session = ort.InferenceSession(
            ckpt_path,
            sess_options=sess_options,
            providers=providers,
        )
        self.input_name = self.session.get_inputs()[0].name
        self.output_names = [o.name for o in self.session.get_outputs()]
        # vec-768-layer-12.onnx 输出名是 "embed"，contentvec-f.onnx 是 "unit12"
        self._output_idx = next(
            (i for i, n in enumerate(self.output_names) if n in ("embed", "unit12")),
            0
        )
        self.final_proj = nn.Identity()
        self._device = device
        self.eval()

    def extract_features(self, source, padding_mask=None, output_layer=12):
        if isinstance(source, torch.Tensor):
            wave = source.cpu().numpy().astype(np.float32)
        else:
            wave = np.asarray(source, dtype=np.float32)

        if wave.ndim > 1:
            wave = wave.reshape(-1)

        # vec-768-layer-12.onnx 要求 3D 输入 [batch, 1, time]
        wave = wave.reshape(1, 1, -1)

        ort_inputs = {self.input_name: wave}
        outputs = self.session.run(self.output_names, ort_inputs)

        feat = outputs[self._output_idx]
        if feat.ndim == 2:
            feat = np.expand_dims(feat, axis=0)

        # Copy the numpy array out of ONNX's internal buffer before converting to
        # a PyTorch tensor. torch.from_numpy shares memory — if ONNX reclaims or
        # corrupts the buffer, the tensor's data pointer becomes invalid.
        result = torch.from_numpy(feat.copy())
        return (result,)

    def forward(self, source, padding_mask=None, output_layer=12):
        return self.extract_features(source, padding_mask, output_layer)


MPSHubertFeatureExtractor = ONNXFeatureExtractor
