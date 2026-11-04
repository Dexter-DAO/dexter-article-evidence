import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import { checkOfflineRecord } from './check-record-offline.mjs';

globalThis.fetch = async () => { throw new Error('Network access is disabled'); };
const original = JSON.parse(await readFile(new URL('../record.json', import.meta.url)));
const changedSignature = signature => {
  const parts = signature.split('.');
  const bytes = Buffer.from(parts[2], 'base64url');
  bytes[0] ^= 1;
  parts[2] = bytes.toString('base64url');
  return parts.join('.');
};

test('the exact public record passes all offline checks', async () => {
  const result = await checkOfflineRecord(original);
  assert.equal(result.ok, true, JSON.stringify(result.checks.filter(row => !row.ok)));
});

test('a changed agent voucher channel fails its signature check', async () => {
  const record = structuredClone(original);
  record.actions[0].proof.voucher.channelId = '00'.repeat(32);
  const result = await checkOfflineRecord(record);
  assert.equal(result.ok, false);
  assert.equal(result.checks.find(row => row.name === 'purchase.1.agent_voucher_signature').ok, false);
});

test('a changed signed seller quote fails saved seller evidence', async () => {
  const record = structuredClone(original);
  record.seller.offerJws = changedSignature(record.seller.offerJws);
  const result = await checkOfflineRecord(record);
  assert.equal(result.ok, false);
  assert.equal(result.checks.find(row => row.name === 'seller.saved_evidence').ok, false);
});

test('a changed seller receipt signature fails verification', async () => {
  const record = structuredClone(original);
  const receipt = record.actions[1].deliveryProof.bookstoreReceipt;
  receipt.storeReceipt.signature = changedSignature(receipt.storeReceipt.signature);
  const result = await checkOfflineRecord(record);
  assert.equal(result.ok, false);
  assert.equal(result.checks.find(row => row.name === 'purchase.2.seller_receipt_signature').ok, false);
});
