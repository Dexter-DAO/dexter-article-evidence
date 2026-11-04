"""Native-v1 test infrastructure for newly reconstructed financing programs.

This uses generated current IDLs and real SBF/SPL programs. No account data,
lookup-table state, token balances or program processors are patched.
"""
from __future__ import annotations
import base64
import hashlib
import json
import struct
import sys
from pathlib import Path

from solders.clock import Clock
from solders.compute_budget import ComputeBudget
from solders.instruction import AccountMeta, Instruction
from solders.message import MessageV1, TransactionConfig
from solders.pubkey import Pubkey
from solders.system_program import create_account, CreateAccountParams
from solders.transaction import VersionedTransaction
from solders.transaction_metadata import FailedTransactionMetadata

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "foundation/money"))
from svm_common import Harness, M, key, TOKEN, ATA, SYSTEM, u64

OUT = ROOT / ".local/anchor"
REGISTRY = Pubkey.from_string("9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G")
CORE = Pubkey.from_string("727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz")
POOL = Pubkey.from_string("3yceApSVxLYihHjLLrCycV8T4Bg1MYggbxQhdzaqB2Sm")
SWIG = Pubkey.from_string("swigypWHEksbC64pWKwah1WTeh9JXwx8H1rJHLdbQMB")
SYSVAR_INSTRUCTIONS = Pubkey.from_string("Sysvar1nstructions1111111111111111111111111")
PROGRAMS = {"principal_registry": REGISTRY, "claims_core": CORE, "pool_issuer": POOL}
NOW = 1_790_000_000
SWIG_SHA256 = "bb744d9811c392e3f343952d1a99bef9c97aa1936248a23b9d7efc1adb8939c1"


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def ident(label):
    return hashlib.sha256(label.encode()).digest()


def pda(program, *seeds):
    return Pubkey.find_program_address([bytes(seed) for seed in seeds], program)[0]


def json_value(value):
    if isinstance(value, Pubkey):
        return str(value)
    if isinstance(value, bytes):
        return value.hex()
    raise TypeError(type(value).__name__)


