from __future__ import annotations

import io
import json
import urllib.error
import unittest

from ozon_app.ai import (
    AIClient,
    AIError,
    _extract_response_text,
    build_chat_completions_url,
)


class FakeResponse:
    def __init__(self, payload):
        self.raw = json.dumps(payload, ensure_ascii=False).encode("utf-8")

    def __enter__(self):
        return self

    def __exit__(self, *_args):
        return False

    def read(self):
        return self.raw


class AIClientTests(unittest.TestCase):
    def test_build_endpoint_from_v1_base(self) -> None:
        self.assertEqual(
            build_chat_completions_url("https://api.gpt.ge/v1/"),
            "https://api.gpt.ge/v1/chat/completions",
        )

    def test_preserves_complete_endpoint_and_query(self) -> None:
        self.assertEqual(
            build_chat_completions_url(
                "https://relay.example/v1/chat/completions?channel=default"
            ),
            "https://relay.example/v1/chat/completions?channel=default",
        )

    def test_host_only_url_gets_v1(self) -> None:
        self.assertEqual(
            build_chat_completions_url("https://relay.example"),
            "https://relay.example/v1/chat/completions",
        )

    def test_analyze_posts_chat_completion_with_authorization(self) -> None:
        captured = {}

        def urlopen(request, timeout):
            captured["url"] = request.full_url
            captured["timeout"] = timeout
            captured["authorization"] = request.get_header("Authorization")
            captured["content_type"] = request.get_header("Content-type")
            captured["body"] = json.loads(request.data.decode("utf-8"))
            return FakeResponse(
                {"choices": [{"message": {"content": "广告表现良好。"}}]}
            )

        client = AIClient(
            "https://api.gpt.ge/v1", "secret-key", "gpt-test",
            timeout=12, urlopen=urlopen,
        )
        result = client.analyze("请分析", {"acos": 10.5, "sales": 1000})

        self.assertEqual(result, "广告表现良好。")
        self.assertEqual(captured["url"], "https://api.gpt.ge/v1/chat/completions")
        self.assertEqual(captured["timeout"], 12)
        self.assertEqual(captured["authorization"], "Bearer secret-key")
        self.assertEqual(captured["content_type"], "application/json")
        self.assertEqual(captured["body"]["model"], "gpt-test")
        self.assertFalse(captured["body"]["stream"])
        self.assertEqual(captured["body"]["messages"][0]["role"], "system")
        user_text = captured["body"]["messages"][1]["content"]
        self.assertIn("请分析", user_text)
        self.assertIn('"acos": 10.5', user_text)

    def test_test_connection_uses_minimal_request(self) -> None:
        captured = {}

        def urlopen(request, timeout):
            captured["body"] = json.loads(request.data.decode("utf-8"))
            return FakeResponse({"choices": [{"message": {"content": "连接成功"}}]})

        result = AIClient(
            "https://relay.example/v1", "key", "model", urlopen=urlopen
        ).test_connection()
        self.assertIn("连接成功", result)
        self.assertEqual(len(captured["body"]["messages"]), 1)

    def test_extracts_responses_output_text(self) -> None:
        self.assertEqual(
            _extract_response_text({"output_text": "这是 Responses 内容"}),
            "这是 Responses 内容",
        )

    def test_extracts_responses_output_array(self) -> None:
        payload = {
            "output": [
                {
                    "type": "message",
                    "content": [
                        {"type": "output_text", "text": "第一段"},
                        {"type": "output_text", "text": "第二段"},
                    ],
                }
            ]
        }
        self.assertEqual(_extract_response_text(payload), "第一段\n第二段")

    def test_missing_config_errors_are_in_chinese(self) -> None:
        with self.assertRaisesRegex(AIError, "API Key"):
            AIClient("https://relay.example/v1", "", "model").analyze("分析")
        with self.assertRaisesRegex(AIError, "模型"):
            AIClient("https://relay.example/v1", "key", "").analyze("分析")
        with self.assertRaisesRegex(AIError, "问题"):
            AIClient("https://relay.example/v1", "key", "model").analyze("  ")

    def test_http_error_redacts_api_key(self) -> None:
        api_key = "sk-very-secret-value"

        def urlopen(request, timeout):
            payload = json.dumps(
                {"error": {"message": f"invalid Bearer {api_key}"}}
            ).encode("utf-8")
            raise urllib.error.HTTPError(
                request.full_url,
                401,
                "Unauthorized",
                {},
                io.BytesIO(payload),
            )

        client = AIClient(
            "https://relay.example/v1", api_key, "model", urlopen=urlopen
        )
        with self.assertRaises(AIError) as raised:
            client.analyze("分析")
        message = str(raised.exception)
        self.assertIn("HTTP 401", message)
        self.assertNotIn(api_key, message)
        self.assertIn("***", message)

    def test_non_json_response_has_clear_error(self) -> None:
        class InvalidJsonResponse:
            def __enter__(self):
                return self

            def __exit__(self, *_args):
                return False

            def read(self):
                return b"<html>proxy error</html>"

        client = AIClient(
            "https://relay.example/v1", "key", "model",
            urlopen=lambda *_args, **_kwargs: InvalidJsonResponse(),
        )
        with self.assertRaisesRegex(AIError, "JSON"):
            client.analyze("分析")


if __name__ == "__main__":
    unittest.main()
