"""Anthropic (Claude) provider.

Converts OpenAI canonical message format ↔ Anthropic format internally.
"""
from __future__ import annotations

import json
from typing import Any

import anthropic

from .base import BaseLLMProvider, LLMResponse, ToolCall


class AnthropicProvider(BaseLLMProvider):
    def __init__(self, model: str = "claude-sonnet-4-6", api_key: str | None = None):
        self._model = model
        self._client = anthropic.AsyncAnthropic(api_key=api_key)

    @property
    def model_name(self) -> str:
        return self._model

    # ── Format conversion ──────────────────────────────────────────────────────

    @staticmethod
    def _tools_to_anthropic(tools: list[dict]) -> list[dict]:
        """OpenAI function schema → Anthropic tool schema."""
        result = []
        for t in tools:
            fn = t["function"]
            result.append(
                {
                    "name": fn["name"],
                    "description": fn.get("description", ""),
                    "input_schema": fn["parameters"],
                }
            )
        return result

    @staticmethod
    def _messages_to_anthropic(messages: list[dict]) -> list[dict]:
        """Convert OpenAI-format messages to Anthropic format.

        Rules:
        - role=user  → {"role": "user", "content": str}
        - role=assistant with tool_calls → {"role": "assistant", "content": [tool_use blocks]}
        - role=assistant text only → {"role": "assistant", "content": str}
        - role=tool (one or more) → grouped into a single {"role": "user", "content": [tool_result blocks]}
        """
        result: list[dict] = []
        i = 0
        while i < len(messages):
            msg = messages[i]
            role = msg["role"]

            if role == "user":
                result.append({"role": "user", "content": msg["content"]})
                i += 1

            elif role == "assistant":
                tool_calls = msg.get("tool_calls") or []
                if tool_calls:
                    content_blocks: list[dict] = []
                    if msg.get("content"):
                        content_blocks.append({"type": "text", "text": msg["content"]})
                    for tc in tool_calls:
                        content_blocks.append(
                            {
                                "type": "tool_use",
                                "id": tc["id"],
                                "name": tc["function"]["name"],
                                "input": json.loads(tc["function"]["arguments"]),
                            }
                        )
                    result.append({"role": "assistant", "content": content_blocks})
                else:
                    result.append(
                        {"role": "assistant", "content": msg.get("content", "")}
                    )
                i += 1

            elif role == "tool":
                # Group all consecutive tool-result messages into one user turn
                tool_results: list[dict] = []
                while i < len(messages) and messages[i]["role"] == "tool":
                    tr = messages[i]
                    tool_results.append(
                        {
                            "type": "tool_result",
                            "tool_use_id": tr["tool_call_id"],
                            "content": tr["content"],
                        }
                    )
                    i += 1
                result.append({"role": "user", "content": tool_results})

            else:
                i += 1  # skip unknown roles

        return result

    # ── Core completion ────────────────────────────────────────────────────────

    async def complete(
        self,
        system: str,
        messages: list[dict],
        tools: list[dict],
        max_tokens: int = 4096,
    ) -> LLMResponse:
        anthropic_messages = self._messages_to_anthropic(messages)
        kwargs: dict[str, Any] = dict(
            model=self._model,
            max_tokens=max_tokens,
            system=system,
            messages=anthropic_messages,
        )
        if tools:
            kwargs["tools"] = self._tools_to_anthropic(tools)

        response = await self._client.messages.create(**kwargs)

        text = ""
        tool_calls: list[ToolCall] = []
        for block in response.content:
            if hasattr(block, "text"):
                text += block.text
            elif block.type == "tool_use":
                tool_calls.append(
                    ToolCall(id=block.id, name=block.name, arguments=block.input)
                )

        return LLMResponse(content=text, tool_calls=tool_calls)

    async def stream_complete(self, system, messages, max_tokens=4096):
        anthropic_messages = self._messages_to_anthropic(messages)
        async with self._client.messages.stream(
            model=self._model,
            max_tokens=max_tokens,
            system=system,
            messages=anthropic_messages,
        ) as stream:
            async for text in stream.text_stream:
                yield text
