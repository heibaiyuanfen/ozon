from __future__ import annotations

import json
import time
import urllib.error
import urllib.parse
import urllib.request
from datetime import date, datetime, timedelta
from typing import Any, Callable, Iterable


class WBApiError(RuntimeError):
    pass


class _HttpClient:
    def request(
        self,
        method: str,
        url: str,
        *,
        headers: dict[str, str] | None = None,
        timeout: int = 60,
    ) -> Any:
        request = urllib.request.Request(url, headers=headers or {}, method=method.upper())
        try:
            with urllib.request.urlopen(request, timeout=timeout) as response:
                raw = response.read()
        except urllib.error.HTTPError as error:
            detail = error.read().decode("utf-8", errors="replace")
            raise WBApiError(f"WB API 返回 HTTP {error.code}：{detail[:600]}") from error
        except urllib.error.URLError as error:
            raise WBApiError(f"无法连接 WB API：{error.reason}") from error
        if not raw:
            return {}
        try:
            return json.loads(raw.decode("utf-8-sig"))
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise WBApiError("WB API 返回了无法解析的内容。") from error


class WBApiClient:
    """Small client for the current WB Statistics, Promotion and warehouse APIs."""

    STATS = "https://statistics-api.wildberries.ru"
    ADVERT = "https://advert-api.wildberries.ru"
    MARKETPLACE = "https://marketplace-api.wildberries.ru"

    def __init__(
        self,
        token: str,
        *,
        http: Any | None = None,
        sleeper: Callable[[float], None] = time.sleep,
    ) -> None:
        self.token = token.strip()
        self.http = http or _HttpClient()
        self.sleeper = sleeper

    def _get(self, base: str, path: str, params: dict[str, Any] | None = None) -> Any:
        if not self.token:
            raise WBApiError("请先填写 WB API Token。")
        url = base + path
        if params:
            url += "?" + urllib.parse.urlencode(params, doseq=True)
        return self.http.request(
            "GET", url, headers={"Authorization": self.token, "Accept": "application/json"}
        )

    def test_connection(self) -> str:
        today = date.today().isoformat()
        result = self._get(self.STATS, "/api/v1/supplier/orders", {"dateFrom": today, "flag": 1})
        if not isinstance(result, list):
            raise WBApiError("WB Statistics API 返回格式异常。")
        return f"WB API 连接成功，今日订单变更 {len(result)} 条。"

    def orders(self, date_from: str, date_to: str) -> list[dict[str, Any]]:
        result = self._get(
            self.STATS,
            "/api/v1/supplier/orders",
            {"dateFrom": f"{date_from}T00:00:00", "flag": 0},
        )
        if not isinstance(result, list):
            raise WBApiError("WB 订单接口返回格式异常。")
        return [row for row in result if date_from <= _day(row.get("date")) <= date_to]

    def seller_warehouses(self) -> list[dict[str, Any]]:
        result = self._get(self.MARKETPLACE, "/api/v3/warehouses")
        return result if isinstance(result, list) else []

    def wb_offices(self) -> list[dict[str, Any]]:
        result = self._get(self.MARKETPLACE, "/api/v3/offices")
        return result if isinstance(result, list) else []

    def campaign_ids(self) -> list[int]:
        result = self._get(self.ADVERT, "/adv/v1/promotion/count")
        values: set[int] = set()
        _collect_campaign_ids(result, values)
        return sorted(values)

    def product_ad_daily(self, date_from: str, date_to: str) -> list[dict[str, Any]]:
        campaign_ids = self.campaign_ids()
        if not campaign_ids:
            return []
        rows: dict[tuple[str, int, int], dict[str, Any]] = {}
        calls: list[tuple[list[int], str, str]] = []
        for start, end in _date_windows(date_from, date_to, 31):
            for chunk in _chunks(campaign_ids, 50):
                calls.append((chunk, start, end))
        for index, (ids, start, end) in enumerate(calls):
            if index:
                self.sleeper(20.0)
            result = self._get(
                self.ADVERT,
                "/adv/v3/fullstats",
                {"ids": ",".join(str(value) for value in ids), "beginDate": start, "endDate": end},
            )
            for row in _parse_fullstats(result):
                key = (row["date"], row["nm_id"], row["campaign_id"])
                if key not in rows:
                    rows[key] = dict(row)
                else:
                    current = rows[key]
                    for field in ("spend_rub", "ad_orders", "ad_sales_rub", "views", "clicks"):
                        current[field] = float(current.get(field) or 0) + float(row.get(field) or 0)
        return list(rows.values())


def _parse_fullstats(result: Any) -> list[dict[str, Any]]:
    campaigns = result if isinstance(result, list) else []
    output: list[dict[str, Any]] = []
    for campaign in campaigns:
        if not isinstance(campaign, dict):
            continue
        campaign_id = int(campaign.get("advertId") or campaign.get("advert_id") or 0)
        for day in campaign.get("days") or []:
            if not isinstance(day, dict):
                continue
            day_value = _day(day.get("date"))
            for app in day.get("apps") or []:
                if not isinstance(app, dict):
                    continue
                for product in app.get("nm") or []:
                    if not isinstance(product, dict):
                        continue
                    nm_id = int(product.get("nmId") or product.get("nm_id") or 0)
                    if not day_value or not nm_id:
                        continue
                    output.append(
                        {
                            "date": day_value,
                            "nm_id": nm_id,
                            "campaign_id": campaign_id,
                            "spend_rub": float(product.get("sum") or 0),
                            "ad_orders": int(product.get("orders") or 0),
                            "ad_sales_rub": float(product.get("sum_price") or product.get("sumPrice") or 0),
                            "views": int(product.get("views") or 0),
                            "clicks": int(product.get("clicks") or 0),
                        }
                    )
    return output


def _collect_campaign_ids(value: Any, target: set[int]) -> None:
    if isinstance(value, dict):
        for key in ("advertId", "advert_id"):
            if value.get(key) not in (None, ""):
                try:
                    target.add(int(value[key]))
                except (TypeError, ValueError):
                    pass
        for child in value.values():
            _collect_campaign_ids(child, target)
    elif isinstance(value, list):
        for child in value:
            _collect_campaign_ids(child, target)


def _day(value: Any) -> str:
    return str(value or "")[:10]


def _chunks(values: list[int], size: int) -> Iterable[list[int]]:
    for index in range(0, len(values), size):
        yield values[index:index + size]


def _date_windows(date_from: str, date_to: str, days: int) -> Iterable[tuple[str, str]]:
    current = datetime.strptime(date_from, "%Y-%m-%d").date()
    end = datetime.strptime(date_to, "%Y-%m-%d").date()
    while current <= end:
        window_end = min(current + timedelta(days=days - 1), end)
        yield current.isoformat(), window_end.isoformat()
        current = window_end + timedelta(days=1)
