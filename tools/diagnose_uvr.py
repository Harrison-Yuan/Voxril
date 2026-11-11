#!/usr/bin/env python3
"""
UVR音频分离诊断工具
检测硬件能力并诊断音频质量问题
"""
import os
import sys
import json
import torch
import numpy as np
import soundfile as sf
from pathlib import Path

def check_hardware():
    """检测硬件配置和精度支持"""
    info = {
        "device": "cpu",
        "device_name": "CPU",
        "supports_fp16": False,
        "supports_fp32": True,
        "gpu_memory_gb": 0,
        "recommended_precision": "fp32",
        "warnings": []
    }
    
    if torch.cuda.is_available():
        device_id = 0
        info["device"] = f"cuda:{device_id}"
        info["device_name"] = torch.cuda.get_device_name(device_id)
        
        gpu_mem_bytes = torch.cuda.get_device_properties(device_id).total_memory
        info["gpu_memory_gb"] = round(gpu_mem_bytes / (1024**3), 2)
        
        gpu_name_upper = info["device_name"].upper()
        
        fp16_blacklist = [
            "GTX 16", "GTX 1060", "GTX 1070", "GTX 1080",
            "P40", "P10", "V100"
        ]
        
        is_blacklisted = any(name in gpu_name_upper for name in fp16_blacklist)
        
        if is_blacklisted:
            info["supports_fp16"] = False
            info["recommended_precision"] = "fp32"
            info["warnings"].append(
                f"GPU {info['device_name']} 不建议使用fp16，可能导致音频噪音"
            )
        else:
            try:
                test_tensor = torch.randn(100, 100).half().cuda()
                _ = test_tensor @ test_tensor
                info["supports_fp16"] = True
                info["recommended_precision"] = "fp16"
                del test_tensor
                torch.cuda.empty_cache()
            except Exception as e:
                info["supports_fp16"] = False
                info["recommended_precision"] = "fp32"
                info["warnings"].append(f"fp16测试失败: {str(e)}")
        
        if info["gpu_memory_gb"] <= 4:
            info["warnings"].append(
                f"显存较小({info['gpu_memory_gb']}GB)，建议使用较低的agg参数"
            )
    
    elif hasattr(torch.backends, 'mps') and torch.backends.mps.is_available():
        info["device"] = "mps"
        info["device_name"] = "Apple Silicon (MPS)"
        info["supports_fp16"] = False
        info["recommended_precision"] = "fp32"
        info["warnings"].append("MPS设备必须使用fp32")
    
    else:
        info["warnings"].append("未检测到GPU，使用CPU处理会很慢")
    
    return info

def analyze_audio_file(audio_path):
    """分析音频文件质量"""
    try:
        data, sr = sf.read(audio_path, always_2d=True)
        
        analysis = {
            "file": os.path.basename(audio_path),
            "sample_rate": sr,
            "channels": data.shape[1] if data.ndim > 1 else 1,
            "duration_sec": round(len(data) / sr, 2),
            "file_size_mb": round(os.path.getsize(audio_path) / (1024**2), 2),
            "peak_amplitude": float(np.max(np.abs(data))),
            "rms_level": float(np.sqrt(np.mean(np.square(data)))),
            "dynamic_range_db": 0,
            "has_clipping": False,
            "is_silent": False,
            "quality_issues": []
        }
        
        if analysis["peak_amplitude"] > 0:
            analysis["dynamic_range_db"] = round(
                20 * np.log10(analysis["peak_amplitude"] / (analysis["rms_level"] + 1e-10)), 
                2
            )
        
        if analysis["peak_amplitude"] >= 0.99:
            analysis["has_clipping"] = True
            analysis["quality_issues"].append(
                f"检测到削波失真(peak={analysis['peak_amplitude']:.4f})，这会产生沙沙声"
            )
        
        if analysis["rms_level"] < 0.001:
            analysis["is_silent"] = True
            analysis["quality_issues"].append("音频几乎静音，可能分离失败")
        
        if analysis["sample_rate"] != 44100:
            analysis["quality_issues"].append(
                f"采样率{sr}Hz不是标准的44100Hz，可能需要重采样"
            )
        
        if analysis["channels"] != 2:
            analysis["quality_issues"].append(
                f"声道数{analysis['channels']}不是标准的立体声(2)，可能影响分离质量"
            )
        
        noise_estimate = np.std(data[:min(4410, len(data))])
        if noise_estimate > 0.05:
            analysis["quality_issues"].append(
                f"检测到高噪音水平(std={noise_estimate:.4f})，可能是分离参数不当"
            )
        
        return analysis
    
    except Exception as e:
        return {
            "file": os.path.basename(audio_path),
            "error": str(e),
            "quality_issues": [f"无法读取文件: {str(e)}"]
        }

