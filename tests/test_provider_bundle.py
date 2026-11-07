import base64,hashlib,importlib.util,io,tarfile,tempfile,unittest
from pathlib import Path
spec=importlib.util.spec_from_file_location('provider_bundle',Path(__file__).resolve().parents[1]/'scripts/provider_bundle.py');module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
class BundleTest(unittest.TestCase):
 def test_private_complete_repeatable_bundle(self):
  with tempfile.TemporaryDirectory() as d:
   r=Path(d);(r/'frpc').write_bytes(b'official-binary');(r/'ca').write_text('certificate');(r/'transport').write_text('transport-secret');(r/'license').write_text('Apache license')
   config={'frpc_binary':str(r/'frpc'),'frpc_sha256':hashlib.sha256(b'official-binary').hexdigest(),'ca_file':str(r/'ca'),'transport_token_file':str(r/'transport'),'credential_dir':str(r/'credentials'),'license_file':str(r/'license'),'server_addr':'frps.example.com','server_port':7000,'tls_server_name':'frps.example.com'}
   project={'project_id':'11111111-1111-4111-8111-111111111111','launch_id':'launch-11111111-1111-4111-8111-111111111111','provider_id':'0x'+'ab'*32,'deadline':1234567}
   first=module.build(config,project);second=module.build(config,project);self.assertEqual(first['credential_sha256'],second['credential_sha256'])
   with tarfile.open(fileobj=io.BytesIO(base64.b64decode(first['bundle_base64'])),mode='r:gz') as archive:
    members=archive.getmembers();self.assertTrue(all(not x.name.startswith('/') and '..' not in x.name.split('/') for x in members));files={x.name.split('/',1)[1]:x for x in members if x.isfile()}
    for name in ['bin/frpc','config/frpc.toml','certs/ca.crt','private/token','private/transport-token','start.sh','stop.sh','manifest.json','LICENSE.frp']:self.assertIn(name,files)
    self.assertEqual(files['private/token'].mode,0o600);self.assertEqual(files['bin/frpc'].mode,0o700)
    config_text=archive.extractfile(files['config/frpc.toml']).read().decode();self.assertIn(project['provider_id'],config_text);self.assertIn('transport.tls.trustedCaFile',config_text);self.assertIn('localIP = "127.0.0.1"',config_text)
    compile(archive.extractfile(files['manage.py']).read(),'manage.py','exec')
   config['frpc_sha256']='0'*64
   with self.assertRaises(ValueError):module.build(config,project)
