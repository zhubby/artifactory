# [INPUT]: 本机 Pingora 环境生成器。
# [OUTPUT]: 验证网关不继承数据库、密钥或对象凭据。
# [POS]: 本机服务开发辅助的离线回归。
# [PROTOCOL]: Update this header on change, then check AGENTS.md
import unittest
import local_gateway


class GatewayEnvironmentTests(unittest.TestCase):
    def test_gateway_environment_excludes_control_and_object_credentials(self):
        config = {'ARTIFACTORY_GATEWAY_TOKEN': 'gateway-fixture-token-' + 'x' * 32,
                  'ARTIFACTORY_S3_ENDPOINT': 'http://127.0.0.1:19000',
                  'ARTIFACTORY_MASTER_KEY': 'must-not-propagate',
                  'ARTIFACTORY_S3_SECRET_KEY': 'must-not-propagate',
                  'DATABASE_URL': 'must-not-propagate', 'ARTIFACTORY_INSECURE_DEV': 'true'}
        result = local_gateway.environment(config, '127.0.0.1:8081', 'http://127.0.0.1:8080')
        self.assertNotIn('must-not-propagate', result.values())
        self.assertEqual(result['ARTIFACTORY_GATEWAY_TOKEN'], config['ARTIFACTORY_GATEWAY_TOKEN'])
        self.assertEqual(result['ARTIFACTORY_GATEWAY_CONTROL'], 'http://127.0.0.1:8080')
        self.assertEqual(result['ARTIFACTORY_GATEWAY_WEB'], 'http://127.0.0.1:3000')

    def test_configured_web_origin_is_preserved_for_ai_and_recovery(self):
        result = local_gateway.environment({'ARTIFACTORY_GATEWAY_TOKEN': 'fixture-token-' + 'x' * 32,
            'ARTIFACTORY_S3_ENDPOINT': 'https://private.example.invalid',
            'ARTIFACTORY_GATEWAY_WEB': 'http://127.0.0.1:39000'}, '127.0.0.1:38080', 'http://127.0.0.1:38082')
        self.assertEqual(result['ARTIFACTORY_GATEWAY_WEB'], 'http://127.0.0.1:39000')
        self.assertEqual(result['ARTIFACTORY_GATEWAY_CONTROL'], 'http://127.0.0.1:38082')


if __name__ == '__main__':
    unittest.main()