def diagnose_separation_output(vocal_dir, instrumental_dir):
    """诊断分离输出的质量"""
    results = {
        "vocals": [],
        "instrumentals": [],
        "summary": {
            "total_files": 0,
            "files_with_issues": 0,
            "common_issues": []
        }
    }
    
    issue_counter = {}
    
    if os.path.exists(vocal_dir):
        for filename in os.listdir(vocal_dir):
            if filename.endswith(('.wav', '.flac', '.mp3')) and not filename.startswith('.'):
                filepath = os.path.join(vocal_dir, filename)
                analysis = analyze_audio_file(filepath)
                results["vocals"].append(analysis)
                results["summary"]["total_files"] += 1
                
                if analysis.get("quality_issues"):
                    results["summary"]["files_with_issues"] += 1
                    for issue in analysis["quality_issues"]:
                        issue_key = issue.split('(')[0].strip()
                        issue_counter[issue_key] = issue_counter.get(issue_key, 0) + 1
    
    if os.path.exists(instrumental_dir):
        for filename in os.listdir(instrumental_dir):
            if filename.endswith(('.wav', '.flac', '.mp3')) and not filename.startswith('.'):
                filepath = os.path.join(instrumental_dir, filename)
                analysis = analyze_audio_file(filepath)
                results["instrumentals"].append(analysis)
                results["summary"]["total_files"] += 1
                
                if analysis.get("quality_issues"):
                    results["summary"]["files_with_issues"] += 1
                    for issue in analysis["quality_issues"]:
                        issue_key = issue.split('(')[0].strip()
                        issue_counter[issue_key] = issue_counter.get(issue_key, 0) + 1
    
    results["summary"]["common_issues"] = [
        f"{issue} (出现{count}次)" 
        for issue, count in sorted(issue_counter.items(), key=lambda x: -x[1])
    ]
    
    return results

def generate_recommendations(hardware_info, diagnosis_results):
    """生成修复建议"""
    recommendations = []
    
    if not hardware_info["supports_fp16"] and hardware_info["recommended_precision"] == "fp32":
        recommendations.append({
            "priority": "HIGH",
            "issue": "硬件不支持fp16或fp16不稳定",
            "solution": "在configs/config.py中强制使用fp32",
            "action": "确保Config类的device_config()方法正确设置self.is_half=False"
        })
    
    has_clipping = any(
        "削波失真" in issue 
        for result in diagnosis_results.get("vocals", []) + diagnosis_results.get("instrumentals", [])
        for issue in result.get("quality_issues", [])
    )
    
    if has_clipping:
        recommendations.append({
            "priority": "CRITICAL",
            "issue": "输出音频存在削波失真(沙沙声的主要原因)",
            "solution": "修复vr.py中的音频保存代码",
            "action": "将 (np.array(wav) * 32768).astype('int16') 改为 np.clip(wav, -1, 1) * 32767"
        })
    
    has_noise = any(
        "高噪音水平" in issue
        for result in diagnosis_results.get("vocals", []) + diagnosis_results.get("instrumentals", [])
        for issue in result.get("quality_issues", [])
    )
    
    if has_noise:
        recommendations.append({
            "priority": "HIGH",
            "issue": "输出音频噪音水平过高",
            "solution": "降低agg(aggressiveness)参数",
            "action": "将agg从默认的10降低到5-8，或使用不同的UVR模型"
        })
    
    if hardware_info["gpu_memory_gb"] > 0 and hardware_info["gpu_memory_gb"] <= 4:
        recommendations.append({
            "priority": "MEDIUM",
            "issue": f"显存较小({hardware_info['gpu_memory_gb']}GB)",
            "solution": "使用较保守的处理参数",
            "action": "降低batch size或使用CPU处理"
        })
    
    return recommendations

