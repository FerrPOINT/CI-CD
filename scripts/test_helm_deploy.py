"""Scoped client contract checks; real Helm/API qualification is separate."""
import copy
from contextlib import contextmanager
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import helm_deploy as deploy


class HelmCustody(unittest.TestCase):
    def setUp(self):
        self.offer={key:key for key in ("environmentId","deploymentId","pipelineId",
            "jobId","attemptId","leaseId")}
        self.offer.update(generation=2,sourceCommit="a"*40,pipelineOrder=12,
            target={"cluster":"pdlc","namespace":"app-dev","release":"app",
                "chart":"chart","values":[],"health_url":"https://app.test/health"},
            images={"images.web":{"imageRef":"registry.test/app@sha256:"+"b"*64}})
        self.target=type("Target",(),{"grant":{"namespace":"app-dev","release":"app"}})()

    @contextmanager
    def workspace(self):
        previous=Path.cwd()
        with tempfile.TemporaryDirectory() as path:
            os.chdir(path)
            try:yield Path(path)
            finally:os.chdir(previous)

    def test_absent_release_after_possible_submission_is_unknown(self):
        with self.workspace(),patch.object(deploy,"release",return_value=None):
            self.assertEqual(deploy.readback(self.target,None,self.offer)["phase"],"no-effect")
            deploy.effect_marker(self.offer,create=True)
            self.assertEqual(deploy.readback(self.target,None,self.offer)["phase"],"held")
            foreign=copy.deepcopy(self.offer);foreign["generation"]+=1
            with self.assertRaises(ValueError):
                deploy.effect_marker(foreign)

    def test_marker_rejects_repository_symlink(self):
        with self.workspace() as root:
            (root/"other").write_text("{}")
            (root/".forge-helm-effect.json").symlink_to(root/"other")
            with self.assertRaises(ValueError):
                deploy.effect_marker(self.offer,create=True)

    def test_exact_retry_reads_current_release_without_second_upgrade(self):
        current={"version":3,"info":{"status":"deployed"},
                 "observedValues":{"_forge":deploy.image_values(self.offer)["_forge"]}}
        with self.workspace() as root,patch.object(deploy,"release",return_value=current),\
             patch.object(deploy.subprocess,"check_output",return_value="a"*40),\
             patch.object(deploy,"readback",return_value={"phase":"deployed"}) as readback,\
             patch.object(deploy,"helm") as helm:
            self.assertEqual(deploy.apply(self.target,None,self.offer,root)["phase"],"deployed")
            helm.assert_not_called();readback.assert_called_once()

    def test_older_pipeline_and_unowned_releases_are_rejected_before_writes(self):
        for metadata in (None,{"pipelineOrder":13}):
            current={"version":3,"info":{"status":"deployed"},
                     "observedValues":{} if metadata is None else {"_forge":metadata}}
            with self.workspace() as root,patch.object(deploy,"release",return_value=current),\
                 patch.object(deploy.subprocess,"check_output",return_value="a"*40),\
                 patch.object(deploy,"helm") as helm:
                with self.assertRaises(ValueError):
                    deploy.apply(self.target,None,self.offer,root)
                helm.assert_not_called()

    def test_repository_paths_reject_escape_but_allow_nested_chart(self):
        with self.workspace() as root:
            (root/"deploy/chart").mkdir(parents=True)
            self.assertEqual(deploy.portable(root,"deploy/chart"),root/"deploy/chart")
            (root/"link").symlink_to("/etc")
            for path in ("../elsewhere","/etc","link/passwd","-option","C:/chart"):
                with self.assertRaises((ValueError,FileNotFoundError)):
                    deploy.portable(root,path)

    def test_chart_cannot_add_hooks_cluster_authority_or_public_routes(self):
        valid={"apiVersion":"v1","kind":"Service","metadata":{"name":"app"},
               "spec":{"type":"ClusterIP","ports":[{"port":80}]}}
        deploy.validate_manifest(json.dumps(valid),"app-dev")
        for value in [
            {**valid,"metadata":{"name":"app","namespace":"other"}},
            {**valid,"metadata":{"name":"app","annotations":{"helm.sh/hook":"pre-install"}}},
            {**valid,"kind":"ClusterRole"},
            {**valid,"kind":"Ingress"},
            {**valid,"spec":{"type":"NodePort"}},
        ]:
            with self.assertRaises(ValueError):
                deploy.validate_manifest(json.dumps(value),"app-dev")

    def test_failed_health_never_becomes_successful_deployment(self):
        current={"version":3,"info":{"status":"deployed"},
                 "observedValues":{"_forge":deploy.image_values(self.offer)["_forge"]}}
        with self.workspace(),patch.object(deploy,"release",return_value=current),\
             patch.object(deploy,"observed_images"),\
             patch.object(deploy,"application_http",side_effect=ValueError("not ready")):
            observed=deploy.readback(self.target,None,self.offer)
            self.assertEqual(observed,{"phase":"failed","releaseRevision":3})
            self.assertNotIn("receipt",observed)

if __name__=="__main__":
    unittest.main()
