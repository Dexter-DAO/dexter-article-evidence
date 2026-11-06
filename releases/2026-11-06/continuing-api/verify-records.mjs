import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createHash, createPublicKey, verify } from 'node:crypto';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// Offline inspection of this fixed local-run record set. No SDK, network or signing keys.
const directory = process.argv[2] ? resolve(process.argv[2]) : dirname(fileURLToPath(import.meta.url));
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
const read = name => readFileSync(join(directory, name));
const manifest = JSON.parse(read('EXPORT-MANIFEST.json'));
for (const name of ['SELECTED-RECORDS.json', 'SELECTED-RECEIPT.json', 'verify-records.mjs', 'README.md']) {
  const file = read(name);
  assert.equal(file.length, manifest.files[name].bytes, `${name}: size`);
  assert.equal(sha256(file), manifest.files[name].sha256, `${name}: SHA-256`);
}
const selection = JSON.parse(read('SELECTED-RECORDS.json'));
const summary = JSON.parse(read('SELECTED-RECEIPT.json'));
assert.equal(selection.schema, 'dexter-selected-api551-records/v1');
assert.equal(selection.source_report_sha256, '028599afe1fdacf3710c91db0e05adfecb473dc5f5b0d82443a2aa7ce031ea87');
assert.equal(summary.derivation.source_sha256, selection.source_report_sha256);
const r = selection.report;
assert.equal(r.status, 'passed');
assert.equal(r.genesisHash, selection.lifecycle.genesis);
assert.equal(r.genesisHash, summary.source.genesis);
assert.equal(selection.lifecycle.publicTransactionsSent, 0);
assert.deepEqual(summary.limits, selection.limits);
assert.equal(summary.native_binary_provenance, selection.lifecycle.nativeBinaryBoundary);

const alphabet = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';
function base58(value) {
  assert.equal(typeof value, 'string');
  let number = 0n;
  for (const character of value) {
    const digit = alphabet.indexOf(character);
    assert.ok(digit >= 0, 'invalid base58 character');
    number = number * 58n + BigInt(digit);
  }
  const bytes = [];
  while (number) {
    bytes.unshift(Number(number & 255n));
    number >>= 8n;
  }
  const leadingZeros = value.match(/^1*/)[0].length;
  return Buffer.concat([Buffer.alloc(leadingZeros), Buffer.from(bytes)]);
}

