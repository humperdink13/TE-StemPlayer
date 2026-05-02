#!/usr/bin/env python3
"""Stem separation sidecar using Demucs v4"""
import sys
import json
import subprocess
import tempfile
import os
from pathlib import Path


def separate_stems(input_path: str, output_dir: str, model: str = "htdemucs") -> dict:
    """Run Demucs stem separation on an audio file."""
    cmd = [
        sys.executable, "-m", "demucs",
        "--two-stems=None",
        "-n", model,
        "-o", output_dir,
        input_path
    ]
    
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode != 0:
        return {"error": result.stderr}
    
    # Find output stems
    track_name = Path(input_path).stem
    stems_dir = Path(output_dir) / model / track_name
    
    stems = {}
    for stem_name in ["vocals", "drums", "bass", "other"]:
        stem_path = stems_dir / f"{stem_name}.wav"
        if stem_path.exists():
            stems[stem_name] = str(stem_path)
    
    return {"stems": stems, "output_dir": str(stems_dir)}


def download_youtube(url: str, output_dir: str) -> str:
    """Download audio from YouTube URL using yt-dlp."""
    output_path = os.path.join(output_dir, "%(title)s.%(ext)s")
    cmd = [
        "yt-dlp",
        "-x", "--audio-format", "wav",
        "--audio-quality", "0",
        "-o", output_path,
        url
    ]
    
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode != 0:
        raise RuntimeError(f"yt-dlp failed: {result.stderr}")
    
    # Find the downloaded file
    for f in os.listdir(output_dir):
        if f.endswith(".wav"):
            return os.path.join(output_dir, f)
    
    raise RuntimeError("No WAV file found after download")


if __name__ == "__main__":
    if len(sys.argv) < 3:
        print(json.dumps({"error": "Usage: stem_separator.py <command> <args>"}))
        sys.exit(1)
    
    command = sys.argv[1]
    
    if command == "separate":
        input_path = sys.argv[2]
        output_dir = sys.argv[3] if len(sys.argv) > 3 else tempfile.mkdtemp()
        result = separate_stems(input_path, output_dir)
        print(json.dumps(result))
    
    elif command == "download":
        url = sys.argv[2]
        output_dir = sys.argv[3] if len(sys.argv) > 3 else tempfile.mkdtemp()
        try:
            path = download_youtube(url, output_dir)
            print(json.dumps({"path": path}))
        except RuntimeError as e:
            print(json.dumps({"error": str(e)}))
    
    elif command == "pipeline":
        # Full pipeline: download + separate
        url = sys.argv[2]
        output_dir = sys.argv[3] if len(sys.argv) > 3 else tempfile.mkdtemp()
        try:
            wav_path = download_youtube(url, output_dir)
            result = separate_stems(wav_path, output_dir)
            print(json.dumps(result))
        except RuntimeError as e:
            print(json.dumps({"error": str(e)}))
    
    else:
        print(json.dumps({"error": f"Unknown command: {command}"}))
