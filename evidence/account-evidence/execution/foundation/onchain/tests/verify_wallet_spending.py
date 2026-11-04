#!/usr/bin/env python3
"""Verify signed local wallet traces, separate asset legs and actual deltas.

This offline checker authenticates recorded native-v1 packets, immutable
operation/financial receipts, complete mint-specific book changes, spending
consumption, canonical reserves and SPL pool/fee effects. It does not attest
validator admission, public-chain execution or customer SDK/RPC recovery.
"""
from __future__ import annotations
import argparse
import base64
from collections import defaultdict
import json
import lzma
from pathlib import Path

from financing_support import *
from verify_financing_evidence import Snapshot, accounts_of_type, token_amounts, token_effects
from wallet_spending import check_spending_receipt
from spl_token_swap_fixture import SPL_SWAP, constant_product_quote


def token_deltas(before, after):
    old, new = token_amounts(before), token_amounts(after)
    return {address: new.get(address, 0) - old.get(address, 0)
        for address in set(old) | set(new) if new.get(address, 0) != old.get(address, 0)}


def token_mint(snapshot, address):
    account = snapshot.get_account(address)
    assert account is not None and account.owner == TOKEN and len(account.data) == 165
    return Pubkey.from_bytes(account.data[:32])


def mint_supply(snapshot, address):
    account = snapshot.get_account(address)
    assert account is not None and account.owner == TOKEN and len(account.data) == 82
    return int.from_bytes(account.data[36:44], "little")


def decode_instruction(idl, name, data):
    definition = idl.instructions[name]
    assert data[:8] == bytes(definition["discriminator"])
    offset, decoded = 8, {}
    for field in definition["args"]:
        decoded[field["name"]], offset = idl.decode(field["type"], data, offset)
    assert offset == len(data), "Noncanonical instruction bytes"
    return decoded


