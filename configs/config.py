import argparse
import os
import sys
import json
from multiprocessing import cpu_count

import torch
import logging

logger = logging.getLogger(__name__)


version_config_list = [
    "v2/40k.json",
    "v2/48k.json",
    "v2/32k.json",
]


def singleton_variable(func):
    def wrapper(*args, **kwargs):
        if not wrapper.instance:
            wrapper.instance = func(*args, **kwargs)
        return wrapper.instance

    wrapper.instance = None
    return wrapper


@singleton_variable
class Config:
    def __init__(self):
        self.device = "cuda:0"
        self.is_half = False  # Force fp32 for local environment
        self.use_jit = False
        self.n_cpu = 0
        self.gpu_name = None
        self.json_config = self.load_config_json()
        self.gpu_mem = None
        (
            self.python_cmd,
            self.listen_port,
            self.iscolab,
            self.noparallel,
            self.noautoopen,
            self.dml,
        ) = self.arg_parse()
        self.instead = ""
        self.preprocess_per = 3.7
        self.x_pad, self.x_query, self.x_center, self.x_max = self.device_config()

    @staticmethod
    def load_config_json() -> dict:
        d = {}
        # ⚠️ DMG 兼容：RVC_CONFIG_DIR 指向可写数据目录下的 configs/ 副本
        # 开发模式下 fallback 到相对路径
        config_dir = os.environ.get("RVC_CONFIG_DIR", "configs")
        for config_file in version_config_list:
            p = os.path.join(config_dir, config_file)
            with open(p, "r") as f:
                d[config_file] = json.load(f)
        return d

    @staticmethod
    def arg_parse() -> tuple:
        exe = sys.executable or "python"
        parser = argparse.ArgumentParser()
        parser.add_argument("--port", type=int, default=7865, help="Listen port")
        parser.add_argument("--pycmd", type=str, default=exe, help="Python command")
        parser.add_argument("--colab", action="store_true", help="Launch in colab")
        parser.add_argument(
            "--noparallel", action="store_true", help="Disable parallel processing"
        )
        parser.add_argument(
            "--noautoopen",
            action="store_true",
            help="Do not open in browser automatically",
        )
        parser.add_argument(
            "--dml",
            action="store_true",
            help="torch_dml",
        )
        cmd_opts = parser.parse_args()

        cmd_opts.port = cmd_opts.port if 0 <= cmd_opts.port <= 65535 else 7865

        return (
            cmd_opts.pycmd,
            cmd_opts.port,
            cmd_opts.colab,
            cmd_opts.noparallel,
            cmd_opts.noautoopen,
            cmd_opts.dml,
        )

    # has_mps is only available in nightly pytorch (for now) and MasOS 12.3+.
    # check `getattr` and try it for compatibility
    @staticmethod
    def has_mps() -> bool:
        if not torch.backends.mps.is_available():
            return False
        try:
            torch.zeros(1).to(torch.device("mps"))
            return True
        except Exception:
            return False

    @staticmethod
    def has_xpu() -> bool:
        if hasattr(torch, "xpu") and torch.xpu.is_available():
            return True
        else:
            return False

    def use_fp32_config(self):
        config_dir = os.environ.get("RVC_CONFIG_DIR", "configs")
        for config_file in version_config_list:
            p = os.path.join(config_dir, config_file)
            with open(p, "r") as f:
                strr = f.read().replace("true", "false")
            with open(p, "w") as f:
                f.write(strr)
            logger.info("overwrite " + config_file)
        self.preprocess_per = 3.0
        logger.info("overwrite preprocess_per to %d" % (self.preprocess_per))

    def device_config(self) -> tuple:
        # Force fp32 for local environment
        logger.info("Local environment: forcing fp32 precision")
        self.use_fp32_config()
        
        if torch.cuda.is_available():
            if self.has_xpu():
                self.device = self.instead = "xpu:0"
                self.is_half = False  # Force fp32
            i_device = int(self.device.split(":")[-1])
            self.gpu_name = torch.cuda.get_device_name(i_device)
            logger.info("Found GPU %s, using fp32", self.gpu_name)
            self.is_half = False  # Force fp32
            self.gpu_mem = int(
                torch.cuda.get_device_properties(i_device).total_memory
                / 1024
                / 1024
                / 1024
                + 0.4
            )
            if self.gpu_mem <= 4:
                self.preprocess_per = 3.0
        elif self.has_mps():
            logger.info("No supported Nvidia GPU found")
            self.device = self.instead = "mps"
            self.is_half = False
            self.use_fp32_config()
        else:
            logger.info("No supported Nvidia GPU found")
            self.device = self.instead = "cpu"
            self.is_half = False
            self.use_fp32_config()

        if self.n_cpu == 0:
            self.n_cpu = cpu_count()

        if self.is_half:
            # 6G显存配置
            x_pad = 3
            x_query = 10
            x_center = 60
            x_max = 65
        else:
            # 5G显存配置 (默认)
            x_pad = 1
            x_query = 6
            x_center = 38
            x_max = 41

        # MPS设备使用MLX优化的短分段参数（降低内存峰值）
        if self.device == "mps":
            x_pad = 1
            x_query = 6
            x_center = 10  # 10秒分段（vs 默认38秒）
            x_max = 13     # 超过13秒才分段（vs 默认41秒）

        if self.gpu_mem is not None and self.gpu_mem <= 4:
            x_pad = 1
            x_query = 5
            x_center = 30
            x_max = 32
        if self.dml:
            logger.info("Use DirectML instead")
            if (
                os.path.exists(
                    "runtime\Lib\site-packages\onnxruntime\capi\DirectML.dll"
                )
                == False
            ):
                try:
                    os.rename(
                        "runtime\Lib\site-packages\onnxruntime",
                        "runtime\Lib\site-packages\onnxruntime-cuda",
                    )
                except:
                    pass
                try:
                    os.rename(
                        "runtime\Lib\site-packages\onnxruntime-dml",
                        "runtime\Lib\site-packages\onnxruntime",
                    )
                except:
                    pass
            # if self.device != "cpu":
            import torch_directml

            self.device = torch_directml.device(torch_directml.default_device())
            self.is_half = False
        else:
            if self.instead:
                logger.info(f"Use {self.instead} instead")
            if (
                os.path.exists(
                    "runtime\Lib\site-packages\onnxruntime\capi\onnxruntime_providers_cuda.dll"
                )
                == False
            ):
                try:
                    os.rename(
                        "runtime\Lib\site-packages\onnxruntime",
                        "runtime\Lib\site-packages\onnxruntime-dml",
                    )
                except:
                    pass
                try:
                    os.rename(
                        "runtime\Lib\site-packages\onnxruntime-cuda",
                        "runtime\Lib\site-packages\onnxruntime",
                    )
                except:
                    pass
        logger.info(
            "Half-precision floating-point: %s, device: %s"
            % (self.is_half, self.device)
        )
        return x_pad, x_query, x_center, x_max
