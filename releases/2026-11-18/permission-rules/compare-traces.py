"""Compare every trace of main's build (old, 865bd86) with the permission-rules build (new).

Usage: <python> compare-traces.py OLD_TRACES NEW_TRACES OLD_IDL_DIR NEW_IDL_DIR TESTS_DIR

OLD_TRACES and NEW_TRACES hold the `*-trace.json` files that every
`./dev anchor-smoke` suite wrote under `.local/anchor/` on each build; the IDL
directories hold each build's `principal_registry.json` and `claims_core.json`;
TESTS_DIR is this checkout's `foundation/onchain/tests`, for its IDL codec.

Every difference must match one named class below, each checked exactly; anything
else is printed as UNEXPLAINED. Outcomes (transaction name, expectation, pass,
error), token amounts and every non-permission account must be identical.
A passkey proof that differs is decoded in each run (proofs_explained): it may
differ only because the instruction it approves changed, which the script
recomputes from that run's transaction.
"""
import base64, collections, hashlib, json, re, struct, sys
from pathlib import Path

old_dir, new_dir, old_idl_dir, new_idl_dir, tests = map(Path, sys.argv[1:6])
sys.path.insert(0, str(tests))
sys.path.insert(0, str(tests.parents[2] / "money"))
from financing_support import Idl  # noqa: E402
from solders.pubkey import Pubkey  # noqa: E402

REGISTRY = "9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G"
CORE = "727zmMkqPEPHaBDbxY4iAhzt6uZxxAd8CFoPUjdGXPPz"
SECP256R1 = "Secp256r1SigVerify1111111111111111111111111"
DEFAULT = "11111111111111111111111111111111"
old_reg, new_reg = Idl(old_idl_dir / "principal_registry.json"), Idl(new_idl_dir / "principal_registry.json")
old_core, new_core = Idl(old_idl_dir / "claims_core.json"), Idl(new_idl_dir / "claims_core.json")
MANDATE = bytes(old_reg.accounts["Mandate"]["discriminator"])
assert MANDATE == bytes(new_reg.accounts["Mandate"]["discriminator"])
RENT = lambda size: (size + 128) * 6960          # LiteSVM's rent-exempt minimum
OLD_SIZE, NEW_SIZE = 517, 1070
PER_PERMISSION = RENT(NEW_SIZE) - RENT(OLD_SIZE)
RENAMED = {"Instruction: CreatePinnedMandate": "Instruction: CreateMandate",
    "Instruction: PinMandateTerms": "Instruction: SetMandateRules"}
MESSAGES = {"This permission pins order terms; the order must show its terms": "This permission has term rules; the order must show its terms",
    "Order terms do not contain a term the owner pinned": "Order terms do not meet a rule the owner set"}
CU = re.compile(r"consumed \d+ of \d+ compute units")
LINE = re.compile(r"programs/principal-registry/src/(\w+\.rs):(\d+)")
# The one outcome this change moves: create_pinned_mandate is merged into
# create_mandate, so a permission created with no rules is the plain permission.
BEHAVIOUR = {("pinned-terms-created-pinned-trace.json", "refused: the buyer's owner creates a pinned permission with no terms"):
    "ACCEPTED: the buyer's owner creates a permission with no terms, which is the plain permission"}

PASSKEY = bytes(old_reg.accounts["PasskeyAuthority"]["discriminator"])
AUTHORIZE = bytes(old_reg.instructions["authorize_passkey_operation"]["discriminator"])
assert AUTHORIZE == bytes(new_reg.instructions["authorize_passkey_operation"]["discriminator"])
PROOF = "passkey proof of the changed instruction"

stats = collections.Counter()
moved, unexplained, notes, behaviour = collections.Counter(), [], collections.defaultdict(set), []
# Per trace: (main's approved-instruction digest, client data hash) -> this branch's, from decoded approvals.
approved = collections.defaultdict(dict)
payer_deltas = collections.Counter()
cu_deltas = []
assertion_diffs = []


def instruction_name(idl, data):
    for name, info in idl.instructions.items():
        if bytes(info["discriminator"]) == data[:8]:
            return name
    return None


