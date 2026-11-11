"""
Live Karaoke Worker — real-time voice conversion with accompaniment playback.
Audio flow: mic → VAD → (voice? RVC convert → mix acc : only acc) → output
Protocol: stdin f32 bytes in, stdout f32 bytes out, stderr for logs/metrics.
"""
import sys
import os

if len(sys.argv) < 7:
    print("Usage: python realtime_worker.py <model_path> <index_path> <f0_up_key> <hop_length> <sr> <accompaniment_path>", file=sys.stderr)
    sys.exit(1)

model_path = sys.argv[1]
index_path = sys.argv[2]
f0_up_key = int(sys.argv[3])
hop_length = int(sys.argv[4])
input_sr = int(sys.argv[5])
accompaniment_path = sys.argv[6] if len(sys.argv) > 6 and sys.argv[6] else ""
monitor_gain = float(sys.argv[7]) if len(sys.argv) > 7 else 0.3

sys.argv = [sys.argv[0]]

now_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.append(now_dir)

import numpy as np
import torch
import traceback
import time
import json
import logging
from collections import deque
logging.getLogger("numba").setLevel(logging.WARNING)
logging.getLogger("faiss").setLevel(logging.WARNING)
from infer.lib.rtrvc import RVC
from configs.config import Config


# ── VAD (energy-based, adaptive threshold, with hangover) ──
# 2026 best practice for Apple Silicon: simple RMS adaptive VAD.
# No ML overhead → minimal latency, no thread contention on M4's unified memory.
class AdaptiveVAD:
    def __init__(self, sr, threshold_ratio=2.5, hangover_frames=8, init_noise_frames=30):
        self.sr = sr
        self.threshold_ratio = threshold_ratio
        self.hangover_frames = hangover_frames
        self.voice_counter = 0
        self.silence_counter = 0
        self.noise_floor = 0.0
        self.noise_samples = deque(maxlen=100)
        self.init_frames_remaining = init_noise_frames
        self.warmup_done = False

    def _rms(self, audio):
        return float(np.sqrt(np.mean(np.square(audio)) + 1e-12))

    def is_voice(self, audio_chunk):
        rms = self._rms(audio_chunk)
        self.noise_samples.append(rms)

        if self.init_frames_remaining > 0:
            self.init_frames_remaining -= 1
            return False

        if not self.warmup_done:
            self.noise_floor = float(np.median(self.noise_samples)) if self.noise_samples else 0.0
            self.warmup_done = True

        self.noise_floor = self.noise_floor * 0.95 + rms * 0.05

        threshold = max(self.noise_floor * self.threshold_ratio, 0.001)
        voice_now = rms > threshold

        if voice_now:
            self.voice_counter += 1
            self.silence_counter = 0
        else:
            self.silence_counter += 1

        if self.silence_counter > self.hangover_frames:
            self.voice_counter = 0

        return self.voice_counter > 0


# ── Accompaniment player ──
class AccompanimentPlayer:
    def __init__(self, path, target_sr, target_channels=1):
        import soundfile as sf
        self.data = None
        self.position = 0
        self.loop = True

        if path and os.path.exists(path):
            audio, orig_sr = sf.read(path, dtype="float32")
            if audio.ndim > 1:
                audio = np.mean(audio, axis=1)
            if orig_sr != target_sr:
                import librosa
                audio = librosa.resample(audio, orig_sr=orig_sr, target_sr=target_sr)
            self.data = audio
            sys.stderr.write(f"[KARAOKE] 伴奏已加载: {path} ({len(audio)/target_sr:.1f}s)\n")
            sys.stderr.flush()
        elif path:
            sys.stderr.write(f"[KARAOKE] 伴奏文件不存在: {path}\n")
            sys.stderr.flush()

    def read_block(self, samples):
        if self.data is None or len(self.data) == 0:
            return np.zeros(samples, dtype=np.float32)
        end = self.position + samples
        if end <= len(self.data):
            chunk = self.data[self.position:end].copy()
            self.position = end
        elif self.loop:
            first_part = self.data[self.position:].copy()
            remaining = samples - len(first_part)
            wraps = remaining // len(self.data)
            last_part_len = remaining % len(self.data)
            if wraps > 0:
                from numpy import tile, concatenate
                chunk = concatenate([first_part] + [self.data.copy()] * wraps + [self.data[:last_part_len].copy()])
            else:
                chunk = np.concatenate([first_part, self.data[:remaining].copy()])
            self.position = last_part_len
        else:
            chunk = np.concatenate([self.data[self.position:].copy(), np.zeros(end - len(self.data), dtype=np.float32)])
            self.position = len(self.data)
        return chunk.astype(np.float32)


# ── Main worker ──
def emit_metrics(inference_ms, realtime_ratio, voice_active):
    sys.stderr.write(f"[METRIC] {json.dumps({'inference_ms': inference_ms, 'realtime_ratio': realtime_ratio, 'voice_active': voice_active})}\n")
    sys.stderr.flush()


