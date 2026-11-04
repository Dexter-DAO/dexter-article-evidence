#!/usr/bin/env python3
"""Offline verification of correction results and their exact recorded effects.

Signed native-v1 packets, account continuity, journal hashes and full book
coverage use the existing evidence verifier. This layer independently derives
the declared correction economics and authenticates immutable result sidecars,
per-original-receipt usage and exact token legs. It is not a public-validator
attestation or a replacement for executing the compiled programs.
"""
from collections import defaultdict
import copy

from verify_financing_evidence import *
sys.path.insert(0, str(ROOT / "foundation/money/model"))
from financed_corrections import apply as correction_oracle, record_repayment_refund


class VersionedCore(Idl):
    """Strict historical decoding for the two explicitly appended v2 accounts."""
    def __init__(self, current, previous=None):
        super().__init__(current)
        self.previous = Idl(previous) if previous else None

    def read(self, svm, address, account_name):
        account = svm.get_account(address)
        if self.previous and account and account_name in ("FundedClaim", "FundedRun") \
                and account.data[8:10] == b"\x01\x00":
            result = self.previous.read(svm, address, account_name)
            # This is only a decoding view. Actual migration must be proved by
            # the recorded signed instruction and the subsequent v2 bytes.
            result["corrections"] = dict(unpaid_reduced=0, collected_returned=0,
                principal_credited=0, prior_repayment_refunded=0)
            if account_name == "FundedClaim":
                result["principal_extinguished"] = 0
            return result
        return super().read(svm, address, account_name)


def result_check(core, after, address, account_type, seed, domain):
    value = core.read(after, address, account_type)
    assert value["version"] == 1
    expected, bump = Pubkey.find_program_address([seed, bytes(value["run"]), value["operation_id"]], CORE)
    assert address == expected and value["bump"] == bump
    zero = {**value, "result_hash": bytes(32)}
    assert value["result_hash"] == hashlib.sha256(domain + bytes(CORE)
        + core.encode({"defined": {"name": account_type}}, zero)).digest()
    return value


def exact_token_legs(before, after, transfers):
    expected = defaultdict(int)
    for source, destination, amount in transfers:
        if amount:
            assert source != destination
            expected[str(source)] -= amount
            expected[str(destination)] += amount
    old, new = token_amounts(before), token_amounts(after)
    actual = {key: new.get(key, 0)-old.get(key, 0) for key in set(old)|set(new)
        if new.get(key, 0) != old.get(key, 0)}
    assert actual == {key: value for key, value in expected.items() if value}, (actual, expected)
    assert sum(actual.values()) == 0


def exact_books(core, before, after, changes):
    old_books = accounts_of_type(core, before, "FinanceBook")
    new_books = accounts_of_type(core, after, "FinanceBook")
    for address, original in old_books.items():
        data = original["data"]
        desired = copy.deepcopy(data["positions"])
        for field, amount in changes.get(str(data["principal"]), {}).items():
            desired[field] += amount
        assert new_books[address]["data"]["positions"] == desired, ("independent complete book", address, desired)