let signaturesVerified = 0;
assert.equal(r.cases.length, 16);
assert.equal(new Set(r.cases.map(c => c.signature)).size, 16);
for (const c of r.cases) {
  assert.equal(c.status, 'finalized', c.label);
  const wire = Buffer.from(c.signedTransaction, 'base64');
  assert.equal(wire.toString('base64'), c.signedTransaction, `${c.label}: canonical base64`);
  assert.equal(wire.length, c.bytes, `${c.label}: wire size`);
  assert.equal(sha256(wire), c.wireSha256, `${c.label}: wire hash`);

  // v1 message-first envelope and signer offset follow @solana/transactions 8.6.0.
  // This verifies the signed bytes and signer list, not every instruction's semantics.
  assert.equal(wire[0], 0x81, `${c.label}: this checker only supports retained v1 wires`);
  const count = wire[1];
  const messageEnd = wire.length - count * 64;
  assert.ok(count > 0 && messageEnd >= 42 + count * 32, `${c.label}: envelope length`);
  const message = wire.subarray(0, messageEnd);
  const captured = c.finalized.transaction;
  assert.equal(captured.signatures.length, count, `${c.label}: signature count`);
  assert.equal(captured.signatures[0], c.signature, `${c.label}: transaction identifier`);
  assert.equal(captured.message.header.numRequiredSignatures, count);
  assert.equal(captured.message.header.numReadonlySignedAccounts, wire[2]);
  assert.equal(captured.message.header.numReadonlyUnsignedAccounts, wire[3]);
  assert.deepEqual(message.subarray(8, 40), base58(captured.message.recentBlockhash));
  const keyCount = message[41];
  assert.equal(keyCount, captured.message.accountKeys.length, `${c.label}: static key count`);
  assert.ok(messageEnd >= 42 + keyCount * 32);
  for (let i = 0; i < keyCount; i++) {
    assert.deepEqual(message.subarray(42 + i * 32, 74 + i * 32), base58(captured.message.accountKeys[i]), `${c.label}: key ${i}`);
  }
  for (let i = 0; i < count; i++) {
    const signature = wire.subarray(messageEnd + i * 64, messageEnd + (i + 1) * 64);
    assert.deepEqual(signature, base58(captured.signatures[i]), `${c.label}: captured signature ${i}`);
    const publicBytes = message.subarray(42 + i * 32, 74 + i * 32);
    const publicKey = createPublicKey({key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), publicBytes]), format: 'der', type: 'spki'});
    assert.ok(verify(null, message, publicKey, signature), `${c.label}: Ed25519 signature ${i}`);
    signaturesVerified++;
  }
  assert.equal(c.finalized.meta.err, null, `${c.label}: captured error`);
  assert.deepEqual(c.finalized.meta.status, {Ok: null});
  assert.equal(c.finalized.meta.fee, c.feeQuote.value, `${c.label}: fee quote`);
  assert.equal(c.instructions.length, captured.message.instructions.length);
  c.instructions.forEach((instruction, i) => {
    const decoded = captured.message.instructions[i];
    assert.equal(instruction.programAddress, captured.message.accountKeys[decoded.programIdIndex]);
    assert.deepEqual(Buffer.from(instruction.dataHex, 'hex'), base58(decoded.data));
    assert.deepEqual(instruction.accounts.map(a => a.address), decoded.accounts.map(index => captured.message.accountKeys[index]));
  });
  const entry = summary.transactions.find(t => t.signature === c.signature);
  assert.ok(entry, `${c.label}: summary transaction`);
  assert.equal(entry.wire_sha256, c.wireSha256);
}

const f = r.financial;
assert.deepEqual(summary.financial.before_cash, f.beforeCash);
assert.deepEqual(summary.financial.after_cash, f.afterCash);
for (const [label, original] of [['earned', f.claim], ['unused_bill', f.zeroClaim]]) {
  for (const [key, value] of Object.entries(summary.financial[label])) assert.equal(value, original[key], `summary ${label}.${key}`);
}
const collection = r.cases.find(c => c.label === 'independent-api-governed-collection');
const keys = collection.finalized.transaction.message.accountKeys;
const balances = collection.finalized.meta;
for (const [role, address] of [['financier', f.payerRoles.economicCashSource], ['supplier', f.payerRoles.beneficiaryToken]]) {
  const index = keys.indexOf(address);
  assert.ok(index >= 0, `${role}: token account present`);
  const before = balances.preTokenBalances.find(b => b.accountIndex === index);
  const after = balances.postTokenBalances.find(b => b.accountIndex === index);
  assert.ok(before && after, `${role}: retained token balances`);
  assert.equal(before.mint, f.claim.terms.asset);
  assert.equal(after.mint, before.mint);
  assert.equal(before.uiTokenAmount.amount, f.beforeCash[role]);
  assert.equal(after.uiTokenAmount.amount, f.afterCash[role]);
  if (role === 'supplier') {
    assert.equal(before.owner, f.claim.terms.supplier);
    assert.equal(after.owner, f.payerRoles.beneficiaryReceivingPda);
  }
}
const debit = BigInt(f.beforeCash.financier) - BigInt(f.afterCash.financier);
const credit = BigInt(f.afterCash.supplier) - BigInt(f.beforeCash.supplier);
assert.equal(debit, 900n);
assert.equal(credit, debit);
assert.equal(BigInt(f.claim.paid), credit);
assert.equal(f.claim.debt, '900');
assert.equal(f.claim.repaid, '0');
assert.equal(BigInt(f.claim.released) + BigInt(f.claim.paid), BigInt(f.claim.terms.cap));
assert.equal(f.zeroClaim.debt, '0');
assert.equal(f.zeroClaim.paid, '0');
assert.equal(f.zeroClaim.released, f.zeroClaim.terms.cap);
assert.equal(f.claim.feeAuthorityRemaining, '90');
assert.equal(f.claim.releasedFeeCaps, '110');

