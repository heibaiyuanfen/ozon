from __future__ import annotations

import json
import time
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Iterable


class FeishuError(RuntimeError):
    pass


class _JsonHttpClient:
    def request(
        self,
        method: str,
        url: str,
        *,
        headers: dict[str, str] | None = None,
        body: dict[str, Any] | None = None,
        timeout: int = 30,
    ) -> dict[str, Any]:
        payload = None if body is None else json.dumps(body, ensure_ascii=False).encode("utf-8")
        request = urllib.request.Request(
            url,
            data=payload,
            headers={"Content-Type": "application/json; charset=utf-8", **(headers or {})},
            method=method.upper(),
        )
        try:
            with urllib.request.urlopen(request, timeout=timeout) as response:
                raw = response.read()
        except urllib.error.HTTPError as error:
            detail = error.read().decode("utf-8", errors="replace")
            raise FeishuError(f"飞书 API 返回 HTTP {error.code}：{detail[:500]}") from error
        except urllib.error.URLError as error:
            raise FeishuError(f"无法连接飞书 API：{error.reason}") from error
        try:
            result = json.loads(raw.decode("utf-8-sig"))
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise FeishuError("飞书 API 返回了无法解析的内容。") from error
        if not isinstance(result, dict):
            raise FeishuError("飞书 API 返回格式异常。")
        return result


