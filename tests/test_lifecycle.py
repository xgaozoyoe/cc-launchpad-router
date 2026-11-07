import copy
import hashlib
import importlib.util
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('lifecycle', ROOT/'frp/server/lifecycle.py')
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class LifecycleTests(unittest.TestCase):
    def setUp(self):
        self.now = 2000
        self.chain = {'chain_id': 123, 'staking_contract': '0xcontract'}  # Test-only chain.
        self.tx = '0x' + 'a'*64
        self.profile = {'model':'Qwen/Qwen3-8B-AWQ','revision':'rev','precision':'AWQ 4-bit','context':4096}
        self.flow = m.Lifecycle({'api_domain':'api.example.com'}, self.chain, self.verify, lambda:self.now)
        self.project = self.flow.create('owner', 'credits', self.profile, 2000)
    def verify(self, tx, project, chain):
        return {'chain_id':chain['chain_id'],'contract':chain['staking_contract'],'tx_hash':tx,
                'project_id':project['id'],'owner':project['owner'],'amount':2000,
                'block_timestamp':1000,'block_hash':'canonical-block'}
    def test_no_launch_or_identity_before_verified_stake(self):
        self.assertNotIn('launch_id', self.project)
        self.flow.verifier = None
        with self.assertRaises(ValueError):self.flow.confirm_stake(self.project['id'], self.tx)
        self.assertFalse(self.flow.data['launches'])
    def test_tx_identity_block_deadline_and_no_retry_reset(self):
        p, secret = self.flow.confirm_stake(self.project['id'], self.tx)
        self.assertEqual(p['provider_id'], self.tx)
        self.assertEqual(p['verification_deadline'], 1000+86400)
        provider = self.flow.data['providers'][self.tx]
        self.assertLessEqual(len(provider['tunnel_id']),63)
        self.assertEqual(hashlib.sha256(secret.encode()).hexdigest(),provider['credential_sha256'])
        self.now += 100
        self.assertIsNone(self.flow.confirm_stake(p['id'],self.tx)[1])
        self.assertFalse(self.flow.can_sell(p['launch_id']))
    def test_late_confirmation_and_late_probe_cannot_open_sales(self):
        self.now=87400
        with self.assertRaises(ValueError):self.flow.confirm_stake(self.project['id'],self.tx)
        self.now=2000
        p,_=self.flow.confirm_stake(self.project['id'],self.tx)
        def slow(*_):
            self.now=87400
            return {k:True for k in ('authentication','model','usage','capacity')}
        with self.assertRaises(ValueError):self.flow.accept_rpc(p['id'],slow)
        self.assertFalse(self.flow.can_sell(p['launch_id']))
    def test_acceptance_is_required_and_duplicate_stake_is_rejected(self):
        p,_=self.flow.confirm_stake(self.project['id'],self.tx)
        other=self.flow.create('owner','other',self.profile,2000)
        with self.assertRaises(ValueError):self.flow.confirm_stake(other['id'],self.tx)
        with self.assertRaises(ValueError):self.flow.accept_rpc(p['id'],lambda *_:{'model':True})
        self.flow.accept_rpc(p['id'],lambda *_:{k:True for k in ('authentication','model','usage','capacity')})
        self.assertTrue(self.flow.can_sell(p['launch_id']))
    def test_wrong_chain_and_reorg_are_rejected(self):
        self.flow.verifier=lambda tx,p,c:self.verify(tx,p,c)|{'chain_id':999}
        with self.assertRaises(ValueError):self.flow.confirm_stake(self.project['id'],self.tx)
        self.flow.verifier=self.verify
        p,_=self.flow.confirm_stake(self.project['id'],self.tx)
        self.flow.verifier=lambda tx,p,c:self.verify(tx,p,c)|{'block_hash':'reorg'}
        with self.assertRaises(ValueError):self.flow.accept_rpc(p['id'],lambda *_:{})