def bind_operation_to_signed_request(core, registry, before, instructions, payer, address, receipt):
    """Reconstruct the request from actual signed SignV2 bytes, not trace prose."""
    assert len(instructions) == 2 and instructions[0].program_id == REGISTRY and instructions[1].program_id == SWIG
    authorize, swig = instructions
    raw = bytes(swig.data)
    assert len(raw) >= 16
    selector, compact_len, role = struct.unpack_from("<HHI", raw)
    assert selector == 11 and role == 0 and len(raw) == 8 + compact_len + 1
    assert raw[-1] == 2 and raw[8] == 1
    program_index, count = raw[9], raw[10]
    assert swig.accounts[program_index].pubkey == CORE
    indices = raw[11:11 + count]
    assert len(indices) == count and all(index < len(swig.accounts) for index in indices)
    data_len = struct.unpack_from("<H", raw, 11 + count)[0]
    data = raw[13 + count:13 + count + data_len]
    assert len(data) == data_len and 13 + count + data_len == len(raw) - 1
    name = "wallet_send" if receipt["request"]["kind"] == 5 else "wallet_swap_exact_input"
    args = decode_instruction(core, name, data)["args"]
    names = [field["name"] for field in core.instructions[name]["accounts"]]
    assert len(indices) == len(names) + (2 if name == "wallet_send" else 4)
    selected = [swig.accounts[index] for index in indices]
    accounts = {name: selected[index].pubkey for index, name in enumerate(names)}
    swap = name == "wallet_swap_exact_input"
    request = dict(request_id=args["request_id"], kind=6 if swap else 5,
        principal=accounts["principal"], binding=accounts["binding"],
        wallet_authority=accounts["wallet_authority"], source_custody=accounts["source_custody"],
        source=accounts["source"], input_mint=accounts["input_mint"], destination=accounts["destination"],
        destination_authority=accounts["wallet_authority"] if swap else args["destination_authority"],
        output_mint=accounts["output_mint"] if swap else accounts["input_mint"],
        output_custody=accounts["output_custody"] if swap else Pubkey.default(),
        adapter_program=accounts["swap_program"] if swap else Pubkey.default(),
        adapter_pool=accounts["swap_state"] if swap else Pubkey.default(),
        amount=args["amount"], minimum_output=args["minimum_output"] if swap else 0,
        authority_generation=args["authority_generation"])
    assert receipt["request"] == request, "Receipt describes a different request from the signed Core invocation"
    assert accounts["operation"] == Pubkey.from_string(address)
    assert accounts["permission"] == receipt["permission"] and accounts["executor"] == receipt["executor"]
    assert accounts["input_receipt"] == receipt["input_receipt"]
    if swap:
        assert accounts["output_receipt"] == receipt["output_receipt"]
    books = []
    for mint in ([request["input_mint"], request["output_mint"]] if swap else [request["input_mint"]]):
        book = pda(CORE, b"financial-book", request["principal"], mint)
        books.extend([book, pda(POOL, b"financial-projection", book)])
    assert [meta.pubkey for meta in selected[len(names):]] == books

    envelope = decode_instruction(registry, "authorize_funding_wallet", bytes(authorize.data))["args"]
    fields = [field["name"] for field in registry.instructions["authorize_funding_wallet"]["accounts"]]
    assert len(authorize.accounts) == len(fields) + 2
    root = {name: authorize.accounts[index].pubkey for index, name in enumerate(fields)}
    assert root["principal"] == request["principal"] and root["binding"] == request["binding"]
    assert root["caller"] == receipt["executor"] and root["wallet"] == request["wallet_authority"]
    assert swig.accounts[0].pubkey == root["config"] and swig.accounts[1].pubkey == root["wallet"]
    assert swig.accounts[2].pubkey == root["instructions"] == SYSVAR_INSTRUCTIONS
    assert [meta.pubkey for meta in authorize.accounts[-2:]] == [receipt["permission"], Pubkey.from_string(address)]
    assert envelope["binding"] == request["binding"] and envelope["action"] == request["kind"]
    assert envelope["authority_generation"] == request["authority_generation"]
    assert envelope["nonce"] == registry.read(before, request["binding"], "FundingWalletBinding")["nonce"]
    principal = registry.read(before, request["principal"], "Principal")
    flags = {str(payer): [True, True]}
    for instruction in instructions:
        for meta in instruction.accounts:
            flag = flags.setdefault(str(meta.pubkey), [False, False])
            flag[0] |= meta.is_signer
            flag[1] |= meta.is_writable
    encoded = struct.pack("<H", len(swig.accounts))
    for meta in swig.accounts:
        signer, writable = flags[str(meta.pubkey)]
        encoded += bytes([int(signer) | int(writable) << 1]) + bytes(meta.pubkey)
    encoded += bytes(SWIG) + struct.pack("<H", len(raw)) + raw
    assert envelope["bundle_hash"] == hashlib.sha256(b"DEXTER:FUNDING-SWIG-BUNDLE:1"
        + bytes(REGISTRY) + bytes(CORE) + bytes(SWIG) + principal["domain"]
        + bytes(request["principal"]) + bytes(request["binding"]) + u64(envelope["nonce"])
        + struct.pack("<q", envelope["expires_at"]) + u64(request["authority_generation"])
        + bytes([request["kind"]]) + encoded).digest()


