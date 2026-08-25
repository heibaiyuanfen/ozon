"""OpenAI-compatible client used by the local AI analysis page.

The client deliberately uses only Python's standard library so the desktop
application does not need the OpenAI SDK (or any other runtime dependency).
"""

from __future__ import annotations

import json
import re
import socket
import ssl
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from typing import Any, Callable


DEFAULT_BASE_URL = "https://api.openai.com/v1"
DEFAULT_TIMEOUT = 90


class AIError(RuntimeError):
    """A safe, user-facing error raised for AI API failures."""


@dataclass(frozen=True)
class AIConfig:
    """Connection settings for an OpenAI-compatible relay."""

    base_url: str = DEFAULT_BASE_URL
    api_key: str = ""
    model: str = ""
    timeout: int = DEFAULT_TIMEOUT


def build_chat_completions_url(base_url: str) -> str:
    """Return a Chat Completions endpoint for a base URL or full endpoint.

    Both ``https://host/v1`` and
    ``https://host/v1/chat/completions`` are accepted.  A host-only URL gets
    the conventional ``/v1/chat/completions`` path.
    """

    value = str(base_url or "").strip()
    if not value:
        raise AIError("请先填写 AI API 地址。")
    if "://" not in value:
        # A missing scheme is almost always a pasted host.  Requiring it here
        # prevents urllib from treating the host as a local/relative path.
        raise AIError("AI API 地址无效，请填写以 http:// 或 https:// 开头的地址。")

    parsed = urllib.parse.urlsplit(value)
    if parsed.scheme.lower() not in {"http", "https"} or not parsed.netloc:
        raise AIError("AI API 地址无效，请填写以 http:// 或 https:// 开头的地址。")

    path = parsed.path.rstrip("/")
    lowered = path.lower()
    if lowered.endswith("/chat/completions"):
        endpoint_path = path
    elif not path:
        endpoint_path = "/v1/chat/completions"
    else:
        endpoint_path = path + "/chat/completions"

    return urllib.parse.urlunsplit(
        (parsed.scheme, parsed.netloc, endpoint_path, parsed.query, "")
    )


class AIClient:
    """Small synchronous client for OpenAI-compatible relay services."""

    def __init__(
        self,
        base_url: str = DEFAULT_BASE_URL,
        api_key: str = "",
        model: str = "",
        *,
        timeout: int = DEFAULT_TIMEOUT,
        urlopen: Callable[..., Any] | None = None,
    ) -> None:
        self.base_url = str(base_url or "").strip()
        self.api_key = str(api_key or "").strip()
        self.model = str(model or "").strip()
        self.timeout = max(1, int(timeout))
        # Keeping this injectable makes deterministic offline testing easy.
        self._urlopen = urlopen or urllib.request.urlopen

    @property
    def endpoint(self) -> str:
        return build_chat_completions_url(self.base_url)

    def test_connection(self) -> str:
        """Make a minimal completion request and return a display message."""

        self._complete(
            [
                {
                    "role": "user",
                    "content": "这是连接测试。请只回复：连接成功",
                }
            ]
        )
        return "AI API 连接成功。"

    def analyze(self, prompt: str, data_context: Any = None) -> str:
        """Analyze local Ozon data using the configured chat model.

        ``data_context`` may be a string or any JSON-serializable object.  It
        is sent as text, so the relay never needs direct access to the local
        database.
        """

        question = str(prompt or "").strip()
        if not question:
            raise AIError("请输入要让 AI 分析的问题。")

        context = _format_context(data_context)
        user_content = question
        if context:
            user_content += (
                "\n\n以下是本次分析使用的 Ozon 本地数据（仅依据数据作答；"
                "数据不足时请明确说明）：\n" + context
            )

        return self._complete(
            [
                {
                    "role": "system",
                    "content": (
                        "你是一名 Ozon 电商经营与广告投放分析师。"
                        "请用中文给出清晰、可执行、以数据为依据的结论；"
                        "不要编造输入中不存在的数据。"
                    ),
                },
                {"role": "user", "content": user_content},
            ]
        )

    def _complete(self, messages: list[dict[str, str]]) -> str:
        self._validate_config()
        payload = {"model": self.model, "messages": messages, "stream": False}
        request = urllib.request.Request(
            self.endpoint,
            data=json.dumps(payload, ensure_ascii=False).encode("utf-8"),
            headers={
                "Authorization": f"Bearer {self.api_key}",
                "Accept": "application/json",
                "Content-Type": "application/json",
                "User-Agent": "OzonAnalyticsDesktop/AI",
            },
            method="POST",
        )

        try:
            with self._urlopen(request, timeout=self.timeout) as response:
                raw = response.read()
        except urllib.error.HTTPError as error:
            detail = _read_http_error(error)
            message = _http_error_message(error.code, detail)
            raise AIError(self._safe(message)) from error
        except (urllib.error.URLError, ssl.SSLError, socket.timeout, TimeoutError) as error:
            reason = getattr(error, "reason", error)
            raise AIError(
                self._safe(f"无法连接 AI API，请检查地址和网络后重试。详情：{reason}")
            ) from error
        except OSError as error:
            raise AIError(self._safe(f"AI API 网络请求失败：{error}")) from error

        if not raw:
            raise AIError("AI API 返回了空响应。")
        try:
            response_data = json.loads(raw.decode("utf-8-sig"))
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise AIError("AI API 返回的内容不是有效的 JSON。") from error

        text = _extract_response_text(response_data)
        if not text:
            detail = _extract_api_error(response_data)
            if detail:
                raise AIError(self._safe(f"AI API 请求失败：{detail}"))
            raise AIError("AI API 未返回可显示的文本，请检查所选模型是否支持文本对话。")
        return text

    def _validate_config(self) -> None:
        # Resolve the endpoint here as validation, before constructing headers.
        build_chat_completions_url(self.base_url)
        if not self.api_key:
            raise AIError("请先填写 AI API Key。")
        if not self.model:
            raise AIError("请先填写 AI 模型名称。")

    def _safe(self, message: Any) -> str:
        return _redact_secret(str(message), self.api_key)


