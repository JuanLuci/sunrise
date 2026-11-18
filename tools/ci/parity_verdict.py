#!/usr/bin/env python3
"""Reduce this attempt's private parity log to harness names and outcomes for the daily summary."""
import json
import re
import shutil
import sys
from pathlib import Path

RECORD = re.compile(r"^(PASS|FAIL) ([a-z0-9_]+(?:#\d+)?) \d+s(?: |$)")
FINISHED = re.compile(r"^(\d+) passed, (\d+) failed in [\d.]+ min\.$")


def verdict(log, expected, status):
    """Latest outcome per selected shard. A footer alone does not prove the selected set ran."""
    labels, counts = {}, None
    for line in log.splitlines():
        if (found := RECORD.match(line)) and found[2] in expected:
            labels[found[2]] = found[1] == "PASS"
        if finished := FINISHED.fullmatch(line):
            counts = (int(finished[1]), int(finished[2]))
    passes = sum(labels.values())
    failures = len(labels) - passes
    complete = (bool(expected) and labels.keys() == expected and counts == (passes, failures)
                and status == str(int(failures != 0)))
    harnesses = {}
    for label, passed in labels.items():
        name = label.split("#", 1)[0]
        harnesses[name] = harnesses.get(name, True) and passed
    # An incomplete run proves no harness passed: its passes stay out of the uploaded verdict.
    return ({"passed": sorted(name for name, passed in harnesses.items() if passed) if complete else [],
             "failed": sorted(name for name, passed in harnesses.items() if not passed),
             "complete": complete}, labels)


def main(folder, report):
    folder = Path(folder)
    run_id, run_attempt, commit = (folder / "scope").read_text().splitlines()
    log, selection, exit_status = (folder / name for name in ("parity.log", "selection.json", "exit"))
    result, labels = verdict(log.read_text(errors="replace") if log.is_file() else "",
                             set(json.loads(selection.read_text())) if selection.is_file() else set(),
                             exit_status.read_text().strip() if exit_status.is_file() else "")
    result["scope"] = {"run_id": run_id, "run_attempt": run_attempt, "commit": commit}
    Path(report).write_text(json.dumps(result, indent=2) + "\n")
    for name in result["passed"] if result["complete"] else []:
        print(f"PASS {name}")
    for name in result["failed"]:
        print(f"FAIL {name}")

    # Preserve failed-shard and first-retry logs across reuse of the slot's scratch directory.
    logs = Path(".local/parity-runner/logs")
    for path in [*logs.glob("*.first-run.log"), *(logs / f"{label}.log" for label, passed in labels.items() if not passed)]:
        if path.is_file():
            shutil.copyfile(path, folder / path.name)


if __name__ == "__main__":
    main(*sys.argv[1:])
