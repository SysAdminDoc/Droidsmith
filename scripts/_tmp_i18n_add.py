import re
import sys
import json as _json

# Usage: the SPEC dict below drives insertion into a single nested block.
# anchor_open must match the block's opening line (stripped), e.g. '"layout": {'.
# indent is the number of spaces for the inserted keys.

SPEC = _json.loads(sys.argv[1])
anchor_open = SPEC["anchor_open"]
guard = SPEC["guard"]
indent = SPEC.get("indent", 6)
new_keys = SPEC["keys"]


def esc(s):
    return s.replace(chr(92), chr(92) * 2).replace('"', chr(92) + '"')


for loc, pairs in new_keys.items():
    path = f"src/locales/{loc}.json"
    with open(path, "rb") as f:
        raw = f.read()
    assert b"\r\n" not in raw, f"{loc} already has CRLF"
    text = raw.decode("utf-8")
    lines = text.split("\n")
    idx = None
    for i, l in enumerate(lines):
        if l.strip() == anchor_open:
            idx = i
            break
    assert idx is not None, f"block {anchor_open!r} not found in {loc}"
    # Guard check is scoped to the target block so a same-named key in another
    # block does not cause a false "already inserted" skip.
    depth = 0
    block_end = idx
    for j in range(idx, len(lines)):
        depth += lines[j].count("{") - lines[j].count("}")
        if depth == 0 and j > idx:
            block_end = j
            break
    if any(guard in lines[k] for k in range(idx, block_end + 1)):
        print(f"{loc}: already inserted, skipping")
        continue
    pad = " " * indent
    add = [f'{pad}"{k}": "{esc(v)}",' for k, v in pairs]
    lines[idx + 1 : idx + 1] = add
    with open(path, "wb") as f:
        f.write(("\n".join(lines)).encode("utf-8"))
    print(f"{loc}: inserted {len(pairs)} keys into {anchor_open}")
