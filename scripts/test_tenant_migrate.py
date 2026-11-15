import unittest

from tenant_migrate import pg_env


class TenantMigrationConnections(unittest.TestCase):
    def test_backup_keeps_credentials_and_tls_in_environment(self):
        env = pg_env("postgresql://user:p%40ss@db.internal:5433/control?sslmode=verify-full&sslrootcert=%2Fprivate%2Fca.pem&options=-c%20search_path%3Dtenant")
        self.assertEqual(env["PGPASSWORD"], "p@ss")
        self.assertEqual(env["PGPORT"], "5433")
        self.assertEqual(env["PGDATABASE"], "control")
        self.assertEqual(env["PGSSLMODE"], "verify-full")
        self.assertEqual(env["PGSSLROOTCERT"], "/private/ca.pem")
        self.assertEqual(env["PGOPTIONS"], "-c search_path=tenant")

    def test_unsupported_options_fail_without_echoing_the_url(self):
        with self.assertRaises(ValueError) as error:
            pg_env("postgres://user:private-value@db/control?unexpected=secret")
        self.assertNotIn("private-value", str(error.exception))
        self.assertNotIn("secret", str(error.exception))
