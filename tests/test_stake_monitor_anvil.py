"""Optional integration with real compiled contracts and a disposable Anvil node (no real funds)."""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import time
import unittest
from unittest.mock import patch

ROOT=Path(__file__).resolve().parents[1]
def load(name,path):
    spec=importlib.util.spec_from_file_location(name,ROOT/path)
    mod=importlib.util.module_from_spec(spec);spec.loader.exec_module(mod);return mod
monitor=load('monitor_anvil','frp/server/stake_monitor.py')
admin=load('admin_anvil','scripts/frp_admin.py')


@unittest.skipUnless(os.environ.get('RUN_ANVIL_TESTS')=='1','Set RUN_ANVIL_TESTS=1 after forge build')
class AnvilMonitorTests(unittest.TestCase):
    def test_real_stake_event_issues_once_and_refund_disables_routes(self):
        with socket.socket() as sock:
            sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
        process=subprocess.Popen(['anvil','--host','127.0.0.1','--port',str(port),'--silent'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        rpc=monitor.RPC(f'http://127.0.0.1:{port}')
        try:
            for _ in range(100):
                try:accounts=rpc.call('eth_accounts',[]);break
                except OSError:time.sleep(.05)
            else:self.fail('Anvil did not start')
            owner,provider=accounts[:2]
            def word(n):return format(n,'064x')
            def addr(a):return a[2:].lower().rjust(64,'0')
            def selector(sig):return subprocess.check_output(['cast','sig',sig],text=True).strip()
            def send(sender,to,data):
                tx={'from':sender,'data':data,'gas':'0x900000'}
                if to:tx['to']=to
                h=rpc.call('eth_sendTransaction',[tx])
                receipt=None
                for _ in range(100):
                    receipt=rpc.call('eth_getTransactionReceipt',[h])
                    if receipt:break
                    time.sleep(.01)
                self.assertEqual(receipt['status'],'0x1')
                return receipt
            def bytecode(path):return json.loads((ROOT/path).read_text())['bytecode']['object']
            token=send(owner,None,bytecode('contracts/out/LaunchStake.t.sol/MockPRL.json'))['contractAddress']
            contract=send(owner,None,bytecode('contracts/out/LaunchStake.sol/LaunchStake.json')+addr(token)+addr(owner))['contractAddress']
            with tempfile.TemporaryDirectory() as tmp,patch.object(admin,'ROOT',Path(tmp)):
                class Args:
                    server='frps.example.com';api_domain='api.example.com'
                admin.initialize(Args())
                server=Path(tmp)/'.secrets/frp-server'
                config={'chain_id':31337,'staking_contract':contract,'token_address':token,'deployment_block':0,'batch_size':1000}
                clock=lambda:int(rpc.call('eth_getBlockByNumber',['latest',False])['timestamp'],16)
                service=monitor.Monitor(server,config,rpc,clock)
                amount=2000*10**18
                p=service.create_project(provider,'Qwen credits',admin.MODEL,amount)
                key=monitor.project_key(p['id'])
                send(owner,contract,selector('registerProject(bytes32,address,uint256)')+key[2:]+addr(provider)+word(amount))
                send(owner,token,selector('mint(address,uint256)')+addr(provider)+word(amount))
                send(provider,token,selector('approve(address,uint256)')+addr(contract)+word(amount))
                receipt=send(provider,contract,selector('stake(bytes32)')+key[2:])
                # Finalize the stake without advancing beyond its 24h deployment window.
                rpc.call('anvil_mine',['0x80'])
                # Commit succeeds but disk export crashes: restart must recover the same credential/ID.
                with patch.object(service,'export',side_effect=OSError('simulated export failure')):
                    with self.assertRaises(OSError):service.tick()
                service.db.close()
                service=monitor.Monitor(server,config,rpc,clock)
                self.assertEqual(service.db.execute('SELECT count(*) FROM events').fetchone()[0],1)
                wrong_chain=monitor.StakeVerifier(rpc,config|{'chain_id':999})
                with self.assertRaises(ValueError):wrong_chain.finalized()
                wrong_token=monitor.StakeVerifier(rpc,config|{'token_address':'0x'+'1'*40})
                with self.assertRaises(ValueError):wrong_token.finalized()
                data=service.data();project=data['projects'][p['id']];launch_id=project['launch_id']
                self.assertEqual(project['provider_id'],receipt['transactionHash'])
                block=rpc.call('eth_getBlockByNumber',[receipt['blockNumber'],False])
                self.assertEqual(project['verification_deadline'],int(block['timestamp'],16)+86400)
                bundle=server/'deliveries'/launch_id
                first=(bundle/'frpc.toml').read_bytes()
                self.assertEqual((bundle/'frpc.toml').stat().st_mode&0o777,0o600)
                real_call=rpc.call
                def reverted(method,params):
                    value=real_call(method,params)
                    return value|{'status':'0x0'} if method=='eth_getTransactionReceipt' else value
                with patch.object(rpc,'call',side_effect=reverted):
                    with self.assertRaises(ValueError):service.verifier(receipt['transactionHash'],project,data['default_chain'])
                self.assertEqual(service.tick(),0)
                service.db.close()
                restarted=monitor.Monitor(server,config,rpc,clock)
                self.assertEqual(restarted.data()['projects'][p['id']]['launch_id'],launch_id)
                self.assertEqual((bundle/'frpc.toml').read_bytes(),first)
                self.assertEqual(restarted.db.execute('SELECT count(*) FROM deliveries').fetchone()[0],1)
                # Replay from deployment block; deduplication survives cursor replay.
                restarted.db.execute('DELETE FROM cursor')
                self.assertEqual(restarted.tick(),0)
                self.assertFalse(restarted.data()['launches'][launch_id]['status']=='live')
                probe=lambda *_:{k:True for k in ('authentication','model','usage','capacity')}
                with self.assertRaises(ValueError):restarted.accept_rpc(p['id'],probe)
                role=rpc.call('eth_call',[{'to':contract,'data':selector('VERIFIER_ROLE()')},'latest'])
                send(owner,contract,selector('grantRole(bytes32,address)')+role[2:]+addr(owner))
                send(owner,contract,selector('verifyProject(bytes32)')+key[2:])
                rpc.call('anvil_mine',['0x80'])
                restarted.accept_rpc(p['id'],probe)
                self.assertEqual(restarted.data()['launches'][launch_id]['status'],'live')
                # A separate pending project can refund; the already verified launch remains enabled.
                p2=restarted.create_project(provider,'Second project',admin.MODEL,amount)
                key2=monitor.project_key(p2['id'])
                send(owner,contract,selector('registerProject(bytes32,address,uint256)')+key2[2:]+addr(provider)+word(amount))
                send(owner,token,selector('mint(address,uint256)')+addr(provider)+word(amount))
                send(provider,token,selector('approve(address,uint256)')+addr(contract)+word(amount))
                receipt2=send(provider,contract,selector('stake(bytes32)')+key2[2:])
                rpc.call('anvil_mine',['0x80'])
                self.assertEqual(restarted.tick(),1)
                launch2=restarted.data()['projects'][p2['id']]['launch_id']
                rpc.call('evm_increaseTime',[86400]);rpc.call('evm_mine',[])
                send(provider,contract,selector('refundExpired(bytes32)')+key2[2:])
                restarted.tick()
                self.assertFalse(restarted.data()['providers'][receipt2['transactionHash']]['enabled'])
                self.assertFalse(restarted.data()['launches'][launch2]['enabled'])
                self.assertTrue(restarted.data()['launches'][launch_id]['enabled'])
                restarted.db.close()
        finally:
            process.terminate();process.wait(timeout=5)
