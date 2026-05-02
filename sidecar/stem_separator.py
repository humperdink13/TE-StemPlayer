#!/usr/bin/env python3
"""
Stem separator using Demucs v4 (HTDemucs) for 4-stem separation.
Communicates via JSON-line protocol over stdin/stdout for Tauri sidecar.
Outputs: vocals.wav, drums.wav, bass.wav, other.wav
"""

import json
import os
import subprocess
import sys
import time
from pathlib import Path
from typing import Optional, Dict, Any


# ---------------------------------------------------------------------------
# Device detection
# ---------------------------------------------------------------------------
def get_device() -> str:
    """Auto-detect the best available torch device."""
    try:
        import torch
        if torch.cuda.is_available():
            return "cuda"
        if hasattr(torch.backends, "mps") and torch.backends.mps.is_available():
            return "mps"
    except ImportError:
        pass
    return "cpu"


# ---------------------------------------------------------------------------
# FFmpeg helpers
# ---------------------------------------------------------------------------
def run_ffmpeg(cmd: list, description: str) -> None:
    """Run an ffmpeg command, raising on failure."""
    result = subprocess.run(
        cmd,
        capture_output=True,
        text=True,
        timeout=600,
    )
    if result.returncode != 0:
        err = result.stderr.strip()[-500:] if result.stderr else "unknown error"
        raise RuntimeError(f"FFmpeg {description} failed: {err}")


def normalize_audio(input_path: str, output_path: str) -> None:
    """Convert input audio to 44.1kHz stereo 16-bit WAV for Demucs."""
    run_ffmpeg([
        "ffmpeg",
        "-i", input_path,
        "-ar", "44100",
        "-ac", "2",
        "-sample_fmt", "s16",
        "-y",
        output_path,
    ], "normalization")


def resample_stem(input_path: str, output_path: str) -> None:
    """Resample a separated stem to 48kHz 32-bit int stereo WAV."""
    run_ffmpeg([
        "ffmpeg",
        "-i", input_path,
        "-ar", "48000",
        "-ac", "2",
        "-sample_fmt", "s32",
        "-y",
        output_path,
    ], "resampling")


# ---------------------------------------------------------------------------
# Demucs separation
# ---------------------------------------------------------------------------
def run_demucs_separation(
    input_wav: str,
    output_dir: str,
    model: str = "htdemucs",
    device: str = "auto",
) -> Dict[str, str]:
    """
    Run Demucs separation and return a dict mapping stem name -> WAV path.
    """
    input_path = Path(input_wav)
    input_name = input_path.stem

    cmd = [
        sys.executable, "-m", "demucs",
        "-n", model,
        "-o", output_dir,
        "--device", device,
        str(input_path),
    ]

    result = subprocess.run(cmd, capture_output=True, text=True)

    if result.returncode != 0:
        err = result.stderr.strip()[-800:] if result.stderr else "Demucs failed"
        raise RuntimeError(f"Demucs separation error: {err}")

    stem_dir = Path(output_dir) / model / input_name
    if not stem_dir.exists():
        stem_dir = Path(output_dir) / input_name

    stems = {}
    for stem_name in ["vocals", "drums", "bass", "other"]:
        wav_path = stem_dir / f"{stem_name}.wav"
        if not wav_path.exists():
            raise FileNotFoundError(
                f"Expected stem output not found: {wav_path}\n"
                f"Demucs stderr: {result.stderr[-500:]}"
            )
        stems[stem_name] = str(wav_path)

    return stems


# ---------------------------------------------------------------------------
# Pipeline
# ---------------------------------------------------------------------------
def emit_json_progress(percent: int, stage: str, message: str = ""):
    """Emit JSON progress line for Tauri sidecar consumption."""
    print(json.dumps({
        "percent": percent,
        "stage": stage,
        "message": message or stage,
    }), flush=True)


def process(
    input_path: str,
    output_dir: str,
    model: str = "htdemucs",
    device: str = "auto",
    emit_progress: callable = None,
) -> Dict[str, str]:
    """
    Full pipeline: normalize -> separate -> resample.
    Returns dict mapping stem name -> final 48kHz WAV path in output_dir.
    Final stems are placed directly in output_dir as vocals.wav, drums.wav, etc.
    """
    if device == "auto":
        device = get_device()

    if emit_progress is None:
        emit_progress = lambda pct, stage, msg="": None

    base_tmp = Path("/tmp/te-stemplayer")
    norm_dir = base_tmp / "normalized"
    sep_dir = base_tmp / "separated"
    out_dir = Path(output_dir)

    for d in [norm_dir, sep_dir, out_dir]:
        d.mkdir(parents=True, exist_ok=True)

    emit_progress(5, "normalizing", "Normalizing audio to 44.1kHz stereo...")

    normalized_wav = str(norm_dir / f"{Path(input_path).stem}_norm.wav")
    normalize_audio(input_path, normalized_wav)
    emit_progress(15, "normalized", "Audio normalized")

    emit_progress(20, "loading_model", "Loading Demucs model...")
    stems_44k = run_demucs_separation(normalized_wav, str(sep_dir), model, device)
    emit_progress(80, "separated", "Stem separation complete")

    # Resample to 48kHz and place directly in output_dir as {stem_name}.wav
    stems_final = {}
    for i, (stem_name, wav_44k) in enumerate(stems_44k.items()):
        emit_progress(80 + (i + 1) * 4, f"resampling_{stem_name}",
                       f"Resampling {stem_name} to 48kHz...")
        final_path = str(out_dir / f"{stem_name}.wav")
        resample_stem(wav_44k, final_path)
        stems_final[stem_name] = final_path

    emit_progress(100, "complete", "All stems ready")

    return stems_final


