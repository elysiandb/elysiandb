import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
GROUPS_FILE = ROOT / ".github" / "ci" / "test-groups"
EXCLUSIONS_FILE = ROOT / ".github" / "ci" / "uninstrumented-tests"
CODECOV_FILE = ROOT / "codecov.yml"
REST = "*"
UNINSTRUMENTED = "uninstrumented"
SUPPORTED_KINDS = {"lib", "bin", "test"}

ANSI = re.compile(r"\x1b\[[0-9;]*[A-Za-z]")
RUNNING = re.compile(r"^\s+Running (?:unittests )?(\S+) \((\S+)\)\s*$")
DOC_TESTS = re.compile(r"^\s+Doc-tests (\S+)\s*$")
RESULT = re.compile(
    r"^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; "
    r"(\d+) measured; (\d+) filtered out;"
)
LISTED = re.compile(r"^(\d+) tests?, (\d+) benchmarks?\s*$")
HASH = re.compile(r"-[0-9a-f]{16}$")


class Failure(Exception):
    pass


def metadata():
    done = subprocess.run(
        [
            "cargo",
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
            str(ROOT / "Cargo.toml"),
        ],
        capture_output=True,
        text=True,
    )
    if done.returncode:
        raise Failure(f"cargo metadata failed: {done.stderr.strip()}")
    return json.loads(done.stdout)


def key(src, name):
    return f"{src}|{name}"


def workspace_targets():
    targets = []
    doc = set()
    for package in metadata()["packages"]:
        root = Path(package["manifest_path"]).parent
        for target in package["targets"]:
            kinds = set(target["kind"])
            if kinds == {"custom-build"}:
                continue
            if not target.get("test", False):
                continue
            if not kinds <= SUPPORTED_KINDS or len(kinds) != 1:
                raise Failure(
                    f"{package['name']}: target {target['name']} of kind "
                    f"{sorted(kinds)} has tests but no CI group can select it"
                )
            kind = kinds.pop()
            src = os.path.relpath(target["src_path"], root)
            name = target["name"].replace("-", "_")
            targets.append(
                {
                    "package": package["name"],
                    "kind": kind,
                    "name": target["name"],
                    "key": key(src, name),
                }
            )
            if kind == "lib" and target.get("doctest", False):
                doc.add(key("doc", name))
    keys = [t["key"] for t in targets]
    duplicates = sorted({k for k in keys if keys.count(k) > 1})
    if duplicates:
        raise Failure(f"test binaries that cannot be told apart: {duplicates}")
    names = [t["name"] for t in targets if t["kind"] == "test"]
    shared = sorted({n for n in names if names.count(n) > 1})
    if shared:
        raise Failure(f"integration tests sharing a name across packages: {shared}")
    return targets, doc


def read_groups():
    groups = {}
    order = []
    current = None
    for number, raw in enumerate(GROUPS_FILE.read_text().splitlines(), 1):
        line = raw.strip()
        if not line:
            continue
        header = re.fullmatch(r"\[([a-z0-9][a-z0-9-]*)\]", line)
        if header:
            current = header.group(1)
            if current == UNINSTRUMENTED or current in groups:
                raise Failure(f"{GROUPS_FILE.name}:{number}: group {current} is not available")
            groups[current] = []
            order.append(current)
            continue
        if current is None or not re.fullmatch(r"[A-Za-z0-9_*-]+", line):
            raise Failure(f"{GROUPS_FILE.name}:{number}: unexpected line {raw!r}")
        groups[current].append(line)
    if not groups:
        raise Failure(f"{GROUPS_FILE.name}: no group")
    rest = [g for g in order if REST in groups[g]]
    if len(rest) != 1 or groups[rest[0]] != [REST]:
        raise Failure(f"{GROUPS_FILE.name}: exactly one group must hold {REST} and nothing else")
    return order, groups, rest[0]


def plan():
    targets, doc = workspace_targets()
    order, groups, rest = read_groups()
    tests = {t["name"]: t for t in targets if t["kind"] == "test"}
    owner = {}
    for group in order:
        for name in groups[group]:
            if name == REST:
                continue
            if name not in tests:
                raise Failure(f"group {group}: {name} is not an integration test of the workspace")
            if name in owner:
                raise Failure(f"{name} is listed in groups {owner[name]} and {group}")
            owner[name] = group
    members = {group: [] for group in order}
    args = {group: [] for group in order}
    for target in targets:
        group = owner.get(target["name"], rest) if target["kind"] == "test" else rest
        members[group].append(target["key"])
    for group in order:
        if group == rest:
            args[group] = ["--lib", "--bins"] + [
                arg
                for t in targets
                if t["kind"] == "test" and t["name"] not in owner
                for arg in ("--test", t["name"])
            ]
        else:
            args[group] = [arg for name in groups[group] for arg in ("--test", name)]
        if not members[group]:
            raise Failure(f"group {group} selects no test binary")
    return {
        "order": order,
        "members": members,
        "args": args,
        "all": {t["key"] for t in targets},
        "doc": doc,
    }


