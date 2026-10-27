#!/usr/bin/env python3
"""Pinned Helm adapter for an explicitly granted application namespace."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import ssl
import subprocess
import sys
import tempfile
import urllib.error
import urllib.parse
import urllib.request

MAX_RESPONSE = 4 * 1024**2


class Target:
    def __init__(self, path, grant):
        value = json.loads(path.read_text())
        required = {"schema", "environmentId", "cluster", "namespace", "namespaceUid",
                    "clusterCaSha256", "release", "kubeconfig"}
        if required - value.keys() or value.keys() - required - {"healthService"} or value["schema"] != "forge/helm-target/v1":
            raise ValueError("Closed operator target configuration required")
        for key in required - {"schema", "kubeconfig"}:
            if value[key] != grant[key]:
                raise ValueError("Target differs from operator grant")
        config = value["kubeconfig"]
        if not all(len(config.get(key, [])) == 1 for key in ["clusters", "contexts", "users"]):
            raise ValueError("One explicit kubeconfig identity required")
        context = config["contexts"][0]
        cluster = config["clusters"][0]
        user = config["users"][0]
        if (config["current-context"] != context["name"]
                or context["context"]["cluster"] != cluster["name"]
                or context["context"]["user"] != user["name"]
                or context["context"].get("namespace") != value["namespace"]
                or set(user["user"]) != {"token"}):
            raise ValueError("Unexpected kubeconfig context or authentication provider")
        data = cluster["cluster"]
        if set(data) != {"server", "certificate-authority-data"}:
            raise ValueError("Explicit API and CA required, without insecure overrides")
        ca = base64.b64decode(data["certificate-authority-data"], validate=True)
        if hashlib.sha256(ca).hexdigest() != value["clusterCaSha256"]:
            raise ValueError("Target cluster CA differs")
        server = urllib.parse.urlsplit(data["server"])
        if (server.scheme != "https" or not server.hostname or server.username
                or server.password or server.query or server.fragment or server.path not in ("", "/")):
            raise ValueError("Invalid target API endpoint")
        self.server = data["server"].rstrip("/")
        self.token = user["user"]["token"]
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}),
            urllib.request.HTTPSHandler(context=ssl.create_default_context(cadata=ca.decode())))
        self.config = config
        self.grant = grant
        self.health_service = value.get("healthService")
        actual = self.api("/api/v1/namespaces/" + value["namespace"])["metadata"]["uid"]
        if actual != value["namespaceUid"]:
            raise ValueError("Target namespace UID differs")

    def api(self, path):
        request = urllib.request.Request(self.server + path,
            headers={"Authorization": "Bearer " + self.token, "Accept": "application/json"})
        with self.opener.open(request, timeout=15) as response:
            raw = response.read(MAX_RESPONSE + 1)
        if len(raw) > MAX_RESPONSE:
            raise ValueError("Target API response exceeds bound")
        return json.loads(raw)


def portable(root, path):
    if (not path or path.startswith("-") or "\\" in path or ":" in path
            or any(part in ("", ".", "..") for part in path.split("/"))):
        raise ValueError("Portable repository path required")
    resolved = (root / path).resolve(strict=True)
    if not resolved.is_relative_to(root):
        raise ValueError("Deployment path escapes the exact checkout")
    return resolved


def image_values(offer):
    result = {"_forge": {key: offer[key] for key in
              ["environmentId", "deploymentId", "pipelineId", "attemptId", "leaseId",
               "generation", "sourceCommit", "pipelineOrder"]}}
    result["_forge"]["images"] = {key: value["imageRef"] for key, value in offer["images"].items()}
    for key, image in offer["images"].items():
        parts = key.split(".")
        if not all(part and all(c.isalnum() or c in "_-" for c in part) for part in parts) or parts[0] == "_forge":
            raise ValueError("Invalid image value path")
        nested = result
        for part in parts[:-1]:
            child = nested.setdefault(part, {})
            if not isinstance(child, dict):
                raise ValueError("Overlapping image value paths")
            nested = child
        if parts[-1] in nested:
            raise ValueError("Duplicate image value path")
        nested[parts[-1]] = image["imageRef"]
    return result


def helm(arguments, config, *, timeout=30):
    environment = {key: value for key, value in os.environ.items()
                   if key in ("PATH", "HOME", "TMPDIR", "SSL_CERT_FILE", "SSL_CERT_DIR")}
    environment["HELM_PLUGINS"] = "/nonexistent"
    environment["HELM_DRIVER"] = "secret"
    environment["HOME"] = str(config.parent)
    return subprocess.run(["/usr/local/bin/helm", *arguments, "--kubeconfig", str(config)],
        capture_output=True, text=True, timeout=timeout, env=environment)


def release(target, config):
    namespace = target.grant["namespace"]
    name = target.grant["release"]
    query = urllib.parse.urlencode({"labelSelector": "owner=helm,name=" + name})
    secrets = target.api("/api/v1/namespaces/" + namespace + "/secrets?" + query)["items"]
    if not secrets:
        return None
    status = helm(["status", name, "--namespace", namespace, "-o", "json"], config)
    values = helm(["get", "values", name, "--namespace", namespace, "-o", "json"], config)
    if status.returncode or values.returncode:
        raise RuntimeError("Helm release readback unavailable")
    value = json.loads(status.stdout)
    value["observedValues"] = json.loads(values.stdout) or {}
    return value


def verify_offer(offer, grant):
    for key, value in {"environmentId": offer["environmentId"], "cluster": offer["target"]["cluster"],
                       "namespace": offer["target"]["namespace"], "release": offer["target"]["release"]}.items():
        if grant[key] != value:
            raise ValueError("Deployment offer outside operator target")
    if not offer["images"] or len(offer["images"]) > 16:
        raise ValueError("Bounded publication inputs required")


def observed_images(target, offer):
    namespace = target.grant["namespace"]
    release_name = target.grant["release"]
    owned = set()
    for resource in ["deployments", "statefulsets", "daemonsets"]:
        items = target.api("/apis/apps/v1/namespaces/" + namespace + "/" + resource)["items"]
        for item in items:
            annotations = item["metadata"].get("annotations", {})
            if (annotations.get("meta.helm.sh/release-name") == release_name
                    and annotations.get("meta.helm.sh/release-namespace") == namespace):
                owned.add(item["metadata"]["uid"])
    replicas = target.api("/apis/apps/v1/namespaces/" + namespace + "/replicasets")["items"]
    for item in replicas:
        if any(ref["uid"] in owned for ref in item["metadata"].get("ownerReferences", [])):
            owned.add(item["metadata"]["uid"])
    expected = {value["imageRef"] for value in offer["images"].values()}
    actual = set()
    for pod in target.api("/api/v1/namespaces/" + namespace + "/pods")["items"]:
        if (not any(ref["uid"] in owned for ref in pod["metadata"].get("ownerReferences", []))
                or pod.get("status", {}).get("phase") != "Running"
                or not any(c["type"] == "Ready" and c["status"] == "True"
                           for c in pod.get("status", {}).get("conditions", []))):
            continue
        specs = {c["name"]: c["image"] for c in pod["spec"].get("containers", [])}
        for container in pod.get("status", {}).get("containerStatuses", []):
            image = specs.get(container["name"])
            if image in expected and container.get("ready") and container.get("imageID", "").endswith(image.split("@", 1)[1]):
                actual.add(image)
    if actual != expected:
        raise ValueError("Running release Pods do not prove every published image digest")


def application_http(target, offer):
    service = target.health_service
    public = urllib.parse.urlsplit(offer["target"]["health_url"])
    headers = {"User-Agent": "Forge-Deployment-Check/1"}
    if service is None:
        endpoint = offer["target"]["health_url"]
        transport = "public-https"
    else:
        if (set(service) != {"name", "port", "path"} or service["path"] != public.path
                or type(service["port"]) is not int or not 1 <= service["port"] <= 65535):
            raise ValueError("Invalid operator health service")
        import ipaddress
        import re
        if not re.fullmatch("[a-z0-9][a-z0-9-]{0,62}", service["name"]):
            raise ValueError("Invalid health service identity")
        observed = target.api("/api/v1/namespaces/" + target.grant["namespace"] +
                              "/services/" + service["name"])
        annotations = observed["metadata"].get("annotations", {})
        ports = observed["spec"].get("ports", [])
        if (annotations.get("meta.helm.sh/release-name") != target.grant["release"]
                or annotations.get("meta.helm.sh/release-namespace") != target.grant["namespace"]
                or not any(port["port"] == service["port"] for port in ports)
                or observed["spec"].get("type", "ClusterIP") not in ("ClusterIP", "NodePort")):
            raise ValueError("Health service is not owned by the expected Helm release")
        address = str(ipaddress.IPv4Address(observed["spec"]["clusterIP"]))
        endpoint = "http://" + address + ":" + str(service["port"]) + service["path"]
        headers["Host"] = public.netloc
        transport = "cluster-service-http"
    # Kubernetes credentials are never passed to the application HTTP request.
    request = urllib.request.Request(endpoint, headers=headers)
    opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with opener.open(request, timeout=15) as response:
        code = response.status
    if not 200 <= code < 300:
        raise ValueError("Application HTTP is not ready")
    return code, transport


def expected_metadata(offer):
    rollback = offer.get("rollback")
    return rollback["metadata"] if rollback else image_values(offer)["_forge"]


def effect_marker(offer, *, create=False):
    # The exact attempt workspace survives loss of the client Pod. An absent
    # release after possible API submission is unknown, never permission to retry.
    path=Path.cwd()/".forge-helm-effect.json"
    expected={key:offer[key] for key in
        ("deploymentId","attemptId","leaseId","generation","sourceCommit")}
    if path.is_symlink():
        raise ValueError("Deployment effect marker must be a regular owned file")
    if path.exists():
        if not path.is_file() or path.stat().st_size>4096 or json.loads(path.read_text())!=expected:
            raise ValueError("Deployment effect marker belongs to another execution")
        return True
    if create:
        descriptor=os.open(path,os.O_CREAT|os.O_EXCL|os.O_WRONLY|os.O_NOFOLLOW,0o600)
        with os.fdopen(descriptor,"w") as stream:
            json.dump(expected,stream);stream.flush();os.fsync(stream.fileno())
        parent=os.open(path.parent,os.O_DIRECTORY)
        try:os.fsync(parent)
        finally:os.close(parent)
        return True
    return False


def readback(target, config, offer):
    current = release(target, config)
    if current is None:
        return {"phase": "held" if effect_marker(offer) else "no-effect"}
    metadata = current["observedValues"].get("_forge", {})
    identity = expected_metadata(offer)
    rollback = offer.get("rollback")
    if rollback:
        expected = rollback["expectedCurrentRevision"] + 1
        if current["version"] == rollback["expectedCurrentRevision"]:
            return {"phase": "held" if effect_marker(offer) else "no-effect"}
        if current["version"] != expected or metadata != identity:
            return {"phase": "held"}
    elif metadata != identity:
        if effect_marker(offer):
            return {"phase":"held"}
        history = helm(["history", target.grant["release"], "--namespace",
                        target.grant["namespace"], "-o", "json"], config)
        if history.returncode:
            raise RuntimeError("Release history readback unavailable")
        if any(item.get("description") == "forge:" + offer["deploymentId"]
               for item in json.loads(history.stdout)):
            return {"phase": "held"}
        if metadata.get("pipelineOrder", -1) > offer["pipelineOrder"]:
            return {"phase": "superseded"}
        return {"phase": "no-effect"}
    status = current["info"]["status"]
    if status.startswith("pending"):
        return {"phase": "held", "releaseRevision": current["version"]}
    if status != "deployed":
        return {"phase": "failed", "releaseRevision": current["version"]}
    try:
        observed_images(target, offer)
        code, transport = application_http(target, offer)
    except (OSError, ValueError, urllib.error.URLError):
        return {"phase": "failed", "releaseRevision": current["version"]}
    receipt = {key: offer[key] for key in ["deploymentId", "pipelineId", "attemptId",
               "leaseId", "generation", "sourceCommit"]}
    receipt.update(schema="forge/kubernetes-deployment/v1",
        cluster=target.grant["cluster"], namespace=target.grant["namespace"],
        namespaceUid=target.grant["namespaceUid"], release=target.grant["release"],
        releaseRevision=current["version"],
        images={key: value["imageRef"] for key, value in offer["images"].items()},
        healthStatus=code, healthTransport=transport)
    return {"phase": "deployed", "receipt": receipt}


def validate_manifest(raw, namespace):
    import yaml
    if len(raw.encode())>MAX_RESPONSE:
        raise ValueError("Rendered deployment exceeds the bound")
    allowed={"ConfigMap","Secret","Service","PersistentVolumeClaim",
             "Deployment","StatefulSet","DaemonSet","Job","CronJob"}
    count=0
    for document in yaml.safe_load_all(raw):
        if document is None:
            continue
        count+=1
        if count>128 or not isinstance(document,dict) or document.get("kind") not in allowed:
            raise ValueError("Rendered object is outside namespace deployment permissions")
        metadata=document.get("metadata",{})
        if metadata.get("namespace",namespace)!=namespace:
            raise ValueError("Rendered object belongs to another namespace")
        if "helm.sh/hook" in metadata.get("annotations",{}):
            raise ValueError("External Helm hooks are not a qualified application lifecycle")
        if document["kind"]=="Service":
            spec=document.get("spec",{})
            if spec.get("type","ClusterIP")!="ClusterIP" or spec.get("externalIPs") or spec.get("externalName"):
                raise ValueError("Public routing belongs to the operator")
    if count==0:
        raise ValueError("Empty deployment manifest")


def apply(target, config, offer, root):
    actual = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
    if actual != offer["sourceCommit"]:
        raise ValueError("Deployment checkout differs from its pipeline commit")
    current = release(target, config)
    rollback = offer.get("rollback")
    if rollback:
        if current is None:
            raise ValueError("Rollback release is missing")
        if (current["version"] == rollback["expectedCurrentRevision"] + 1
                and current["observedValues"].get("_forge") == expected_metadata(offer)):
            return readback(target, config, offer)
        if (current["version"] != rollback["expectedCurrentRevision"]
                or current["info"]["status"].startswith("pending")):
            raise ValueError("Rollback release has an unknown newer operation")
        effect_marker(offer,create=True)
        result = helm(["rollback", target.grant["release"], str(rollback["revision"]),
            "--namespace", target.grant["namespace"], "--wait", "--timeout", "600s",
            "--history-max", "20"], config, timeout=660)
        state = readback(target, config, offer)
        if result.returncode or state["phase"] != "deployed":
            raise RuntimeError("Rollback did not pass release, digest and HTTP checks")
        return state
    if current:
        metadata = current["observedValues"].get("_forge")
        if not isinstance(metadata, dict):
            raise ValueError("Refuse adoption of an unowned Helm release")
        if metadata.get("deploymentId") == offer["deploymentId"]:
            return readback(target, config, offer)  # Exact retry performs no new effect.
        if current["info"]["status"].startswith("pending"):
            raise ValueError("Existing release operation remains unresolved")
        if metadata.get("pipelineOrder", -1) > offer["pipelineOrder"]:
            raise ValueError("A newer pipeline is already deployed")
        if (metadata.get("pipelineOrder") == offer["pipelineOrder"]
                and metadata.get("pipelineId") != offer["pipelineId"]):
            raise ValueError("Pipeline ordering identity differs")
    chart = portable(root, offer["target"]["chart"])
    with tempfile.TemporaryDirectory(prefix="forge-helm-values-") as temporary:
        overlay = Path(temporary) / "images.json"
        overlay.write_text(json.dumps(image_values(offer)))
        arguments = ["upgrade", "--install", target.grant["release"], str(chart),
            "--namespace", target.grant["namespace"], "--wait", "--timeout", "600s",
            "--history-max", "20", "--description", "forge:" + offer["deploymentId"]]
        value_arguments=[]
        for path in offer["target"]["values"]:
            value_arguments += ["--values", str(portable(root, path))]
        for key, path in offer["target"].get("files", {}).items():
            if key.split(".")[0] == "_forge":
                raise ValueError("Forge release metadata is reserved")
            value_arguments += ["--set-file", key + "=" + str(portable(root, path))]
        value_arguments += ["--values", str(overlay)]
        rendered=helm(["template",target.grant["release"],str(chart),
            "--namespace",target.grant["namespace"],*value_arguments],config)
        if rendered.returncode:
            raise ValueError("Application chart preflight failed")
        validate_manifest(rendered.stdout,target.grant["namespace"])
        effect_marker(offer,create=True)
        result = helm(arguments+value_arguments, config, timeout=660)
        # Never print template NOTES or errors: a chart can include secret values.
        state = readback(target, config, offer)
        if result.returncode or state["phase"] != "deployed":
            raise RuntimeError("Deployment did not pass release, digest and HTTP checks")
        return state


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", type=Path, default=Path("/run/forge-deploy-target/target.json"))
    parser.add_argument("--offer", required=True)
    parser.add_argument("--grant", required=True)
    parser.add_argument("--readback", action="store_true")
    args = parser.parse_args()
    offer, grant = json.loads(args.offer), json.loads(args.grant)
    verify_offer(offer, grant)
    target = Target(args.target, grant)
    with tempfile.TemporaryDirectory(prefix="forge-helm-target-") as directory:
        config = Path(directory) / "kubeconfig.json"
        config.write_text(json.dumps(target.config))
        config.chmod(0o600)
        result = readback(target, config, offer) if args.readback else apply(target, config, offer, Path.cwd().resolve())
        print(json.dumps(result), flush=True)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, RuntimeError, subprocess.SubprocessError) as error:
        # Only fixed operator diagnostics reach the job log.
        print("Helm deployment failed: " + type(error).__name__, file=sys.stderr)
        raise SystemExit(1)
