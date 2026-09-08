"""Disposable MCP edit entry point used only by the native acceptance fixture."""
from pathlib import Path
import sys
from mcp.server.fastmcp import FastMCP

root = Path(sys.argv[1]).resolve(strict=True)
if not (root.parent.name.startswith("lsp-native-") and root.name == "workspace with spaces"):
    raise RuntimeError("This edit fixture accepts only its disposable native test workspace")
server = FastMCP("lsp-edit-fixture")


@server.tool()
def rename_with_error() -> dict[str, str | list[str]]:
    """Rename index.ts and create a dependent source, with an intentional error."""
    source, target = root / "index.ts", root / "renamed.ts"
    if target.exists() or not source.is_file():
        raise ValueError("Unexpected fixture state before rename")
    _ = source.rename(target)
    _ = target.write_text('export const value: number = "wrong";\n', encoding="utf-8")
    _ = (root / "consumer.ts").write_text('import { value } from "./renamed";\nexport const answer: number = value;\n', encoding="utf-8")
    return {"sentinel": "MCP_RENAME_SENTINEL", "renamed": ["index.ts", "renamed.ts"], "created": "consumer.ts"}


@server.tool()
def correct_renamed_source() -> dict[str, str]:
    """Correct the intentional TypeScript error while retaining the renamed file."""
    if not (root / "renamed.ts").is_file() or (root / "index.ts").exists():
        raise ValueError("Unexpected fixture state before correction")
    _ = (root / "renamed.ts").write_text('export const value: number = 2;\n', encoding="utf-8")
    return {"sentinel": "MCP_CORRECTION_SENTINEL", "corrected": "renamed.ts"}


if __name__ == "__main__":
    server.run(transport="stdio")
