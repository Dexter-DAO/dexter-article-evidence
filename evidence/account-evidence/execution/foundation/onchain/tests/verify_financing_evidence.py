#!/usr/bin/env python3
"""Verify a recorded reconstruction trace without trusting its summary counts.

This checks signed native-v1 bytes, recorded account continuity/rollback,
canonical receipt hashes and addresses, complete atomic book changes, and exact
token legs represented by each financial receipt. It does not independently
attest that a recorded local runtime result came from a public validator.
"""
from __future__ import annotations
import argparse
import base64
from collections import defaultdict
import json
import lzma
from pathlib import Path
from types import SimpleNamespace

from financing_support import *


class Snapshot:
    def __init__(self, values):
        self.values = values

    def get_account(self, key):
        value = self.values.get(str(key))
        if value is None:
            return None
        return SimpleNamespace(owner=Pubkey.from_string(value["owner"]), lamports=value["lamports"],
            executable=value["executable"], data=base64.b64decode(value["data"]))


def accounts_of_type(idl, snapshot, account_type):
    selected = {}
    discriminator = bytes(idl.accounts[account_type]["discriminator"])
    for address, raw in snapshot.values.items():
        if raw is None or raw["owner"] != str(idl.program):
            continue
        if base64.b64decode(raw["data"])[:8] == discriminator:
            selected[address] = idl.read(snapshot, Pubkey.from_string(address), account_type)
    return selected


def token_amounts(snapshot):
    result = {}
    for address, raw in snapshot.values.items():
        if raw and raw["owner"] == str(TOKEN):
            data = base64.b64decode(raw["data"])
            if len(data) == 165:
                result[address] = int.from_bytes(data[64:72], "little")
    return result


def token_effects(core, registry, before, after, receipt):
    expected = defaultdict(int)
    kind, amount, mint = receipt["kind"], receipt["amount"], receipt["mint"]
    def move(source, target, number):
        if number:
            assert source != target
            expected[str(source)] -= number
            expected[str(target)] += number
    def ata(owner):
        return Pubkey.find_program_address([bytes(owner), bytes(TOKEN), bytes(mint)], ATA)[0]
    def custody(key, snapshot=after):
        return core.read(snapshot, key, "FinancierCustody")
    if kind in (10, 11, 12, 14, 21):
        pass
    elif kind == 13:
        claim = core.read(after, receipt["claim"], "FundedClaim")
        supplier = ata(claim["terms"]["supplier"])
        move(receipt["debit_from"], supplier, receipt["supplier_net"])
        move(receipt["debit_from"], claim["seller_fee_terms"]["destination"], receipt["seller_fee"])
        move(receipt["sponsor_from"], receipt["sponsor_destination"],
            receipt["sponsor_reimbursement"] + receipt["sponsor_margin"])
        assert receipt["supplier_net"] + receipt["seller_fee"] == amount
    elif kind == 15:
        principal = registry.read(before, receipt["principal"], "Principal")
        move(receipt["debit_from"], ata(principal["owner"]), amount)
    elif kind in (16, 18):
        if kind == 16:
            terms = core.read(before, receipt["run"], "FundedRun")["interest_terms"]
        else:
            terms = core.read(before, receipt["claim"], "FundingFacility")["interest_terms"]
        assert receipt["principal_paid"] + receipt["interest_paid"] == amount
        move(receipt["debit_from"], receipt["creditor_destination"], amount - receipt["protocol_interest"])
        move(receipt["debit_from"], terms["protocol_destination"], receipt["protocol_interest"])
    elif kind == 17:
        old_run = core.read(before, receipt["run"], "FundedRun")
        old_custody = custody(old_run["creditor_custody"], before)
        move(receipt["debit_from"], old_custody["token_account"], receipt["assignment_consideration"])
        move(receipt["debit_from"], receipt["assignment_cost_destination"], receipt["assignment_acquisition_cost"])
        new_run = core.read(after, receipt["run"], "FundedRun")
        for field in ("supplier", "supplier_principal", "debtor", "mint", "gross_earned", "gross_paid", "principal", "interest_terms"):
            assert new_run[field] == old_run[field], ("assignment changed accepted entitlement", field)
        assert new_run["current_creditor"] == receipt["current_creditor"]
        assert new_run["ownership_revision"] == old_run["ownership_revision"] + 1 == receipt["ownership_revision"]
    elif kind == 19:
        claim = core.read(before, receipt["claim"], "FundedClaim")
        move(receipt["debit_from"], ata(claim["terms"]["supplier"]), receipt["seller_fee_credit"])
        assert receipt["seller_fee_credit"] == amount
    elif kind == 20:
        target = custody(receipt["capital_custody"])["token_account"]
        move(receipt["capital_from"], target, receipt["capital_token_inflow"])
        assert receipt["capital_token_inflow"] == receipt["capital_allocated"] == amount
    else:
        raise AssertionError(("Unknown receipt financial kind", kind))
    old, new = token_amounts(before), token_amounts(after)
    actual = {key: new.get(key, 0) - old.get(key, 0) for key in set(old) | set(new)
        if new.get(key, 0) != old.get(key, 0)}
    expected = {key: value for key, value in expected.items() if value}
    assert actual == expected, ("receipt token legs differ from observed account deltas", kind, actual, expected)
    assert sum(actual.values()) == 0, "Financial operation changed token supply"