def correction_token_effects(core, registry, before, after, receipt):
    if receipt["kind"] not in (22, 23):
        return token_effects(core, registry, before, after, receipt)
    run = core.read(before, receipt["run"], "FundedRun")
    claim = core.read(before, receipt["claim"], "FundedClaim")
    changes = {}
    def change(principal, field, amount):
        current = changes.setdefault(str(principal), {})
        current[field] = current.get(field, 0) + amount
    if receipt["kind"] == 22:
        result = result_check(core, after, receipt["related_receipt"], "CorrectionReceipt",
            b"correction-result", b"DEXTER:FUNDED-CORRECTION-RESULT:1")
        args = result["request"]
        corrections, principal, plan = correction_oracle(run["principal"], run["gross_paid"], run["corrections"],
            args["unpaid_reduction"], args["collected_return"], args["creditor_compensation"])
        assert result["plan"] == plan and result["principal_before"] == run["principal"]
        assert result["principal_after"] == principal and result["corrections_before"] == run["corrections"]
        assert result["corrections_after"] == corrections
        current = core.read(after, receipt["run"], "FundedRun")
        assert current["principal"] == principal and current["corrections"] == corrections
        for field in ("supplier", "supplier_principal", "debtor", "mint", "gross_earned", "gross_paid",
                "capture_receipt", "capture_digest", "units", "unit_price", "current_creditor",
                "creditor_custody", "ownership_revision", "acquisition_basis"):
            assert current[field] == run[field], ("correction rewrote original lineage", field)
        assert result["capture_receipt"] == run["capture_receipt"] and result["capture_digest"] == run["capture_digest"]
        assert result["acceptance_hash"] == claim["acceptance_hash"]
        assert result["supplier_principal"] == claim["supplier_principal"]
        assert result["frozen_supplier_wallet"] == claim["terms"]["supplier"]
        assert receipt["amount"] == plan["total"]
        accrued, waived, released = result["newly_accrued_interest"], args["interest_waiver"], result["unused_charge_authority_released"]
        # The native suite separately computes elapsed-time accrual from its
        # controlled clock. Offline evidence verifies that this exact result is
        # reflected throughout every book and the durable accrued state.
        assert current["interest"]["accrued"] == run["interest"]["accrued"] + accrued
        assert current["interest"]["waived"] == run["interest"]["waived"] + waived
        for field in ("collected", "protocol_collected", "protocol_waived"):
            assert current["interest"][field] == run["interest"][field]
        change(receipt["principal"], "customer_principal", -plan["principal_credit"])
        change(run["current_creditor"], "owned_principal", -plan["principal_credit"])
        change(receipt["principal"], "interest_owed", accrued-waived)
        change(run["current_creditor"], "interest_owned", accrued-waived)
        change(receipt["principal"], "credit_charge_exposure", -waived-released)
        if args["unpaid_reduction"]:
            custody = core.read(before, claim["custody"], "FinancierCustody")
            change(claim["issuer"], "unpaid_supplier", -args["unpaid_reduction"])
            change(claim["supplier_principal"], "supplier_receivable", -args["unpaid_reduction"])
            change(custody["principal"], "funding_reserved", -args["unpaid_reduction"])
        if plan["required_cash"]:
            change(result["funding_principal"], "cash_sent", plan["required_cash"])
        if plan["creditor_cash"]:
            change(run["current_creditor"], "cash_received", plan["creditor_cash"])
        if plan["prior_repayment_refund"]:
            original = core.read(before, result["repayment_receipt"], "FinanceReceipt")["data"]
            assert original["kind"] == 16 and original["run"] == receipt["run"]
            assert result["refund_principal"] == original["debit_principal"]
            assert result["refund_authority"] == original["debit_authority"]
            assert result["refund_destination"] == original["debit_from"]
            usage = pda(CORE, b"correction-repayment", result["repayment_receipt"])
            old = core.read(before, usage, "RepaymentRefundRecord")["usage"] if before.get_account(usage) else {}
            expected = record_repayment_refund(old, original, principal=plan["prior_repayment_refund"])
            assert core.read(after, usage, "RepaymentRefundRecord")["usage"] == expected
            change(result["refund_principal"], "cash_received", plan["prior_repayment_refund"])
        if args["collected_return"]:
            original = core.read(before, result["collection_receipt"], "FinanceReceipt")["data"]
            assert original["kind"] == 13 and original["run"] == receipt["run"]
            usage = pda(CORE, b"correction-collection", result["collection_receipt"])
            old = core.read(before, usage, "CollectionCorrectionRecord")["usage"]["collected_returned"] if before.get_account(usage) else 0
            used = core.read(after, usage, "CollectionCorrectionRecord")["usage"]["collected_returned"]
            assert used == old + args["collected_return"] <= original["amount"]
        exact_token_legs(before, after, [(result["funding_from"], result["creditor_destination"], plan["creditor_cash"]),
            (result["funding_from"], result["refund_destination"], plan["prior_repayment_refund"])])
    else:
        result = result_check(core, after, receipt["related_receipt"], "InterestRefundReceipt",
            b"interest-refund-result", b"DEXTER:FUNDED-INTEREST-REFUND-RESULT:1")
        args = result["request"]
        original = core.read(before, result["original_repayment"], "FinanceReceipt")["data"]
        assert original["kind"] == 16 and original["run"] == receipt["run"]
        assert result["historical_lender"] == original["current_creditor"]
        assert result["protocol_beneficiary"] == run["interest_terms"]["protocol_beneficiary"]
        assert result["refund_destination"] == original["debit_from"]
        assert result["refund_principal"] == original["debit_principal"]
        assert result["original_effects_hash"] == original["effects_hash"]
        old = core.read(before, result["repayment_usage"], "RepaymentRefundRecord")["usage"] \
            if before.get_account(result["repayment_usage"]) else dict(principal_refunded=0,lender_interest_refunded=0,protocol_interest_refunded=0)
        usage = record_repayment_refund(old, original, lender_interest=args["lender_interest_refund"],
            protocol_interest=args["protocol_interest_refund"])
        assert result["usage_before"] == old and result["usage_after"] == usage
        assert core.read(after, result["repayment_usage"], "RepaymentRefundRecord")["usage"] == usage
        assert core.read(after, receipt["run"], "FundedRun") == run
        assert core.read(after, receipt["claim"], "FundedClaim") == claim
        total = args["lender_interest_refund"]+args["protocol_interest_refund"]
        assert receipt["amount"] == total
        change(result["refund_principal"], "cash_received", total)
        if args["lender_interest_refund"]:
            change(result["historical_lender"], "cash_sent", args["lender_interest_refund"])
        if args["protocol_interest_refund"]:
            change(result["protocol_beneficiary"], "cash_sent", args["protocol_interest_refund"])
        exact_token_legs(before, after, [(result["lender_from"], result["refund_destination"], args["lender_interest_refund"]),
            (result["protocol_from"], result["refund_destination"], args["protocol_interest_refund"])])
    assert result["financial_receipt"] == pda(CORE,b"finance-operation",receipt["principal"],receipt["operation_id"])
    assert result["financial_effect_hash"] == receipt["effects_hash"] and result["request_digest"] == receipt["request_digest"]
    assert result["operation_id"] == receipt["operation_id"]
    exact_books(core, before, after, changes)