def decode_args(idl, name, data):
    offset, out = 8, {}
    for arg in idl.instructions[name]["args"]:
        out[arg["name"]], offset = idl.decode(arg["type"], data, offset)
    assert offset == len(data), (name, offset, len(data))
    return out


def encode_args(idl, name, values):
    info = idl.instructions[name]
    return bytes(info["discriminator"]) + b"".join(idl.encode(a["type"], values[a["name"]]) for a in info["args"])


def pin_rule(item):
    return dict(name=item["name"], kind="List", value_count=1, values=[item["value"]] + [bytes(32)] * 3, min=0, max=0)


def mandate_of(states, address):
    """The old trace's permission record at this address: its seller."""
    for state in states:
        account = state.get(address)
        if account and base64.b64decode(account["data"])[:8] == MANDATE:
            return old_reg.decode({"defined": {"name": "Mandate"}}, base64.b64decode(account["data"]), 8)[0]
    return None


def same_instruction(x, y, row_old, row_new, trace):
    """Old and new data of one top-level instruction: identical, or one of the named encodings."""
    a, b = bytes.fromhex(x["data_hex"]), bytes.fromhex(y["data_hex"])
    if a == b:
        return "same"
    program = x["program_id"]
    if program == REGISTRY:
        old_name, new_name = instruction_name(old_reg, a), instruction_name(new_reg, b)
        old, new = decode_args(old_reg, old_name, a), decode_args(new_reg, new_name, b)
        if old_name in ("create_mandate", "create_pinned_mandate") and new_name == "create_mandate":
            args = dict(old["args"]); supplier = args.pop("supplier")
            pinned = old.get("required_terms", [])
            ok = new["args"] == args and new["rules"] == dict(sellers=[supplier], terms=[pin_rule(t) for t in pinned])
            return f"{old_name} -> create_mandate: the seller as a list of one, each pin a list rule of one value" if ok else None
        if old_name == "pin_mandate_terms" and new_name == "set_mandate_rules":
            mandate = mandate_of([row_old["observed_before"]], str(x["ordered_metas"][2]["pubkey"]))
            ok = mandate is not None and new == dict(expected_revision=old["expected_revision"],
                rules=dict(sellers=[mandate["supplier"]], terms=[pin_rule(t) for t in old["required_terms"]]))
            return "pin_mandate_terms -> set_mandate_rules: the permission's seller kept, each pin a list rule of one value" if ok else None
        if old_name == new_name and old_name in ("reserve_pinned", "reserve_pinned_billing"):
            old["order_terms"] = [dict(t, number=None) for t in old["order_terms"]]
            return f"{old_name}: each shown term carries no number" if encode_args(new_reg, new_name, old) == b else None
        if old_name == new_name == "authorize_passkey_operation":
            return PROOF          # decoded and checked with its whole transaction by proofs_explained
        return None
    if program == CORE:
        old_name, new_name = instruction_name(old_core, a), instruction_name(new_core, b)
        if old_name != new_name or "order_terms" not in [arg["name"] for arg in old_core.instructions[old_name]["args"]]:
            return None
        old = decode_args(old_core, old_name, a)
        if old["order_terms"] is not None:
            old["order_terms"] = [dict(t, number=None) for t in old["order_terms"]]
        return f"{old_name}: each shown term carries no number" if encode_args(new_core, new_name, old) == b else None
    if program == SECP256R1:
        return PROOF              # decoded and checked with its whole transaction by proofs_explained
    return None


def target_digest(row, index):
    """The Registry's passkey_target_digest of one top-level instruction, with each account's privileges as
    the transaction grants them: the fee payer signs and is writable, and every account takes the signer and
    writable flags it has in any instruction of the transaction."""
    ixs = row["instructions"]
    privileges = {row["account_keys"][0]: [True, True]}
    for ix in ixs:
        for meta in ix["ordered_metas"]:
            flags = privileges.setdefault(meta["pubkey"], [False, False])
            flags[0] |= meta["is_signer"]; flags[1] |= meta["is_writable"]
    ix = ixs[index]
    data, metas = bytes.fromhex(ix["data_hex"]), ix["ordered_metas"]
    raw = b"DEXTER:ACCOUNT-TARGET:1" + bytes(Pubkey.from_string(ix["program_id"])) + struct.pack("<H", len(metas))
    for meta in metas:
        signer, writable = privileges[meta["pubkey"]]
        raw += bytes(Pubkey.from_string(meta["pubkey"])) + bytes([int(signer) | int(writable) << 1])
    return hashlib.sha256(raw + struct.pack("<I", len(data)) + data).digest()