# ---------------------------------------------------------------------------
# Sidecar JSON protocol
# ---------------------------------------------------------------------------
def handle_command(cmd: Dict[str, Any]) -> Dict[str, Any]:
    """Process a command dict, return a response dict."""
    command = cmd.get("cmd", "")

    if command == "separate":
        return handle_separate(cmd)
    elif command == "status":
        try:
            import demucs
            import torch
            return {
                "type": "result",
                "success": True,
                "demucs_version": getattr(demucs, "__version__", "unknown"),
                "torch_version": torch.__version__,
                "device": get_device(),
            }
        except ImportError as e:
            return {"type": "error", "message": f"Dependencies not installed: {e}"}
    elif command == "test":
        return {"type": "result", "success": True, "message": "Sidecar is running"}
    else:
        return {"type": "error", "message": f"Unknown command: {command}"}


def handle_separate(cmd: Dict[str, Any]) -> Dict[str, Any]:
    """Execute the separate pipeline, streaming progress."""
    input_path = cmd.get("input")
    output_dir = cmd.get("output_dir", "/tmp/te-stemplayer/output")
    model = cmd.get("model", "htdemucs")
    device = cmd.get("device", "auto")

    if not input_path:
        return {"type": "error", "message": "Missing 'input' parameter"}
    if not os.path.exists(input_path):
        return {"type": "error", "message": f"Input file not found: {input_path}"}

    def emit_progress(percent, stage, message=""):
        print(json.dumps({
            "type": "progress",
            "stage": stage,
            "percent": percent,
            "message": message or stage,
        }), flush=True)

    try:
        stems = process(
            input_path=input_path,
            output_dir=output_dir,
            model=model,
            device=device,
            emit_progress=emit_progress,
        )
        return {
            "type": "result",
            "success": True,
            "stems": stems,
            "sample_rate": 48000,
            "model": model,
        }
    except Exception as e:
        return {"type": "error", "message": str(e)}


# ---------------------------------------------------------------------------
# Entry points
# ---------------------------------------------------------------------------
def main_sidecar():
    """Sidecar mode: read JSON commands from stdin loop."""
    print(json.dumps({
        "type": "status",
        "ready": True,
        "pid": os.getpid(),
        "device": get_device(),
    }), flush=True)

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            cmd = json.loads(line)
        except json.JSONDecodeError:
            print(json.dumps({"type": "error", "message": "Invalid JSON"}), flush=True)
            continue
        response = handle_command(cmd)
        print(json.dumps(response), flush=True)


def main_cli():
    """Command-line interface matching Tauri sidecar invocation."""
    import argparse

    parser = argparse.ArgumentParser(
        description="TE-StemPlayer: Separate audio into 4 stems using Demucs"
    )
    parser.add_argument("--input", required=True, help="Input audio file path")
    parser.add_argument(
        "--output", required=True,
        help="Output directory for stems (vocals.wav, drums.wav, bass.wav, other.wav)",
    )
    parser.add_argument(
        "-m", "--model",
        default="htdemucs",
        choices=["htdemucs", "htdemucs_ft", "mdx_extra", "demucs"],
        help="Demucs model (default: htdemucs)",
    )
    parser.add_argument(
        "-d", "--device",
        default="auto",
        choices=["auto", "cuda", "mps", "cpu"],
        help="Torch device (default: auto)",
    )
    args = parser.parse_args()

    def cli_progress(percent, stage, message=""):
        emit_json_progress(percent, stage, message)

    try:
        stems = process(
            input_path=args.input,
            output_dir=args.output,
            model=args.model,
            device=args.device,
            emit_progress=cli_progress,
        )
        emit_json_progress(100, "complete", "All stems ready")
    except Exception as e:
        print(json.dumps({"percent": 0, "stage": "error", "message": str(e)}), flush=True)
        sys.exit(1)


if __name__ == "__main__":
    if len(sys.argv) > 1:
        main_cli()
    else:
        main_sidecar()
