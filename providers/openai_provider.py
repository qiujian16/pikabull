"""OpenAI-compatible provider.

Works for:
  - OpenAI API          (model=gpt-4o, api_key=sk-...)
  - Hermes via Ollama   (model=hermes3, base_url=http://localhost:11434/v1, api_key=ollama)
  - Any OpenAI-compat   endpoint (just set base_url + model)
"""
from __future__ import annotations

import json

from openai import AsyncOpenAI

from .base import BaseLLMProvider, LLMResponse, ToolCall


class OpenAIProvider(BaseLLMProvider):
    def __init__(
        self,
        model: str = "gpt-4o",
        api_key: str | None = None,
        base_url: str | None = None,
    ):
        self._model = model
        self._client = AsyncOpenAI(api_key=api_key, base_url=base_url)

    @property
    def model_name(self) -> str:
        return self._model

    async def complete(
        self,
        system: str,
        messages: list[dict],
        tools: list[dict],
        max_tokens: int = 4096,
    ) -> LLMResponse:
        all_messages = [{"role": "system", "content": system}, *messages]

        kwargs: dict = dict(
            model=self._model,
            max_tokens=max_tokens,
            messages=all_messages,
        )
        if tools:
            kwargs["tools"] = tools

        response = await self._client.chat.completions.create(**kwargs)
        choice = response.choices[0]
        msg = choice.message

        text = msg.content or ""
        tool_calls: list[ToolCall] = []
        if msg.tool_calls:
            for tc in msg.tool_calls:
                tool_calls.append(
                    ToolCall(
                        id=tc.id,
                        name=tc.function.name,
                        arguments=json.loads(tc.function.arguments),
                    )
                )

        return LLMResponse(content=text, tool_calls=tool_calls)

    async def stream_complete(self, system, messages, max_tokens=4096):
        all_messages = [{"role": "system", "content": system}, *messages]
        stream = await self._client.chat.completions.create(
            model=self._model,
            max_tokens=max_tokens,
            messages=all_messages,
            stream=True,
        )
        async for chunk in stream:
            delta = chunk.choices[0].delta
            if delta.content:
                yield delta.content
