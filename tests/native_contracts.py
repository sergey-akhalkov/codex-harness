"""JSON contracts consumed by the native MCP acceptance scripts."""
from typing import NotRequired, TypeAlias, TypedDict

Json: TypeAlias = str | int | float | bool | None | list['Json'] | dict[str, 'Json']


class Dependency(TypedDict):
    id: str
    paths: dict[str, str]
    shared_service: NotRequired[dict[str, str]]


class Inventory(TypedDict):
    mcp: list[Dependency]
