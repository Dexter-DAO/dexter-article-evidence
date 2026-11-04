"""Actual official SPL Token Swap instructions and synthetic constant-product pools.

The adapter ELF is source-built, never a replacement processor. Pool state,
token balances and liquidity are created through signed System/SPL/swap calls.
Only the local fixture fee schedule is selected here, with no production policy.
"""
from __future__ import annotations
from dataclasses import dataclass
import hashlib
import json
import struct

from solders.keypair import Keypair
from financing_support import ROOT, Pubkey, Instruction, M, TOKEN, u64, create_account, CreateAccountParams

ADAPTER_PIN = ROOT / "foundation/onchain/adapters/spl-token-swap.json"
SPL_SWAP = Pubkey.from_string("SwapsVeCiPHMUAtzQWZw7RjsKjgCjhwU55QGu4U1Szw")
FIXTURE_FEES = (25, 10_000, 5, 10_000, 0, 0, 0, 0)


def fixture_key(label):
    return Keypair.from_seed(hashlib.sha256(("dexter-spl-swap-fixture:" + label).encode()).digest())


def load_adapter(harness):
    pin = json.loads(ADAPTER_PIN.read_text())
    receipt_path = ROOT / ".local/adapters/spl-token-swap/build-receipt.json"
    receipt = json.loads(receipt_path.read_text())
    assert receipt["status"] == "passed"
    assert receipt["source_commit"] == pin["source_commit"]
    artifact = receipt["artifact"]
    actual = harness.load(SPL_SWAP, artifact["path"])
    assert actual == artifact["sha256"] == pin["qualified_artifact_sha256"]
    return pin, receipt


