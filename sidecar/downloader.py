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
    # Try system yt-dlp first
    try:
        subprocess.run(["yt-dlp", "--version"], capture_output=True, check=True)
        return "yt-dlp"
    except (subprocess.CalledProcessError, FileNotFoundError):
        pass

    # Try python module
    try:
        subprocess.run([sys.executable, "-m", "yt_dlp", "--version"],
                       capture_output=True, check=True)
        return "yt-dlp-module"
    except (subprocess.CalledProcessError, FileNotFoundError):
        pass

    raise RuntimeError("yt-dlp not found. Install with: pip install yt-dlp")


def download_audio(
    url: str,
    output_dir: str,
    quality: str = "best",
    retries: int = 3,
    progress_callback: Optional[callable] = None,
) -> str:
    """
    Download audio from YouTube URL as WAV.

    Args:
        url: YouTube URL
        output_dir: Directory to save output
        quality: Audio quality ('best' or '0' for best)
        retries: Number of retry attempts on network failure
        progress_callback: Optional callback(percent, stage) for progress

    Returns:
        Path to downloaded WAV file
    """
    yt_dlp = find_yt_dlp()
    os.makedirs(output_dir, exist_ok=True)

    output_template = os.path.join(output_dir, "%(title)s.%(ext)s")

    cmd_base = []
    if yt_dlp == "yt-dlp":
        cmd_base = ["yt-dlp"]
    else:
        cmd_base = [sys.executable, "-m", "yt_dlp"]

    cmd = cmd_base + [
        "--extract-audio",
        "--audio-format", "wav",
        "--audio-quality", quality,
        "--output", output_template,
        "--no-playlist",
        "--no-progress",
        "--print", "filename",
        url,
    ]

    for attempt in range(retries):
        try:
            if progress_callback:
                progress_callback(10 + attempt * 5, "downloading")

            result = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                timeout=300,  # 5 minute timeout
                check=True,
            )

            # Parse the output filename from yt-dlp
            output_path = result.stdout.strip().split('\n')[-1]

            if not output_path.endswith('.wav'):
                # yt-dlp might have changed extension; find the wav file
                wav_files = list(Path(output_dir).glob("*.wav"))
                if wav_files:
                    output_path = str(max(wav_files, key=os.path.getctime))

            if os.path.exists(output_path):
                if progress_callback:
                    progress_callback(100, "download_complete")
                return output_path

            raise FileNotFoundError(f"Expected output not found: {output_path}")

        except subprocess.TimeoutExpired:
            if progress_callback:
                progress_callback(0, f"timeout_attempt_{attempt + 1}")
            if attempt == retries - 1:
                raise RuntimeError(f"Download timed out after {retries} attempts")
            time.sleep(2 ** attempt)  # exponential backoff

        except subprocess.CalledProcessError as e:
            if progress_callback:
                progress_callback(0, f"error_attempt_{attempt + 1}")
            if attempt == retries - 1:
                error_msg = e.stderr.strip() if e.stderr else str(e)
                raise RuntimeError(f"yt-dlp failed: {error_msg}")
            time.sleep(2 ** attempt)

    raise RuntimeError("Download failed after all retries")


def handle_json_command(cmd: dict) -> dict:
    """Process a JSON command from stdin."""
    command = cmd.get("cmd", "")
    output_dir = cmd.get("output_dir", "/tmp/te-stemplayer/downloads")
    url = cmd.get("url", "")

    if command == "download":
        if not url:
            return {
                "type": "error",
                "message": "Missing 'url' parameter for download command",
            }

        try:
            wav_path = download_audio(
                url=url,
                output_dir=output_dir,
                progress_callback=None,  # We handle progress via stdout
            )
            return {
                "type": "result",
                "success": True,
                "output_path": wav_path,
                "filename": os.path.basename(wav_path),
                "size_bytes": os.path.getsize(wav_path),
            }
        except Exception as e:
            return {
                "type": "error",
                "message": str(e),
            }

    elif command == "validate":
        if not url:
            return {"type": "error", "message": "Missing 'url' for validation"}
        try:
            yt_dlp = find_yt_dlp()
            cmd_base = ["yt-dlp"] if yt_dlp == "yt-dlp" else [sys.executable, "-m", "yt_dlp"]
            result = subprocess.run(
                cmd_base + ["--dump-json", "--no-playlist", url],
                capture_output=True, text=True, timeout=30
            )
            if result.returncode == 0:
                info = json.loads(result.stdout)
                return {
                    "type": "result",
                    "success": True,
                    "valid": True,
                    "title": info.get("title", "Unknown"),
                    "duration_secs": info.get("duration", 0),
                    "uploader": info.get("uploader", "Unknown"),
                }
            return {
                "type": "result",
                "success": False,
                "valid": False,
                "message": "URL not accessible",
            }
        except Exception as e:
            return {"type": "error", "message": str(e)}

    elif command == "status":
        return {
            "type": "result",
            "success": True,
            "yt_dlp_available": True,
        }

    else:
        return {
            "type": "error",
            "message": f"Unknown command: {command}",
        }


def main_cli():
    """Command-line entry point."""
    parser = argparse.ArgumentParser(
        description="Download YouTube audio as WAV for TE-StemPlayer"
    )
    parser.add_argument("url", help="YouTube URL to download")
    parser.add_argument(
        "-o", "--output-dir",
        default="/tmp/te-stemplayer/downloads",
        help="Output directory (default: /tmp/te-stemplayer/downloads)",
    )
    parser.add_argument(
        "-q", "--quality",
        default="0",
        help="Audio quality (0=best, default: 0)",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Output result as JSON",
    )
    args = parser.parse_args()

    try:
        wav_path = download_audio(
            url=args.url,
            output_dir=args.output_dir,
            quality=args.quality,
        )

        if args.json:
            print(json.dumps({
                "success": True,
                "output_path": wav_path,
                "filename": os.path.basename(wav_path),
                "size_bytes": os.path.getsize(wav_path),
            }))
        else:
            print(f"Downloaded: {wav_path}")

    except Exception as e:
        if args.json:
            print(json.dumps({"success": False, "error": str(e)}))
        else:
            print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)


def main_sidecar():
    """
    Sidecar mode: Read JSON commands from stdin, write JSON responses to stdout.
    One JSON object per line.
    """
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue

        try:
            cmd = json.loads(line)
        except json.JSONDecodeError:
            response = {"type": "error", "message": "Invalid JSON"}
            print(json.dumps(response), flush=True)
            continue

        response = handle_json_command(cmd)
        print(json.dumps(response), flush=True)


if __name__ == "__main__":
    # If arguments are provided, run CLI mode; otherwise sidecar mode
    if len(sys.argv) > 1:
        main_cli()
    else:
        main_sidecar()
