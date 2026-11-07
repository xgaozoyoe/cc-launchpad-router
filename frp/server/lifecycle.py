"""Server-side lifecycle. Chain verifier and RPC probe are trusted backend adapters, never client assertions."""
import hashlib
import re
import secrets
import time
import uuid

WINDOW = 86400


def tunnel_id(provider_id):
    # Full transaction hashes exceed DNS label length; never truncate the public identity.
    return 'p-' + hashlib.sha256(provider_id.encode()).hexdigest()[:40]


class Lifecycle:
    def __init__(self, data, default_chain=None, stake_verifier=None, clock=time.time):
        self.data = data
        self.chain = default_chain
        self.verifier = stake_verifier
        self.clock = clock
        for key in ('projects', 'providers', 'launches'):
            self.data.setdefault(key, {})

    def create(self, owner, name, model, stake_required):
        if not owner or not name or not model or type(stake_required) is not int or stake_required <= 0:
            raise ValueError('Owner, project name and model profile are required')
        project = {'id': str(uuid.uuid4()), 'owner': owner, 'name': name,
                   'profile': dict(model), 'stake_required': stake_required, 'status': 'awaiting-stake'}
        self.data['projects'][project['id']] = project
        return project

    def confirm_stake(self, project_id, tx_hash):
        p = self.data['projects'][project_id]
        tx_hash = tx_hash.lower()
        if not self.chain or not self.verifier:
            raise ValueError('Platform default-chain staking verifier is not configured')
        if not re.fullmatch(r'0x[0-9a-f]{64}', tx_hash):
            raise ValueError('This adapter requires a 32-byte transaction hash')
        if p.get('provider_id') == tx_hash:
            return p, None  # No deadline reset, new identity, or reissued credential on retry.
        if p['status'] != 'awaiting-stake' or tx_hash in self.data['providers']:
            raise ValueError('Project or stake transaction is already assigned')
        # Adapter must check canonical successful receipt, finality, staking event,
        # contract, sender/beneficiary, project binding, amount, and block timestamp.
        receipt = self.verifier(tx_hash, p, self.chain)
        required = ('chain_id', 'contract', 'tx_hash', 'project_id', 'owner', 'amount', 'block_timestamp', 'block_hash')
        if not isinstance(receipt, dict) or any(k not in receipt for k in required):
            raise ValueError('Incomplete verified stake receipt')
        if (receipt['chain_id'] != self.chain['chain_id'] or receipt['contract'].lower() != self.chain['staking_contract'].lower()
                or receipt['tx_hash'].lower() != tx_hash or receipt['project_id'] != p['id'] or receipt['owner'] != p['owner']
                or receipt['amount'] != p['stake_required']):
            raise ValueError('Verified stake does not match the project/default chain')
        staked_at = receipt['block_timestamp']
        now = int(self.clock())
        if type(staked_at) is not int or staked_at <= 0 or staked_at > now or now >= staked_at + WINDOW:
            raise ValueError('Stake block timestamp invalid or deployment window expired')
        launch_id = 'launch-' + str(uuid.uuid4())
        credential = secrets.token_urlsafe(32)
        alias = tunnel_id(tx_hash)
        provider = {'id': tx_hash, 'tunnel_id': alias, **p['profile'], 'enabled': True,
                    'credential_sha256': hashlib.sha256(credential.encode()).hexdigest(),
                    'project_id': p['id'], 'stake': receipt, 'launch_id': launch_id,
                    'status': 'awaiting-rpc', 'verification_deadline': staked_at + WINDOW}
        launch = {'id': launch_id, 'provider_id': tx_hash, **p['profile'], 'enabled': True,
                  'status': 'awaiting-rpc', 'staked_at': staked_at, 'verification_deadline': staked_at + WINDOW,
                  'project_id': p['id'], 'endpoint': f"https://{self.data['api_domain']}/launches/{launch_id}/v1"}
        self.data['providers'][tx_hash] = provider
        self.data['launches'][launch_id] = launch
        p.update(status='awaiting-rpc', provider_id=tx_hash, launch_id=launch_id, stake=receipt,
                 staked_at=staked_at, verification_deadline=staked_at + WINDOW)
        return p, credential  # Deliver only through an authenticated private configuration channel.

    def accept_rpc(self, project_id, probe):
        p = self.data['projects'][project_id]
        if p['status'] == 'live':
            return p
        if p['status'] != 'awaiting-rpc' or int(self.clock()) >= p['verification_deadline']:
            raise ValueError('RPC acceptance deadline exceeded or project is not awaiting RPC')
        if not self.verifier or not self.chain:
            raise ValueError('Default-chain verifier unavailable')
        current = self.verifier(p['provider_id'], p, self.chain)
        if current != p['stake']:
            raise ValueError('Stake receipt changed; reorg or inconsistent verification')
        result = probe(self.data['launches'][p['launch_id']], self.data['providers'][p['provider_id']])
        if not isinstance(result, dict) or not all(result.get(k) is True for k in ('authentication', 'model', 'usage', 'capacity')):
            raise ValueError('RPC/model/capacity acceptance failed')
        now = int(self.clock())  # Probe must finish, not just start, before the deadline.
        if now >= p['verification_deadline']:
            raise ValueError('RPC acceptance finished after the deadline')
        p.update(status='live', verified_at=now)
        self.data['providers'][p['provider_id']]['status'] = 'live'
        self.data['launches'][p['launch_id']].update(status='live', verified_at=now)
        return p

    def can_sell(self, launch_id):
        launch = self.data['launches'].get(launch_id)
        return bool(launch and launch.get('enabled') and launch.get('status') == 'live'
                    and self.data['providers'].get(launch['provider_id'], {}).get('enabled'))