def main():
    config = Config()
    device = config.device
    config.is_half = False

    effective_index_path = index_path if index_path and os.path.exists(index_path) else ""
    effective_index_rate = 0.3 if effective_index_path else 0

    rvc = RVC(
        key=f0_up_key,
        formant=0,
        pth_path=model_path,
        index_path=effective_index_path,
        index_rate=effective_index_rate,
        n_cpu=1,
        inp_q=None,
        opt_q=None,
        config=config,
    )

    target_sr = rvc.tgt_sr

    if rvc.if_f0 == 1:
        rvc.get_f0(torch.zeros(16000, device=device), f0_up_key, 1, method="rmvpe")
    warmup = torch.zeros(16000, device=device)
    for _ in range(3):
        _ = rvc.infer(warmup, block_frame_16k=warmup.shape[0], skip_head=0, return_length=warmup.shape[0], f0method="rmvpe")

    from torchaudio.transforms import Resample
    resampler_to_16k = Resample(orig_freq=input_sr, new_freq=16000).to(device)
    resampler_to_out = Resample(orig_freq=target_sr, new_freq=input_sr).to(device)

    block_size = hop_length * 32
    overlap = hop_length * 4
    fade_len = overlap // 2

    # ── VAD + Accompaniment ──
    vad = AdaptiveVAD(sr=input_sr, threshold_ratio=1.6, hangover_frames=4)
    accompaniment = AccompanimentPlayer(accompaniment_path, input_sr)
    has_accompaniment = accompaniment.data is not None

    # Mix gain: voice level in final output
    voice_gain = 0.85
    acc_gain = 0.7
    direct_monitor_gain = monitor_gain

    prev_output = None

    import collections
    recent_times = collections.deque(maxlen=10)
    frame_count = 0
    voice_active = False

    print("READY", flush=True)
    emit_metrics(0, 0, voice_active)

    while True:
        try:
            raw_bytes = sys.stdin.buffer.read(block_size * 4)
            if not raw_bytes or len(raw_bytes) < block_size * 4:
                break

            mic_chunk = np.frombuffer(raw_bytes, dtype=np.float32).copy()
            if len(mic_chunk) == 0:
                continue

            # ── VAD ──
            voice_active = vad.is_voice(mic_chunk)

            t0 = time.perf_counter()

            if voice_active:
                audio_t = torch.from_numpy(mic_chunk).to(device)
                audio_16k = resampler_to_16k(audio_t)

                out_audio = rvc.infer(
                    audio_16k,
                    block_frame_16k=audio_16k.shape[0],
                    skip_head=0,
                    return_length=audio_16k.shape[0],
                    f0method="rmvpe",
                )

                out_resampled = resampler_to_out(out_audio.float()).cpu().numpy()

                if prev_output is not None and len(prev_output) >= overlap and len(out_resampled) >= overlap:
                    combined = np.zeros(len(prev_output) + len(out_resampled) - overlap, dtype=np.float32)
                    combined[:len(prev_output) - fade_len] = prev_output[:len(prev_output) - fade_len]
                    for i in range(fade_len):
                        t = i / fade_len
                        combined[len(prev_output) - fade_len + i] = (
                            prev_output[len(prev_output) - fade_len + i] * (1 - t)
                            + out_resampled[i] * t
                        )
                    combined[len(prev_output):] = out_resampled[overlap:]
                    voice_out = combined
                else:
                    voice_out = out_resampled

                prev_output = out_resampled
            else:
                voice_out = np.zeros(block_size, dtype=np.float32)
                prev_output = np.zeros(1, dtype=np.float32)

            elapsed = time.perf_counter() - t0
            recent_times.append(elapsed)
            frame_count += 1

            # ── Mix with accompaniment ──
            if has_accompaniment:
                acc_chunk = accompaniment.read_block(block_size)
                if voice_active:
                    output_chunk = voice_out[:block_size] * voice_gain + acc_chunk * acc_gain
                else:
                    output_chunk = acc_chunk * acc_gain + mic_chunk * direct_monitor_gain
                max_val = np.abs(output_chunk).max()
                if max_val > 1.0:
                    output_chunk = output_chunk / max_val * 0.99
            elif voice_active:
                output_chunk = voice_out[:block_size]
            else:
                output_chunk = mic_chunk * direct_monitor_gain

            sys.stdout.buffer.write(output_chunk.astype(np.float32).tobytes())
            sys.stdout.buffer.flush()

            if frame_count % 10 == 0:
                avg_ms = sum(recent_times) / max(len(recent_times), 1) * 1000
                block_duration_ms = block_size / input_sr * 1000
                realtime_ratio = avg_ms / block_duration_ms if block_duration_ms > 0 else 0
                emit_metrics(round(avg_ms, 1), round(realtime_ratio, 2), voice_active)

        except Exception as e:
            sys.stderr.write(f"Error in worker: {traceback.format_exc()}\n")
            sys.stderr.flush()
            break


if __name__ == "__main__":
    main()
