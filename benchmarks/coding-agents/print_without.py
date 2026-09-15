#!/usr/bin/env python3
import json
from pathlib import Path

rows = json.loads(
    Path(
        r"C:\Users\SergiiZiborov\Documents\GitHub\MyProjects\sweeploom\file_output\agent_bench\fresh_context_spend.json"
    ).read_text(encoding="utf-8")
)
for row in rows:
    if row["lane"] != "without":
        continue
    print(
        f"{row['task']}|{row['model']}|user={row['user_tok']}|gen={row['gen_tok']}|peak={row['peak_tok']}|tools={row['tools']}|time={row['file_elapsed_seconds']}"
    )
