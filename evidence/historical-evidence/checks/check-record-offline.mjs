#!/usr/bin/env node
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import { isDeepStrictEqual } from 'node:util';

const require = createRequire(new URL('../verifier/package.json', import.meta.url));
// Use the same ESM entry point as the pinned verifier. Node's require resolver
// selects the package's separate CommonJS export instead.
const sellerSdk = await import(new URL('../verifier/node_modules/@dexterai/x402/dist/seller/index.js', import.meta.url));
const { PublicKey } = await import(pathToFileURL(require.resolve('@solana/web3.js')));
const { contextBoundVoucherV2Message } = await import(pathToFileURL(require.resolve('@dexterai/vault/messages')));
const { verifySolanaEd25519Strict } = await import(pathToFileURL(require.resolve('@dexterai/vault/precompile')));
const { deriveLockedClaimPda } = await import(pathToFileURL(require.resolve('@dexterai/vault/instructions')));
const { bytes, registrationOf, rerunCopycat } = await import('../verifier/verify.mjs');

const PROGRAM = new PublicKey('Hg3wRaydFtJhYrdvYrKECacpJYDsC9Px7yKmpncj2fhc');
export const EXPECTED_RECORD_SHA256 = '9c2052a9e07706d1596d819521615c187b76fc215087cc37cd13b1c17cbd152a';
const hash = value => createHash('sha256').update(value).digest('hex');
const requireCheck = (condition, detail) => { if (!condition) throw new Error(detail); };
const hex = (value, length = 32) => {
  requireCheck(typeof value === 'string' && new RegExp(`^[0-9a-f]{${length * 2}}$`).test(value), 'Noncanonical hex bytes');
  return Buffer.from(value, 'hex');
};

