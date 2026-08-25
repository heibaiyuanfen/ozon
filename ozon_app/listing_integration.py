from __future__ import annotations

import json
import os
import subprocess
from pathlib import Path
from typing import Any


class ListingToolError(RuntimeError):
    """Raised when the external listing tool cannot be configured or opened."""


def find_listing_tool_executable(
    source_root: Path | None = None,
    package_root: Path | None = None,
) -> Path | None:
    """Find the packaged or source-distribution RFBS listing executable."""
    candidates: list[Path] = []
    if package_root is not None:
        root = Path(package_root)
        candidates.extend((
            root / "tools" / "Ozon_RFBS_ListingTool" / "Ozon_RFBS_ListingTool.exe",
            root / "tools" / "Ozon_RFBS上品工具" / "Ozon_RFBS上品工具.exe",
            root / "Ozon_RFBS上品工具" / "Ozon_RFBS上品工具.exe",
        ))
    if source_root is not None:
        root = Path(source_root)
        candidates.extend((
            root / "发布" / "Ozon_RFBS上品工具" / "Ozon_RFBS上品工具.exe",
            root / "Ozon_RFBS上品工具.exe",
        ))
    for candidate in candidates:
        if candidate.is_file():
            return candidate
    return None


def sync_listing_shop_profile(
    config_path: Path,
    *,
    profile_id: str,
    name: str,
    client_id: str,
    api_key: str,
    proxy_url: str = "",
) -> dict[str, Any]:
    """Select/upsert one Seller API profile without disturbing other tool settings."""
    clean_id = str(profile_id or "").strip()
    clean_name = str(name or "").strip()
    clean_client = str(client_id or "").strip()
    clean_key = str(api_key or "").strip()
    if not clean_id or not clean_name:
        raise ListingToolError("上品工具的店铺配置缺少名称或 ID。")
    if not clean_client or not clean_key:
        raise ListingToolError("当前跨境店尚未填写 Seller Client ID / API Key。")

    path = Path(config_path)
    state: dict[str, Any] = {}
    if path.is_file():
        try:
            loaded = json.loads(path.read_text(encoding="utf-8"))
            if isinstance(loaded, dict):
                state = loaded
        except (OSError, ValueError) as error:
            raise ListingToolError(f"无法读取上品工具配置：{error}") from error

    ozon = state.get("ozon")
    if not isinstance(ozon, dict):
        ozon = {}
        state["ozon"] = ozon
    raw_shops = ozon.get("shops")
    shops = [dict(item) for item in raw_shops if isinstance(item, dict)] \
        if isinstance(raw_shops, list) else []
    profile = {
        "id": clean_id,
        "name": clean_name,
        "client_id": clean_client,
        "api_key": clean_key,
        "proxy_url": str(proxy_url or "").strip(),
    }
    replaced = False
    for index, existing in enumerate(shops):
        if str(existing.get("id") or "") == clean_id:
            shops[index] = {**existing, **profile}
            replaced = True
            break
    if not replaced:
        shops.append(profile)
    ozon["shops"] = shops
    ozon["selected_shop_id"] = clean_id

    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        temporary = path.with_suffix(path.suffix + ".tmp")
        temporary.write_text(
            json.dumps(state, ensure_ascii=False, indent=2), encoding="utf-8",
        )
        temporary.replace(path)
    except OSError as error:
        raise ListingToolError(f"无法写入上品工具配置：{error}") from error
    return {"selected_shop_id": clean_id, "shop_count": len(shops)}


def launch_listing_tool(executable: Path, data_dir: Path) -> int:
    """Launch the listing tool detached, sharing a chosen persistent data folder."""
    exe = Path(executable)
    if not exe.is_file():
        raise ListingToolError(f"未找到上品工具：{exe}")
    target_data = Path(data_dir)
    try:
        target_data.mkdir(parents=True, exist_ok=True)
    except OSError as error:
        raise ListingToolError(f"无法创建上品工具数据目录：{error}") from error
    environment = os.environ.copy()
    environment["OZON_RFBS_DATA_DIR"] = str(target_data.resolve())
    try:
        process = subprocess.Popen(
            [str(exe)], cwd=str(exe.parent), env=environment,
            close_fds=True,
            creationflags=getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0),
        )
    except OSError as error:
        raise ListingToolError(f"无法启动上品工具：{error}") from error
    return int(process.pid)