def r1_parts(ix):
    """A Secp256r1 proof in the one framing the Registry admits: signature, key and message inside the instruction."""
    data = bytes.fromhex(ix["data_hex"])
    if (ix["program_id"] != SECP256R1 or len(data) < 113 + 37 + 32 or data[:2] != b"\x01\x00"
            or struct.unpack_from("<7H", data, 2) != (16, 0xFFFF, 80, 0xFFFF, 113, len(data) - 113, 0xFFFF)):
        return None
    return dict(signature=data[16:80], key=data[80:113], authenticator=data[113:-32], client_hash=data[-32:])


def b64url(raw):
    return base64.urlsafe_b64encode(raw).rstrip(b"=").decode()


def approvals(row, reg):
    """Each passkey approval in one transaction, decoded and recomputed from the instruction it approves.

    The challenge in the client data must be the Registry's challenge over this run's
    approved-instruction digest, the proof's message must end with the hash of this
    run's client data, and the proof's key must be the passkey record's key.
    """
    out, ixs = [], row.get("instructions") or []
    for k, ix in enumerate(ixs):
        data = bytes.fromhex(ix["data_hex"])
        if ix["program_id"] != REGISTRY or data[:8] != AUTHORIZE:
            continue
        args = decode_args(reg, "authorize_passkey_operation", data)["args"]
        alias = ix["ordered_metas"][0]["pubkey"]
        record = (row.get("observed_before") or {}).get(alias)
        proof = r1_parts(ixs[k - 1]) if k > 0 else None
        if record is None or proof is None or not k < args["target_index"] < len(ixs):
            return None
        authority = reg.decode({"defined": {"name": "PasskeyAuthority"}}, base64.b64decode(record["data"]), 8)[0]
        digest = target_digest(row, args["target_index"])
        challenge = hashlib.sha256(b"DEXTER:ACCOUNT-AUTHORITY:1" + bytes(authority["genesis_hash"])
            + bytes(Pubkey.from_string(REGISTRY)) + bytes(authority["principal"]) + bytes(Pubkey.from_string(alias))
            + struct.pack("<QBQH", args["nonce"], args["role"], args["authority_generation"], args["target_index"])
            + digest + struct.pack("<q", args["expires_at"]) + bytes(args["ceremony_nonce"])).digest()
        client_json = bytes(args["client_data_json"])
        client = json.loads(client_json)
        if (client.get("challenge") != b64url(challenge) or proof["client_hash"] != hashlib.sha256(client_json).digest()
                or proof["key"] != bytes(authority["public_key"])):
            return None
        out.append(dict(index=k, target=args["target_index"], digest=digest, client_hash=proof["client_hash"],
            args={name: value for name, value in args.items() if name != "client_data_json"},
            client={name: value for name, value in client.items() if name != "challenge"},
            key=proof["key"], authenticator=proof["authenticator"]))
    return out


def proofs_explained(x, y, kinds, pairs):
    """Proofs and approvals that differ only because the instruction they approve changed in a named class."""
    p, q = approvals(x, old_reg), approvals(y, new_reg)
    if p is None or q is None or [(a["index"], a["target"]) for a in p] != [(b["index"], b["target"]) for b in q]:
        return False
    for a, b in zip(p, q):
        if kinds[a["target"]] in ("same", None, PROOF):
            return False
        if (a["args"], a["client"], a["key"], a["authenticator"]) != (b["args"], b["client"], b["key"], b["authenticator"]):
            return False
        pairs[(a["digest"], a["client_hash"])] = (b["digest"], b["client_hash"])
    covered = {a["index"] for a in p} | {a["index"] - 1 for a in p}
    if not all(i in covered for i, kind in enumerate(kinds) if kind == PROOF):
        return False
    stats["passkey approvals decoded: challenge, client data hash and key recomputed in each run"] += len(p)
    return True