// This checker never contacts an RPC, website, or server. It checks the retained
// record's bytes and signatures. Chain outcomes are reread by the pinned verifier.
export async function checkOfflineRecord(record) {
  const checks = [];
  const check = async (name, operation) => {
    try { const detail = await operation(); checks.push({name, ok: true, detail}); }
    catch (error) { checks.push({name, ok: false, detail: error.message}); }
  };
  await check('record.identity', () => {
    requireCheck(record.schema === 'dexter-mandate-case/v1'
      && record.caseId === 'b0ee0521-898e-828b-9c14-265bfb59216c'
      && record.manifest.caseId === record.caseId, 'Unexpected case or schema');
    return record.caseId;
  });
  await check('manifest.digest', () => {
    const digest = hash(sellerSdk.canonicalBytes(record.manifest));
    requireCheck(digest === record.manifestDigest, 'Manifest digest differs');
    return digest;
  });
  const registration = registrationOf(record);
  const saved = record.seller;
  let binding;
  await check('seller.saved_evidence', async () => {
    const result = await sellerSdk.verifySellerEvidence({
      binding: bytes(saved.binding, 'seller binding'), salt: bytes(saved.salt, 'salt', 32),
      ceremonyNonce: hex(saved.ceremonyNonce),
      didDocument: bytes(saved.didDocument, 'seller document'), offerJws: saved.offerJws,
      registration: {vault: registration.vault, counterparty: registration.seller, sessionPubkey: registration.executionKey},
      history: [],
    });
    requireCheck(result.verified && result.binding && result.checks.every(row => row.ok),
      `Saved seller checks failed: ${JSON.stringify(result.checks.filter(row => !row.ok))}`);
    requireCheck(hash(bytes(saved.binding, 'seller binding')) === saved.bindingSha256
      && saved.bindingSha256 === record.manifest.seller.bindingSha256
      && saved.registerTx === registration.registrationSignature, 'Saved seller manifest or registration differs');
    binding = result.binding;
    return {checks: result.checks, nonce_source: 'retained record; approval transaction is checked by the full verifier'};
  });
  await check('seller.document_and_archive_digest', () => {
    requireCheck(binding, 'Seller binding must pass');
    const digest = hash(bytes(saved.didDocument, 'seller document'));
    requireCheck(digest === binding.didDocumentSha256 && digest === saved.archive.sha256, 'Seller document digest differs');
    return {sha256: digest, archive_url: saved.archive.url, archive_fetch: 'not performed by this offline check'};
  });

  const known = sellerSdk.knownSellers([{binding: bytes(saved.binding, 'seller binding'),
    salt: bytes(saved.salt, 'salt', 32), ceremonyNonce: hex(saved.ceremonyNonce),
    registerTx: saved.registerTx, registeredAt: record.blockTimes.approval}], {vault: registration.vault});
  await check('copycats.complete', () => {
    requireCheck(record.impostorAttempts.length === 5 && ['V1','V2','V3','V4','V5'].every(variant =>
      record.impostorAttempts.filter(item => item.variant === variant).length === 1), 'Missing or duplicate copycat variant');
    return 'V1 through V5 retained';
  });
  for (const attempt of record.impostorAttempts) await check(`copycat.${attempt.variant}`, async () => {
    requireCheck(binding && known.length === 1 && attempt.knownBindingSha256 === saved.bindingSha256,
      'Copycat must name the retained seller history');
    const result = await rerunCopycat(attempt, known);
    return {approve: result.approve, rule: result.refusal?.rule ?? result.notices?.[0]?.rule,
      code: result.refusal?.code ?? result.notices?.[0]?.code};
  });
  await check('first_contact', async () => {
    const result = await rerunCopycat(record.firstContact, [], {firstContact: true});
    return {approve: result.approve, relation: result.seller.relation};
  });
  await check('purchases.complete', () => {
    requireCheck(record.actions.length === 2 && record.manifest.purchases.length === 2, 'Expected two purchases');
    requireCheck(new Set(record.actions.map(action => action.id)).size === 2, 'Duplicate purchase id');
    return 'Two distinct purchases retained';
  });
  for (const [index, action] of record.actions.entries()) {
    const purchase = record.manifest.purchases[index];
    const proof = action.proof;
    await check(`purchase.${index + 1}.agent_voucher_signature`, () => {
      const voucher = proof.voucher;
      requireCheck(new PublicKey(hex(voucher.sessionPublicKey)).toBase58() === registration.executionKey
        && voucher.sessionNonce === registration.nonce, 'Voucher agent or nonce differs');
      const message = contextBoundVoucherV2Message({programId: PROGRAM, vaultPda: new PublicKey(registration.vault),
        sessionPda: new PublicKey(registration.sessionAddress), seller: new PublicKey(registration.seller),
        sessionNonce: registration.nonce, channelId: hex(voucher.channelId),
        cumulativeAmount: BigInt(voucher.cumulativeAmount), sequenceNumber: voucher.sequenceNumber});
      requireCheck(verifySolanaEd25519Strict(message, hex(voucher.sessionSignature, 64), new PublicKey(registration.executionKey).toBytes()),
        'Agent voucher signature failed');
      const digest = hash(message);
      requireCheck(digest === proof.voucherHash && deriveLockedClaimPda(new PublicKey(registration.vault), hex(digest)).toBase58() === proof.claimAddress,
        'Voucher digest or claim address differs');
      return {claim: proof.claimAddress, voucher_sha256: digest};
    });
    await check(`purchase.${index + 1}.retained_order_and_response`, () => {
      const requested = purchase.requestedAction, delivery = action.deliveryProof, receipt = delivery.bookstoreReceipt;
      requireCheck(purchase.id === action.id && requested.amountAtomic === action.amountAtomic
        && proof.amountAtomic === action.amountAtomic, 'Purchase id or amount differs');
      requireCheck(hash(Buffer.from(requested.orderRequestBody)) === requested.payloadDigest
        && isDeepStrictEqual(JSON.parse(requested.orderRequestBody), requested.bookstoreOrder)
        && isDeepStrictEqual(requested.bookstoreOrder, receipt.order), 'Retained order bytes differ');
      requireCheck(hash(Buffer.from(delivery.responseBody)) === delivery.responseDigest
        && isDeepStrictEqual(JSON.parse(delivery.responseBody), receipt), 'Retained response bytes differ');
      requireCheck(receipt.payment.seller === registration.seller && receipt.payment.channelId === proof.voucher.channelId
        && receipt.payment.sequenceNumber === proof.voucher.sequenceNumber
        && receipt.payment.cumulativeAtomic === proof.voucher.cumulativeAmount
        && receipt.payment.amountAtomic === action.amountAtomic, 'Retained receipt payment fields differ');
      return {order_id: action.id, request_sha256: requested.payloadDigest, response_sha256: delivery.responseDigest,
        evidence: 'Internal agreement of retained order, response and voucher fields'};
    });
    await check(`purchase.${index + 1}.seller_receipt_signature`, async () => {
      requireCheck(binding, 'Seller binding must pass');
      const result = await sellerSdk.verifySellerReceipt({receiptJws: action.deliveryProof.bookstoreReceipt.storeReceipt.signature,
        didDocument: bytes(saved.didDocument, 'seller document'), kid: binding.kid,
        expected: {resourceUrl: purchase.requestedAction.resourceUrl, payer: registration.vault,
          network: sellerSdk.SOLANA_MAINNET, issuedAtMin: 0, issuedAtMax: Number.MAX_SAFE_INTEGER}});
      requireCheck(result.ok, `Seller receipt check failed: ${result.code}`);
      return {payload: result.payload, checked_fields: ['signature', 'resourceUrl', 'payer', 'network'],
        chain_time_window: 'Checked by full verifier against reservation and lock transactions'};
    });
  }
  await check('redirect.same_claim_record', () => {
    const redirect = record.redirectRefusals.find(item => item.kind === 'payout-redirect');
    requireCheck(redirect.error.code === 6070 && redirect.claimAddress === record.actions[1].proof.claimAddress
      && redirect.impostor !== registration.seller, 'Redirect and purchase 2 do not name the same claim');
    return {claim: redirect.claimAddress, rejected_transaction: redirect.signature,
      successful_transaction: record.actions[1].proof.settlementSignature,
      evidence: 'Retained references; transaction execution and balances are checked by full verifier'};
  });
  for (const refusal of record.refusals) await check(`refusal.${refusal.error.code}.agent_signature`, () => {
    const request = refusal.agentRequest;
    const message = contextBoundVoucherV2Message({programId: PROGRAM, vaultPda: new PublicKey(registration.vault),
      sessionPda: new PublicKey(registration.sessionAddress), seller: new PublicKey(registration.seller),
      sessionNonce: registration.nonce, channelId: hex(request.voucher.channelId),
      cumulativeAmount: BigInt(request.voucher.cumulativeAmount), sequenceNumber: request.voucher.sequenceNumber});
    requireCheck(Buffer.from(message).equals(hex(request.message))
      && verifySolanaEd25519Strict(message, hex(request.sessionSignature, 64), new PublicKey(registration.executionKey).toBytes()),
      'Refusal voucher message or signature failed');
    return {signature_valid: true, recorded_outcome: refusal.error, transaction: refusal.signature};
  });
  return {ok: checks.every(row => row.ok), checks};
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  globalThis.fetch = async () => { throw new Error('Network access is disabled for this offline check'); };
  const recordBytes = await readFile(new URL('../record.json', import.meta.url));
  const result = await checkOfflineRecord(JSON.parse(recordBytes));
  result.record_sha256 = hash(recordBytes);
  result.exact_published_bytes = result.record_sha256 === EXPECTED_RECORD_SHA256;
  result.ok &&= result.exact_published_bytes;
  result.executed_at = new Date().toISOString();
  result.node = process.version;
  result.network_access = 'disabled';
  result.scope = 'Saved record hashes, seller and agent signatures, and deterministic copycat decisions';
  result.chain_verification = 'Run verifier/verify.mjs with an archival mainnet RPC to reread chain outcomes';
  console.log(JSON.stringify(result, null, 2));
  process.exitCode = result.ok ? 0 : 1;
}