def verify_corrections(trace_path, idl_directory, baseline_idl_directory=None):
    core = VersionedCore(idl_directory/"claims_core.json",
        baseline_idl_directory/"claims_core.json" if baseline_idl_directory else None)
    result = verify(trace_path, idl_directory, core=core, token_validator=correction_token_effects)
    raw = lzma.decompress(trace_path.read_bytes()) if trace_path.suffix==".xz" else trace_path.read_bytes()
    trace = json.loads(raw)
    immutable = {}
    for row in trace["transactions"]:
        snapshot = Snapshot(row["observed_after"])
        for kind in ("CorrectionReceipt", "InterestRefundReceipt"):
            for address, value in accounts_of_type(core,snapshot,kind).items():
                if value["version"] == 0:
                    continue
                current = snapshot.get_account(Pubkey.from_string(address)).data
                assert address not in immutable or immutable[address] == current, "Durable correction result was changed later"
                immutable[address] = current
    result["immutable_correction_results"] = len(immutable)
    result["independent_economic_oracle"] = "foundation/money/model/financed_corrections.py"
    return result


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trace",type=Path)
    parser.add_argument("--idl-directory",type=Path,default=OUT/"idl")
    parser.add_argument("--baseline-idl-directory",type=Path)
    args=parser.parse_args()
    print(json.dumps(verify_corrections(args.trace,args.idl_directory,args.baseline_idl_directory),indent=2))


if __name__ == "__main__":
    main()