def exclusions():
    patterns = []
    for number, raw in enumerate(EXCLUSIONS_FILE.read_text().splitlines(), 1):
        line = raw.strip()
        if not line:
            continue
        if line.startswith("-") or re.search(r"\s", line):
            raise Failure(f"{EXCLUSIONS_FILE.name}:{number}: unexpected pattern {raw!r}")
        if line in patterns:
            raise Failure(f"{EXCLUSIONS_FILE.name}:{number}: {line} is listed twice")
        patterns.append(line)
    return patterns


def check_codecov(count):
    found = [int(n) for n in re.findall(r"after_n_builds:\s*(\d+)", CODECOV_FILE.read_text())]
    if len(found) < 2 or any(n != count for n in found):
        raise Failure(
            f"{CODECOV_FILE.name}: codecov.notify.after_n_builds and comment.after_n_builds "
            f"must both be {count}, the number of coverage groups, found {found}"
        )


def parse(path, list_mode):
    blocks = {}
    current = None
    pattern = LISTED if list_mode else RESULT
    for raw in Path(path).read_text(errors="replace").splitlines():
        line = ANSI.sub("", raw).rstrip("\r")
        running = RUNNING.match(line)
        doc = DOC_TESTS.match(line)
        if running or doc:
            if current is not None and current not in blocks:
                raise Failure(f"{path}: {current} reported no result")
            if running:
                name = HASH.sub("", Path(running.group(2)).name)
                current = key(running.group(1), name)
            else:
                current = key("doc", doc.group(1).replace("-", "_"))
            if current in blocks:
                raise Failure(f"{path}: {current} ran twice")
            continue
        match = pattern.match(line)
        if match:
            if current is None or current in blocks:
                raise Failure(f"{path}: a result line belongs to no test binary: {line!r}")
            numbers = [int(n) for n in match.groups()]
            if list_mode:
                blocks[current] = {"listed": numbers[0], "benchmarks": numbers[1]}
            else:
                passed, failed, ignored, measured, filtered = numbers
                blocks[current] = {
                    "passed": passed,
                    "failed": failed,
                    "ignored": ignored,
                    "measured": measured,
                    "filtered": filtered,
                }
    if current is not None and current not in blocks:
        raise Failure(f"{path}: {current} reported no result")
    if not blocks:
        raise Failure(f"{path}: no test binary ran")
    return blocks


def summarize(name, logs):
    summary = {"name": name}
    for item in logs:
        kind, _, path = item.partition("=")
        if kind not in {"run", "list", "ignored", "doc"} or not path:
            raise Failure(f"unexpected log argument {item!r}")
        summary[kind] = parse(path, kind in {"list", "ignored"})
    print(json.dumps(summary, indent=1, sort_keys=True))


