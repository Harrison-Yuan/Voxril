import os, glob


def get_index_path_from_model(sid):
    weight_root = os.getenv("weight_root", "")
    if weight_root:
        matches = sorted(glob.glob(os.path.join(weight_root, 'added_*.index')), reverse=True)
        if matches:
            return matches[0]
    sid0 = sid.split(".")[0]
    index_root = os.getenv("index_root", "")
    matches = []
    for root, _, files in os.walk(index_root, topdown=False):
        for name in files:
            if name.endswith(".index") and "trained" not in name:
                full = os.path.join(root, name)
                if os.path.basename(root) == sid0:
                    matches.append(full)
    matches.sort(reverse=True)
    return matches[0] if matches else ""


def load_hubert(config):
    from infer.lib.hubert_mps import ONNXFeatureExtractor
    return ONNXFeatureExtractor(device=config.device)
