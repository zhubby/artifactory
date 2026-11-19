# [INPUT]: 临时源码验收规则与 Kubernetes 资源夹具。
# [OUTPUT]: 验证临时卷、材料隔离、AppArmor 语义与任务清理。
# [POS]: 只读验收脚本的离线回归。
# [PROTOCOL]: Update this header on change, then check AGENTS.md
import unittest

try:
    from scripts.check_ephemeral_source import violations
except ModuleNotFoundError:
    from check_ephemeral_source import violations


def pod(volumes=None, mounts=None):
    return {
        "kind": "Pod",
        "metadata": {"name": "image-test", "labels": {"app.kubernetes.io/name": "artifactory-image-worker"},
                     "annotations": {"container.apparmor.security.beta.kubernetes.io/buildkit": "unconfined"},
                     "ownerReferences": [{"apiVersion": "images.artifactory.dev/v1alpha1", "kind": "ImageBuild", "name": "build", "uid": "build-uid", "controller": True}]},
        "spec": {
            "restartPolicy": "Never",
            "automountServiceAccountToken": False,
            "volumes": volumes or [{"name": "workspace", "emptyDir": {}}],
            "initContainers": [{"name": "source", "volumeMounts": [{"name": "source-auth"}, {"name": "source-identity"}]}],
            "containers": [
                {"name": "worker", "volumeMounts": mounts or [{"name": "workspace", "readOnly": True}]},
                {"name": "buildkit", "command": ["artifactory-image-buildkit"], "volumeMounts": [{"name": "socket"}, {"name": "buildkit-state"}]},
            ],
        },
    }


class EphemeralSourceChecks(unittest.TestCase):
    def test_task_requires_direct_cr_owner_and_never_restarts(self):
        task=pod()
        task["metadata"]["ownerReferences"][0]["kind"]="Job"
        self.assertTrue(violations([task]))
        task=pod()
        task["spec"]["restartPolicy"]="OnFailure"
        self.assertTrue(violations([task]))
        task=pod()
        self.assertTrue(violations([{"kind":"Job", "metadata":task["metadata"], "spec":{"template":task}}]))

    def test_task_source_is_temporary_and_main_container_cannot_write_it(self):
        self.assertEqual(violations([pod()]), [])
        self.assertTrue(violations([pod(mounts=[{"name": "workspace"}])]))
        self.assertTrue(violations([pod(mounts=[{"name": "source-auth"}])]))

    def test_shared_pvc_and_host_directories_are_rejected(self):
        for source in ({"persistentVolumeClaim": {"claimName": "cache"}}, {"hostPath": {"path": "/cache"}}):
            self.assertTrue(violations([pod(volumes=[{"name": "workspace", **source}])]))
        self.assertTrue(violations([{"kind": "PersistentVolumeClaim", "metadata": {"name": "cache-sources-0"}}]))
        self.assertTrue(violations([{"kind": "StatefulSet", "metadata": {"name": "sources"}}]))

    def test_native_sidecar_is_rejected(self):
        task = pod()
        task["spec"]["initContainers"].append({"name": "buildkit", "restartPolicy": "Always"})
        self.assertTrue(violations([task]))

    def test_equivalent_api_defaulted_apparmor_is_accepted(self):
        self.assertEqual(violations([pod()]), [])
        for annotation in [True, False]:
            task = pod()
            if not annotation:
                task["metadata"]["annotations"] = {}
            task["spec"]["securityContext"] = {"appArmorProfile": {"type": "RuntimeDefault"}}
            task["spec"]["containers"][1]["securityContext"] = {"appArmorProfile": {"type": "Unconfined"}}
            self.assertEqual(violations([task]), [])

    def test_incorrect_or_conflicting_buildkit_apparmor_is_rejected(self):
        for profile in [{"type": "RuntimeDefault"}, {"type": "Localhost", "localhostProfile": "buildkit"}]:
            for annotation in [True, False]:
                with self.subTest(profile=profile, annotation=annotation):
                    task = pod()
                    if not annotation:
                        task["metadata"]["annotations"] = {}
                    task["spec"]["containers"][1]["securityContext"] = {"appArmorProfile": profile}
                    self.assertTrue(violations([task]))
        task = pod()
        task["metadata"]["annotations"] = {}
        self.assertTrue(violations([task]))
        task = pod()
        task["metadata"]["annotations"]["container.apparmor.security.beta.kubernetes.io/buildkit"] = "runtime/default"
        task["spec"]["containers"][1]["securityContext"] = {"appArmorProfile": {"type": "Unconfined"}}
        self.assertTrue(violations([task]))

    def test_unconfined_apparmor_cannot_spread_to_source_or_runner(self):
        for container in ["source", "worker"]:
            task = pod()
            task["metadata"]["annotations"]["container.apparmor.security.beta.kubernetes.io/" + container] = "unconfined"
            self.assertTrue(violations([task]), container)
        task = pod()
        task["spec"]["securityContext"] = {"appArmorProfile": {"type": "Unconfined"}}
        self.assertTrue(violations([task]))

    def test_buildkit_cannot_mount_runner_or_source_material(self):
        for name in ("task", "work", "workspace", "source-auth", "source-identity", "renamed-private-volume"):
            task = pod()
            task["spec"]["containers"][1]["volumeMounts"].append({"name": name})
            self.assertTrue(violations([task]), name)
        task = pod()
        task["spec"]["shareProcessNamespace"] = True
        self.assertTrue(violations([task]))
        task = pod()
        task["spec"]["automountServiceAccountToken"] = True
        self.assertTrue(violations([task]))

    def test_publisher_has_no_source_init_or_buildkit(self):
        task = pod()
        task["spec"]["containers"] = [{"name": "worker", "command": ["artifactory-image-publisher"]}]
        self.assertTrue(violations([task]))
        task["spec"]["initContainers"] = []
        self.assertEqual(violations([task]), [])

    def test_idle_requires_task_cleanup_and_preserves_service_secrets(self):
        secret = "image-" + "a" * 32 + "-task"
        self.assertTrue(violations([pod()], idle=True))
        self.assertTrue(violations([], [secret], idle=True))
        self.assertEqual(violations([], ["artifactory-credentials", "artifactory-pki"], idle=True), [])


if __name__ == "__main__":
    unittest.main()
