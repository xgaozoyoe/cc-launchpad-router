import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import tomllib

ROOT = Path(__file__).resolve().parents[1]
def module(name, path):
    spec = importlib.util.spec_from_file_location(name, ROOT / path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result

admin = module('admin', 'scripts/frp_admin.py')
registry = module('registry', 'frp/server/registry.py')
router = module('router', 'frp/server/router.py')


class PolicyTests(unittest.TestCase):
    def setUp(self):
        self.entries = {'gpu-001': {'id': 'gpu-001', 'enabled': True,
            'credential_sha256': hashlib.sha256(b'private-provider-token').hexdigest(),
            **admin.MODEL, 'endpoint': 'https://gpu-001.providers.internal/v1'}}
        self.user = {'user': 'gpu-001', 'metas': {'provider_token': 'private-provider-token'}}
        self.proxy = {'user': self.user, 'proxy_type': 'http', 'proxy_name': 'gpu-001.inference', 'subdomain': 'gpu-001'}

    def test_authentication_rejects_unknown_wrong_and_disabled_providers(self):
        self.assertFalse(registry.authorize('Login', self.user, self.entries)['reject'])
        for user in ({'user': 'other'}, {'user': 'gpu-001', 'metas': {'provider_token': 'wrong'}}):
            self.assertTrue(registry.authorize('Login', user, self.entries)['reject'])
        self.entries['gpu-001']['enabled'] = False
        self.assertTrue(registry.authorize('Login', self.user, self.entries)['reject'])

    def test_route_ownership_and_proxy_type_are_enforced(self):
        self.assertFalse(registry.authorize('NewProxy', self.proxy, self.entries)['reject'])
        for change in ({'subdomain': 'other'}, {'proxy_type': 'tcp'}, {'proxy_name': 'other.inference'},
                       {'custom_domains': ['evil.example.com']}, {'group': 'pool'}, {'locations': ['/admin']},
                       {'host_header_rewrite': 'other.compute.example.com'}, {'http_user': 'shadow'}):
            self.assertTrue(registry.authorize('NewProxy', self.proxy | change, self.entries)['reject'], change)
        self.assertTrue(registry.authorize('Unexpected', {}, self.entries)['reject'])

    def test_public_record_excludes_credentials_and_does_not_claim_readiness(self):
        record = registry.provider_list(self.entries)[0]
        self.assertNotIn('credential_sha256', record)
        self.assertEqual(record['status'], 'registered-unverified')

    def test_launch_id_routes_are_isolated_and_model_bound(self):
        provider = self.entries['gpu-001']
        launch = {'id': 'launch-a', 'provider_id': 'gpu-001', 'enabled': True, 'status': 'live',
                  **{k: provider[k] for k in admin.MODEL}}
        data = {'providers': self.entries, 'launches': {'launch-a': launch}}
        route = router.resolve('/launches/launch-a/v1/chat/completions', 'POST', data)
        self.assertEqual(route['host'], 'gpu-001.providers.internal')
        self.assertEqual(route['path'], '/v1/chat/completions')
        self.assertIsNone(router.resolve('/launches/unknown/v1/models', 'GET', data))
        self.assertIsNone(router.resolve('/launches/launch-a/v1/models', 'POST', data))
        self.assertIsNone(router.resolve('/launches/../v1/models', 'GET', data))
        launch['revision'] = 'different'
        self.assertIsNone(router.resolve('/launches/launch-a/v1/models', 'GET', data))
        launch['revision'] = provider['revision']
        provider['enabled'] = False
        self.assertIsNone(router.resolve('/launches/launch-a/v1/models', 'GET', data))

    def test_stake_hash_identity_uses_bound_internal_alias_and_expires(self):
        provider_id='0x'+'a'*64
        alias='p-'+hashlib.sha256(provider_id.encode()).hexdigest()[:40]
        item={**self.entries['gpu-001'],'id':provider_id,'tunnel_id':alias,
              'status':'awaiting-rpc','verification_deadline':3000}
        entries={provider_id:item}
        user={'user':provider_id,'metas':self.user['metas']}
        proxy={'user':user,'proxy_type':'http','proxy_name':alias+'.inference','subdomain':alias}
        with patch.object(registry.time,'time',return_value=2000):
            self.assertFalse(registry.authorize('Login',user,entries)['reject'])
            self.assertFalse(registry.authorize('NewProxy',proxy,entries)['reject'])
            self.assertTrue(registry.authorize('NewProxy',proxy|{'subdomain':'gpu-001'},entries)['reject'])
        with patch.object(registry.time,'time',return_value=3000):
            self.assertTrue(registry.authorize('Login',user,entries)['reject'])

    def test_provisioned_bundle_has_bound_identity_and_valid_toml(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(admin, 'ROOT', Path(tmp)):
            class Args:
                server = 'frps.example.com'
                api_domain = 'api.example.com'
                id = 'gpu-001'
            admin.initialize(Args())
            admin.add_provider(Args())
            Args.id = 'launch-001'
            Args.provider = 'gpu-001'
            admin.add_launch(Args())
            Args.id = 'gpu-001'
            base = Path(tmp) / '.secrets'
            server = tomllib.loads((base / 'frp-server/frps.toml').read_text())
            client = tomllib.loads((base / 'frp-bundles/gpu-001/frpc.toml').read_text())
            self.assertEqual(server['subDomainHost'], 'providers.internal')
            self.assertNotIn('gpu-001.', (base / 'frp-server/Caddyfile').read_text())
            self.assertIn('api.example.com', (base / 'frp-server/Caddyfile').read_text())
            self.assertEqual(client['user'], Args.id)
            self.assertEqual(client['proxies'][0]['localIP'], 'inference')
            self.assertEqual(client['transport']['tls']['serverName'], Args.server)
            self.assertTrue(client['transport']['tls']['enable'])
            self.assertEqual((base / 'frp-server/server.key').stat().st_mode & 0o777, 0o600)
            config = json.loads((base / 'frp-server/registry.json').read_text())
            login = {'user': Args.id, 'metas': {'provider_token': client['metadatas']['provider_token']}}
            self.assertFalse(registry.authorize('Login', login, config['providers'])['reject'])
            with self.assertRaises(ValueError):
                admin.add_provider(Args())


if __name__ == '__main__':
    unittest.main()
