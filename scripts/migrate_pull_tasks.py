#!/usr/bin/env python3
"""Inventory historical PR mappings; apply only explicitly confirmed TaskRefs."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import urllib.error
import urllib.parse
import urllib.request


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def request(origin, token, path, body=None):
    data = None if body is None else json.dumps(body).encode("utf-8")
    call = urllib.request.Request(origin.rstrip("/") + "/api/v1" + path, data=data,
        headers={"Authorization": "Bearer " + token, "Content-Type": "application/json"})
    with urllib.request.build_opener(NoRedirect).open(call, timeout=30) as response:
        return json.load(response)


def inventory(call):
    result = []
    offset = 0
    while True:
        page = call(f"/catalog/repositories?limit=100&offset={offset}")
        for repo in page["items"]:
            if repo.get("kind") == "external":
                continue
            for pull in call(f"/catalog/repositories/{repo['id']}/pulls"):
                if pull.get("primary_task_id"):
                    continue
                links = []
                link_offset = 0
                while True:
                    part = call(f"/catalog/repositories/{repo['id']}/pulls/{pull['number']}/tasks?limit=100&offset={link_offset}")
                    links.extend(part)
                    if len(part) < 100:
                        break
                    link_offset += len(part)
                result.append({"repository_id": repo["id"], "pull_request_id": pull["id"],
                    "number": pull["number"], "expected_updated_at": pull["updated_at"],
                    "verified_links": [link["task"] for link in links],
                    "confirmed_task": None, "reason": ""})
        offset += len(page["items"])
        if offset >= page["total"]:
            return result
        if not page["items"]:
            raise ValueError("Incomplete repository catalog")


def apply_mapping(call, entries):
    applied = 0
    for entry in entries:
        task = entry["confirmed_task"]
        if not task or not entry["reason"].strip():
            raise ValueError("Each mapping needs an explicit confirmed_task and reason")
        path = f"/catalog/repositories/{entry['repository_id']}/pulls/{entry['number']}"
        current = call(path)
        if current["id"] != entry["pull_request_id"]:
            raise ValueError("PR identity changed")
        primary = {"tracker_instance_id": current.get("primary_tracker_instance_id"),
                   "task_id": current.get("primary_task_id")}
        if primary == task:
            applied += 1
            continue
        if current.get("primary_task_id") or current["updated_at"] != entry["expected_updated_at"]:
            raise ValueError("PR changed after inventory; regenerate the mapping")
        try:
            call(path + "/tasks", {**task, "primary": True})
        except urllib.error.HTTPError:
            raise
        except (urllib.error.URLError, TimeoutError):
            # Observe the original mapping, never invent a replacement operation.
            observed = call(path)
            if (observed.get("primary_task_id") != task["task_id"]
                    or observed.get("primary_tracker_instance_id") != task["tracker_instance_id"]):
                raise RuntimeError("Mapping outcome is unresolved; inspect the original PR") from None
        observed = call(path)
        if (observed.get("primary_task_id") != task["task_id"]
                or observed.get("primary_tracker_instance_id") != task["tracker_instance_id"]):
            raise RuntimeError("Mapping readback differs")
        applied += 1
    return applied


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--apply", type=Path, help="Apply an operator-reviewed JSON mapping")
    args = parser.parse_args()
    origin = os.environ["CICD_API_URL"]
    parsed = urllib.parse.urlsplit(origin)
    if parsed.scheme not in {"http", "https"} or not parsed.hostname or parsed.username or parsed.password:
        raise ValueError("Invalid configured Forge origin")
    token = os.environ["CICD_API_TOKEN"]
    call = lambda path, body=None: request(origin, token, path, body)
    if args.apply:
        count = apply_mapping(call, json.loads(args.apply.read_text(encoding="utf-8")))
        print(f"Verified {count} mappings; run inventory again to verify zero unresolved PRs")
    else:
        if args.output is None:
            parser.error("--output is required for inventory")
        entries = inventory(call)
        with args.output.open("x", encoding="utf-8", newline="\n") as output:
            json.dump(entries, output, ensure_ascii=False, indent=2)
            output.write("\n")
        print(f"Unresolved PRs: {len(entries)}; no mappings changed")


if __name__ == "__main__":
    main()