class FeishuClient:
    def __init__(
        self,
        app_id: str,
        app_secret: str,
        *,
        base_url: str = "https://open.feishu.cn/open-apis",
        http: Any | None = None,
    ) -> None:
        self.app_id = app_id.strip()
        self.app_secret = app_secret.strip()
        self.base_url = base_url.rstrip("/")
        self.http = http or _JsonHttpClient()
        self._token = ""
        self._token_expires_at = 0.0

    def tenant_access_token(self) -> str:
        if self._token and time.time() < self._token_expires_at - 120:
            return self._token
        if not self.app_id or not self.app_secret:
            raise FeishuError("请先填写飞书 App ID 和 App Secret。")
        result = self._request(
            "POST",
            "/auth/v3/tenant_access_token/internal/",
            body={"app_id": self.app_id, "app_secret": self.app_secret},
            authenticated=False,
        )
        token = str(result.get("tenant_access_token") or "")
        if not token:
            raise FeishuError("飞书认证成功响应中没有 tenant_access_token。")
        self._token = token
        self._token_expires_at = time.time() + int(result.get("expire") or 7200)
        return token

    def test_connection(self, app_token: str = "", table_id: str = "") -> str:
        self.tenant_access_token()
        if app_token and table_id:
            fields = self.list_fields(app_token, table_id)
            return f"飞书认证及多维表格连接成功，共读取 {len(fields)} 个字段。"
        return "飞书应用认证成功；填写 App Token 和 Table ID 后可继续测试多维表格。"

    def list_fields(self, app_token: str, table_id: str) -> list[dict[str, Any]]:
        items: list[dict[str, Any]] = []
        page_token = ""
        while True:
            # The field-list endpoint allows at most 100 per page.  Record
            # listing below has a separate 500-row maximum.
            params: dict[str, Any] = {"page_size": 100}
            if page_token:
                params["page_token"] = page_token
            result = self._request(
                "GET", self._table_path(app_token, table_id) + "/fields", params=params
            )
            data = result.get("data") or {}
            items.extend(data.get("items") or [])
            if not data.get("has_more"):
                break
            page_token = str(data.get("page_token") or "")
            if not page_token:
                break
        return items

    def create_field(
        self, app_token: str, table_id: str, field_name: str, field_type: int,
    ) -> dict[str, Any]:
        return self._request(
            "POST",
            self._table_path(app_token, table_id) + "/fields",
            body={"field_name": field_name, "type": field_type},
        )

    def ensure_fields(
        self,
        app_token: str,
        table_id: str,
        definitions: dict[str, int],
    ) -> tuple[dict[str, dict[str, Any]], str]:
        fields = self.list_fields(app_token, table_id)
        by_name = {str(item.get("field_name") or ""): item for item in fields}
        for field_name, field_type in definitions.items():
            if field_name not in by_name:
                self.create_field(app_token, table_id, field_name, field_type)
        if any(name not in by_name for name in definitions):
            fields = self.list_fields(app_token, table_id)
            by_name = {str(item.get("field_name") or ""): item for item in fields}
        primary = next(
            (str(item.get("field_name") or "") for item in fields if item.get("is_primary")),
            "",
        )
        return by_name, primary

    def list_records(self, app_token: str, table_id: str) -> list[dict[str, Any]]:
        items: list[dict[str, Any]] = []
        page_token = ""
        while True:
            params: dict[str, Any] = {"page_size": 500, "automatic_fields": "true"}
            if page_token:
                params["page_token"] = page_token
            result = self._request(
                "GET", self._table_path(app_token, table_id) + "/records", params=params
            )
            data = result.get("data") or {}
            items.extend(data.get("items") or [])
            if not data.get("has_more"):
                break
            page_token = str(data.get("page_token") or "")
            if not page_token:
                break
        return items

    def batch_create(
        self, app_token: str, table_id: str, records: Iterable[dict[str, Any]],
    ) -> int:
        values = list(records)
        written = 0
        for batch in _chunks(values, 500):
            self._request(
                "POST",
                self._table_path(app_token, table_id) + "/records/batch_create",
                body={"records": batch},
            )
            written += len(batch)
        return written

    def batch_update(
        self, app_token: str, table_id: str, records: Iterable[dict[str, Any]],
    ) -> int:
        values = list(records)
        written = 0
        for batch in _chunks(values, 500):
            self._request(
                "POST",
                self._table_path(app_token, table_id) + "/records/batch_update",
                body={"records": batch},
            )
            written += len(batch)
        return written

    def send_text(self, chat_id: str, text: str) -> str:
        if not chat_id.strip():
            raise FeishuError("请先填写接收周报的飞书群 Chat ID。")
        result = self._request(
            "POST",
            "/im/v1/messages",
            params={"receive_id_type": "chat_id"},
            body={
                "receive_id": chat_id.strip(),
                "msg_type": "text",
                "content": json.dumps({"text": text}, ensure_ascii=False),
            },
        )
        return str(((result.get("data") or {}).get("message_id")) or "")

    def send_card(self, chat_id: str, card: dict[str, Any]) -> str:
        """Send a Feishu interactive card to a group chat.

        ``content`` must be a JSON string even though the card itself is a
        nested object.  Keeping this conversion here prevents report builders
        from depending on Feishu's wire format.
        """
        if not chat_id.strip():
            raise FeishuError("请先填写接收报表的飞书群 Chat ID。")
        if not isinstance(card, dict) or not card.get("elements"):
            raise FeishuError("飞书卡片内容为空，无法发送。")
        result = self._request(
            "POST",
            "/im/v1/messages",
            params={"receive_id_type": "chat_id"},
            body={
                "receive_id": chat_id.strip(),
                "msg_type": "interactive",
                "content": json.dumps(card, ensure_ascii=False),
            },
        )
        return str(((result.get("data") or {}).get("message_id")) or "")

    def _table_path(self, app_token: str, table_id: str) -> str:
        if not app_token.strip() or not table_id.strip():
            raise FeishuError("请填写飞书多维表格 App Token 和 Table ID。")
        app = urllib.parse.quote(app_token.strip(), safe="")
        table = urllib.parse.quote(table_id.strip(), safe="")
        return f"/bitable/v1/apps/{app}/tables/{table}"

    def _request(
        self,
        method: str,
        path: str,
        *,
        params: dict[str, Any] | None = None,
        body: dict[str, Any] | None = None,
        authenticated: bool = True,
    ) -> dict[str, Any]:
        url = self.base_url + path
        if params:
            url += "?" + urllib.parse.urlencode(params)
        headers: dict[str, str] = {}
        if authenticated:
            headers["Authorization"] = "Bearer " + self.tenant_access_token()
        result = self.http.request(method, url, headers=headers, body=body, timeout=30)
        code = int(result.get("code") or 0)
        if code != 0:
            message = str(result.get("msg") or result.get("message") or "未知错误")
            raise FeishuError(f"飞书 API 错误 {code}：{message}")
        return result


def field_text(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, str):
        return value.strip()
    if isinstance(value, (int, float)):
        return str(value)
    if isinstance(value, list):
        parts = []
        for item in value:
            if isinstance(item, dict):
                parts.append(str(item.get("text") or item.get("name") or item.get("value") or ""))
            else:
                parts.append(str(item))
        return "".join(parts).strip()
    if isinstance(value, dict):
        return str(value.get("text") or value.get("name") or value.get("value") or "").strip()
    return str(value).strip()


def field_number(value: Any) -> float | None:
    if value in (None, "", []):
        return None
    if isinstance(value, (int, float)):
        return float(value)
    text = field_text(value).replace(" ", "").replace(",", ".")
    try:
        return float(text)
    except ValueError:
        return None


def _chunks(values: list[dict[str, Any]], size: int) -> Iterable[list[dict[str, Any]]]:
    for start in range(0, len(values), size):
        yield values[start:start + size]
