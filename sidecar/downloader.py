#!/usr/bin/env python3
"""
yt-dlp integration for downloading YouTube audio as WAV.
Supports both command-line usage and JSON-line protocol (stdin/stdout)
for Tauri sidecar communication.
"""

import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path
from typing import Optional


def find_yt_dlp() -> str:
    """Locate yt-dlp binary or module."""
    try:
        subprocess.run(["yt-dlp", "--version"], capture_output=True, check=True)
        return "yt-dlp"
    except (subprocess.CalledProcessError, FileNotFoundError):
        pass

    try:
        subprocess.run([sys.executable, "-m", "yt_dlp", "--version"],
                       capture_output=True, check=True)
        return "yt-dlp-module"
    except (subprocess.CalledProcessError, FileNotFoundError):
        pass

    raise RuntimeError("yt-dlp not found. Install with: pip install yt-dlp")


def emit_progress(percent, message):
    """Emit JSON progress line for Tauri sidecar consumption."""
    print(json.dumps({"percent": percent, "message": message}), flush=True)


def download_audio(
    url: str,
    output_path: str,
    quality: str = "best",
    retries: int = 3,
) -> str:
    """
    Download audio from YouTube URL as WAV.

    Args:
        url: YouTube URL
        output_path: Full path for output WAV file (e.g. /tmp/stem-app/jobid/source.wav)
        quality: Audio quality ('best' or '0' for best)
        retries: Number of retry attempts on network failure

    Returns:
        Path to downloaded WAV file
    """
    yt_dlp = find_yt_dlp()
    output_dir = os.path.dirname(output_path)
    os.makedirs(output_dir, exist_ok=True)

    cmd_base = ["yt-dlp"] if yt_dlp == "yt-dlp" else [sys.executable, "-m", "yt_dlp"]

    # Download to a temp name, then rename to output_path
    temp_template = os.path.join(output_dir, "download.%(ext)s")

    cmd = cmd_base + [
        "--extract-audio",
        "--audio-format", "wav",
        "--audio-quality", quality,
        "--output", temp_template,
        "--no-playlist",
        "--no-progress",
        url,
    ]

    for attempt in range(retries):
        try:
            emit_progress(10 + attempt * 5, f"Downloading audio (attempt {attempt + 1})...")

            result = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                timeout=300,
                check=True,
            )

            # Find the downloaded wav file
            wav_files = list(Path(output_dir).glob("download*.wav"))
            if not wav_files:
                # yt-dlp may have used a different name
                wav_files = list(Path(output_dir).glob("*.wav"))

            if wav_files:
                downloaded = str(max(wav_files, key=os.path.getctime))
                # Rename to expected output path
                if downloaded != output_path:
                    os.rename(downloaded, output_path)
                emit_progress(100, "Download complete")
                return output_path

            raise FileNotFoundError(f"No WAV file found in {output_dir}. yt-dlp stderr: {result.stderr}")

        except subprocess.TimeoutExpired:
            emit_progress(0, f"Download timed out (attempt {attempt + 1})")
            if attempt == retries - 1:
                raise RuntimeError(f"Download timed out after {retries} attempts")
            time.sleep(2 ** attempt)

        except subprocess.CalledProcessError as e:
            error_msg = e.stderr.strip() if e.stderr else str(e)
            emit_progress(0, f"Download error (attempt {attempt + 1}): {error_msg}")
            if attempt == retries - 1:
                raise RuntimeError(f"yt-dlp failed: {error_msg}")
            time.sleep(2 ** attempt)

    raise RuntimeError("Download failed after all retries")


def main_cli():
    """Command-line entry point compatible with Tauri sidecar invocation."""
    parser = argparse.ArgumentParser(
        description="Download YouTube audio as WAV for TE-StemPlayer"
    )
    parser.add_argument("--url", required=True, help="YouTube URL to download")
    parser.add_argument(
        "--output", required=True,
        help="Full output path for WAV file",
    )
    parser.add_argument(
        "-q", "--quality",
        default="0",
        help="Audio quality (0=best, default: 0)",
    )
    args = parser.parse_args()

    try:
        wav_path = download_audio(
            url=args.url,
            output_path=args.output,
            quality=args.quality,
        )
        emit_progress(100, f"Downloaded: {wav_path}")
    except Exception as e:
        emit_progress(0, f"Error: {e}")
        sys.exit(1)


if __name__ == "__main__":
    main_cli()
