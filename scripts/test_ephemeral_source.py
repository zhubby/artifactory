# [INPUT]: 临时源码验收规则与 Kubernetes 资源夹具。
# [OUTPUT]: 验证持久卷、材料泄漏与未清理任务的检测。
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
        "metadata": {"name": "image-test", "labels": {"app.kubernetes.io/name": "artifactory-image-worker"}},
        "spec": {
            "volumes": volumes or [{"name": "workspace", "emptyDir": {}}],
            "initContainers": [{"name": "source", "volumeMounts": [{"name": "source-auth"}, {"name": "source-identity"}]}],
            "containers": [{"name": "worker", "volumeMounts": mounts or [{"name": "workspace", "readOnly": True}]}],
        },
    }


class EphemeralSourceChecks(unittest.TestCase):
    def test_task_source_is_temporary_and_main_container_cannot_write_it(self):
        self.assertEqual(violations([pod()]), [])
        self.assertTrue(violations([pod(mounts=[{"name": "workspace"}])]))
        self.assertTrue(violations([pod(mounts=[{"name": "source-auth"}])]))

    def test_shared_pvc_and_host_directories_are_rejected(self):
        for source in ({"persistentVolumeClaim": {"claimName": "cache"}}, {"hostPath": {"path": "/cache"}}):
            self.assertTrue(violations([pod(volumes=[{"name": "workspace", **source}])]))
        self.assertTrue(violations([{"kind": "PersistentVolumeClaim", "metadata": {"name": "cache-sources-0"}}]))
        self.assertTrue(violations([{"kind": "StatefulSet", "metadata": {"name": "sources"}}]))

    def test_idle_requires_task_cleanup_and_preserves_service_secrets(self):
        secret = "image-" + "a" * 32 + "-task"
        self.assertTrue(violations([pod()], idle=True))
        self.assertTrue(violations([], [secret], idle=True))
        self.assertEqual(violations([], ["artifactory-credentials", "artifactory-pki"], idle=True), [])


if __name__ == "__main__":
    unittest.main()