def constant_product_quote(amount_in, reserve_in, reserve_out, fees=FIXTURE_FEES):
    """Independent integer reconstruction of upstream fees and checked_ceil_div.

    Upstream can charge less than amount_in after rounding. Dexter's exact-input
    postcondition intentionally refuses that result instead of pretending the
    instruction argument proves the actual debit.
    """
    assert 0 < amount_in < 2**64 and reserve_in > 0 and reserve_out > 0
    def fee(numerator, denominator):
        return 0 if numerator == 0 else max(1, amount_in * numerator // denominator)
    trade_fee = fee(*fees[:2])
    owner_fee = fee(*fees[2:4])
    net = amount_in - trade_fee - owner_fee
    if net <= 0:
        return None
    invariant = reserve_in * reserve_out
    new_in = reserve_in + net
    new_out, remainder = divmod(invariant, new_in)
    if new_out == 0:
        return None
    if remainder:
        new_out += 1
        new_in = (invariant + new_out - 1) // new_out
    output = reserve_out - new_out
    if output <= 0:
        return None
    return {"actual_input": new_in - reserve_in + trade_fee + owner_fee,
            "output": output, "trade_fee": trade_fee, "owner_fee": owner_fee}


@dataclass(frozen=True)
class TokenSwapPool:
    state: Pubkey
    authority: Pubkey
    vault_a: Pubkey
    vault_b: Pubkey
    mint_a: Pubkey
    mint_b: Pubkey
    pool_mint: Pubkey
    fee_account: Pubkey
    initial_liquidity_account: Pubkey
    fees: tuple[int, ...]

    def oriented(self, reverse=False):
        return (self.vault_b, self.vault_a, self.mint_b, self.mint_a) if reverse else \
               (self.vault_a, self.vault_b, self.mint_a, self.mint_b)

    def quote(self, harness, amount, reverse=False):
        source, destination, _, _ = self.oriented(reverse)
        return constant_product_quote(amount, harness.amount(source), harness.amount(destination), self.fees)

    def exact_input(self, harness, start, reverse=False, limit=10_000):
        for amount in range(start, start + limit):
            quote = self.quote(harness, amount, reverse)
            if quote and quote["actual_input"] == amount:
                return amount, quote["output"]
        raise AssertionError("No exactly debited fixture input in bounded search")

    def instruction(self, authority, source, destination, amount, minimum_output, reverse=False):
        pool_in, pool_out, mint_in, mint_out = self.oriented(reverse)
        return Instruction(SPL_SWAP, bytes([1]) + u64(amount) + u64(minimum_output), [
            M(self.state), M(self.authority), M(authority, True), M(source, w=True),
            M(pool_in, w=True), M(pool_out, w=True), M(destination, w=True),
            M(self.pool_mint, w=True), M(self.fee_account, w=True),
            M(mint_in), M(mint_out), M(TOKEN), M(TOKEN), M(TOKEN)])

    def accounts(self):
        return [self.state, self.vault_a, self.vault_b, self.mint_a, self.mint_b,
                self.pool_mint, self.fee_account, self.initial_liquidity_account]


def create_pool(harness, mint_a, mint_b, *, label="pool", reserve_a=1_000_000,
                reserve_b=1_000_000, fees=FIXTURE_FEES):
    assert mint_a != mint_b and len(fees) == 8
    state_key = fixture_key(label + ":state")
    pool_mint_key = fixture_key(label + ":pool-mint")
    state, pool_mint = state_key.pubkey(), pool_mint_key.pubkey()
    authority, bump = Pubkey.find_program_address([bytes(state)], SPL_SWAP)
    fee_owner = fixture_key(label + ":fee-owner").pubkey()
    pool = TokenSwapPool(state, authority, harness.ata(authority, mint_a),
        harness.ata(authority, mint_b), mint_a, mint_b, pool_mint,
        harness.ata(fee_owner, pool_mint), harness.ata(harness.payer.pubkey(), pool_mint), tuple(fees))
    for account in pool.accounts():
        if account not in harness.observe:
            harness.observe.append(account)
    instructions = [
        create_account(CreateAccountParams(from_pubkey=harness.payer.pubkey(), to_pubkey=state,
            lamports=harness.svm.minimum_balance_for_rent_exemption(324), space=324, owner=SPL_SWAP)),
        create_account(CreateAccountParams(from_pubkey=harness.payer.pubkey(), to_pubkey=pool_mint,
            lamports=harness.svm.minimum_balance_for_rent_exemption(82), space=82, owner=TOKEN)),
        Instruction(TOKEN, bytes([20, 6]) + bytes(authority) + bytes([0]), [M(pool_mint, w=True)]),
        harness.ata_ix(authority, mint_a), harness.ata_ix(authority, mint_b),
        harness.ata_ix(fee_owner, pool_mint), harness.ata_ix(harness.payer.pubkey(), pool_mint),
    ]
    for mint, destination, amount in [(mint_a, pool.vault_a, reserve_a), (mint_b, pool.vault_b, reserve_b)]:
        instructions.append(Instruction(TOKEN, bytes([12]) + u64(amount) + bytes([6]),
            [M(harness.ata(harness.payer.pubkey(), mint), w=True), M(mint),
             M(destination, w=True), M(harness.payer.pubkey(), True)]))
    harness.send("create actual SPL Token Swap pool accounts and funded reserves", instructions,
                 [state_key, pool_mint_key])
    initialize = Instruction(SPL_SWAP, bytes([0]) + struct.pack("<8Q", *fees) + bytes(33), [
        M(state, True, True), M(authority), M(pool.vault_a), M(pool.vault_b),
        M(pool_mint, w=True), M(pool.fee_account), M(pool.initial_liquidity_account, w=True), M(TOKEN)])
    harness.send("initialize actual upstream constant-product pool", [initialize], [state_key])
    raw = harness.svm.get_account(state)
    assert raw.owner == SPL_SWAP and len(raw.data) == 324
    expected = bytes([1, 1, bump]) + b"".join(bytes(x) for x in [TOKEN, pool.vault_a,
        pool.vault_b, pool_mint, mint_a, mint_b, pool.fee_account]) + struct.pack("<8Q", *fees) + bytes(33)
    harness.check("actual initialized pool matches canonical upstream state", raw.data.hex(), expected.hex())
    harness.check("pool A reserve came from real funded transfer", harness.amount(pool.vault_a), reserve_a)
    harness.check("pool B reserve came from real funded transfer", harness.amount(pool.vault_b), reserve_b)
    return pool
