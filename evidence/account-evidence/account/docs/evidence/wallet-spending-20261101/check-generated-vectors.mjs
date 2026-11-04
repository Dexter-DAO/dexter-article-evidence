/** Recheck exact native evidence with current generated codecs/builders.
 * Run from the repository root after ./dev client generate && ./dev client build:
 * ./dev exec node docs/evidence/wallet-spending-20261101/check-generated-vectors.mjs
 * This constructs local instructions and decodes recorded accounts; no RPC call,
 * signer invocation, transaction submission or package publication occurs.
 */
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import * as core from '../../../foundation/onchain/client/dist/generated/claimsCore/index.js';
import * as registry from '../../../foundation/onchain/client/dist/generated/principalRegistry/index.js';

const require = createRequire(new URL('../../../foundation/onchain/client/package.json', import.meta.url));
const { createNoopSigner, getAddressEncoder, getProgramDerivedAddress } = await import(require.resolve('@solana/kit'));
const vectorsBytes = await readFile(new URL('./composition-vectors.json', import.meta.url));
const document = JSON.parse(vectorsBytes);
const registryIdl = JSON.parse(await readFile(new URL('./idl/principal_registry.json', import.meta.url)));
const manifestBytes = await readFile(new URL('../../../foundation/onchain/client/src/generated/manifest.json', import.meta.url));
const manifest = JSON.parse(manifestBytes);
const sha = (...items) => createHash('sha256').update(Buffer.concat(items.map(item => Buffer.from(item)))).digest();
const publicBytes = key => getAddressEncoder().encode(key);
const hex = value => Buffer.from(value).toString('hex');
const camel = text => text.replace(/_([a-z])/g, (_, letter) => letter.toUpperCase());
function typed(value) {
  if (typeof value === 'string' && /^[0-9a-f]{64}$/.test(value)) return new Uint8Array(Buffer.from(value, 'hex'));
  if (Array.isArray(value)) return value.map(typed);
  if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([key, entry]) => [camel(key), typed(entry)]));
  return value;
}
function sameBytes(actual, expected, description) {
  assert.equal(hex(actual), typeof expected === 'string' ? expected : hex(expected), description);
}
function sameAccountRoundTrip(bytes, decode, encode, description, padded = false) {
  const decoded = decode().decode(bytes);
  const encoded = encode().encode(decoded);
  sameBytes(encoded, bytes.subarray(0, encoded.length), description);
  if (padded) assert(bytes.subarray(encoded.length).every(byte => byte === 0));
  else assert.equal(encoded.length, bytes.length);
  return decoded;
}
const verified = [];
for (const vector of document.vectors) {
  const request = typed(vector.request);
  const requestBytes = core.getWalletSpendRequestEncoder().encode(request);
  sameBytes(requestBytes, vector.request_borsh_hex, 'generated WalletSpendRequest exact Borsh');
  sameBytes(sha('DEXTER:WALLET-SPEND:1', publicBytes(document.programs.claims_core), requestBytes), vector.request_digest, 'request digest');
  const operationId = sha('DEXTER:WALLET-SPEND-ID:1', publicBytes(document.programs.claims_core), publicBytes(request.principal), request.requestId);
  sameBytes(operationId, vector.operation_id, 'operation id');
  const [operationPda] = await getProgramDerivedAddress({ programAddress: document.programs.claims_core,
    seeds: [Buffer.from('wallet-operation'), publicBytes(request.principal), operationId] });
  assert.equal(operationPda, vector.operation_receipt);

  const permissionBeforeBytes = Buffer.from(vector.permission_account_before_base64, 'base64');
  const permission = sameAccountRoundTrip(permissionBeforeBytes,
    registry.getSpendingPermissionDecoder, registry.getSpendingPermissionEncoder, 'permission account round trip');
  const permissionAfter = sameAccountRoundTrip(Buffer.from(vector.permission_account_after_base64, 'base64'),
    registry.getSpendingPermissionDecoder, registry.getSpendingPermissionEncoder, 'consumed permission account round trip');
  const termsBytes = registry.getSpendingPermissionTermsEncoder().encode(typed(vector.permission.terms));
  sameBytes(termsBytes, vector.permission_terms_borsh_hex, 'generated spending terms exact Borsh');
  sameBytes(sha('DEXTER:SPENDING-PERMISSION:1', publicBytes(document.programs.principal_registry),
    publicBytes(request.principal), permission.permissionId, publicBytes(permission.grantOwner), termsBytes), permission.termsDigest, 'permission digest');
  const [permissionPda] = await getProgramDerivedAddress({ programAddress: document.programs.principal_registry,
    seeds: [Buffer.from('spending-permission'), publicBytes(request.principal), permission.permissionId] });
  assert.equal(permissionPda, vector.permission_address);
  assert.equal(permissionAfter.consumed.amount - permission.consumed.amount, BigInt(request.amount));
  assert.equal(permissionAfter.consumed.operations - permission.consumed.operations, 1n);

  const receiptBytes = Buffer.from(vector.receipt_account_base64, 'base64');
  const receipt = sameAccountRoundTrip(receiptBytes, core.getWalletSpendReceiptDecoder,
    core.getWalletSpendReceiptEncoder, 'aggregate receipt exact account encoding').data;
  sameBytes(core.getWalletSpendRequestEncoder().encode(receipt.request), requestBytes, 'recorded request equals encoded transaction request');
  sameBytes(receipt.requestDigest, vector.request_digest, 'recorded request digest');
  const zeroHashReceipt = core.getWalletSpendReceiptDataEncoder().encode({ ...receipt, receiptHash: new Uint8Array(32) });
  sameBytes(sha('DEXTER:WALLET-SPEND-RESULT:1', publicBytes(document.programs.claims_core), zeroHashReceipt), receipt.receiptHash, 'aggregate receipt hash');
  for (const leg of vector.finance_legs) {
    const legId = sha('DEXTER:WALLET-SPEND-LEG-ID:1', publicBytes(document.programs.claims_core), operationId, Uint8Array.of(leg.leg));
    sameBytes(legId, leg.operation_id, 'mint-specific leg id');
    const [legPda] = await getProgramDerivedAddress({ programAddress: document.programs.claims_core,
      seeds: [Buffer.from('finance-operation'), publicBytes(request.principal), legId] });
    assert.equal(legPda, leg.receipt);
    const financial = sameAccountRoundTrip(Buffer.from(leg.account_base64, 'base64'),
      core.getFinanceReceiptDecoder, core.getFinanceReceiptEncoder, 'financial leg exact encoded prefix and zero padding', true).data;
    sameBytes(financial.operationId, legId, 'financial leg identity');
    sameBytes(financial.requestDigest, leg.request_digest, 'financial leg request digest');
    sameBytes(sha('DEXTER:WALLET-SPEND-LEG:1', publicBytes(document.programs.claims_core), receipt.requestDigest, Uint8Array.of(leg.leg)), financial.requestDigest, 'derived financial leg digest');
    const encoded = core.getFinanceReceiptDataEncoder().encode({ ...financial, effectsHash: new Uint8Array(32) });
    sameBytes(sha('DEXTER:FINANCIAL-RECEIPT:1', publicBytes(document.programs.claims_core), encoded), financial.effectsHash, 'financial effects hash');
    assert.equal(financial.relatedReceipt, operationPda);
  }

  const accounts = vector.core_ordered_accounts.slice(0, vector.core_fixed_account_count);
  const builderInput = Object.fromEntries(accounts.map(meta => [camel(meta.name), meta.declared_signer ? createNoopSigner(meta.pubkey) : meta.pubkey]));
  Object.assign(builderInput, typed(vector.core_instruction_args));
  const built = vector.kind === 'wallet_send' ? core.getWalletSendInstruction(builderInput) : core.getWalletSwapExactInputInstruction(builderInput);
  assert.equal(built.programAddress, document.programs.claims_core);
  sameBytes(built.data, vector.core_instruction_data_hex, 'generated Core instruction bytes');
  assert.equal(built.accounts.length, vector.core_fixed_account_count);
  for (const [index, meta] of accounts.entries()) {
    assert.equal(built.accounts[index].address, meta.pubkey, 'generated fixed account order');
    assert.equal(built.accounts[index].role, Number(meta.declared_writable) | Number(meta.declared_signer) << 1, 'generated declared inner privilege');
  }
  sameBytes(registry.getAuthorizeFundingWalletInstructionDataEncoder().encode(typed(vector.registry_authorization)),
    vector.native_v1_instructions[0].data_hex, 'generated Registry authorization instruction bytes');
  const grant = vector.permission_grant_instruction;
  const grantFields = registryIdl.instructions.find(instruction => instruction.name === 'grant_spending_permission').accounts;
  const grantInput = Object.fromEntries(grantFields.map((field, index) => [camel(field.name),
    field.signer ? createNoopSigner(grant.ordered_metas[index].pubkey) : grant.ordered_metas[index].pubkey]));
  Object.assign(grantInput, { permissionId: permission.permissionId, terms: permission.terms });
  const grantBuilt = registry.getGrantSpendingPermissionInstruction(grantInput);
  sameBytes(grantBuilt.data, grant.data_hex, 'generated individual approval instruction bytes');
  for (const [index, meta] of grant.ordered_metas.entries()) {
    assert.equal(grantBuilt.accounts[index].address, meta.pubkey);
    assert.equal(grantBuilt.accounts[index].role, Number(meta.is_writable) | Number(meta.is_signer) << 1);
  }
  verified.push({ kind: vector.kind, signedPacketBytes: vector.signed_packet_bytes,
    requestBorshBytes: requestBytes.length, permissionTermsBorshBytes: termsBytes.length,
    aggregateReceiptBytes: receiptBytes.length, fixedCoreAccounts: built.accounts.length,
    requiredFinancialBookAccounts: vector.core_ordered_accounts.length - built.accounts.length,
    exactInstructionBytes: built.data.length, financeLegs: vector.finance_legs.length,
    requestDigest: vector.request_digest, aggregateReceipt: operationPda });
}
console.log(JSON.stringify({ status: 'passed', vectorsSha256: sha(vectorsBytes).toString('hex'),
  generatedManifestSha256: sha(manifestBytes).toString('hex'),
  sourceProvenance: manifest.provenance, verified,
  scope: 'Generated TypeScript instruction builders/account codecs match exact signed local program evidence. No customer SDK preparation, signer invocation, RPC admission, submission or recovery qualification.' }, null, 2));
