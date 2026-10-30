#!/usr/bin/env python3
"""Fail when the Rust sources use a symbolic icon that has no bundled SVG.

Every `"<name>-symbolic"` string literal in crates/ and src/ must have a matching
`src/resources/icons/scalable/actions/<name>-symbolic.svg`, so the icon renders even when the
system icon theme cannot provide it. gen-gresource.py bundles the files automatically.

Usage: check-icons.py <project root>
"""
import re
import sys
from pathlib import Path

LITERAL = re.compile(r'"([a-z0-9]+(?:-[a-z0-9]+)*-symbolic)"')


def used(root):
    names = {}
    for base in ("crates", "src"):
        for path in sorted((root / base).rglob("*.rs")):
            for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
                for name in LITERAL.findall(line):
                    names.setdefault(name, f"{path.relative_to(root)}:{number}")
    return names


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    root = Path(sys.argv[1])
    icons = root / "src" / "resources" / "icons" / "scalable" / "actions"
    missing = {n: w for n, w in used(root).items() if not (icons / f"{n}.svg").is_file()}
    for name, where in sorted(missing.items()):
        print(f"check-icons: {name}.svg is missing (used at {where})", file=sys.stderr)
    if missing:
        sys.exit(1)


if __name__ == "__main__":
    main()
