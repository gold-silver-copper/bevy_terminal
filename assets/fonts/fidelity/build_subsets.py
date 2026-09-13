#!/usr/bin/env python3
"""Rebuild pinned coverage fixtures; manual maintenance only, never run by CI.

Usage: python3 build_subsets.py SOURCE_DIRECTORY
Install fonttools==4.63.0 first. Downloads are revision/hash pinned in the manifest.
The source directory also supports full-font versus subset raster verification.
"""

import hashlib
import json
from pathlib import Path
import sys
import urllib.request

import fontTools
from fontTools import subset
from fontTools.ttLib import TTFont

assert fontTools.__version__ == "4.63.0", "use fonttools==4.63.0"
root = Path(__file__).resolve().parent
cache = Path(sys.argv[1])
cache.mkdir(parents=True, exist_ok=True)
outputs = {}
for source in json.loads((root / "subset-sources.json").read_text()):
    original = cache / source["input_name"]
    if not original.exists():
        original.write_bytes((root / source["local"]).read_bytes() if "local" in source
                             else urllib.request.urlopen(source["url"]).read())
    assert hashlib.sha256(original.read_bytes()).hexdigest() == source["sha256"]
    font = TTFont(original, recalcTimestamp=False)
    options = subset.Options()
    options.layout_features = ["*"]
    options.name_IDs = ["*"]
    options.name_languages = ["*"]
    options.name_legacy = True
    options.notdef_glyph = True
    options.notdef_outline = True
    sub = subset.Subsetter(options=options)
    sub.populate(text=source["text"])
    sub.subset(font)
    # Give modified fonts their own names, retaining copyright/license records.
    names = {1: source["family"], 2: "Regular", 3: source["family"] + " coverage fixture",
             4: source["family"], 6: source["family"].replace(" ", ""),
             16: source["family"], 17: "Regular"}
    for record in font["name"].names:
        if record.nameID in names:
            record.string = names[record.nameID].encode(record.getEncoding())
    if "CFF " in font:
        cff = font["CFF "].cff
        cff.fontNames = [names[6]]
        cff.topDictIndex[0].FullName = names[4]
        cff.topDictIndex[0].FamilyName = names[1]
    path = root / source["output"]
    font.save(path)
    outputs[path.name] = {"sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                          "bytes": path.stat().st_size}
(root / "subset-hashes.json").write_text(json.dumps(outputs, indent=2) + "\n")
