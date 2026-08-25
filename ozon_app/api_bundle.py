"""Portable, explicit API configuration bundles for named Ozon shops.

The local database stores secrets with platform-specific encryption.  A migration
bundle intentionally uses plain JSON so it can be imported on another computer;
the receiving application encrypts secrets again through ``AppService``.
"""

from __future__ import annotations

import json
from dataclasses import asdict
from pathlib import Path
from typing import Any, Iterable

from .services import Credentials
from .shops import ShopProfile


BUNDLE_FORMAT = "ozon-api-profiles"
BUNDLE_VERSION = 1


class ApiBundleError(ValueError):
    """Raised when an API configuration migration file is invalid or unsafe."""


def _required_text(value: Any, label: str) -> str:
    text = str(value or "").strip()
    if not text:
        raise ApiBundleError(f"配置包缺少{label}。")
    return text


def _shop_kind(value: Any) -> str:
    kind = str(value or "").strip()
    if kind not in {"local", "cross_border"}:
        raise ApiBundleError("店铺类型只能是 local 或 cross_border。")
    return kind


def _credentials(value: Any) -> Credentials:
    if not isinstance(value, dict):
        raise ApiBundleError("配置包中的 credentials 必须是对象。")
    allowed = set(Credentials.__dataclass_fields__)
    unexpected = set(value) - allowed
    if unexpected:
        names = "、".join(sorted(str(item) for item in unexpected))
        raise ApiBundleError(f"配置包包含不支持的凭证字段：{names}。")
    values = {
        name: str(value.get(name, getattr(Credentials(), name)) or "")
        for name in Credentials.__dataclass_fields__
    }
    return Credentials(**values)


def build_api_bundle(
    profiles: Iterable[tuple[ShopProfile, Credentials]],
) -> dict[str, Any]:
    """Build a serialisable migration bundle without writing it to disk."""
    items: list[dict[str, Any]] = []
    seen_names: set[str] = set()
    for profile, credentials in profiles:
        api_name = _required_text(profile.resolved_api_name, "API 配置名称")
        key = api_name.casefold()
        if key in seen_names:
            raise ApiBundleError(f"API 配置名称重复：{api_name}。")
        seen_names.add(key)
        items.append({
            "api_name": api_name,
            "shop_name": _required_text(profile.name, "店铺名称"),
            "shop_kind": _shop_kind(profile.kind),
            "credentials": asdict(credentials),
        })
    if not items:
        raise ApiBundleError("没有可导出的 API 配置。")
    return {
        "format": BUNDLE_FORMAT,
        "version": BUNDLE_VERSION,
        "profiles": items,
    }


def write_api_bundle(path: Path, bundle: dict[str, Any]) -> Path:
    """Write a migration bundle atomically using UTF-8 JSON."""
    if not isinstance(bundle, dict):
        raise ApiBundleError("API 配置包必须是对象。")
    target = Path(path)
    if not target.name:
        raise ApiBundleError("请提供 API 配置包文件名。")
    target.parent.mkdir(parents=True, exist_ok=True)
    temporary = target.with_name(target.name + ".tmp")
    temporary.write_text(
        json.dumps(bundle, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    temporary.replace(target)
    return target


def read_api_bundle(path: Path) -> list[dict[str, Any]]:
    """Validate and deserialize a migration bundle into safe profile records."""
    source = Path(path)
    try:
        raw = json.loads(source.read_text(encoding="utf-8-sig"))
    except OSError as error:
        raise ApiBundleError(f"无法读取 API 配置包：{error}") from error
    except json.JSONDecodeError as error:
        raise ApiBundleError(f"API 配置包不是有效 JSON：{error.msg}") from error
    if not isinstance(raw, dict):
        raise ApiBundleError("API 配置包顶层必须是对象。")
    if raw.get("format") != BUNDLE_FORMAT:
        raise ApiBundleError("不是受支持的 Ozon API 配置包。")
    if raw.get("version") != BUNDLE_VERSION:
        raise ApiBundleError("API 配置包版本不受支持。")
    profiles = raw.get("profiles")
    if not isinstance(profiles, list) or not profiles:
        raise ApiBundleError("API 配置包不包含任何店铺配置。")
    result: list[dict[str, Any]] = []
    seen_names: set[str] = set()
    for index, item in enumerate(profiles, start=1):
        if not isinstance(item, dict):
            raise ApiBundleError(f"第 {index} 个配置不是对象。")
        api_name = _required_text(item.get("api_name"), f"第 {index} 个配置的 API 名称")
        key = api_name.casefold()
        if key in seen_names:
            raise ApiBundleError(f"API 配置名称重复：{api_name}。")
        seen_names.add(key)
        result.append({
            "api_name": api_name,
            "shop_name": _required_text(item.get("shop_name"), f"第 {index} 个配置的店铺名称"),
            "shop_kind": _shop_kind(item.get("shop_kind")),
            "credentials": _credentials(item.get("credentials")),
        })
    return result
