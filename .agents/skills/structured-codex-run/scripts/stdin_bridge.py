"""Feed exact UTF-8 stdin under the existing observer's owned process job."""
import json
from pathlib import Path
import subprocess
import sys
import threading
import time
import os


def main():
    request = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    env = os.environ.copy()
    if request.get("codex_home"):
        env["CODEX_HOME"] = request["codex_home"]
    child = subprocess.Popen(request["argv"], stdin=subprocess.PIPE, env=env)
    stdin = child.stdin
    assert stdin is not None
    errors = []

    def feed():
        try:
            stdin.write(Path(request["stdin"]).read_bytes())
            stdin.close()
        except (OSError, ValueError) as error:
            errors.append(str(error))

    writer = threading.Thread(target=feed, daemon=True)
    writer.start()
    final = Path(request["final"])
    while child.poll() is None:
        if final.exists() and final.stat().st_size > request["output_limit"]:
            # Exiting the bridge lets the observer close its job and reap descendants.
            Path(request["limit_marker"]).write_text("final output limit", encoding="utf-8")
            return 125
        time.sleep(.05)
    writer.join(timeout=1)
    if child.returncode == 0 and (writer.is_alive() or errors):
        print("stdin delivery failed", file=sys.stderr)
        return 124
    return child.returncode


if __name__ == "__main__":
    raise SystemExit(main())
