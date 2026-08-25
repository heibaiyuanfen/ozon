"""Windows Task Scheduler helpers for the optional Feishu weekly report check."""

from __future__ import annotations

import os
import subprocess
from pathlib import Path


TASK_NAME = "OzonAnalyticsWeeklyFeishu"


def _task_command(root_path: Path) -> str:
    root = Path(root_path)
    executable = root / "OzonAnalytics.exe"
    if executable.is_file():
        return f'"{executable}" --weekly-feishu-sync'
    python = root / "runtime" / "python.exe"
    if python.is_file() and (root / "main.py").is_file():
        return f'"{python}" "{root / "main.py"}" --weekly-feishu-sync'
    return f'python "{root / "main.py"}" --weekly-feishu-sync'


def _ensure_windows() -> None:
    if os.name != "nt":
        raise RuntimeError("Windows 后台计划任务只能在 Windows 系统中安装或移除。")


def install_weekly_task(root_path: Path) -> str:
    """Install a daily 09:00 task; the program itself prevents duplicate sends."""
    _ensure_windows()
    command = _task_command(Path(root_path))
    result = subprocess.run(
        [
            "schtasks", "/Create", "/TN", TASK_NAME, "/SC", "DAILY", "/ST", "09:00",
            "/TR", command, "/F", "/RL", "LIMITED",
        ],
        capture_output=True, text=True, encoding="utf-8", errors="replace", check=False,
    )
    if result.returncode != 0:
        detail = (result.stderr or result.stdout).strip()
        raise RuntimeError(f"无法安装 Windows 计划任务：{detail or 'schtasks 返回失败'}")
    return "已安装 Windows 后台计划任务：每天 09:00 检查最新完整周并按配置发送飞书周报。"


def remove_weekly_task() -> str:
    """Remove the optional task.  Missing tasks are treated as already removed."""
    _ensure_windows()
    result = subprocess.run(
        ["schtasks", "/Delete", "/TN", TASK_NAME, "/F"],
        capture_output=True, text=True, encoding="utf-8", errors="replace", check=False,
    )
    if result.returncode != 0:
        detail = (result.stderr or result.stdout).strip()
        if "cannot find" not in detail.casefold() and "找不到" not in detail:
            raise RuntimeError(f"无法移除 Windows 计划任务：{detail or 'schtasks 返回失败'}")
        return "未找到已安装的 Windows 后台计划任务。"
    return "已移除 Windows 后台计划任务。"