def verify(trace_path, idl_directory, *, core=None, token_validator=token_effects):
    raw = lzma.decompress(trace_path.read_bytes()) if trace_path.suffix == ".xz" else trace_path.read_bytes()
    trace = json.loads(raw)
    assert trace["schema"] == "dexter-financing-reconstruction-native/v1"
    assert trace["status"] == "passed" and trace["reconstructed"] and trace["public_transactions_sent"] == 0
    core = core or Idl(idl_directory / "claims_core.json")
    registry = Idl(idl_directory / "principal_registry.json")
    immutable_receipts, previous = {}, {}
    financial, refusals, signed_packets = 0, 0, 0
    for row in trace["transactions"]:
        assert row["passed"]
        packet = base64.b64decode(row["transaction_base64"])
        tx = VersionedTransaction.from_bytes(packet)
        assert isinstance(tx.message, MessageV1) and len(packet) == row["wire_bytes"] <= 4096
        assert all(tx.verify_with_results()), (row["name"], "invalid native transaction signature")
        instructions = [Instruction(Pubkey.from_string(ix["program_id"]), bytes.fromhex(ix["data_hex"]),
            [M(Pubkey.from_string(m["pubkey"]), m["is_signer"], m["is_writable"]) for m in ix["ordered_metas"]])
            for ix in row["instructions"]]
        message = MessageV1.try_compile(tx.message.account_keys[0], instructions, tx.message.recent_blockhash, tx.message.config)
        assert bytes(VersionedTransaction.populate(message, tx.signatures)) == packet, "Recorded instruction metas differ from signed packet"
        assert json.loads(tx.message.config.to_json()) == row["config"]
        signed_packets += 1
        for address in set(previous) & set(row["observed_before"]):
            assert previous[address] == row["observed_before"][address], ("unrecorded protected state change", row["name"], address)
        before, after = Snapshot(row["observed_before"]), Snapshot(row["observed_after"])
        previous = row["observed_after"]
        rejected = "err" in row["result"]
        assert rejected == (row["expected"] == "reject"), (row["name"], "result status")
        if rejected:
            assert row["rollback_accounts"], "No protected state was checked on refusal"
            for address in row["rollback_accounts"]:
                assert row["observed_before"][address] == row["observed_after"][address], (row["name"], "partial rejected write")
            refusals += 1
            continue
        before_receipts = accounts_of_type(core, before, "FinanceReceipt")
        after_receipts = accounts_of_type(core, after, "FinanceReceipt")
        new_receipts = []
        for address, value in after_receipts.items():
            value = value["data"]
            if value["version"] == 0:
                continue
            encoded = after.get_account(address).data
            if address in immutable_receipts:
                assert encoded == immutable_receipts[address], "Historical receipt was rewritten"
            else:
                assert address not in before_receipts or before_receipts[address]["data"]["version"] == 0
                immutable_receipts[address] = encoded
                new_receipts.append((address, check_receipt(core, after, Pubkey.from_string(address))))
        if not new_receipts:
            if set(before_receipts) & {str(k) for k in tx.message.account_keys}:
                assert token_amounts(before) == token_amounts(after), "Receipt recovery or projection moved money"
                assert accounts_of_type(core, before, "FinanceBook") == accounts_of_type(core, after, "FinanceBook"), \
                    "Receipt recovery or projection changed an authoritative financial book"
            continue
        assert len(new_receipts) == 1, "Suite expects one financial operation per transaction"
        address, receipt = new_receipts[0]
        before_books = accounts_of_type(core, before, "FinanceBook")
        after_books = accounts_of_type(core, after, "FinanceBook")
        changed_books = {key for key, value in after_books.items()
            if before_books.get(key) != value and value["data"]["sequence"] != 0}
        expected_books = {str(effect["book"]) for effect in receipt["effects"]}
        assert changed_books == expected_books, "Receipt omitted or invented an atomic book change"
        for effect in receipt["effects"]:
            old, new = before_books[str(effect["book"])]["data"], after_books[str(effect["book"])]["data"]
            assert old["positions"] == effect["before"] and new["positions"] == effect["after"]
            assert old["head"] == effect["previous_hash"] and new["head"] == effect["hash"]
            assert old["sequence"] + 1 == effect["sequence"] == new["sequence"]
        token_validator(core, registry, before, after, receipt)
        financial += 1
    assert signed_packets == len(trace["transactions"])
    assert financial == len(trace["receipts"])
    for assertion in trace["assertions"]:
        assert assertion["passed"] and assertion["actual"] == assertion["expected"]
    return dict(status="passed", trace_sha256=hashlib.sha256(raw).hexdigest(),
        signed_packets=signed_packets, verified_financial_receipts=financial,
        refusals_with_rollback=refusals, recorded_state_assertions=len(trace["assertions"]),
        scope="Offline signature/encoding/receipt/journal/token-delta checks over recorded local evidence; no new SVM execution")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trace", type=Path)
    parser.add_argument("--idl-directory", type=Path, default=OUT / "idl")
    args = parser.parse_args()
    print(json.dumps(verify(args.trace, args.idl_directory), indent=2))


if __name__ == "__main__":
    main()