def main():
    print("=" * 60)
    print("UVR音频分离诊断工具")
    print("=" * 60)
    
    print("\n[1/3] 检测硬件配置...")
    hardware_info = check_hardware()
    
    print(f"\n设备: {hardware_info['device_name']}")
    print(f"设备类型: {hardware_info['device']}")
    print(f"支持fp16: {hardware_info['supports_fp16']}")
    print(f"支持fp32: {hardware_info['supports_fp32']}")
    print(f"推荐精度: {hardware_info['recommended_precision']}")
    
    if hardware_info['gpu_memory_gb'] > 0:
        print(f"显存: {hardware_info['gpu_memory_gb']} GB")
    
    if hardware_info['warnings']:
        print("\n⚠️  硬件警告:")
        for warning in hardware_info['warnings']:
            print(f"  - {warning}")
    
    if len(sys.argv) >= 3:
        vocal_dir = sys.argv[1]
        instrumental_dir = sys.argv[2]
        
        print(f"\n[2/3] 分析输出音频质量...")
        print(f"人声目录: {vocal_dir}")
        print(f"伴奏目录: {instrumental_dir}")
        
        diagnosis = diagnose_separation_output(vocal_dir, instrumental_dir)
        
        print(f"\n总文件数: {diagnosis['summary']['total_files']}")
        print(f"有问题的文件: {diagnosis['summary']['files_with_issues']}")
        
        if diagnosis['summary']['common_issues']:
            print("\n常见问题:")
            for issue in diagnosis['summary']['common_issues']:
                print(f"  - {issue}")
        
        print("\n详细分析:")
        for vocal in diagnosis['vocals']:
            print(f"\n  人声: {vocal['file']}")
            print(f"    采样率: {vocal.get('sample_rate', 'N/A')} Hz")
            print(f"    峰值: {vocal.get('peak_amplitude', 0):.4f}")
            print(f"    RMS: {vocal.get('rms_level', 0):.4f}")
            if vocal.get('quality_issues'):
                print(f"    问题:")
                for issue in vocal['quality_issues']:
                    print(f"      - {issue}")
        
        for inst in diagnosis['instrumentals']:
            print(f"\n  伴奏: {inst['file']}")
            print(f"    采样率: {inst.get('sample_rate', 'N/A')} Hz")
            print(f"    峰值: {inst.get('peak_amplitude', 0):.4f}")
            print(f"    RMS: {inst.get('rms_level', 0):.4f}")
            if inst.get('quality_issues'):
                print(f"    问题:")
                for issue in inst['quality_issues']:
                    print(f"      - {issue}")
        
        print("\n[3/3] 生成修复建议...")
        recommendations = generate_recommendations(hardware_info, diagnosis)
        
        if recommendations:
            print("\n修复建议:")
            for i, rec in enumerate(recommendations, 1):
                print(f"\n{i}. [{rec['priority']}] {rec['issue']}")
                print(f"   解决方案: {rec['solution']}")
                print(f"   操作: {rec['action']}")
        else:
            print("\n✓ 未发现明显问题")
        
        report_path = "uvr_diagnosis_report.json"
        with open(report_path, 'w', encoding='utf-8') as f:
            json.dump({
                "hardware": hardware_info,
                "diagnosis": diagnosis,
                "recommendations": recommendations
            }, f, indent=2, ensure_ascii=False)
        print(f"\n完整报告已保存到: {report_path}")
    
    else:
        print("\n[2/3] 跳过音频分析(未提供目录)")
        print("\n使用方法:")
        print(f"  python {sys.argv[0]} <人声目录> <伴奏目录>")
        print("\n示例:")
        print(f"  python {sys.argv[0]} ./separated/vocals ./separated/instrumental")
    
    print("\n" + "=" * 60)

if __name__ == "__main__":
    main()