def normalize(line):
    for before, after in RENAMED.items():
        if before in line:
            line = line.replace(before, after); stats["log lines: instruction renamed"] += 1
    for before, after in MESSAGES.items():
        if before in line:
            line = line.replace(before, after); stats["log lines: error message reworded"] += 1
    return CU.sub("consumed N", line)


EVENT = bytes(old_reg.value["events"][0]["discriminator"])
assert old_reg.value["events"][0]["name"] == "PasskeyOperationAuthorized"
DIGESTS = ("target_digest", "client_data_hash")


def passkey_event_same(p, q, pairs):
    """A PasskeyOperationAuthorized event that differs only in the digests of a decoded approval."""
    if not (p.startswith("Program data: ") and q.startswith("Program data: ")):
        return False
    a, b = base64.b64decode(p[14:]), base64.b64decode(q[14:])
    if a[:8] != EVENT or b[:8] != EVENT:
        return False
    u = old_reg.decode({"defined": {"name": "PasskeyOperationAuthorized"}}, a, 8)[0]
    v = new_reg.decode({"defined": {"name": "PasskeyOperationAuthorized"}}, b, 8)[0]
    return (all(u[k] == v[k] for k in u if k not in DIGESTS)
        and pairs.get((bytes(u["target_digest"]), bytes(u["client_data_hash"]))) == (bytes(v["target_digest"]), bytes(v["client_data_hash"])))


def without_seller_keys(lines):
    """main's seller check was require_keys_eq!, which logs both keys after IntentMismatch at lib.rs:350."""
    out, i = [], 0
    while i < len(lines):
        out.append(lines[i])
        if "lib.rs:350. Error Code: IntentMismatch" in lines[i] and lines[i + 1:i + 5:2] == ["Program log: Left:", "Program log: Right:"]:
            stats["log lines: the seller check's Left/Right keys (require_keys_eq) are not printed"] += 1
            i += 5
            continue
        i += 1
    return out


def logs_same(lx, ly, pairs):
    lx = without_seller_keys(lx)
    if len(lx) != len(ly):
        return False
    for p, q in zip(lx, ly):
        p, q = normalize(p), normalize(q)
        if p == q:
            continue
        if passkey_event_same(p, q, pairs):
            stats["log lines: passkey approval event, digests of the changed instruction"] += 1
            continue
        mp, mq = LINE.search(p), LINE.search(q)
        if mp and mq and LINE.sub("registry:L", p) == LINE.sub("registry:L", q):
            error = re.search(r"Error Code: (\w+)", p)
            moved[(error.group(1) if error else "?", mp.group(0).split("src/")[1], mq.group(0).split("src/")[1])] += 1
            continue
        return False
    return True


def mandate_equivalent(u, v):
    a, b = base64.b64decode(u["data"]), base64.b64decode(v["data"])
    if a[:8] != MANDATE or b[:8] != MANDATE or (len(a), len(b)) != (OLD_SIZE, NEW_SIZE):
        return False
    old = old_reg.decode({"defined": {"name": "Mandate"}}, a, 8)[0]
    new = new_reg.decode({"defined": {"name": "Mandate"}}, b, 8)[0]
    same = all(old[k] == new[k] for k in ("version", "bump", "principal", "actor", "budget", "mandate_id", "asset",
        "source", "per_operation_limit", "expires_at", "revision", "active"))
    pins = old["required_terms"][:old["required_terms_count"]]
    rules = dict(sellers=new["sellers"][:new["seller_count"]], terms=new["rules"][:new["rule_count"]])
    unused = [str(s) for s in new["sellers"][new["seller_count"]:]] == [DEFAULT] * (4 - new["seller_count"]) and all(
        r == dict(name=bytes(32), kind="List", value_count=0, values=[bytes(32)] * 4, min=0, max=0) for r in new["rules"][new["rule_count"]:])
    return (same and unused and rules == dict(sellers=[old["supplier"]], terms=[pin_rule(t) for t in pins])
        and (u["lamports"], v["lamports"]) == (RENT(OLD_SIZE), RENT(NEW_SIZE)) and u["owner"] == v["owner"] == REGISTRY)