def verify(directory):
    p = plan()
    check_codecov(len(p["order"]))
    found = {}
    for path in sorted(Path(directory).glob("*.json")):
        data = json.loads(path.read_text())
        if data["name"] in found:
            raise Failure(f"two summaries for {data['name']}")
        found[data["name"]] = data
    expected = set(p["order"]) | {UNINSTRUMENTED}
    if set(found) != expected:
        raise Failure(
            f"summaries found for {sorted(found)}, expected {sorted(expected)}"
        )
    for name, data in found.items():
        needed = {"list", "ignored", "run", "doc"} if name == UNINSTRUMENTED else {"run"}
        if not needed <= set(data):
            raise Failure(f"the summary of {name} lacks {sorted(needed - set(data))}")
    errors = []
    u = found[UNINSTRUMENTED]
    for kind in ("list", "ignored", "run"):
        if set(u[kind]) != p["all"]:
            missing = sorted(p["all"] - set(u[kind]))
            extra = sorted(set(u[kind]) - p["all"])
            errors.append(f"uninstrumented {kind}: missing {missing}, unexpected {extra}")
    if set(u["doc"]) != p["doc"]:
        errors.append(f"doc tests ran for {sorted(u['doc'])}, expected {sorted(p['doc'])}")
    owner = {}
    for group in p["order"]:
        ran = set(found[group]["run"])
        members = set(p["members"][group])
        if ran != members:
            errors.append(
                f"group {group}: missing {sorted(members - ran)}, unexpected {sorted(ran - members)}"
            )
        for k in ran & members:
            owner[k] = group
    totals = {group: {"binaries": 0, "executed": 0, "ignored": 0, "skipped": 0} for group in p["order"]}
    listed_total = ignored_total = executed_total = uninstrumented_total = 0
    for k in sorted(p["all"]):
        if k not in owner or k not in u["list"] or k not in u["ignored"] or k not in u["run"]:
            continue
        g = found[owner[k]]["run"][k]
        r = u["run"][k]
        listed = u["list"][k]["listed"]
        ignored = u["ignored"][k]["listed"]
        for label, block in ((owner[k], g), (UNINSTRUMENTED, r)):
            if block["failed"] or block["measured"]:
                errors.append(f"{label}: {k} reported {block['failed']} failed, {block['measured']} measured")
        if r["passed"] + r["ignored"] + r["filtered"] != listed:
            errors.append(f"{k}: {listed} tests listed, the uninstrumented job accounts for "
                          f"{r['passed'] + r['ignored'] + r['filtered']}")
        if g["passed"] + g["ignored"] + g["filtered"] != listed:
            errors.append(f"{k}: {listed} tests listed, group {owner[k]} accounts for "
                          f"{g['passed'] + g['ignored'] + g['filtered']}")
        if g["filtered"] != r["passed"] + r["ignored"]:
            errors.append(f"{k}: group {owner[k]} left out {g['filtered']} tests, "
                          f"the uninstrumented job ran {r['passed'] + r['ignored']}")
        if g["ignored"] + r["ignored"] != ignored:
            errors.append(f"{k}: {ignored} ignored tests listed, {g['ignored'] + r['ignored']} reported")
        if g["passed"] + r["passed"] != listed - ignored:
            errors.append(f"{k}: {listed - ignored} tests to execute, "
                          f"{g['passed'] + r['passed']} executed")
        t = totals[owner[k]]
        t["binaries"] += 1
        t["executed"] += g["passed"]
        t["ignored"] += g["ignored"]
        t["skipped"] += g["filtered"]
        listed_total += listed
        ignored_total += ignored
        executed_total += g["passed"]
        uninstrumented_total += r["passed"]
    if uninstrumented_total == 0:
        errors.append("the uninstrumented job executed no test")
    doc_failed = sum(b["failed"] for b in u["doc"].values())
    doc_passed = sum(b["passed"] for b in u["doc"].values())
    if doc_failed:
        errors.append(f"{doc_failed} doc tests failed")
    width = max(len(g) for g in p["order"] + [UNINSTRUMENTED])
    print(f"{'group':<{width}}  binaries  executed  ignored  left out")
    for group in p["order"]:
        t = totals[group]
        print(f"{group:<{width}}  {t['binaries']:>8}  {t['executed']:>8}  {t['ignored']:>7}  {t['skipped']:>8}")
    print(f"{UNINSTRUMENTED:<{width}}  {len(u['run']):>8}  {uninstrumented_total:>8}")
    print()
    print(f"test binaries: {len(p['all'])}")
    print(f"tests listed: {listed_total}, ignored: {ignored_total}")
    print(f"tests executed: {executed_total} with coverage + {uninstrumented_total} without "
          f"= {executed_total + uninstrumented_total}, expected {listed_total - ignored_total}")
    print(f"doc tests executed: {doc_passed}")
    if errors:
        raise Failure("\n".join(errors))


def check():
    p = plan()
    exclusions()
    check_codecov(len(p["order"]))
    for group in p["order"]:
        print(f"{group}: {len(p['members'][group])} test binaries")
    print(f"total: {len(p['all'])} test binaries, {len(p['doc'])} crates with doc tests")


def main(argv):
    command, args = (argv[0], argv[1:]) if argv else ("", [])
    if command == "check" and not args:
        check()
    elif command == "targets" and len(args) == 1:
        p = plan()
        if args[0] not in p["args"]:
            raise Failure(f"unknown group {args[0]}, the groups are {p['order']}")
        print(" ".join(p["args"][args[0]]))
    elif command == "members" and len(args) == 1:
        p = plan()
        if args[0] not in p["members"]:
            raise Failure(f"unknown group {args[0]}, the groups are {p['order']}")
        print("\n".join(sorted(p["members"][args[0]])))
    elif command == "skip-args" and not args:
        print(" ".join(f"--skip {pattern}" for pattern in exclusions()))
    elif command == "filter-args" and not args:
        patterns = exclusions()
        if not patterns:
            raise Failure(f"{EXCLUSIONS_FILE.name} is empty: the uninstrumented job would run every test")
        print(" ".join(patterns))
    elif command == "summarize" and len(args) >= 2:
        summarize(args[0], args[1:])
    elif command == "verify" and len(args) == 1:
        verify(args[0])
    else:
        raise Failure(
            "usage: tests.py check | targets GROUP | members GROUP | skip-args | "
            "filter-args | summarize NAME KIND=LOG... | verify DIRECTORY"
        )


if __name__ == "__main__":
    try:
        main(sys.argv[1:])
    except Failure as e:
        print(f"error: {e}", file=sys.stderr)
        sys.exit(1)
