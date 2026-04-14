"""Provider-agnostic agent loop.

Uses OpenAI message format as the canonical representation for the conversation
history. Each provider converts internally when it needs to call its API.

Tool schema expected in OpenAI function-calling format:
    {"type": "function", "function": {"name": ..., "description": ..., "parameters": {...}}}
"""
from __future__ import annotations

import asyncio
import json
from typing import AsyncGenerator, Callable

from providers.base import BaseLLMProvider
from skills.akshare_tools import execute_tool


async def run_agent(
    provider: BaseLLMProvider,
    system_prompt: str,
    user_message: str,
    tools: list[dict],
    max_tokens: int = 4096,
    tool_executor: Callable[[str, dict], str] = execute_tool,
) -> str:
    """
    Run a single agent to completion.

    Loops until the LLM stops requesting tool calls, executing each tool in a
    thread-pool executor (akshare is synchronous/blocking).

    Returns the final text response.
    """
    messages: list[dict] = [{"role": "user", "content": user_message}]
    loop = asyncio.get_event_loop()

    while True:
        response = await provider.complete(
            system=system_prompt,
            messages=messages,
            tools=tools,
            max_tokens=max_tokens,
        )

        if not response.has_tool_calls:
            return response.content

        # ── Append assistant turn with tool calls (OpenAI format) ──────────────
        assistant_msg: dict = {
            "role": "assistant",
            "content": response.content or None,
            "tool_calls": [
                {
                    "id": tc.id,
                    "type": "function",
                    "function": {
                        "name": tc.name,
                        "arguments": json.dumps(tc.arguments, ensure_ascii=False),
                    },
                }
                for tc in response.tool_calls
            ],
        }
        messages.append(assistant_msg)

        # ── Execute tools concurrently ─────────────────────────────────────────
        async def _exec(name: str, args: dict) -> str:
            return await loop.run_in_executor(None, tool_executor, name, args)

        results = await asyncio.gather(
            *[_exec(tc.name, tc.arguments) for tc in response.tool_calls]
        )

        # ── Append tool results (OpenAI format, one message per result) ─────────
        for tc, result in zip(response.tool_calls, results):
            messages.append(
                {
                    "role": "tool",
                    "tool_call_id": tc.id,
                    "name": tc.name,
                    "content": str(result),
                }
            )


async def run_agent_stream(
    provider: BaseLLMProvider,
    system_prompt: str,
    user_message: str,
    max_tokens: int = 4096,
) -> AsyncGenerator[str, None]:
    """Stream text chunks from a no-tool agent via the provider's stream_complete."""
    messages = [{"role": "user", "content": user_message}]
    async for chunk in provider.stream_complete(
        system=system_prompt,
        messages=messages,
        max_tokens=max_tokens,
    ):
        yield chunk
