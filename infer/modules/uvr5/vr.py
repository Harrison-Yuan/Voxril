import os
import logging

logging.basicConfig(level=logging.INFO, format='%(levelname)s - %(message)s')
logger = logging.getLogger(__name__)

import librosa
import numpy as np
import soundfile as sf
import torch

from infer.lib.uvr5_pack.lib_v5 import nets_61968KB as Nets
from infer.lib.uvr5_pack.lib_v5 import spec_utils
from infer.lib.uvr5_pack.lib_v5.model_param_init import ModelParameters
from infer.lib.uvr5_pack.lib_v5.nets_new import CascadedNet
from infer.lib.uvr5_pack.utils import inference


AUDIO_SCALE_FACTOR = 32767
AUDIO_CLIP_MIN = -1.0
AUDIO_CLIP_MAX = 1.0


def _normalize_audio_for_int16(wav_data):
    """
    Normalize float audio data to int16 range, preventing clipping distortion.
    
    Args:
        wav_data: Audio data as numpy array in float format (typically -1.0 to 1.0)
    
    Returns:
        Audio data as int16 numpy array
    """
    wav_array = np.array(wav_data)
    wav_clipped = np.clip(wav_array, AUDIO_CLIP_MIN, AUDIO_CLIP_MAX)
    return (wav_clipped * AUDIO_SCALE_FACTOR).astype("int16")


def _save_audio_file(wav_data, output_dir, filename_prefix, base_name, agg, format, sample_rate):
    filename = f"{filename_prefix}{base_name}_{agg}.{format}"
    output_path = os.path.join(output_dir, filename)
    
    wav_array = np.array(wav_data, dtype=np.float64)
    
    logger.info(f"[8] Audio shape before processing: {wav_array.shape}, dtype={wav_array.dtype}, range=[{wav_array.min():.6f}, {wav_array.max():.6f}]")
    
    if wav_array.ndim == 2:
        if wav_array.shape[0] == 2 and wav_array.shape[1] > 2:
            wav_array = wav_array.T
            logger.info(f"[8] Transposed audio from (2, {wav_data.shape[1]}) to {wav_array.shape}")
    
    peak = float(np.max(np.abs(wav_array)))
    if peak > 1e-8:
        wav_array = wav_array * (0.95 / peak)
        logger.info(f"[8] Peak normalized: peak={peak:.6f} → target=0.95")
    else:
        logger.info(f"[8] Skipped normalization: peak={peak:.6e} too low")
    
    wav_clipped = np.clip(wav_array, AUDIO_CLIP_MIN, AUDIO_CLIP_MAX)
    normalized_audio = (wav_clipped * AUDIO_SCALE_FACTOR).astype("int16")
    
    logger.info(f"[8] Audio after normalization: shape={normalized_audio.shape}, dtype={normalized_audio.dtype}, range=[{normalized_audio.min()}, {normalized_audio.max()}]")
    
    if format in ["wav", "flac"]:
        sf.write(output_path, normalized_audio, sample_rate)
    else:
        temp_wav_path = os.path.join(output_dir, f"{filename_prefix}{base_name}_{agg}.wav")
        sf.write(temp_wav_path, normalized_audio, sample_rate)
        
        if os.path.exists(temp_wav_path):
            final_path = temp_wav_path[:-4] + f".{format}"
            os.system(f'ffmpeg -i "{temp_wav_path}" -vn "{final_path}" -q:a 2 -y')
            
            if os.path.exists(final_path):
                try:
                    os.remove(temp_wav_path)
                except Exception:
                    pass


def _process_spectrogram_to_wave(spec_m, mp, high_end_process, input_high_end_h=None, input_high_end=None):
    """
    Convert spectrogram back to waveform with optional high-end processing.
    
    Args:
        spec_m: Spectrogram data
        mp: Model parameters
        high_end_process: High-end processing method
        input_high_end_h: High-end height parameter
        input_high_end: High-end input data
    
    Returns:
        Waveform audio data
    """
    logger.info(f"[7] Input spectrogram: shape={spec_m.shape}, dtype={spec_m.dtype}")
    
    if high_end_process.startswith("mirroring") and input_high_end is not None:
        input_high_end_ = spec_utils.mirroring(high_end_process, spec_m, input_high_end, mp)
        wave = spec_utils.cmb_spectrogram_to_wave(spec_m, mp, input_high_end_h, input_high_end_)
    else:
        wave = spec_utils.cmb_spectrogram_to_wave(spec_m, mp)
    
    logger.info(f"[7] Wave from spectrogram: shape={wave.shape}, dtype={wave.dtype}, range=[{wave.min():.6f}, {wave.max():.6f}]")
    
    return wave