def passkey_record_same(u, v, pairs):
    """A passkey record that differs only in the digests of a decoded approval."""
    a, b = base64.b64decode(u["data"]), base64.b64decode(v["data"])
    if a[:8] != PASSKEY or b[:8] != PASSKEY or len(a) != len(b) or (u["owner"], u["lamports"]) != (v["owner"], v["lamports"]):
        return False
    x = old_reg.decode({"defined": {"name": "PasskeyAuthority"}}, a, 8)[0]
    y = new_reg.decode({"defined": {"name": "PasskeyAuthority"}}, b, 8)[0]
    return (all(x[k] == y[k] for k in x if k not in ("last_target_digest", "last_client_data_hash"))
        and pairs.get((bytes(x["last_target_digest"]), bytes(x["last_client_data_hash"])))
            == (bytes(y["last_target_digest"]), bytes(y["last_client_data_hash"])))


def plain_permission(v):
    """The permission the behaviour change creates: the new layout, one seller, no rules."""
    data = base64.b64decode(v["data"])
    if data[:8] != MANDATE or len(data) != NEW_SIZE or v["lamports"] != RENT(NEW_SIZE):
        return False
    new = new_reg.decode({"defined": {"name": "Mandate"}}, data, 8)[0]
    return new["seller_count"] == 1 and new["rule_count"] == 0 and new["revision"] == 1


