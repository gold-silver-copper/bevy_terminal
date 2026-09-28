#!/usr/bin/env python3
"""Compares two tools/pixel-baseline.sh outputs byte for byte.

Usage: pixel-compare.py <base dir> <head dir>

Compares every .png, .rgba and .scenes file; exits 1 if any differs or
exists on one side only.
"""
import filecmp
import os
import sys

EXTENSIONS = (".png", ".rgba", ".scenes")


def files(root):
    found = set()
    for directory, _, names in os.walk(root):
        for name in names:
            if name.endswith(EXTENSIONS):
                found.add(os.path.relpath(os.path.join(directory, name), root))
    return found


base, head = sys.argv[1], sys.argv[2]
before, after = files(base), files(head)
common = sorted(before & after)
different = [
    path
    for path in common
    if not filecmp.cmp(os.path.join(base, path), os.path.join(head, path), shallow=False)
]
only = sorted(before ^ after)
for path in different:
    print(f"differs: {path}")
for path in only:
    print(f"only in {'base' if path in before else 'head'}: {path}")
groups = {}
for path in common:
    group = path.split(os.sep)[0]
    groups[group] = groups.get(group, 0) + 1
print("compared:", ", ".join(f"{count} {group}" for group, count in sorted(groups.items())))
print(f"{len(common)} files compared, {len(different)} differ, {len(only)} unmatched")
sys.exit(1 if different or only else 0)