class AudioProcessorBase:
    """
    Base class for audio separation processors.
    Provides unified interface and common processing logic.
    """
    
    def __init__(self, agg, model_path, device, is_half, tta=False):
        self.model_path = model_path
        self.device = device
        self.data = {
            "postprocess": False,
            "tta": tta,
            "window_size": 512,
            "agg": agg,
            "high_end_process": "mirroring",
        }
        self.mp = None
        self.model = None
        self._initialize_model(model_path, device, is_half)
    
    def _initialize_model(self, model_path, device, is_half):
        """Override in subclass to initialize specific model"""
        raise NotImplementedError("Subclass must implement _initialize_model")
    
    def _get_spectrum_mapping(self, y_spec_m, v_spec_m):
        """
        Map spectrograms to instrumental and vocal outputs.
        
        For HP2 and HP5 models, the model predicts INSTRUMENTALS:
        - y_spec_m (predicted) = instrumental
        - v_spec_m (residual) = vocal
        
        For HP3 model, the model predicts VOCALS instead — the is_hp3
        flag in _path_audio_ handles the swap.
        
        Returns:
            Tuple of (instrumental_spectrum, vocal_spectrum)
        """
        return y_spec_m, v_spec_m
    
    def _path_audio_(
        self, music_file, ins_root=None, vocal_root=None, format="flac", is_hp3=False
    ):
        """
        Unified audio processing interface.
        All subclasses use the same parameter order: (music_file, ins_root, vocal_root, format, is_hp3)
        """
        if ins_root is None and vocal_root is None:
            return "No save root."
        
        name = os.path.basename(music_file)
        if ins_root is not None:
            os.makedirs(ins_root, exist_ok=True)
        if vocal_root is not None:
            os.makedirs(vocal_root, exist_ok=True)
        
        X_wave, X_spec_s = {}, {}
        bands_n = len(self.mp.param["band"])
        input_high_end_h = None
        input_high_end = None
        
        for d in range(bands_n, 0, -1):
            bp = self.mp.param["band"][d]
            if d == bands_n:
                X_wave[d], _ = librosa.load(
                    music_file,
                    sr=bp["sr"],
                    mono=False,
                    dtype=np.float32,
                    res_type=bp["res_type"],
                )
                if X_wave[d].ndim == 1:
                    X_wave[d] = np.asfortranarray([X_wave[d], X_wave[d]])
            else:
                X_wave[d] = librosa.resample(
                    X_wave[d + 1],
                    orig_sr=self.mp.param["band"][d + 1]["sr"],
                    target_sr=bp["sr"],
                    res_type=bp["res_type"],
                )
            
            X_spec_s[d] = spec_utils.wave_to_spectrogram_mt(
                X_wave[d],
                bp["hl"],
                bp["n_fft"],
                self.mp.param["mid_side"],
                self.mp.param["mid_side_b2"],
                self.mp.param["reverse"],
            )
            
            if d == bands_n and self.data["high_end_process"] != "none":
                input_high_end_h = (bp["n_fft"] // 2 - bp["crop_stop"]) + (
                    self.mp.param["pre_filter_stop"] - self.mp.param["pre_filter_start"]
                )
                input_high_end = X_spec_s[d][
                    :, bp["n_fft"] // 2 - input_high_end_h : bp["n_fft"] // 2, :
                ]
        
        X_spec_m = spec_utils.combine_spectrograms(X_spec_s, self.mp)
        logger.info(f"[4] Combined spectrogram: shape={X_spec_m.shape}, dtype={X_spec_m.dtype}")
        
        aggresive_set = float(self.data["agg"] / 100)
        aggressiveness = {
            "value": aggresive_set,
            "split_bin": self.mp.param["band"][1]["crop_stop"],
        }
        
        with torch.no_grad():
            pred, X_mag, X_phase = inference(
                X_spec_m, self.device, self.model, aggressiveness, self.data
            )
        
        logger.info(f"[5] Model inference: pred_range=[{pred.min():.6f}, {pred.max():.6f}], X_mag_range=[{X_mag.min():.6f}, {X_mag.max():.6f}]")
        
        if self.data["postprocess"]:
            pred_inv = np.clip(X_mag - pred, 0, np.inf)
            pred = spec_utils.mask_silence(pred, pred_inv)
        
        y_spec_m = pred * X_phase
        v_spec_m = X_spec_m - y_spec_m
        
        logger.info(f"[6] Spectrum separation: y_spec_range=[{np.abs(y_spec_m).min():.6f}, {np.abs(y_spec_m).max():.6f}], v_spec_range=[{np.abs(v_spec_m).min():.6f}, {np.abs(v_spec_m).max():.6f}]")
        
        ins_spec, vocal_spec = self._get_spectrum_mapping(y_spec_m, v_spec_m)
        
        # HP3 model predicts vocals, not instrumentals — swap the mapping
        if is_hp3:
            ins_spec, vocal_spec = vocal_spec, ins_spec
        
        if ins_root is not None:
            wav_instrument = _process_spectrogram_to_wave(
                ins_spec, self.mp, self.data["high_end_process"], 
                input_high_end_h, input_high_end
            )
            logger.info("%s instruments done" % name)
            _save_audio_file(
                wav_instrument, ins_root, "instrument_", 
                name, self.data["agg"], format, self.mp.param["sr"]
            )
        
        if vocal_root is not None:
            wav_vocals = _process_spectrogram_to_wave(
                vocal_spec, self.mp, self.data["high_end_process"],
                input_high_end_h, input_high_end
            )
            logger.info("%s vocals done" % name)
            _save_audio_file(
                wav_vocals, vocal_root, "vocal_",
                name, self.data["agg"], format, self.mp.param["sr"]
            )


