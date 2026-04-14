"""Abstract base for all LLM providers.

Canonical message format: OpenAI-compatible (system separate, tool calls as
openai tool_calls, tool results as role=tool messages). Each provider converts
internally to its native format.
"""
from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from typing import Any, AsyncGenerator


@dataclass
class ToolCall:
    id: str
    name: str
    arguments: dict[str, Any]


@dataclass
class LLMResponse:
    content: str
    tool_calls: list[ToolCall] = field(default_factory=list)

    @property
    def has_tool_calls(self) -> bool:
        return bool(self.tool_calls)


class BaseLLMProvider(ABC):
    """
    All providers accept messages in OpenAI canonical format and return
    LLMResponse. Tool definitions must use OpenAI function-calling schema:

        {
          "type": "function",
          "function": {
            "name": "...",
            "description": "...",
            "parameters": { "type": "object", "properties": {...}, "required": [...] }
          }
        }
    """

    @abstractmethod
    async def complete(
        self,
        system: str,
        messages: list[dict],
        tools: list[dict],
        max_tokens: int = 4096,
    ) -> LLMResponse:
        """Single completion call. Providers handle format conversion internally."""
        ...

    async def stream_complete(
        self,
        system: str,
        messages: list[dict],
        max_tokens: int = 4096,
    ) -> AsyncGenerator[str, None]:
        """Stream text chunks for no-tool agents.

        Default implementation falls back to complete() and yields the full
        response at once. Providers that support native streaming should override.
        """
        response = await self.complete(system, messages, [], max_tokens)
        yield response.content

    @property
    @abstractmethod
    def model_name(self) -> str: ...