def verify_spending_effects(core, registry, before, after, receipt):
    request = receipt["request"]
    source, destination = request["source"], request["destination"]
    amount, output = request["amount"], receipt["amount_out"]
    expected = defaultdict(int)
    expected[str(source)] -= amount
    expected[str(destination)] += output
    assert token_mint(after, source) == request["input_mint"]
    assert token_mint(after, destination) == request["output_mint"]
    for address, owner in ((source, request["wallet_authority"]), (destination, request["destination_authority"])):
        assert Pubkey.from_bytes(after.get_account(address).data[32:64]) == owner
    if request["kind"] == 6:
        assert request["adapter_program"] == SPL_SWAP
        pool = before.get_account(request["adapter_pool"])
        assert pool.owner == SPL_SWAP and len(pool.data) == 324 and pool.data[:2] == bytes([1, 1])
        assert after.get_account(request["adapter_pool"]).data == pool.data
        read_key = lambda offset: Pubkey.from_bytes(pool.data[offset:offset + 32])
        assert read_key(3) == TOKEN and pool.data[291] == 0
        if read_key(131) == request["input_mint"]:
            pool_source, pool_output = read_key(35), read_key(67)
            assert read_key(163) == request["output_mint"]
        else:
            pool_source, pool_output = read_key(67), read_key(35)
            assert read_key(163) == request["input_mint"] and read_key(131) == request["output_mint"]
        expected[str(pool_source)] += amount
        expected[str(pool_output)] -= output
        pool_mint, fee_account = read_key(99), read_key(195)
        old_tokens, new_tokens = token_amounts(before), token_amounts(after)
        fee_lp = new_tokens[str(fee_account)] - old_tokens[str(fee_account)]
        assert fee_lp >= 0 and token_mint(after, fee_account) == pool_mint
        assert mint_supply(after, pool_mint) - mint_supply(before, pool_mint) == fee_lp
        expected[str(fee_account)] += fee_lp
        fees = tuple(int.from_bytes(pool.data[offset:offset + 8], "little") for offset in range(227, 291, 8))
        quote = constant_product_quote(amount, old_tokens[str(pool_source)], old_tokens[str(pool_output)], fees)
        assert quote and quote["actual_input"] == amount and quote["output"] == output
        assert output >= request["minimum_output"] > 0
        # Input and output mints conserve independently; LP fee issuance has its
        # own supply delta. Adding unlike token atoms would prove nothing.
        mint_totals = defaultdict(int)
        for address, delta in token_deltas(before, after).items():
            mint_totals[token_mint(after, Pubkey.from_string(address))] += delta
        assert mint_totals[request["input_mint"]] == mint_totals[request["output_mint"]] == 0
        assert mint_totals[pool_mint] == fee_lp
    else:
        assert request["kind"] == 5 and output == amount and request["minimum_output"] == 0
    assert token_deltas(before, after) == {address: delta for address, delta in expected.items() if delta}, \
        "Observed token movements differ from the exact authorized wallet effects"

    old_custodies = accounts_of_type(core, before, "FinancierCustody")
    new_custodies = accounts_of_type(core, after, "FinancierCustody")
    affected = {str(request["source_custody"])}
    if request["kind"] == 6:
        affected.add(str(request["output_custody"]))
    assert {key for key in old_custodies if old_custodies[key] != new_custodies[key]} == affected
    for key in affected:
        old, new = old_custodies[key], new_custodies[key]
        assert old["principal"] == new["principal"] == request["principal"]
        assert old["binding"] == new["binding"] == request["binding"]
        assert old["wallet"] == new["wallet"] == request["wallet_authority"]
        assert pda(CORE, b"custody-reserve", new["token_account"]) == Pubkey.from_string(key)
        assert new["revision"] == old["revision"] + 1
        for field in ("unused", "payable", "contributed_capital"):
            assert new[field] == old[field]
        assert token_amounts(after)[str(new["token_account"])] >= new["accounted_cash"] >= new["unused"] + new["payable"]
        if key == str(request["source_custody"]):
            assert old["accounted_cash"] - new["accounted_cash"] == amount
            assert new["withdrawn_cash"] - old["withdrawn_cash"] == amount
        else:
            assert new["accounted_cash"] - old["accounted_cash"] == output
            assert new["withdrawn_cash"] == old["withdrawn_cash"]

    permission = receipt["permission"]
    old = registry.read(before, permission, "SpendingPermission")
    new = registry.read(after, permission, "SpendingPermission")
    terms, used, consumed = old["terms"], old["consumed"], new["consumed"]
    principal = registry.read(before, request["principal"], "Principal")
    assert old["active"] and old["grant_owner"] == principal["owner"]
    assert old["principal"] == request["principal"]
    assert pda(REGISTRY, b"spending-permission", request["principal"], old["permission_id"]) == permission
    raw = registry.encode({"defined": {"name": "SpendingPermissionTerms"}}, terms)
    assert old["terms_digest"] == hashlib.sha256(b"DEXTER:SPENDING-PERMISSION:1" + bytes(REGISTRY)
        + bytes(request["principal"]) + old["permission_id"] + bytes(old["grant_owner"]) + raw).digest()
    assert {key: value for key, value in old.items() if key != "consumed"} == \
        {key: value for key, value in new.items() if key != "consumed"}
    assert terms["authority_generation"] == request["authority_generation"] == principal["authority_generation"]
    mapping = dict(binding="binding", kind="kind", mint="input_mint", debit_from="source",
        output_mint="output_mint", destination="destination", destination_authority="destination_authority",
        adapter_program="adapter_program", adapter_pool="adapter_pool")
    assert all(terms[term] == request[field] for term, field in mapping.items())
    assert terms["executor"] == receipt["executor"]
    assert 0 < amount <= terms["per_operation_amount"]
    assert consumed["amount"] == used["amount"] + amount <= terms["total_amount"]
    assert consumed["operations"] == used["operations"] + 1 <= terms["maximum_operations"]
    assert terms["not_before"] <= consumed["last_executed_at"] < terms["expires_at"]
    if used["operations"]:
        assert consumed["last_executed_at"] - used["last_executed_at"] >= terms["minimum_interval"]
    if terms["exact_request_digest"] != bytes(32):
        assert terms["maximum_operations"] == 1
        assert terms["exact_request_digest"] == receipt["request_digest"]
        assert terms["exact_receipt"] == pda(CORE, b"wallet-operation", request["principal"],
            hashlib.sha256(b"DEXTER:WALLET-SPEND-ID:1" + bytes(CORE) + bytes(request["principal"]) + request["request_id"]).digest())
        assert terms["exact_amount"] == amount
    if request["kind"] == 6:
        denominator = terms["minimum_output_denominator"]
        assert denominator > 0
        floor = (amount * terms["minimum_output_numerator"] + denominator - 1) // denominator
        assert output >= request["minimum_output"] >= floor
    old_binding = registry.read(before, request["binding"], "FundingWalletBinding")
    new_binding = registry.read(after, request["binding"], "FundingWalletBinding")
    assert new_binding["nonce"] == old_binding["nonce"] + 1


