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


# Ensure Homebrew binaries (ffmpeg) are on PATH
HOMEBREW_PATHS = ["/opt/homebrew/bin", "/usr/local/bin"]
for p in HOMEBREW_PATHS:
    if p not in os.environ.get("PATH", "") and os.path.isdir(p):
        os.environ["PATH"] = p + ":" + os.environ.get("PATH", "")


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


def find_ffmpeg() -> Optional[str]:
    """Locate ffmpeg binary."""
    for p in HOMEBREW_PATHS:
        candidate = os.path.join(p, "ffmpeg")
        if os.path.isfile(candidate):
            return candidate
    import shutil
    return shutil.which("ffmpeg")


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

    Uses multiple strategies to work around YouTube's SABR streaming
    restrictions which cause HTTP 403 errors with standard downloads.
    """
    yt_dlp = find_yt_dlp()
    output_dir = os.path.dirname(output_path)
    os.makedirs(output_dir, exist_ok=True)

    cmd_base = ["yt-dlp"] if yt_dlp == "yt-dlp" else [sys.executable, "-m", "yt_dlp"]

    # Find ffmpeg location for --ffmpeg-location
    ffmpeg_path = find_ffmpeg()
    ffmpeg_dir = os.path.dirname(ffmpeg_path) if ffmpeg_path else None

    temp_template = os.path.join(output_dir, "download.%(ext)s")

    # Strategy list: try different approaches to work around YouTube 403/SABR
    strategies = [
        {
            "name": "default with legacy-server-connect",
            "extra_args": ["--legacy-server-connect"],
        },
        {
            "name": "format 18 (360p mp4 with audio)",
            "extra_args": ["-f", "18", "--legacy-server-connect"],
        },
        {
            "name": "cookies from Chrome browser",
            "extra_args": ["--cookies-from-browser", "chrome", "--legacy-server-connect"],
        },
        {
            "name": "cookies from Firefox browser",
            "extra_args": ["--cookies-from-browser", "firefox", "--legacy-server-connect"],
        },
    ]

    last_error = None

    for strategy in strategies:
        emit_progress(5, f"Trying download strategy: {strategy['name']}...")

        # Clean up any previous partial downloads
        for f in Path(output_dir).glob("download*"):
            try:
                f.unlink()
            except OSError:
                pass

        cmd = cmd_base + [
            "--extract-audio",
            "--audio-format", "wav",
            "--audio-quality", quality,
            "--output", temp_template,
            "--no-playlist",
            "--no-progress",
        ]

        if ffmpeg_dir:
            cmd += ["--ffmpeg-location", ffmpeg_dir]

        cmd += strategy["extra_args"]
        cmd.append(url)

        for attempt in range(retries):
            try:
                emit_progress(
                    10 + attempt * 5,
                    f"Downloading ({strategy['name']}, attempt {attempt + 1})...",
                )

                result = subprocess.run(
                    cmd,
                    capture_output=True,
                    text=True,
                    timeout=300,
                )

                if result.returncode != 0:
                    error_msg = result.stderr.strip() if result.stderr else result.stdout.strip()
                    # Extract just the ERROR line
                    for line in (error_msg or "").split("\n"):
                        if "ERROR" in line:
                            error_msg = line.strip()
                            break
                    last_error = error_msg
                    if "403" in str(error_msg) or "Forbidden" in str(error_msg):
                        # 403 means this strategy won't work, try next
                        break
                    # Other errors might be transient, retry
                    time.sleep(2 ** attempt)
                    continue

                # Find the downloaded wav file
                wav_files = list(Path(output_dir).glob("download*.wav"))
                if not wav_files:
                    wav_files = list(Path(output_dir).glob("*.wav"))

                if wav_files:
                    downloaded = str(max(wav_files, key=os.path.getctime))
                    # Check file is not empty
                    if os.path.getsize(downloaded) == 0:
                        last_error = "Downloaded file is empty (0 bytes)"
                        break
                    if downloaded != output_path:
                        os.rename(downloaded, output_path)
                    emit_progress(100, "Download complete")
                    return output_path

                last_error = f"No WAV file found in {output_dir}"
                break

            except subprocess.TimeoutExpired:
                last_error = f"Download timed out (attempt {attempt + 1})"
                emit_progress(0, last_error)
                if attempt < retries - 1:
                    time.sleep(2 ** attempt)

    # All strategies failed
    raise RuntimeError(
        f"YouTube download failed after trying all strategies. Last error: {last_error}\n\n"
        "This is likely due to YouTube's SABR streaming restrictions (see "
        "https://github.com/yt-dlp/yt-dlp/issues/12482).\n\n"
        "Workarounds:\n"
        "1. Update Python to 3.10+ and install latest yt-dlp nightly\n"
        "2. Use a local audio file instead of YouTube URL\n"
        "3. Download the audio manually and use the file input option"
    )


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
