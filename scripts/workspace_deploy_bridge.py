"""Embedded owner inventory validator; never a deployment runner or admission token.

Backend supplies compiled validator sources, not paths/imports from a checkout.
No secret resolution, network, filesystem or lifecycle action is implemented.
"""

import json
import sys
import types


def main():
    result = {"status": "held", "reason": "recipe_rejected"}
    try:
        raw = sys.stdin.buffer.read(1024 * 1024 + 1)
        if len(raw) > 1024 * 1024:
            raise ValueError("owner input bound")
        bundle = json.loads(raw)
        package = types.ModuleType("scripts")
        package.__path__ = []
        sys.modules["scripts"] = package
        for name in ("workspace_deploy_admission", "workspace_deploy_adapter"):
            qualified = "scripts." + name
            module = types.ModuleType(qualified)
            module.__package__ = "scripts"
            sys.modules[qualified] = module
            exec(compile(bundle[name], "<forge-" + name + ">", "exec"), module.__dict__)
        adapter = sys.modules["scripts.workspace_deploy_adapter"]
        operation = bundle.get("operation", "inspect")
        if operation == "inspect":
            result = adapter.read_only_inventory(bundle["input"])
        elif operation == "prepare":
            result = adapter.prepare_deployment(bundle["input"])
    except Exception:
        pass
    sys.stdout.write(json.dumps(result, separators=(",", ":")))


if __name__ == "__main__":
    main()