def states_same(su, sv, trace, i, side, name, after_behaviour):
    if su == sv:
        return True
    ok = True
    for address in sorted(set(su or {}) | set(sv or {})):
        u, v = (su or {}).get(address), (sv or {}).get(address)
        if u == v:
            continue
        if u is not None and v is not None and mandate_equivalent(u, v):
            stats[f"{side}: permission record, 517 -> 1,070 bytes, same meaning"] += 1
            continue
        if u is not None and v is not None and passkey_record_same(u, v, approved[trace]):
            stats[f"{side}: passkey record, digests of the last approved instruction"] += 1
            continue
        empty = u is None or (u["lamports"] == 0 and not base64.b64decode(u["data"]))
        if after_behaviour and empty and v is not None and plain_permission(v):
            stats["the plain permission that exists only on this branch (the behaviour change)"] += 1
            continue
        if u is not None and v is not None and all(u[k] == v[k] for k in ("owner", "data", "executable")):
            delta = u["lamports"] - v["lamports"]
            extra = RENT(NEW_SIZE) if after_behaviour else 0
            if delta - extra >= 0 and (delta - extra) % PER_PERMISSION == 0 and delta > 0:
                payer_deltas[(trace, address, (delta - extra) // PER_PERMISSION, bool(extra))] += 1
                continue
        unexplained.append((trace, i, side, address, name)); ok = False
    return ok


for path in sorted(old_dir.glob("*-trace.json")):
    a, b = json.loads(path.read_text()), json.loads((new_dir / path.name).read_text())
    ta, tb = a.get("transactions") or a.get("cases") or [], b.get("transactions") or b.get("cases") or []
    stats["traces"] += 1; stats["transactions"] += len(ta); stats["assertions"] += len(a.get("assertions") or [])
    if (a.get("status"), len(ta), len(a.get("assertions") or []), a.get("error")) != (b.get("status"), len(tb), len(b.get("assertions") or []), b.get("error")):
        unexplained.append((path.name, "summary", a.get("status"), b.get("status"), len(ta), len(tb), a.get("error"), b.get("error")))
    after_behaviour = False
    for i, (x, y) in enumerate(zip(ta, tb)):
        stats["refused"] += x.get("expected") != "success"
        changed_outcome = BEHAVIOUR.get((path.name, x.get("name")))
        if changed_outcome is not None:
            if y.get("name") != changed_outcome or y.get("expected") != "success" or not y.get("passed"):
                unexplained.append((path.name, i, "behaviour", x.get("name")))
            behaviour.append(dict(trace=path.name, row=i, before=x["name"], before_error=x["result"].get("err"),
                after=y["name"], after_error=y["result"].get("err")))
            after_behaviour = True
            continue
        for k in ("name", "expected", "passed", "account_keys", "signature_verified", "rollback_accounts"):
            if x.get(k) != y.get(k):
                unexplained.append((path.name, i, k, x.get("name")))
        if x["result"].get("err") != y["result"].get("err"):
            unexplained.append((path.name, i, "err", x.get("name"), x["result"].get("err"), y["result"].get("err")))
        xs, ys = x.get("instructions") or [], y.get("instructions") or []
        if [(s["program_id"], s["ordered_metas"]) for s in xs] != [(s["program_id"], s["ordered_metas"]) for s in ys]:
            unexplained.append((path.name, i, "instruction accounts", x.get("name")))
        else:
            kinds = [same_instruction(s, t, x, y, path.name) for s, t in zip(xs, ys)]
            if PROOF in kinds and not proofs_explained(x, y, kinds, approved[path.name]):
                kinds = [None if kind == PROOF else kind for kind in kinds]
            if None in kinds:
                unexplained.append((path.name, i, "instruction data", x.get("name")))
            for kind in kinds:
                if kind not in ("same", None):
                    stats["instruction data: " + kind] += 1
                    notes[kind].add(x.get("wire_bytes") and (y["wire_bytes"] - x["wire_bytes"]))
            if all(kind == "same" for kind in kinds) and x.get("transaction_base64") != y.get("transaction_base64"):
                unexplained.append((path.name, i, "signed bytes with identical instructions", x.get("name")))
        mx = x["result"].get("meta") or x["result"]; my = y["result"].get("meta") or y["result"]
        if not logs_same(mx.get("logs") or [], my.get("logs") or [], approved[path.name]):
            unexplained.append((path.name, i, "logs", x.get("name")))
        if mx.get("compute_units_consumed") != my.get("compute_units_consumed"):
            stats["transactions with different compute units"] += 1
            cu_deltas.append(my.get("compute_units_consumed", 0) - mx.get("compute_units_consumed", 0))
        for side in ("observed_before", "observed_after"):
            states_same(x.get(side), y.get(side), path.name, i, side, x.get("name"), after_behaviour)
    for j, (u, v) in enumerate(zip(a.get("assertions") or [], b.get("assertions") or [])):
        if u != v:
            stats["assertions that differ"] += 1
            assertion_diffs.append(dict(trace=path.name, index=j, old=u, new=v))
            if not (u.get("passed") and v.get("passed")):
                unexplained.append((path.name, "assertion", j, u.get("name")))
    if (a.get("receipts") or []) != (b.get("receipts") or []):
        unexplained.append((path.name, "receipts"))

print(json.dumps(dict(stats), indent=1))
print("wire byte change per instruction class:", json.dumps({k: sorted(v) for k, v in notes.items()}, default=str, indent=1))
print("compute unit change: min", min(cu_deltas, default=0), "max", max(cu_deltas, default=0))
print("source lines moved (error, main, this branch: occurrences):")
for (error, before, after), count in sorted(moved.items()):
    print(f"   {error}: {before} -> {after} ({count})")
print("payer accounts whose lamports differ only by the larger permission's rent (trace, account, permissions paid for: observations):")
for key, count in sorted(payer_deltas.items(), key=str):
    print("  ", key, count)
if not payer_deltas:
    print("   none: no observed account differs only in lamports")
print("behaviour changes:", json.dumps(behaviour, indent=1, default=str))
print("UNEXPLAINED:", len(unexplained))
for u in unexplained[:60]:
    print("  ", json.dumps(u, default=str)[:400])


# -- assertions ---------------------------------------------------------------
RENAMED_ASSERTIONS = {
    "a term is a name fingerprint and a value fingerprint":
        "a term is a name fingerprint, a value fingerprint and the number the value may fingerprint",
    "the permission record ends with the pinned-terms count and list": "the permission record ends with the rule count and the rules",
    "the Registry ran CreatePinnedMandate": "the Registry ran CreateMandate",
    "no permission exists after the creation with no terms": "the permission created with no terms pins nothing",
}
ACCOUNT = {"owner", "lamports", "data", "executable"}
assertion_classes, assertion_left = collections.Counter(), []


def equivalent(u, v, path=(), trace=None):
    """Leaf by leaf: identical, or a permission or passkey record that differs only in layout or digests."""
    if u == v:
        return True
    empty = u is None or (isinstance(u, dict) and set(u) == ACCOUNT and u["lamports"] == 0 and not base64.b64decode(u["data"]))
    if empty and isinstance(v, dict) and set(v) == ACCOUNT and any(trace == t for t, _ in BEHAVIOUR) and plain_permission(v):
        assertion_classes["snapshot leaf: the plain permission that exists only on this branch"] += 1; return True
    if isinstance(u, dict) and isinstance(v, dict):
        if set(u) == set(v) == ACCOUNT:
            if mandate_equivalent(u, v):
                assertion_classes["snapshot leaf: permission record, same meaning"] += 1; return True
            if passkey_record_same(u, v, approved[trace]):
                assertion_classes["snapshot leaf: passkey record, digests of the last approved instruction"] += 1; return True
            return False
        return set(u) == set(v) and all(equivalent(u[k], v[k], path + (k,), trace) for k in u)
    if isinstance(u, list) and isinstance(v, list):
        return len(u) == len(v) and all(equivalent(x, y, path, trace) for x, y in zip(u, v))
    return False


for item in assertion_diffs:
    u, v = item["old"], item["new"]
    name = u["name"]
    if not (u["passed"] and v["passed"]):
        unexplained.append((item["trace"], "assertion", item["index"], name)); continue
    if v["name"] != name:
        if RENAMED_ASSERTIONS.get(name) == v["name"]:
            assertion_classes["renamed: " + name] += 1; continue
        unexplained.append((item["trace"], "assertion name", item["index"], name)); continue
    if u["actual"] == u["expected"] and v["actual"] == v["expected"] and equivalent(u["actual"], v["actual"], trace=item["trace"]):
        assertion_classes["state snapshot equal in each run; permission or passkey records differ in layout"] += 1; continue
    digests = {old: new for (old, _), (new, _) in approved[item["trace"]].items()}
    if (name == "independent final privilege resolution is stable" and u["actual"] == u["expected"] and v["actual"] == v["expected"]
            and digests.get(bytes.fromhex(u["actual"])) == bytes.fromhex(v["actual"])):
        assertion_classes["digest of a passkey-approved instruction whose bytes changed: the digest of a decoded approval in each run"] += 1; continue
    if name == "order 2 is the largest signed transaction of this run" and (u["actual"], v["actual"]) == (3821, 3837):
        assertion_classes["largest transaction: 16 shown terms, one byte each for the absent number"] += 1; continue
    if name.startswith("agent ") and name.endswith(" only") and v["actual"] == [u["actual"]] and v["expected"] == [u["expected"]]:
        assertion_classes["the permission's seller read as a list of one"] += 1; continue
    if name == "the pin changed nothing else on the permission":
        a, b = dict(u["actual"]), dict(v["actual"])
        supplier = a.pop("supplier")
        if b.pop("seller_count") == 1 and b.pop("sellers")[:1] == [supplier] and a == b and u["actual"] == u["expected"] and v["actual"] == v["expected"]:
            assertion_classes["permission fields other than its rules: the seller read as a list of one"] += 1; continue
    assertion_left.append(item)
print("assertion classes:", json.dumps(dict(assertion_classes), indent=1))
print("assertions left for review:", len(assertion_left))
for item in assertion_left:
    u, v = item["old"], item["new"]
    print("  ", item["trace"], item["index"], u["name"], "|", json.dumps(u["actual"], default=str)[:160], "->", json.dumps(v["actual"], default=str)[:160])
print("UNEXPLAINED after assertions:", len(unexplained))
