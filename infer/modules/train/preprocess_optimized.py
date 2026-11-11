import multiprocessing
import os
import sys
from multiprocessing import Pool

from scipy import signal

now_dir = os.getcwd()
sys.path.append(now_dir)

import traceback
import numpy as np
from scipy.io import wavfile

try:
    import soxr
    USE_SOXR = True
except ImportError:
    import librosa
    USE_SOXR = False

try:
    import soundfile as sf
    USE_SOUNDFILE = True
except ImportError:
    from infer.lib.audio import load_audio
    USE_SOUNDFILE = False

from infer.lib.slicer2 import Slicer

f = None


def _pipeline_worker(args, sr, exp_dir, per, use_soxr, use_soundfile):
    """Module-level function for multiprocessing imap_unordered.
    Pool.imap_unordered requires a picklable callable with one positional arg.
    """
    path, idx0 = args
    from infer.modules.train.preprocess_optimized import PreProcessOptimized
    pp = PreProcessOptimized(sr, exp_dir, per)
    pp.use_soxr = use_soxr
    pp.use_soundfile = use_soundfile
    return pp.pipeline(path, idx0)


def println(strr):
    print(strr)
    if f is not None:
        f.write("%s\n" % strr)
        f.flush()


class PreProcessOptimized:
    def __init__(self, sr, exp_dir, per=3.7):
        self.slicer = Slicer(
            sr=sr,
            threshold=-42,
            min_length=1500,
            min_interval=400,
            hop_size=15,
            max_sil_kept=500,
        )
        self.sr = sr
        self.bh, self.ah = signal.butter(N=5, Wn=48, btype="high", fs=self.sr)
        self.per = per
        self.overlap = 0.3
        self.tail = self.per + self.overlap
        self.max = 0.9
        self.alpha = 0.75
        self.exp_dir = exp_dir
        self.gt_wavs_dir = f"{exp_dir}/0_gt_wavs"
        self.wavs16k_dir = f"{exp_dir}/1_16k_wavs"

        os.makedirs(self.exp_dir, exist_ok=True)
        os.makedirs(self.gt_wavs_dir, exist_ok=True)
        os.makedirs(self.wavs16k_dir, exist_ok=True)

        self.use_soxr = USE_SOXR
        self.use_soundfile = USE_SOUNDFILE

    def load_audio_fast(self, path):
        if self.use_soundfile:
            try:
                audio, orig_sr = sf.read(path, dtype='float32')
                
                if len(audio.shape) > 1:
                    audio = audio.mean(axis=1)
                
                if orig_sr != self.sr:
                    if self.use_soxr:
                        audio = soxr.resample(audio, orig_sr, self.sr, quality='VHQ')
                    else:
                        audio = librosa.resample(audio, orig_sr=orig_sr, target_sr=self.sr)
                
                return audio
            except Exception as e:
                println(f"soundfile failed for {path}, falling back: {e}")
        
        from infer.lib.audio import load_audio
        return load_audio(path, self.sr)

    def resample_fast(self, audio):
        if self.use_soxr:
            return soxr.resample(audio, self.sr, 16000, quality='VHQ')
        else:
            return librosa.resample(audio, orig_sr=self.sr, target_sr=16000)

    def norm_write(self, tmp_audio, idx0, idx1):
        tmp_max = np.abs(tmp_audio).max()
        if tmp_max > 2.5 or tmp_max < 1e-6:
            return False

        # Applio 风格混合归一化：避免把每个切片都硬拉到 0.95，降低齿音、削波和噪声被放大的风险。
        tmp_audio = (tmp_audio / tmp_max * (self.max * self.alpha)) + (1 - self.alpha) * tmp_audio
        tmp_audio = np.clip(tmp_audio, -0.99, 0.99)
        
        wavfile.write(
            f"{self.gt_wavs_dir}/{idx0}_{idx1}.wav",
            self.sr,
            tmp_audio.astype(np.float32),
        )
        
        tmp_audio_16k = self.resample_fast(tmp_audio)
        wavfile.write(
            f"{self.wavs16k_dir}/{idx0}_{idx1}.wav",
            16000,
            tmp_audio_16k.astype(np.float32),
        )
        
        return True

    def pipeline(self, path, idx0):
        try:
            audio = self.load_audio_fast(path)
            audio = signal.filtfilt(self.bh, self.ah, audio)

            idx1 = 0
            for audio_slice in self.slicer.slice(audio):
                i = 0
                while True:
                    start = int(self.sr * (self.per - self.overlap) * i)
                    i += 1
                    if len(audio_slice[start:]) > self.tail * self.sr:
                        tmp_audio = audio_slice[start : start + int(self.per * self.sr)]
                        if self.norm_write(tmp_audio, idx0, idx1):
                            idx1 += 1
                    else:
                        tmp_audio = audio_slice[start:]
                        if self.norm_write(tmp_audio, idx0, idx1):
                            idx1 += 1
                        break
            
            return f"{path}\t-> Success"
        except Exception as e:
            return f"{path}\t-> Error: {traceback.format_exc()}"

    def pipeline_mp(self, infos):
        for path, idx0 in infos:
            result = self.pipeline(path, idx0)
            println(result)

    def pipeline_mp_inp_dir(self, inp_root, n_p, noparallel=False):
        try:
            valid_extensions = {'.wav', '.mp3', '.flac', '.ogg', '.m4a', '.aac', '.wma'}
            all_files = sorted(list(os.listdir(inp_root)))
            audio_files = [
                name for name in all_files
                if os.path.splitext(name.lower())[1] in valid_extensions
                and not name.startswith('.')
            ]
            
            total = len(audio_files)

            println(f"Found {total} audio files")
            println(f"Using {n_p} processes")
            println(f"Optimization: soxr={'enabled' if self.use_soxr else 'disabled'}, soundfile={'enabled' if self.use_soundfile else 'disabled'}")

            if total == 0:
                println("Warning: no audio files found in input directory")
                return

            infos = [
                (f"{inp_root}/{name}", idx)
                for idx, name in enumerate(audio_files)
            ]

            if noparallel:
                for i in range(n_p):
                    self.pipeline_mp(infos[i::n_p])
            else:
                from functools import partial
                worker = partial(
                    _pipeline_worker,
                    sr=self.sr, exp_dir=self.exp_dir, per=self.per,
                    use_soxr=self.use_soxr, use_soundfile=self.use_soundfile,
                )
                done = 0
                with Pool(processes=n_p) as pool:
                    for result in pool.imap_unordered(worker, infos):
                        done += 1
                        if done % max(1, total // 20) == 0 or done == total:
                            println(f"Progress: {done}/{total} ({100*done//total}%)")
                        if "Error" in result:
                            println(result)
                println(f"Done: {total}/{total} files processed")

        except Exception as e:
            println(f"Fail. {traceback.format_exc()}")


def preprocess_trainset(inp_root, sr, n_p, exp_dir, per, noparallel=False):
    # CPU 核心数一半作为上限，防止把系统 CPU 全部占满导致 UI 卡死
    cpu_limit = max(1, multiprocessing.cpu_count() // 2)
    n_p = min(n_p, cpu_limit)
    pp = PreProcessOptimized(sr, exp_dir, per)
    println("start preprocess (optimized version)")
    pp.pipeline_mp_inp_dir(inp_root, n_p, noparallel)
    println("end preprocess")


def main():
    global f
    
    print(*sys.argv[1:])
    inp_root = sys.argv[1]
    sr = int(sys.argv[2])
    n_p = int(sys.argv[3])
    exp_dir = sys.argv[4]
    noparallel = sys.argv[5] == "True"
    per = float(sys.argv[6])
    
    f = open("%s/preprocess.log" % exp_dir, "a+")
    
    if USE_SOXR:
        println("Using soxr for fast resampling")
    else:
        println("soxr not found, falling back to librosa (slower)")
    
    if USE_SOUNDFILE:
        println("Using soundfile for fast audio loading")
    else:
        println("soundfile not found, using default audio loader")
    
    preprocess_trainset(inp_root, sr, n_p, exp_dir, per, noparallel)
    
    f.close()


if __name__ == "__main__":
    multiprocessing.freeze_support()
    main()
