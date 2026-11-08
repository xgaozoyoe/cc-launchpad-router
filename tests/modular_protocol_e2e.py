"""Disposable Anvil + authenticated HTTP model fixture; no production RPC or wallet.
Requires forge build and Python eth-abi/eth-utils (same venv as stake_rust_e2e.py).
This exercises the modular chain flow, not the existing Rust or browser integration.
"""
import json
import os
import secrets
import socket
import subprocess
import tempfile
import threading
import time
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from eth_abi import encode, decode
from eth_utils import keccak

ROOT = Path(__file__).resolve().parents[1]
FOUNDRY = Path.home() / '.foundry/bin'
PROJECT = keccak(text='modular local integration')
MODEL = keccak(text='qwen3-8b-awq')
ZERO = bytes(32)


def free_port():
    with socket.socket() as s:
        s.bind(('127.0.0.1', 0))
        return s.getsockname()[1]


def wait(fn):
    end = time.monotonic() + 30
    while time.monotonic() < end:
        try:
            result = fn()
            if result:
                return result
        except OSError:
            pass
        time.sleep(0.1)
    raise AssertionError('Local service timed out')


def run():
    port = free_port()
    rpc_url = f'http://127.0.0.1:{port}'
    api_key = secrets.token_urlsafe(24)
    prompts = []

    class Fixture(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_POST(self):
            if self.path != '/v1/chat/completions' or self.headers.get('Authorization') != f'Bearer {api_key}':
                self.send_error(401)
                return
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            assert body['model'] == 'qwen3-8b-awq'
            assert body['max_tokens'] == 100
            prompts.append(body['messages'])
            result = {'model': body['model'], 'choices': [{'message': {'role': 'assistant', 'content': 'HTTP provider fixture response.'}}],
                      'usage': {'prompt_tokens': 12, 'completion_tokens': 49, 'total_tokens': 61}}
            payload = json.dumps(result).encode()
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

    http = ThreadingHTTPServer(('127.0.0.1', 0), Fixture)
    threading.Thread(target=http.serve_forever, daemon=True).start()
    with tempfile.TemporaryDirectory(prefix='modular-e2e-') as tmp:
        with open(Path(tmp)/'anvil.log', 'w+') as log:
            process = subprocess.Popen([str(FOUNDRY/'anvil'), '--port', str(port), '--silent'], stdout=log, stderr=log)
            try:
                def rpc(method, params):
                    req = urllib.request.Request(rpc_url, data=json.dumps({'jsonrpc': '2.0', 'id': 1, 'method': method, 'params': params}).encode(), headers={'Content-Type': 'application/json'})
                    with urllib.request.urlopen(req, timeout=10) as response:
                        result = json.load(response)
                    if 'error' in result:
                        raise AssertionError(result['error'])
                    return result['result']

                operator, provider, buyer = wait(lambda: rpc('eth_accounts', []))[:3]

                def calldata(signature, types=(), values=()):
                    return '0x' + (keccak(text=signature)[:4] + encode(types, values)).hex()

                def send(sender, target, data, value=0, success=True):
                    tx = {'from': sender, 'data': data, 'value': hex(value), 'gas': hex(20_000_000)}
                    if target:
                        tx['to'] = target
                    tx_hash = rpc('eth_sendTransaction', [tx])
                    receipt = wait(lambda: rpc('eth_getTransactionReceipt', [tx_hash]))
                    assert int(receipt['status'], 16) == int(success), receipt
                    return receipt

                def deploy(name, types=(), values=()):
                    artifact = next((ROOT/'contracts/out').glob(f'*/{name}.json'))
                    code = json.loads(artifact.read_text())['bytecode']['object']
                    receipt = send(operator, None, code + encode(types, values).hex())
                    return receipt['contractAddress'], int(receipt['gasUsed'], 16)

                def call(target, signature, types=(), values=(), returns=('uint256',)):
                    result = rpc('eth_call', [{'to': target, 'data': calldata(signature, types, values)}, 'latest'])
                    return decode(returns, bytes.fromhex(result[2:]))

                def action(sender, target, signature, types=(), values=(), value=0, success=True):
                    return send(sender, target, calldata(signature, types, values), value, success)

                prl, _ = deploy('TestPRL', ['address', 'uint256'], [operator, 10_000*10**18])
                governance, _ = deploy('LaunchpadTimelock', ['address[]', 'address[]'], [[operator], [operator]])
                factory, deployment_gas = deploy('TestnetProtocolFactory', ['address']*6, [prl, governance, operator, operator, operator, operator])
                registry, token, usage, payment, stake, sale, settlement = [call(factory, f'{name}()', returns=['address'])[0] for name in
                    ['registry', 'creditToken', 'usageEscrow', 'paymentEscrow', 'stakeVault', 'saleRouter', 'settlementModule']]
                action(operator, registry, 'register(bytes32,address,uint256,uint256,uint256,bytes32)', ['bytes32','address','uint256','uint256','uint256','bytes32'], [PROJECT, provider, 2000*10**18, 1_000_000, 10**9, MODEL])
                action(operator, prl, 'transfer(address,uint256)', ['address','uint256'], [provider,2000*10**18])
                action(provider, prl, 'approve(address,uint256)', ['address','uint256'], [stake,2000*10**18])
                action(provider, stake, 'stake(bytes32)', ['bytes32'], [PROJECT])
                action(operator, registry, 'verifyProject(bytes32)', ['bytes32'], [PROJECT])
                action(buyer, sale, 'buyAndLock(bytes32,uint256)', ['bytes32','uint256'], [PROJECT,1000], 1000*10**9)
                assert int(rpc('eth_getBalance',[payment,'latest']),16)==1000*10**9
                assert int(rpc('eth_getBalance',[sale,'latest']),16)==0
                now = int(rpc('eth_getBlockByNumber',['latest',False])['timestamp'],16)
                reservation = keccak(text='real HTTP request reservation')
                action(buyer, usage, 'reserve(bytes32,bytes32,uint256,uint64)', ['bytes32','bytes32','uint256','uint64'], [PROJECT,reservation,100,now+3600])
                payload = {'model':'qwen3-8b-awq','messages':[{'role':'user','content':'Explain the sky.'}],'max_tokens':100}
                request = urllib.request.Request(f'http://127.0.0.1:{http.server_port}/v1/chat/completions',data=json.dumps(payload).encode(),headers={'Content-Type':'application/json','Authorization':f'Bearer {api_key}'})
                with urllib.request.urlopen(request,timeout=10) as response:
                    generated = json.load(response)
                assert len(prompts)==1 and generated['choices'][0]['message']['content']
                receipt_id = keccak(text='actual HTTP completion')
                action(operator,settlement,'settleProvider(bytes32,bytes32,uint256)',['bytes32','bytes32','uint256'],[reservation,receipt_id,generated['usage']['completion_tokens']])
                assert call(usage,'accounts(bytes32,address)',['bytes32','address'],[PROJECT,buyer],['uint256']*5)==(951,0,49,1000,0)
                assert call(payment,'funds(bytes32)',['bytes32'],[PROJECT],['uint256']*4)==(1000*10**9,951*10**9,49*10**9,0)
                assert call(stake,'collateralState(bytes32)',['bytes32'],[PROJECT],['uint256']*4)==(2000*10**18,1999902*10**15,98*10**15,0)
                action(provider,stake,'withdraw(bytes32,uint256)',['bytes32','uint256'],[PROJECT,40*10**15])
                action(provider,payment,'claimRevenue(bytes32,uint256)',['bytes32','uint256'],[PROJECT,49*10**9])
                assert call(stake,'collateralState(bytes32)',['bytes32'],[PROJECT],['uint256']*4)==(2000*10**18,1999902*10**15,58*10**15,40*10**15)
                action(operator,settlement,'settleProvider(bytes32,bytes32,uint256)',['bytes32','bytes32','uint256'],[reservation,receipt_id,49],success=False)
                action(buyer,usage,'withdraw(bytes32,uint256)',['bytes32','uint256'],[PROJECT,951])
                assert call(token,'balanceOf(address,uint256)',['address','uint256'],[buyer,int.from_bytes(PROJECT,'big')])==(951,)
                assert call(prl,'balanceOf(address)',['address'],[stake])==(2000*10**18-40*10**15,)
                print(json.dumps({'status':'passed','environment':'disposable Anvil + authenticated HTTP fixture; no Rust/UI integration',
                    'factory_deployment_gas':deployment_gas,'purchased':1000,'reserved':100,'provider_burned':49,'buyer_withdrawn_tokens':951,
                    'initial_prl':'2000','free_prl':'0.058','returned_prl':'0.04','provider_revenue_claimed_wei':49*10**9,
                    'production_writes':False},indent=2))
            finally:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
                http.shutdown()
                http.server_close()


if __name__ == '__main__':
    run()