class Idl:
    """IDL-driven Borsh encoder/reader; unsupported shapes fail explicitly."""
    FORMATS = {"u8": "B", "u16": "H", "u32": "I", "u64": "Q",
               "i8": "b", "i16": "h", "i32": "i", "i64": "q"}

    def __init__(self, path):
        self.path = Path(path)
        self.value = json.loads(self.path.read_text())
        self.program = Pubkey.from_string(self.value["address"])
        self.types = {t["name"]: t["type"] for t in self.value["types"]}
        self.instructions = {i["name"]: i for i in self.value["instructions"]}
        self.accounts = {a["name"]: a for a in self.value["accounts"]}

    def encode(self, kind, value):
        if isinstance(kind, str):
            if kind in self.FORMATS:
                return struct.pack("<" + self.FORMATS[kind], value)
            if kind in ("u128", "i128"):
                return value.to_bytes(16, "little", signed=kind == "i128")
            if kind == "bool":
                assert type(value) is bool
                return bytes([value])
            if kind in ("pubkey", "publicKey"):
                return bytes(value)
            raise ValueError("Unsupported primitive " + kind)
        if "array" in kind:
            item, length = kind["array"]
            assert len(value) == length
            return b"".join(self.encode(item, x) for x in value)
        if "option" in kind:
            return b"\0" if value is None else b"\1" + self.encode(kind["option"], value)
        if "vec" in kind:
            assert len(value) <= 256, "Fixture vector bound"
            return struct.pack("<I", len(value)) + b"".join(self.encode(kind["vec"], x) for x in value)
        if "defined" in kind:
            name = kind["defined"]
            name = name["name"] if isinstance(name, dict) else name
            body = self.types[name]
            if body["kind"] == "enum":
                tag = value if isinstance(value, str) else value["__kind"]
                selected = next((i, v) for i, v in enumerate(body["variants"]) if v["name"] == tag)
                index, variant = selected
                assert not variant.get("fields"), "Data enum needs explicit encoder"
                return bytes([index])
            assert body["kind"] == "struct"
            assert set(value) == {f["name"] for f in body["fields"]}, (name, value.keys())
            return b"".join(self.encode(f["type"], value[f["name"]]) for f in body["fields"])
        raise ValueError("Unsupported IDL type " + str(kind))

    def decode(self, kind, data, offset=0):
        if isinstance(kind, str):
            if kind in self.FORMATS:
                fmt = "<" + self.FORMATS[kind]
                return struct.unpack_from(fmt, data, offset)[0], offset + struct.calcsize(fmt)
            if kind in ("u128", "i128"):
                assert offset + 16 <= len(data)
                return int.from_bytes(data[offset:offset + 16], "little", signed=kind == "i128"), offset + 16
            if kind == "bool":
                assert data[offset] in (0, 1)
                return bool(data[offset]), offset + 1
            if kind in ("pubkey", "publicKey"):
                return Pubkey.from_bytes(data[offset:offset + 32]), offset + 32
            raise ValueError("Unsupported primitive " + kind)
        if "array" in kind:
            item, length = kind["array"]
            values = []
            for _ in range(length):
                value, offset = self.decode(item, data, offset)
                values.append(value)
            return (bytes(values) if item == "u8" else values), offset
        if "option" in kind:
            tag = data[offset]
            assert tag in (0, 1), "Noncanonical option"
            return (None, offset + 1) if tag == 0 else self.decode(kind["option"], data, offset + 1)
        if "vec" in kind:
            length = struct.unpack_from("<I", data, offset)[0]
            assert length <= 256, "Fixture decoder vector bound"
            offset += 4
            values = []
            for _ in range(length):
                value, offset = self.decode(kind["vec"], data, offset)
                values.append(value)
            return values, offset
        if "defined" in kind:
            name = kind["defined"]
            name = name["name"] if isinstance(name, dict) else name
            body = self.types[name]
            if body["kind"] == "enum":
                index = data[offset]
                assert index < len(body["variants"])
                variant = body["variants"][index]
                assert not variant.get("fields"), "Data enum needs explicit decoder"
                return variant["name"], offset + 1
            assert body["kind"] == "struct"
            result = {}
            for field in body["fields"]:
                result[field["name"]], offset = self.decode(field["type"], data, offset)
            return result, offset
        raise ValueError("Unsupported IDL type " + str(kind))

    def instruction(self, name, accounts, *, remaining=(), **args):
        info = self.instructions[name]
        assert set(args) == {a["name"] for a in info["args"]}, (name, args.keys())
        data = bytes(info["discriminator"]) + b"".join(
            self.encode(a["type"], args[a["name"]]) for a in info["args"])
        metas = []
        names = set()
        def append_accounts(fields):
            for account in fields:
                if "accounts" in account:
                    append_accounts(account["accounts"])
                    continue
                name = account["name"]
                assert name not in names, "Ambiguous grouped fixture account " + name
                names.add(name)
                public = accounts[name]
                if public is None:
                    assert account.get("optional", False), name
                    metas.append(M(self.program))
                else:
                    metas.append(M(public, account.get("signer", False), account.get("writable", False)))
        append_accounts(info["accounts"])
        return Instruction(self.program, data, [*metas, *remaining])

    def read(self, svm, address, account_name):
        account = svm.get_account(address)
        assert account is not None and account.owner == self.program and not account.executable, account_name
        definition = self.accounts[account_name]
        assert account.data[:8] == bytes(definition["discriminator"])
        result, used = self.decode({"defined": {"name": account_name}}, account.data, 8)
        if account_name == "FinanceReceipt":
            assert not any(account.data[used:]), "Receipt account has nonzero trailing bytes"
            assert len(result["data"]["effects"]) <= 8
        else:
            assert used == len(account.data), (account_name, used, len(account.data))
        return result