def verify(trace_path, idl_directory):
    raw = lzma.decompress(trace_path.read_bytes()) if trace_path.suffix == ".xz" else trace_path.read_bytes()
    trace = json.loads(raw)
    assert trace["schema"] == "dexter-financing-reconstruction-native/v1"
    assert trace["status"] == "passed" and trace["reconstructed"] and trace["public_transactions_sent"] == 0
    core, registry = Idl(idl_directory / "claims_core.json"), Idl(idl_directory / "principal_registry.json")
    immutable_finance, immutable_spending, previous = {}, {}, {}
    financial, spending, swaps, refusals, packets = 0, 0, 0, 0, 0
    for row in trace["transactions"]:
        assert row["passed"]
        packet = base64.b64decode(row["transaction_base64"])
        tx = VersionedTransaction.from_bytes(packet)
        assert isinstance(tx.message, MessageV1) and len(packet) == row["wire_bytes"] <= 4096
        assert all(tx.verify_with_results()), (row["name"], "invalid signed native-v1 message")
        instructions = [Instruction(Pubkey.from_string(ix["program_id"]), bytes.fromhex(ix["data_hex"]),
            [M(Pubkey.from_string(m["pubkey"]), m["is_signer"], m["is_writable"]) for m in ix["ordered_metas"]])
            for ix in row["instructions"]]
        message = MessageV1.try_compile(tx.message.account_keys[0], instructions, tx.message.recent_blockhash, tx.message.config)
        assert bytes(VersionedTransaction.populate(message, tx.signatures)) == packet
        assert json.loads(tx.message.config.to_json()) == row["config"]
        packets += 1
        for address in set(previous) & set(row["observed_before"]):
            assert previous[address] == row["observed_before"][address], (row["name"], "unrecorded protected state change", address)
        before, after = Snapshot(row["observed_before"]), Snapshot(row["observed_after"])
        previous = row["observed_after"]
        rejected = "err" in row["result"]
        assert rejected == (row["expected"] == "reject")
        if rejected:
            assert row["rollback_accounts"]
            assert set(row["rollback_accounts"]) == set(row["observed_before"]) == set(row["observed_after"]), \
                "Refusal omitted a protected account from rollback checking"
            assert all(row["observed_before"][address] == row["observed_after"][address] for address in row["rollback_accounts"])
            refusals += 1
            continue
        new_receipts, new_operations = [], []
        for account_type, immutable, created in (("FinanceReceipt", immutable_finance, new_receipts),
                ("WalletSpendReceipt", immutable_spending, new_operations)):
            old_values = accounts_of_type(core, before, account_type)
            for address, wrapped in accounts_of_type(core, after, account_type).items():
                value = wrapped["data"]
                if value["version"] == 0:
                    continue
                encoded = after.get_account(address).data
                if address in immutable:
                    assert immutable[address] == encoded, "Historical receipt was rewritten"
                else:
                    assert address not in old_values or old_values[address]["data"]["version"] == 0
                    immutable[address] = encoded
                    checked = check_receipt(core, after, Pubkey.from_string(address)) if account_type == "FinanceReceipt" \
                        else check_spending_receipt(core, after, Pubkey.from_string(address))
                    created.append((address, checked))
        if not new_receipts:
            assert not new_operations
            receipt_keys = set(immutable_finance) | set(immutable_spending)
            if receipt_keys & {str(key) for key in tx.message.account_keys}:
                assert token_amounts(before) == token_amounts(after), "Historical recovery or catch-up moved money"
                for account_type in ("FinanceBook", "FinancierCustody"):
                    assert accounts_of_type(core, before, account_type) == accounts_of_type(core, after, account_type)
            continue
        before_books, after_books = accounts_of_type(core, before, "FinanceBook"), accounts_of_type(core, after, "FinanceBook")
        changed = {address for address, value in after_books.items()
            if before_books.get(address) != value and value["data"]["sequence"] != 0}
        effects = [effect for _, receipt in new_receipts for effect in receipt["effects"]]
        assert len({str(effect["book"]) for effect in effects}) == len(effects), "Duplicated book effects in one composition"
        assert changed == {str(effect["book"]) for effect in effects}, "Incomplete or invented atomic book effects"
        for effect in effects:
            old, new = before_books[str(effect["book"])]["data"], after_books[str(effect["book"])]["data"]
            assert old["positions"] == effect["before"] and new["positions"] == effect["after"]
            assert old["head"] == effect["previous_hash"] and new["head"] == effect["hash"]
            assert old["sequence"] + 1 == effect["sequence"] == new["sequence"]
        if new_operations:
            assert len(new_operations) == 1
            address, operation = new_operations[0]
            bind_operation_to_signed_request(core, registry, before, instructions, tx.message.account_keys[0], address, operation)
            request = operation["request"]
            legs = {str(operation["input_receipt"])}
            if request["kind"] == 6:
                legs.add(str(operation["output_receipt"]))
                swaps += 1
            assert {key for key, _ in new_receipts} == legs
            for key, receipt in new_receipts:
                assert len(receipt["effects"]) == 1
                effect = receipt["effects"][0]
                assert effect["principal"] == request["principal"] and receipt["related_receipt"] == Pubkey.from_string(address)
                field = "cash_received" if receipt["kind"] == 26 else "cash_sent"
                assert {name: effect["after"][name] - value for name, value in effect["before"].items()
                    if effect["after"][name] != value} == {field: receipt["amount"]}
            verify_spending_effects(core, registry, before, after, operation)
            spending += 1
        else:
            assert len(new_receipts) == 1, "Non-wallet fixture composition expects one immutable financial receipt"
            token_effects(core, registry, before, after, new_receipts[0][1])
        financial += len(new_receipts)
    assert packets == len(trace["transactions"])
    assert financial == len(trace["receipts"]) == len(immutable_finance)
    assert spending == len(trace["spending_receipts"]) == len(immutable_spending)
    assert all(assertion["passed"] and assertion["actual"] == assertion["expected"] for assertion in trace["assertions"])
    return dict(status="passed", trace_sha256=hashlib.sha256(raw).hexdigest(), signed_packets=packets,
        verified_financial_receipts=financial, verified_spending_operations=spending,
        verified_actual_adapter_swaps=swaps, refusals_with_rollback=refusals,
        recorded_state_assertions=len(trace["assertions"]),
        scope="Offline native-v1 signatures, immutable receipts, separate-asset books, reserve/permission and actual adapter token deltas; no new SVM or RPC execution")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trace", type=Path)
    parser.add_argument("--idl-directory", type=Path, default=OUT / "idl")
    args = parser.parse_args()
    print(json.dumps(verify(args.trace, args.idl_directory), indent=2))


if __name__ == "__main__":
    main()
