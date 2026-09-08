from pathlib import Path
import sys
path = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).with_name("delegation-usage.py")
start = int(sys.argv[2]) if len(sys.argv) > 2 else 1
end = int(sys.argv[3]) if len(sys.argv) > 3 else start + 40
lines = path.read_text(encoding="utf-8").splitlines()
for index, line in enumerate(lines[start - 1:end], start=start):
    print(f"{index}:{line}")