class V1Harness(Harness):
    """Actual SVM execution with signed native-v1 envelopes and no lookup tables."""
    def __init__(self, name):
        super().__init__(name)
        self.transport = {"version": 1, "packet_limit": 4096,
            "compute_unit_limit": 1_400_000, "loaded_accounts_data_size_limit": 64 * 1024**2,
            "priority_fee": 0, "heap_size": None,
            "litesvm_compute_budget_override": 1_400_000,
            "override_reason": "Pinned LiteSVM ignores native-v1 inline resource settings; these local executions do not qualify customer RPC resource admission or priority-fee charging"}
        budget = ComputeBudget(False)
        budget.compute_unit_limit = 1_400_000
        assert budget.heap_size == 32 * 1024
        self.svm = self.svm.with_compute_budget(budget)
        self.svm.set_clock(Clock(100, NOW, 0, 0, NOW))

    def make_mint(self, mint_key, initial=10_000_000):
        """Synthetic six-decimal token with a disposable explicit freeze owner.

        The real freeze/thaw instructions let servicing test an unavailable fee
        route without modifying token accounts or replacing the token program.
        """
        mint = mint_key.pubkey()
        create = create_account(CreateAccountParams(from_pubkey=self.payer.pubkey(), to_pubkey=mint,
            lamports=self.svm.minimum_balance_for_rent_exemption(82), space=82, owner=TOKEN))
        initialize = Instruction(TOKEN, bytes([20, 6]) + bytes(self.payer.pubkey())
            + bytes([1]) + bytes(self.payer.pubkey()), [M(mint, w=True)])
        source = self.ata(self.payer.pubkey(), mint)
        mint_to = Instruction(TOKEN, bytes([7]) + u64(initial),
            [M(mint, w=True), M(source, w=True), M(self.payer.pubkey(), True)])
        self.send("initialize synthetic SPL token and disposable freeze authority",
            [create, initialize, self.ata_ix(self.payer.pubkey(), mint), mint_to], [mint_key])
        return mint, source

    def transaction(self, instructions, signers=()):
        self.svm.expire_blockhash()
        config = TransactionConfig(priority_fee=0, compute_unit_limit=1_400_000,
            loaded_accounts_data_size_limit=64 * 1024**2)
        message = MessageV1.try_compile(self.payer.pubkey(), instructions, self.svm.latest_blockhash(), config)
        keys = {str(k.pubkey()): k for k in (self.payer, *signers)}
        tx = VersionedTransaction(message, list(keys.values()))
        assert bytes(tx)[0] == 0x81 and len(bytes(tx)) <= 4096
        assert all(tx.verify_with_results())
        return tx

    def send(self, name, instructions, signers=(), fail=None, unchanged=(), transaction=None):
        tx = transaction or self.transaction(instructions, signers)
        assert isinstance(tx.message, MessageV1), "New financial native fixture requires v1"
        before = self.snapshot(unchanged)
        observed_before = self.snapshot(self.observe)
        result = self.svm.send_transaction(tx)
        raw = json.loads(result.to_json())
        failed = isinstance(result, FailedTransactionMetadata)
        row = {"name": name, "expected": "reject" if fail else "success", "passed": False,
            "transaction_base64": base64.b64encode(bytes(tx)).decode(),
            "wire_bytes": len(bytes(tx)), "version": 1,
            "config": json.loads(tx.message.config.to_json()),
            "account_keys": [str(k) for k in tx.message.account_keys],
            "signature_verified": all(tx.verify_with_results()), "result": raw,
            "instructions": [{"program_id": str(ix.program_id), "data_hex": bytes(ix.data).hex(),
                "ordered_metas": [{"index": i, "pubkey": str(m.pubkey),
                    "is_signer": m.is_signer, "is_writable": m.is_writable}
                    for i, m in enumerate(ix.accounts)]} for ix in instructions],
            "observed_before": observed_before, "observed_after": self.snapshot(self.observe),
            "rollback_accounts": [str(k) for k in unchanged]}
        self.cases.append(row)
        if fail is None:
            assert not failed, (name, raw)
        else:
            assert failed, (name, "unexpected success")
            if fail != "any":
                assert str(fail) in json.dumps(raw), (name, fail, raw)
            assert self.snapshot(unchanged) == before, (name, "failed operation changed protected state")
        row["passed"] = True
        return result


def verified_artifacts(source_root=ROOT):
    """Select an exact source/artifact pair, including a pinned migration base."""
    source_root = Path(source_root)
    output = source_root / ".local/anchor"
    build = json.loads((output / "build-receipt.json").read_text())
    idl = json.loads((output / "idl-receipt.json").read_text())
    assert build["status"] == idl["status"] == "passed"
    assert build["inputs"] == idl["inputs"], "Programs and generated IDLs have different source"
    for path, checksum in build["inputs"].items():
        assert digest(source_root / path) == checksum, (path, "source changed since build")
    for record in (build, idl):
        for artifact in record["artifacts"].values():
            assert digest(artifact["path"]) == artifact["sha256"]
    return build, idl


