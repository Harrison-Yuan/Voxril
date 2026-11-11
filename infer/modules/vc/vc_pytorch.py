import os
import sys
import time
import logging
import numpy as np

logger = logging.getLogger(__name__)


def _emit_progress(msg, current=0, total=0):
    tag = "[INFER_PROGRESS]"
    if current and total:
        pct = int(current / max(total, 1) * 100)
        sys.stdout.write(f"{tag} {current}/{total} {pct}% {msg}\n")
    else:
        sys.stdout.write(f"{tag} {msg}\n")
    sys.stdout.flush()


class VoiceConverterPyTorch:
    def __init__(self, model_path, hubert_model, device="mps"):
        import torch
        import traceback
        from configs.config import Config

        try:
            self.config = Config()
            self.config.device = "mps" if torch.backends.mps.is_available() else "cpu"
            self.config.is_half = False
            
            logger.info(f"Initializing VoiceConverterPyTorch: device={self.config.device}, is_half={self.config.is_half}")
            logger.info(f"Segment params: x_pad={self.config.x_pad}, x_center={self.config.x_center}, x_max={self.config.x_max}")

            from infer.modules.vc.modules import VC

            self.vc = VC(self.config)
            self.vc.hubert_model = hubert_model
            logger.info("VC instance created with HuBERT model")

            model_filename = os.path.basename(model_path)
            _emit_progress(f"加载声音转换模型({model_filename})...")
            
            self.vc.get_vc(model_filename)
            logger.info(f"VC+Pipeline loaded: {model_filename}")

            _emit_progress("模型加载完成")
            
        except Exception as e:
            error_msg = f"初始化失败: {type(e).__name__}: {str(e)}"
            logger.error(error_msg)
            logger.error(traceback.format_exc())
            _emit_progress(f"错误: {error_msg}")
            raise

    def convert(self, audio_path, transpose=0, f0_method="rmvpe", index_path="",
                index_rate=0.75, filter_radius=3, resample_sr=0,
                rms_mix_rate=0.25, protect=0.33, sid=0):
        import traceback
        
        t_start = time.time()
        logger.info(f"convert: input={audio_path}, transpose={transpose}, f0_method={f0_method}")
        
        try:
            info, opt = self.vc.vc_single(
                sid=sid,
                input_audio_path=audio_path,
                f0_up_key=transpose,
                f0_file=None,
                f0_method=f0_method,
                file_index=index_path,
                file_index2=index_path,
                index_rate=index_rate,
                filter_radius=filter_radius,
                resample_sr=resample_sr,
                rms_mix_rate=rms_mix_rate,
                protect=protect,
            )

            elapsed = time.time() - t_start
            if opt is not None:
                _emit_progress(f"转换完成 ({elapsed:.1f}s)")
                logger.info(f"convert done: {elapsed:.1f}s")
            else:
                _emit_progress(f"转换失败: {info}")
                logger.error(f"convert failed: {info}")

            return info, opt
            
        except Exception as e:
            elapsed = time.time() - t_start
            error_msg = f"{type(e).__name__}: {str(e)}"
            logger.error(f"convert exception after {elapsed:.1f}s: {error_msg}")
            logger.error(traceback.format_exc())
            _emit_progress(f"转换异常: {error_msg}")
            return error_msg, None


_MODEL_CACHE = {}


def get_vc_pytorch(model_path):
    from infer.modules.vc.utils import load_hubert
    from configs.config import Config

    if model_path not in _MODEL_CACHE:
        config = Config()
        hubert = load_hubert(config)
        _MODEL_CACHE[model_path] = VoiceConverterPyTorch(model_path, hubert)
    return _MODEL_CACHE[model_path]