class AudioPre(AudioProcessorBase):
    """
    Standard HP audio separation processor (HP2, HP5).
    Model predicts the instrumental mask, so:
    - y_spec_m (predicted) = instrumental
    - v_spec_m (residual) = vocal
    
    HP3 is handled via the is_hp3 flag in _path_audio_ (model predicts vocal instead).
    """
    
    def _initialize_model(self, model_path, device, is_half):
        self.mp = ModelParameters("infer/lib/uvr5_pack/lib_v5/modelparams/4band_v2.json")
        model = Nets.CascadedASPPNet(self.mp.param["bins"] * 2)
        cpk = torch.load(model_path, map_location=device)
        model.load_state_dict(cpk)
        model.eval()
        if is_half:
            model = model.half()
        self.model = model.to(device)
        logger.info(f"[2] Model loaded: dtype={list(model.state_dict().values())[0].dtype}, device={device}, is_half={is_half}")


class AudioPreDeEcho(AudioProcessorBase):
    """
    DeEcho/DeReverb audio separation processor.
    Model predicts the clean (de-echoed) vocal signal, so:
    - y_spec_m (predicted) = vocal
    - v_spec_m (residual) = instrumental (contains echoes)
    
    _get_spectrum_mapping swaps to return (v_spec_m, y_spec_m) → (instrumental, vocal).
    """
    
    def _initialize_model(self, model_path, device, is_half):
        self.mp = ModelParameters("infer/lib/uvr5_pack/lib_v5/modelparams/4band_v3.json")
        nout = 64 if "DeReverb" in model_path else 48
        model = CascadedNet(self.mp.param["bins"] * 2, nout)
        cpk = torch.load(model_path, map_location=device)
        model.load_state_dict(cpk)
        model.eval()
        if is_half:
            model = model.half()
        self.model = model.to(device)
        logger.info(f"[2] DeEcho model loaded: dtype={list(model.state_dict().values())[0].dtype}, device={device}, is_half={is_half}")
    
    def _get_spectrum_mapping(self, y_spec_m, v_spec_m):
        """
        Override spectrum mapping for DeEcho/DeReverb models.
        These models predict the clean (de-echoed) signal, which is the vocal:
        - y_spec_m (predicted) = vocal
        - v_spec_m (residual) = instrumental (contains echoes)
        So we swap compared to the base HP behavior.
        
        Returns:
            Tuple of (instrumental_spectrum, vocal_spectrum)
        """
        return v_spec_m, y_spec_m