def load_programs(harness, source_root=ROOT):
    source_root = Path(source_root)
    build, idl = verified_artifacts(source_root)
    idls = {name: Idl(source_root / ".local/anchor/idl" / (name + ".json")) for name in PROGRAMS}
    for name, program in PROGRAMS.items():
        assert idls[name].program == program
        harness.load(program, build["artifacts"][name + ".so"]["path"])
    swig = ROOT / "foundation/swig/runtime/artifacts/swig-retained.so"
    assert digest(swig) == SWIG_SHA256
    harness.load(SWIG, swig)
    return idls, build, idl


def compact_sign(config, wallet, inner):
    metas = [M(config, w=True), M(wallet, w=True), M(SYSVAR_INSTRUCTIONS)]
    indexes = {str(m.pubkey): i for i, m in enumerate(metas)}
    program_index = len(metas)
    indexes[str(inner.program_id)] = program_index
    metas.append(M(inner.program_id))
    selected = []
    for meta in inner.accounts:
        signer = meta.is_signer and meta.pubkey != wallet
        index = indexes.get(str(meta.pubkey))
        if index is None:
            index = len(metas)
            indexes[str(meta.pubkey)] = index
            metas.append(M(meta.pubkey, signer, meta.is_writable))
        else:
            old = metas[index]
            metas[index] = M(old.pubkey, old.is_signer or signer, old.is_writable or meta.is_writable)
        selected.append(index)
    assert len(metas) <= 256 and len(selected) <= 255 and len(inner.data) <= 65535
    compact = bytes([1, program_index, len(selected)]) + bytes(selected) + struct.pack("<H", len(inner.data)) + bytes(inner.data)
    return Instruction(SWIG, struct.pack("<HHI", 11, len(compact), 0) + compact + bytes([2]), metas)


def canonical_instruction(instruction, envelope, fee_payer):
    flags = {str(fee_payer): [True, True]}
    for ix in envelope:
        for meta in ix.accounts:
            old = flags.setdefault(str(meta.pubkey), [False, False])
            old[0] |= meta.is_signer
            old[1] |= meta.is_writable
    raw = struct.pack("<H", len(instruction.accounts))
    for meta in instruction.accounts:
        signer, writable = flags[str(meta.pubkey)]
        raw += bytes([int(signer) | int(writable) << 1]) + bytes(meta.pubkey)
    return raw + bytes(instruction.program_id) + struct.pack("<H", len(instruction.data)) + bytes(instruction.data)


def check_receipt(idl, svm, address):
    """Independent Python hash/address checks over actual Core account bytes."""
    receipt = idl.read(svm, address, "FinanceReceipt")["data"]
    assert receipt["version"] == 1
    assert address == pda(CORE, b"finance-operation", receipt["principal"], receipt["operation_id"])
    raw = idl.encode({"defined": {"name": "FinanceReceiptData"}}, receipt)
    expected = hashlib.sha256(b"DEXTER:FINANCIAL-RECEIPT:1" + bytes(CORE) + raw[:-32] + bytes(32)).digest()
    assert receipt["effects_hash"] == expected
    seen = set()
    for effect in receipt["effects"]:
        assert effect["book"] == pda(CORE, b"financial-book", effect["principal"], receipt["mint"])
        assert str(effect["book"]) not in seen and effect["sequence"] > 0
        seen.add(str(effect["book"]))
        values = bytes(address) + bytes(receipt["mint"]) + bytes(effect["book"]) + bytes(effect["principal"])
        values += u64(effect["sequence"]) + effect["previous_hash"]
        values += idl.encode({"defined": {"name": "FinancialPositions"}}, effect["before"])
        values += idl.encode({"defined": {"name": "FinancialPositions"}}, effect["after"])
        expected = hashlib.sha256(b"DEXTER:FINANCE-BOOK-EFFECT:1" + bytes(CORE) + values).digest()
        assert effect["hash"] == expected
    return receipt