const histories = new Map();
for (const row of r.pgHistory) {
  if (!histories.has(row.operation_key)) histories.set(row.operation_key, new Map());
  const history = histories.get(row.operation_key);
  assert.ok(!history.has(row.revision), 'duplicate retained revision');
  history.set(row.revision, row.record_digest);
}
assert.equal(histories.size, 8);
for (const history of histories.values()) {
  const revisions = [...history.keys()].sort((a,b) => a-b);
  assert.deepEqual(revisions, Array.from({length: revisions.length}, (_,i) => i));
}
assert.equal(r.recoveries.length, 26);
assert.equal(new Set(r.recoveries.map(x => x.pid)).size, 26);
for (const recovery of r.recoveries) {
  assert.equal(recovery.passed, true);
  for (const field of ['sendRequests', 'fundingRequests', 'admissionAuthorization']) {
    assert.equal(recovery.counters[field], 0, `recovery ${recovery.pid}: ${field}`);
  }
  if (recovery.offline) assert.equal(recovery.counters.rpcRequests, 0);
  assert.ok(histories.get(recovery.operationKey)?.has(recovery.revision), 'recovery key and revision exist in retained history');
  // recordSha256 hashes JSON.stringify(result); record_digest hashes canonical JSON.
  // The source record preimages are omitted, so these different digests cannot be equated here.
  assert.match(recovery.recordSha256, /^[a-f0-9]{64}$/);
}
const offline = r.recoveries.filter(x => x.offline).length;
assert.equal(offline, 19);
assert.deepEqual(summary.recovery, {distinct_processes: 26, offline, online: 7, all_passed: true, send_requests: 0, funding_requests: 0, admission_authorizations: 0});
assert.equal(r.operations.length, 7);
for (const operation of r.operations) {
  assert.equal(operation.attempts, 1);
  assert.equal(operation.receipt.commitment, 'finalized');
  assert.ok(histories.has(operation.key));
}
assert.equal(r.pending.broadcast, 'not-started');
assert.equal(r.pending.status, 'pending-reconciliation');
assert.equal(r.pending.billAbsent, true);
assert.equal(r.pending.claimAbsent, true);
assert.equal(r.cases.some(c => c.signature === r.pending.signature), false);
assert.deepEqual(summary.api_exposure, r.finalExposure);
assert.equal(r.finalExposure.intents, 3);
assert.equal(r.finalExposure.exposure, '6600');
for (const [key, value] of Object.entries(summary.unfinished_admission)) assert.equal(value, r.pending[key]);

console.log(JSON.stringify({
  status: 'passed',
  scope: 'Offline consistency and Ed25519 verification of retained local-run records; no execution replay or independent confirmation of the captured RPC/SQL observations.',
  finalized_records: r.cases.length,
  ed25519_signatures_verified: signaturesVerified,
  recorded_supplier_credit_atomic: credit.toString(),
  recorded_buyer_debt_atomic: f.claim.debt,
  recovery_records: r.recoveries.length,
  offline_recoveries: offline,
  history_rows: r.pgHistory.length,
  operations_with_recorded_receipts: r.operations.length,
  signed_unsent_admission: r.pending.status,
  raw_claim_state_decoding: 'not included; debt is checked against the retained decoded financial fields',
  recovery_content_equality: 'not checked; recovery and database digest preimages are omitted and use different serialization',
  rpc_observations_independently_authenticated: false,
  physical_device_or_production_proof: false,
  release: '2026-11-06'
}, null, 2));
