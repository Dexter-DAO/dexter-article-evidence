"""Real local SVM helpers. Test keys and genesis lamports are disposable fixtures.

All SPL balances are created/moved by actual System/Token/ATA transactions. This
module does not replace any program processor and never sends to a public RPC.
"""
import base64, hashlib, json, struct
from pathlib import Path
from solders.account import Account
from solders.clock import Clock
from solders.instruction import Instruction, AccountMeta
from solders.keypair import Keypair
from solders.litesvm import LiteSVM
from solders.pubkey import Pubkey
from solders.system_program import create_account, CreateAccountParams
from solders.token.associated import get_associated_token_address
from solders.transaction import Transaction
from solders.transaction_metadata import FailedTransactionMetadata

ROOT = Path(__file__).resolve().parent
TOKEN = Pubkey.from_string('TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA')
ATA = Pubkey.from_string('ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL')
SYSTEM = Pubkey.default()
LOADER = Pubkey.from_string('BPFLoaderUpgradeab1e11111111111111111111111')
ED25519 = Pubkey.from_string('Ed25519SigVerify111111111111111111111111111')
INSTRUCTIONS = Pubkey.from_string('Sysvar1nstructions1111111111111111111111111')
u64 = lambda v: struct.pack('<Q', v)
u16 = lambda v: struct.pack('<H', v)
M = lambda key, s=False, w=False: AccountMeta(key, s, w)

def key(n):
    return Keypair.from_seed(bytes([n]) * 32)

class Harness:
    def __init__(self, name):
        self.name = name
        self.svm = LiteSVM().with_sigverify(True).with_blockhash_check(True)
        self.payer = key(1)
        self.svm.airdrop(self.payer.pubkey(), 100_000_000_000)
        self.svm.set_clock(Clock(100, 1_790_000_000, 0, 0, 1_790_000_000))
        self.cases = []
        self.assertions = []
        self.observe = []

    def load(self, program, file, upgrade_authority=None):
        self.svm.add_program_from_file(program, Path(file))
        if upgrade_authority:
            # Explicit local-deployment fixture. Changes the local loader metadata,
            # never the compiled ELF or the live program. Required to exercise the
            # real Claims set_fee_policy deployment-authority instruction.
            pd, _ = Pubkey.find_program_address([bytes(program)], LOADER)
            a = self.svm.get_account(pd)
            d = bytearray(a.data)
            d[12] = 1
            d[13:45] = bytes(upgrade_authority)
            self.svm.set_account(pd, Account(a.lamports, bytes(d), a.owner, a.executable, a.rent_epoch))
        return hashlib.sha256(Path(file).read_bytes()).hexdigest()

    def snapshot(self, keys):
        result = {}
        for k in keys:
            a = self.svm.get_account(k)
            result[str(k)] = None if a is None else {
                'owner': str(a.owner), 'lamports': a.lamports,
                'data': base64.b64encode(a.data).decode(), 'executable': a.executable}
        return result

    def send(self, name, instructions, signers=(), fail=None, unchanged=(), transaction=None):
        if transaction is None: self.svm.expire_blockhash()
        signer_map = {str(k.pubkey()): k for k in [self.payer, *signers]}
        tx = transaction or Transaction.new_signed_with_payer(instructions, self.payer.pubkey(), list(signer_map.values()), self.svm.latest_blockhash())
        before = self.snapshot(unchanged)
        observed_before = self.snapshot(self.observe)
        result = self.svm.send_transaction(tx)
        failed = isinstance(result, FailedTransactionMetadata)
        raw = json.loads(result.to_json())
        if fail is None:
            assert not failed, (name, raw)
        else:
            assert failed, (name, 'unexpected success')
            if fail != 'any':
                assert str(fail) in str(raw.get('err')), (name, fail, raw)
            assert self.snapshot(unchanged) == before, (name, 'failed transaction mutated protected state')
        self.cases.append({'name': name, 'expected': 'reject' if fail is not None else 'success',
            'passed': True, 'transaction_base64': base64.b64encode(bytes(tx)).decode(),
            'wire_bytes': len(bytes(tx)), 'account_keys': [str(k) for k in tx.message.account_keys],
            'signature_verified': True, 'result': raw,
            'instructions': [{'program_id': str(ix.program_id), 'data_hex': bytes(ix.data).hex(),
                'ordered_metas': [{'index': j, 'pubkey': str(m.pubkey), 'is_signer': m.is_signer, 'is_writable': m.is_writable}
                    for j, m in enumerate(ix.accounts)]} for ix in instructions],
            'observed_before': observed_before, 'observed_after': self.snapshot(self.observe),
            'rollback_accounts': [str(k) for k in unchanged]})
        return result

    def check(self, name, actual, expected):
        assert actual == expected, (name, actual, expected)
        self.assertions.append({'name': name, 'actual': actual, 'expected': expected, 'passed': True})

    def amount(self, token):
        a = self.svm.get_account(token)
        return int.from_bytes(a.data[64:72], 'little') if a else 0

    def ata(self, owner, mint):
        return get_associated_token_address(owner, mint, TOKEN)

    def ata_ix(self, owner, mint):
        address = self.ata(owner, mint)
        return Instruction(ATA, bytes([1]), [M(self.payer.pubkey(), True, True), M(address, w=True),
            M(owner), M(mint), M(SYSTEM), M(TOKEN)])

    def make_mint(self, mint_key, initial=10_000_000):
        mint = mint_key.pubkey()
        ix0 = create_account(CreateAccountParams(from_pubkey=self.payer.pubkey(), to_pubkey=mint,
            lamports=self.svm.minimum_balance_for_rent_exemption(82), space=82, owner=TOKEN))
        ix1 = Instruction(TOKEN, bytes([20, 6]) + bytes(self.payer.pubkey()) + bytes([0]), [M(mint, w=True)])
        ix2 = self.ata_ix(self.payer.pubkey(), mint)
        source = self.ata(self.payer.pubkey(), mint)
        ix3 = Instruction(TOKEN, bytes([7]) + u64(initial), [M(mint, w=True), M(source, w=True), M(self.payer.pubkey(), True)])
        self.send('initialize actual SPL mint and fund source', [ix0, ix1, ix2, ix3], [mint_key])
        return mint, source

    def finish(self, extras):
        out = {'scope': self.name, 'engine': 'solders 0.29.0 LiteSVM with compiled SBF programs and default SPL Token/ATA programs',
            'public_transactions_sent': 0, 'signature_verification': self.svm.get_sigverify(),
            'local_genesis_fixtures': ['deterministic disposable test keys', 'airdrop lamports', 'explicit test Clock'],
            'transaction_cases': self.cases, 'assertions': self.assertions,
            'passed_transaction_cases': len(self.cases), 'passed_state_assertions': len(self.assertions), **extras}
        path = ROOT / (self.name + '-results.json')
        path.write_text(json.dumps(out, indent=2) + '\n')
        print(json.dumps({'file': str(path), 'transaction_cases': len(self.cases), 'state_assertions': len(self.assertions), 'failed': 0}))
        return out

def ed25519_ix(authority, message):
    signature = bytes(authority.sign_message(message))
    offsets = struct.pack('<7H', 16, 65535, 80, 65535, 112, len(message), 65535)
    return Instruction(ED25519, bytes([1, 0]) + offsets + signature + bytes(authority.pubkey()) + message, [])