def _format_context(data_context: Any) -> str:
    if data_context is None:
        return ""
    if isinstance(data_context, str):
        return data_context.strip()
    try:
        return json.dumps(data_context, ensure_ascii=False, indent=2, default=str)
    except (TypeError, ValueError, RecursionError):
        return str(data_context).strip()


def _extract_response_text(payload: Any) -> str:
    if not isinstance(payload, dict):
        return ""

    # Chat Completions format.
    choices = payload.get("choices")
    if isinstance(choices, list) and choices:
        choice = choices[0]
        if isinstance(choice, dict):
            message = choice.get("message")
            if isinstance(message, dict):
                text = _content_to_text(message.get("content"))
                if text:
                    return text
            text = _content_to_text(choice.get("text"))
            if text:
                return text

    # Some OpenAI-compatible relays expose the SDK convenience property in
    # their JSON response even though the official REST object uses `output`.
    text = _content_to_text(payload.get("output_text"))
    if text:
        return text

    # Responses API wire format: output[].content[].text.
    output = payload.get("output")
    if isinstance(output, list):
        parts: list[str] = []
        for item in output:
            if not isinstance(item, dict):
                continue
            text = _content_to_text(item.get("content"))
            if text:
                parts.append(text)
        if parts:
            return "\n".join(parts).strip()

    # A few relays wrap the upstream JSON in `data` or `response`.
    for key in ("data", "response", "result"):
        nested = payload.get(key)
        if isinstance(nested, dict):
            text = _extract_response_text(nested)
            if text:
                return text
    return ""


def _content_to_text(content: Any) -> str:
    if isinstance(content, str):
        return content.strip()
    if isinstance(content, dict):
        value = content.get("text")
        if isinstance(value, dict):
            value = value.get("value")
        if isinstance(value, str):
            return value.strip()
        value = content.get("value")
        return value.strip() if isinstance(value, str) else ""
    if isinstance(content, list):
        parts = [_content_to_text(item) for item in content]
        return "\n".join(part for part in parts if part).strip()
    return ""


def _read_http_error(error: urllib.error.HTTPError) -> str:
    try:
        raw = error.read(32_768)
    except (OSError, TypeError):
        return ""
    try:
        payload = json.loads(raw.decode("utf-8-sig", errors="replace"))
    except json.JSONDecodeError:
        return raw.decode("utf-8", errors="replace").strip()[:500]
    return _extract_api_error(payload)


def _extract_api_error(payload: Any) -> str:
    if isinstance(payload, str):
        return payload.strip()[:500]
    if not isinstance(payload, dict):
        return ""
    error = payload.get("error")
    if isinstance(error, dict):
        for key in ("message", "detail", "code", "type"):
            value = error.get(key)
            if value not in (None, ""):
                return str(value)[:500]
    elif error not in (None, ""):
        return str(error)[:500]
    for key in ("message", "detail", "error_description"):
        value = payload.get(key)
        if value not in (None, ""):
            return str(value)[:500]
    return ""


def _http_error_message(status: int, detail: str) -> str:
    suffix = f" 服务器信息：{detail}" if detail else ""
    if status in {401, 403}:
        return f"AI API 认证失败（HTTP {status}），请检查 API Key 和模型权限。{suffix}"
    if status == 404:
        return "AI API 地址不存在（HTTP 404），请检查中转站地址是否包含 /v1。" + suffix
    if status == 429:
        return "AI API 请求过于频繁或额度不足（HTTP 429），请稍后重试或检查账户额度。" + suffix
    if status >= 500:
        return f"AI API 服务暂时不可用（HTTP {status}），请稍后重试。{suffix}"
    return f"AI API 请求失败（HTTP {status}）。{suffix}"


def _redact_secret(message: str, secret: str) -> str:
    safe = message
    if secret:
        safe = safe.replace(secret, "***")
        # Some servers echo a URL-encoded value.
        safe = safe.replace(urllib.parse.quote(secret, safe=""), "***")
    safe = re.sub(r"(?i)Bearer\s+[A-Za-z0-9._~+/=-]+", "Bearer ***", safe)
    safe = re.sub(
        r'(?i)(["\']?(?:api[_-]?key|token)["\']?\s*[:=]\s*["\']?)[^\s,"\']+',
        r"\1***",
        safe,
    )
    return safe


# Friendly aliases for callers that prefer a more explicit name.
OpenAICompatibleClient = AIClient
OpenAIClient = AIClient


__all__ = [
    "AIClient",
    "AIConfig",
    "AIError",
    "OpenAIClient",
    "OpenAICompatibleClient",
    "build_chat_completions_url",
]
