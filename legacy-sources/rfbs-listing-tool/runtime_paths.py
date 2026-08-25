from __future__ import annotations

import os
import sys
from pathlib import Path


APP_DATA_FOLDER_NAME = "程序数据"


def _ensure_writable_directory(path: Path) -> Path:
    path.mkdir(parents=True, exist_ok=True)
    probe = path / ".write-test.tmp"
    probe.write_bytes(b"")
    probe.unlink()
    return path


def resolve_app_data_dir() -> Path:
    """Return the persistent, writable directory used by source and EXE builds."""
    configured = str(os.environ.get("OZON_RFBS_DATA_DIR") or "").strip()
    if configured:
        return _ensure_writable_directory(Path(configured).expanduser().resolve())

    if not getattr(sys, "frozen", False):
        return Path(__file__).resolve().parent

    portable_dir = Path(sys.executable).resolve().parent / APP_DATA_FOLDER_NAME
    try:
        return _ensure_writable_directory(portable_dir)
    except OSError:
        local_app_data = str(os.environ.get("LOCALAPPDATA") or "").strip()
        fallback_root = (
            Path(local_app_data)
            if local_app_data
            else Path.home() / "AppData" / "Local"
        )
        return _ensure_writable_directory(fallback_root / "OzonRFBS上品工具")


APP_DATA_DIR = resolve_app_data_dir()
