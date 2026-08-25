from __future__ import annotations

import json
import re
import uuid
from dataclasses import asdict, dataclass
from pathlib import Path


@dataclass(frozen=True)
class ShopProfile:
    id: str
    name: str
    kind: str
    database_file: str
    api_name: str = ""

    @property
    def kind_label(self) -> str:
        return "跨境店" if self.kind == "cross_border" else "本土店"

    @property
    def resolved_api_name(self) -> str:
        return self.api_name.strip() or self.name

    @property
    def display_name(self) -> str:
        if self.resolved_api_name == self.name:
            return f"{self.name} · {self.kind_label}"
        return f"{self.resolved_api_name} · {self.name} · {self.kind_label}"

    @property
    def api_display_name(self) -> str:
        return f"{self.resolved_api_name} ｜ {self.name} ｜ {self.kind_label}"


class ShopRegistry:
    """Small JSON registry; every shop owns an independent SQLite database."""

    def __init__(self, data_dir: Path):
        self.data_dir = Path(data_dir)
        self.data_dir.mkdir(parents=True, exist_ok=True)
        self.path = self.data_dir / "shops.json"
        if not self.path.exists():
            self._write({
                "active_shop_id": "default",
                "shops": [{
                    "id": "default",
                    "name": "默认本土店",
                    "kind": "local",
                    "database_file": "ozon_analytics.db",
                    "api_name": "默认本土店 API",
                }],
            })
        self._state = self._read()
        if not self._state.get("shops"):
            self._state = {
                "active_shop_id": "default",
                "shops": [{
                    "id": "default", "name": "默认本土店", "kind": "local",
                    "database_file": "ozon_analytics.db", "api_name": "默认本土店 API",
                }],
            }
            self._save()

    def _read(self) -> dict:
        try:
            value = json.loads(self.path.read_text(encoding="utf-8"))
            return value if isinstance(value, dict) else {}
        except (OSError, ValueError):
            return {}

    def _write(self, state: dict) -> None:
        temporary = self.path.with_suffix(".tmp")
        temporary.write_text(
            json.dumps(state, ensure_ascii=False, indent=2), encoding="utf-8",
        )
        temporary.replace(self.path)

    def _save(self) -> None:
        self._write(self._state)

    def list(self) -> list[ShopProfile]:
        result: list[ShopProfile] = []
        for raw in self._state.get("shops", []):
            try:
                result.append(ShopProfile(
                    id=str(raw["id"]), name=str(raw["name"]),
                    kind="cross_border" if raw.get("kind") == "cross_border" else "local",
                    database_file=str(raw["database_file"]),
                    api_name=str(raw.get("api_name") or raw.get("name") or ""),
                ))
            except (KeyError, TypeError):
                continue
        return result

    def get(self, shop_id: str) -> ShopProfile:
        for shop in self.list():
            if shop.id == shop_id:
                return shop
        raise KeyError(f"店铺不存在：{shop_id}")

    def active(self) -> ShopProfile:
        shop_id = str(self._state.get("active_shop_id") or "")
        try:
            return self.get(shop_id)
        except KeyError:
            shop = self.list()[0]
            self.set_active(shop.id)
            return shop

    def database_path(self, shop: ShopProfile | str) -> Path:
        profile = self.get(shop) if isinstance(shop, str) else shop
        resolved_root = self.data_dir.resolve()
        resolved = (self.data_dir / profile.database_file).resolve()
        if resolved != resolved_root and resolved_root not in resolved.parents:
            raise ValueError("店铺数据库路径超出数据目录。")
        return resolved

    def _clean_api_name(self, value: str, fallback: str) -> str:
        clean = re.sub(r"\s+", " ", str(value or "").strip()) or fallback
        if any(
            shop.resolved_api_name.casefold() == clean.casefold()
            for shop in self.list()
        ):
            raise ValueError(f"API 配置名称“{clean}”已存在，请使用不同名称。")
        return clean

    def add(
        self, name: str, kind: str = "local", *, api_name: str | None = None,
    ) -> ShopProfile:
        clean_name = re.sub(r"\s+", " ", str(name or "").strip())
        if not clean_name:
            raise ValueError("请输入店铺名称。")
        clean_api_name = self._clean_api_name(api_name or "", clean_name)
        shop_id = uuid.uuid4().hex[:12]
        profile = ShopProfile(
            id=shop_id,
            name=clean_name,
            kind="cross_border" if kind == "cross_border" else "local",
            database_file=f"shops/shop_{shop_id}.db",
            api_name=clean_api_name,
        )
        self._state.setdefault("shops", []).append(asdict(profile))
        self._save()
        return profile

    def update(
        self, shop_id: str, *, name: str, kind: str, api_name: str | None = None,
    ) -> ShopProfile:
        clean_name = re.sub(r"\s+", " ", str(name or "").strip())
        if not clean_name:
            raise ValueError("请输入店铺名称。")
        for raw in self._state.get("shops", []):
            if str(raw.get("id")) == shop_id:
                normalized_kind = "cross_border" if kind == "cross_border" else "local"
                current_kind = "cross_border" if raw.get("kind") == "cross_border" else "local"
                clean_api_name = re.sub(
                    r"\s+", " ", str(api_name or raw.get("api_name") or clean_name).strip()
                ) or clean_name
                for shop in self.list():
                    if shop.id != shop_id and shop.resolved_api_name.casefold() == clean_api_name.casefold():
                        raise ValueError(
                            f"API 配置名称“{clean_api_name}”已存在，请使用不同名称。"
                        )
                if normalized_kind != current_kind:
                    history = raw.setdefault("database_history", [])
                    if isinstance(history, list):
                        history.append({
                            "kind": current_kind,
                            "database_file": str(raw.get("database_file") or ""),
                        })
                    raw["database_file"] = (
                        f"shops/shop_{shop_id}_{normalized_kind}_{uuid.uuid4().hex[:8]}.db"
                    )
                raw["name"] = clean_name
                raw["kind"] = normalized_kind
                raw["api_name"] = clean_api_name
                self._save()
                return self.get(shop_id)
        raise KeyError(f"店铺不存在：{shop_id}")

    def set_active(self, shop_id: str) -> ShopProfile:
        profile = self.get(shop_id)
        self._state["active_shop_id"] = profile.id
        self._save()
        return profile

    def remove(self, shop_id: str) -> None:
        shops = self.list()
        if len(shops) <= 1:
            raise ValueError("至少需要保留一个店铺。")
        self.get(shop_id)
        self._state["shops"] = [
            raw for raw in self._state.get("shops", [])
            if str(raw.get("id")) != shop_id
        ]
        if self._state.get("active_shop_id") == shop_id:
            self._state["active_shop_id"] = str(self._state["shops"][0]["id"])
        # Deliberately keep the SQLite file so accidental removal is recoverable.
        self._save()
