from .base import BaseLLMProvider, LLMResponse, ToolCall
from .anthropic_provider import AnthropicProvider
from .openai_provider import OpenAIProvider


def create_provider(provider_type: str, **kwargs) -> BaseLLMProvider:
    """Factory: create a provider from a type string and kwargs."""
    t = provider_type.lower()
    if t == "anthropic":
        return AnthropicProvider(**kwargs)
    elif t in ("openai", "ollama", "minimax"):
        return OpenAIProvider(**kwargs)
    else:
        raise ValueError(
            f"Unknown provider '{provider_type}'. "
            "Choose: anthropic | openai | ollama | minimax"
        )


def create_provider_from_env() -> BaseLLMProvider:
    """Read environment variables and return the configured provider."""
    import os

    provider = os.getenv("LLM_PROVIDER", "anthropic").lower()

    if provider == "anthropic":
        return AnthropicProvider(
            model=os.getenv("ANTHROPIC_MODEL", "claude-sonnet-4-6"),
            api_key=os.getenv("ANTHROPIC_API_KEY"),
        )
    elif provider == "openai":
        return OpenAIProvider(
            model=os.getenv("OPENAI_MODEL", "gpt-4o"),
            api_key=os.getenv("OPENAI_API_KEY"),
        )
    elif provider == "ollama":
        return OpenAIProvider(
            model=os.getenv("OLLAMA_MODEL", "hermes3"),
            api_key=os.getenv("OLLAMA_API_KEY", "ollama"),
            base_url=os.getenv("OLLAMA_BASE_URL", "http://localhost:11434/v1"),
        )
    elif provider == "minimax":
        return OpenAIProvider(
            model=os.getenv("MINIMAX_MODEL", "MiniMax-M2.7-highspeed"),
            api_key=os.getenv("MINIMAX_API_KEY"),
            base_url=os.getenv("MINIMAX_BASE_URL", "https://api.minimax.chat/v1"),
        )
    else:
        raise ValueError(f"Unknown LLM_PROVIDER='{provider}'")
