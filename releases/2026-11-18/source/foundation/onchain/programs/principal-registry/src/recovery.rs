//! Opt-in recovery authority over one continuing Principal. A pre-enrolled
//! guardian quorum may replace a lost or hostile operational owner; it never
//! moves cash, resets an allowance or revokes an accepted financial right.
use anchor_lang::prelude::*;
use anchor_lang::solana_program::instruction::{get_stack_height, TRANSACTION_LEVEL_STACK_HEIGHT};
use dexter_contracts::{plus, SCHEMA_VERSION};
use solana_sha256_hasher::hashv;
use crate::{Principal, RegistryError};

pub const RECOVERY_VERSION: u16 = 1;
pub const MAX_RECOVERY_GUARDIANS: usize = 5;
pub const RECOVERY_POLICY_SEED: &[u8] = b"recovery-policy";
pub const RECOVERY_POLICY_CHANGE_SEED: &[u8] = b"recovery-policy-change";
pub const OWNER_RECOVERY_SEED: &[u8] = b"owner-recovery";
const INSTRUCTIONS_ID: Pubkey = pubkey!("Sysvar1nstructions1111111111111111111111111");

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct RecoveryPolicyTerms {
    pub guardian_count: u8,
    pub threshold: u8,
    pub guardians: [Pubkey; MAX_RECOVERY_GUARDIANS],
}

impl RecoveryPolicyTerms {
    pub fn validate(&self) -> Result<()> {
        let count = usize::from(self.guardian_count);
        require!(count > 0 && count <= MAX_RECOVERY_GUARDIANS
            && self.threshold > 0 && usize::from(self.threshold) <= count,
            RegistryError::RecoveryPolicy);
        for (index, guardian) in self.guardians.iter().enumerate() {
            if index < count {
                // A selected key can also be the canonical immutable passkey
                // alias for this Principal. Selection grants no vote: every
                // supplied quorum member is separately authenticated below.
                require!(*guardian != Pubkey::default()
                    && !self.guardians[..index].contains(guardian), RegistryError::RecoveryPolicy);
            } else {
                require!(*guardian == Pubkey::default(), RegistryError::RecoveryPolicy);
            }
        }
        Ok(())
    }

    pub fn contains(&self, key: &Pubkey) -> bool {
        self.guardians.iter().take(usize::from(self.guardian_count)).any(|value| value == key)
    }

    fn quorum(&self, signers: &[Pubkey]) -> Result<u8> {
        self.validate()?;
        let mut bitmap = 0u8;
        for (index, key) in self.guardians.iter().take(usize::from(self.guardian_count)).enumerate() {
            if signers.contains(key) { bitmap |= 1u8 << index; }
        }
        require!(bitmap.count_ones() >= u32::from(self.threshold), RegistryError::RecoveryQuorum);
        Ok(bitmap)
    }
}

#[account]
#[derive(InitSpace)]
pub struct RecoveryPolicy {
    pub version: u16,
    pub bump: u8,
    pub principal: Pubkey,
    pub revision: u64,
    /// Independent of ordinary owner rotations, which cannot veto recovery.
    pub recovery_sequence: u64,
    pub terms: RecoveryPolicyTerms,
    pub terms_digest: [u8; 32],
}

impl RecoveryPolicy {
    pub fn check(&self, key: Pubkey, principal: Pubkey, domain: &[u8; 32]) -> Result<()> {
        let (expected, bump) = recovery_policy_address(&principal);
        require!(self.version == RECOVERY_VERSION && self.bump == bump && key == expected
            && self.principal == principal && self.revision > 0, RegistryError::RecoveryPolicy);
        self.terms.validate()?;
        require!(self.terms_digest == recovery_policy_digest(domain, &principal, self.revision, &self.terms)?,
            RegistryError::RecoveryPolicy);
        Ok(())
    }
}

pub fn recovery_policy_address(principal: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[RECOVERY_POLICY_SEED, principal.as_ref()], &crate::ID)
}

/// Cooperative owner rotation must observe enrollment before selecting a new
/// owner. An absent canonical sidecar means no recovery authority is enrolled.
pub fn read_recovery_policy(info: &AccountInfo, principal: &Account<Principal>) -> Result<Option<RecoveryPolicy>> {
    require_keys_eq!(info.key(), recovery_policy_address(&principal.key()).0, RegistryError::RecoveryPolicy);
    require!(!info.executable, RegistryError::RecoveryPolicy);
    if info.data_is_empty() {
        require_keys_eq!(*info.owner, System::id(), RegistryError::RecoveryPolicy);
        return Ok(None);
    }
    require!(*info.owner == crate::ID && info.data_len() == 8 + RecoveryPolicy::INIT_SPACE,
        RegistryError::RecoveryPolicy);
    let policy = RecoveryPolicy::try_deserialize(&mut &info.try_borrow_data()?[..])
        .map_err(|_| error!(RegistryError::RecoveryPolicy))?;
    policy.check(info.key(), principal.key(), &principal.domain)?;
    Ok(Some(policy))
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct ConfigureRecoveryPolicyArgs {
    pub request_id: [u8; 32],
    /// Zero only for initial enrollment; changes require the current revision.
    pub expected_revision: u64,
    pub authority_generation: u64,
    pub terms: RecoveryPolicyTerms,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct RecoverPrincipalOwnerArgs {
    pub request_id: [u8; 32],
    pub policy_revision: u64,
    pub recovery_sequence: u64,
    pub policy_digest: [u8; 32],
    pub new_owner: Pubkey,
    pub expires_at: i64,
}

#[account]
#[derive(InitSpace)]
pub struct RecoveryPolicyChangeReceipt {
    pub version: u16,
    pub bump: u8,
    pub principal: Pubkey,
    pub request_id: [u8; 32],
    pub authorizing_owner: Pubkey,
    pub authority_generation: u64,
    pub previous_revision: u64,
    pub revision: u64,
    pub recovery_sequence: u64,
    pub previous_policy_digest: [u8; 32],
    pub policy_digest: [u8; 32],
    pub terms: RecoveryPolicyTerms,
    pub configured_at: i64,
    pub previous_guardian_bitmap: u8,
    pub new_guardian_bitmap: u8,
    pub request_digest: [u8; 32],
}

#[account]
#[derive(InitSpace)]
pub struct OwnerRecoveryReceipt {
    pub version: u16,
    pub bump: u8,
    pub principal: Pubkey,
    pub request_id: [u8; 32],
    pub policy_revision: u64,
    pub policy_digest: [u8; 32],
    pub recovery_sequence: u64,
    pub next_recovery_sequence: u64,
    pub old_owner: Pubkey,
    pub new_owner: Pubkey,
    pub previous_generation: u64,
    pub next_generation: u64,
    pub expires_at: i64,
    pub recovered_at: i64,
    pub guardian_bitmap: u8,
    pub request_digest: [u8; 32],
}

pub fn recovery_policy_digest(domain: &[u8; 32], principal: &Pubkey, revision: u64,
    terms: &RecoveryPolicyTerms) -> Result<[u8; 32]> {
    let mut bytes = Vec::with_capacity(RecoveryPolicyTerms::INIT_SPACE);
    terms.serialize(&mut bytes)?;
    Ok(hashv(&[b"DEXTER:RECOVERY-POLICY:1", crate::ID.as_ref(), domain, principal.as_ref(),
        &revision.to_le_bytes(), &bytes]).to_bytes())
}

pub fn recovery_configuration_digest(domain: &[u8; 32], principal: &Pubkey, owner: &Pubkey,
    args: &ConfigureRecoveryPolicyArgs) -> Result<[u8; 32]> {
    let mut bytes = Vec::with_capacity(ConfigureRecoveryPolicyArgs::INIT_SPACE);
    args.serialize(&mut bytes)?;
    Ok(hashv(&[b"DEXTER:RECOVERY-POLICY-CHANGE:1", crate::ID.as_ref(), domain,
        principal.as_ref(), owner.as_ref(), &bytes]).to_bytes())
}

pub fn owner_recovery_digest(domain: &[u8; 32], principal: &Pubkey,
    args: &RecoverPrincipalOwnerArgs) -> Result<[u8; 32]> {
    let mut bytes = Vec::with_capacity(RecoverPrincipalOwnerArgs::INIT_SPACE);
    args.serialize(&mut bytes)?;
    Ok(hashv(&[b"DEXTER:OWNER-RECOVERY:1", crate::ID.as_ref(), domain,
        principal.as_ref(), &bytes]).to_bytes())
}

#[derive(Accounts)]
#[instruction(args: ConfigureRecoveryPolicyArgs)]
pub struct ConfigureRecoveryPolicy<'info> {
    #[account(mut)] pub payer: Signer<'info>,
    /// CHECK: Fresh enrollment/change requires current native or exact-operation
    /// passkey owner authority;
    /// historical replay only matches its original public identity.
    pub owner: UncheckedAccount<'info>,
    #[account(seeds = [b"principal", principal.seed_owner.as_ref(), principal.principal_id.as_ref()], bump = principal.bump)]
    pub principal: Box<Account<'info, Principal>>,
    #[account(init_if_needed, payer = payer, space = 8 + RecoveryPolicy::INIT_SPACE,
        seeds = [RECOVERY_POLICY_SEED, principal.key().as_ref()], bump)]
    pub policy: Box<Account<'info, RecoveryPolicy>>,
    #[account(init_if_needed, payer = payer, space = 8 + RecoveryPolicyChangeReceipt::INIT_SPACE,
        seeds = [RECOVERY_POLICY_CHANGE_SEED, principal.key().as_ref(), &args.request_id], bump)]
    pub receipt: Box<Account<'info, RecoveryPolicyChangeReceipt>>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(args: RecoverPrincipalOwnerArgs)]
pub struct RecoverPrincipalOwner<'info> {
    #[account(mut)] pub payer: Signer<'info>,
    #[account(mut, seeds = [b"principal", principal.seed_owner.as_ref(), principal.principal_id.as_ref()], bump = principal.bump)]
    pub principal: Box<Account<'info, Principal>>,
    #[account(mut, has_one = principal,
        seeds = [RECOVERY_POLICY_SEED, principal.key().as_ref()], bump = policy.bump)]
    pub policy: Box<Account<'info, RecoveryPolicy>>,
    /// CHECK: Address is bound to the exact request. Possession is checked only
    /// for fresh recovery, so a completed result needs no lost signer again.
    #[account(address = args.new_owner @ RegistryError::RecoveryOwner)]
    pub new_owner: UncheckedAccount<'info>,
    #[account(init_if_needed, payer = payer, space = 8 + OwnerRecoveryReceipt::INIT_SPACE,
        seeds = [OWNER_RECOVERY_SEED, principal.key().as_ref(), &args.request_id], bump)]
    pub receipt: Box<Account<'info, OwnerRecoveryReceipt>>,
    pub system_program: Program<'info, System>,
}

/// Only the canonical instructions sysvar is metadata. Every other supplied
/// account must be one selected, independently authenticated guardian. Canonical
/// passkey aliases are keyed by Principal and cryptographic public key, so
/// credential labels cannot duplicate one key's voting power.
fn guardian_keys(remaining: &[AccountInfo], current: Option<&RecoveryPolicyTerms>,
    proposed: &RecoveryPolicyTerms, mut authenticate: impl FnMut(&AccountInfo) -> Result<()>) -> Result<Vec<Pubkey>> {
    let limit = if current.is_some() { 2 * MAX_RECOVERY_GUARDIANS } else { MAX_RECOVERY_GUARDIANS };
    require!(remaining.len() <= limit + 1, RegistryError::RecoveryQuorum);
    let mut keys = Vec::with_capacity(limit);
    let mut instructions_seen = false;
    for info in remaining {
        let key = info.key();
        if key == INSTRUCTIONS_ID {
            require!(!instructions_seen && !info.is_signer && !info.is_writable
                && !info.executable, RegistryError::RecoveryQuorum);
            instructions_seen = true;
            continue;
        }
        require!(keys.len() < limit && !keys.contains(&key)
            && (proposed.contains(&key) || current.map(|terms| terms.contains(&key)).unwrap_or(false)),
            RegistryError::RecoveryQuorum);
        authenticate(info)?;
        keys.push(key);
    }
    Ok(keys)
}

fn signer_keys(principal: &Account<Principal>, remaining: &[AccountInfo],
    current: Option<&RecoveryPolicyTerms>, proposed: &RecoveryPolicyTerms) -> Result<Vec<Pubkey>> {
    guardian_keys(remaining, current, proposed, |info| {
        // Keep the established native missing-signature outcome. An off-curve
        // account must still pass the canonical passkey proof below; being a
        // PDA or receiving a CPI signer privilege never supplies a vote.
        require!(!info.key().is_on_curve() || info.is_signer, RegistryError::RecoveryQuorum);
        crate::require_guardian_authority(principal, info, remaining)
    })
}

pub fn configure_policy(ctx: Context<ConfigureRecoveryPolicy>, args: ConfigureRecoveryPolicyArgs) -> Result<()> {
    require!(get_stack_height() == TRANSACTION_LEVEL_STACK_HEIGHT, RegistryError::WalletEnvelope);
    let a = ctx.accounts;
    let principal = a.principal.key(); let owner = a.owner.key();
    let digest = recovery_configuration_digest(&a.principal.domain, &principal, &owner, &args)?;
    let next_revision = plus(args.expected_revision, 1)?;
    let policy_digest = recovery_policy_digest(&a.principal.domain, &principal, next_revision, &args.terms)?;
    if a.receipt.version != 0 {
        let r = &a.receipt;
        require!(r.version == RECOVERY_VERSION && r.bump == ctx.bumps.receipt && r.principal == principal
            && r.request_id == args.request_id && r.authorizing_owner == owner
            && r.authority_generation == args.authority_generation && r.previous_revision == args.expected_revision
            && r.revision == next_revision && r.terms == args.terms && r.policy_digest == policy_digest
            && r.request_digest == digest, RegistryError::RecoveryReplay);
        crate::require_proof_free_control_replay(ctx.remaining_accounts)?;
        return Ok(()); // Never reenroll old guardians or reset the recovery sequence.
    }
    require!(a.principal.version == SCHEMA_VERSION && a.principal.authority_generation > 0,
        RegistryError::SchemaVersion);
    require_keys_eq!(owner, a.principal.owner, RegistryError::OwnerAuthority);
    require!(args.authority_generation == a.principal.authority_generation, RegistryError::WalletGeneration);
    crate::require_owner_authority(&a.principal, &a.owner.to_account_info(), ctx.remaining_accounts)?;
    require!(args.request_id != [0; 32], RegistryError::RecoveryPolicy);
    args.terms.validate()?;
    require!(!args.terms.contains(&owner), RegistryError::RecoveryOwner);
    let (sequence, previous_digest, previous_bitmap, next_bitmap) = if a.policy.version == 0 {
        require!(args.expected_revision == 0, RegistryError::RecoveryRevision);
        let keys = signer_keys(&a.principal, ctx.remaining_accounts, None, &args.terms)?;
        (0, [0; 32], 0, args.terms.quorum(&keys)?)
    } else {
        a.policy.check(a.policy.key(), principal, &a.principal.domain)?;
        require!(args.expected_revision == a.policy.revision, RegistryError::RecoveryRevision);
        let keys = signer_keys(&a.principal, ctx.remaining_accounts, Some(&a.policy.terms), &args.terms)?;
        (a.policy.recovery_sequence, a.policy.terms_digest,
            a.policy.terms.quorum(&keys)?, args.terms.quorum(&keys)?)
    };
    let now = Clock::get()?.unix_timestamp;
    a.policy.set_inner(RecoveryPolicy { version: RECOVERY_VERSION, bump: ctx.bumps.policy,
        principal, revision: next_revision, recovery_sequence: sequence,
        terms: args.terms.clone(), terms_digest: policy_digest });
    a.receipt.set_inner(RecoveryPolicyChangeReceipt { version: RECOVERY_VERSION, bump: ctx.bumps.receipt,
        principal, request_id: args.request_id, authorizing_owner: owner,
        authority_generation: args.authority_generation, previous_revision: args.expected_revision,
        revision: next_revision, recovery_sequence: sequence, previous_policy_digest: previous_digest,
        policy_digest, terms: args.terms, configured_at: now,
        previous_guardian_bitmap: previous_bitmap, new_guardian_bitmap: next_bitmap, request_digest: digest });
    emit!(RecoveryPolicyConfigured { principal, revision: next_revision, policy_digest, receipt: a.receipt.key() });
    Ok(())
}

pub fn recover_owner(ctx: Context<RecoverPrincipalOwner>, args: RecoverPrincipalOwnerArgs) -> Result<()> {
    require!(get_stack_height() == TRANSACTION_LEVEL_STACK_HEIGHT, RegistryError::WalletEnvelope);
    let a = ctx.accounts; let principal = a.principal.key();
    let digest = owner_recovery_digest(&a.principal.domain, &principal, &args)?;
    let next_sequence = plus(args.recovery_sequence, 1)?;
    if a.receipt.version != 0 {
        let r = &a.receipt;
        require!(r.version == RECOVERY_VERSION && r.bump == ctx.bumps.receipt && r.principal == principal
            && r.request_id == args.request_id && r.policy_revision == args.policy_revision
            && r.policy_digest == args.policy_digest && r.recovery_sequence == args.recovery_sequence
            && r.next_recovery_sequence == next_sequence && r.new_owner == args.new_owner
            && r.expires_at == args.expires_at && r.next_generation == plus(r.previous_generation, 1)?
            && r.request_digest == digest, RegistryError::RecoveryReplay);
        crate::require_proof_free_control_replay(ctx.remaining_accounts)?;
        return Ok(()); // Historical recognition never overwrites a later owner.
    }
    require!(a.principal.version == SCHEMA_VERSION && a.principal.authority_generation > 0,
        RegistryError::SchemaVersion);
    a.policy.check(a.policy.key(), principal, &a.principal.domain)?;
    require!(args.policy_revision == a.policy.revision && args.policy_digest == a.policy.terms_digest,
        RegistryError::RecoveryRevision);
    require!(args.recovery_sequence == a.policy.recovery_sequence, RegistryError::RecoverySequence);
    require!(args.new_owner != Pubkey::default()
        && args.new_owner != a.principal.owner && !a.policy.terms.contains(&args.new_owner),
        RegistryError::RecoveryOwner);
    require!(!args.new_owner.is_on_curve() || a.new_owner.is_signer, RegistryError::RecoveryOwner);
    let now = Clock::get()?.unix_timestamp;
    require!(now >= 0 && args.request_id != [0; 32] && args.expires_at > now && args.expires_at < i64::MAX,
        RegistryError::RecoveryExpired);
    crate::require_authority_possession(&a.principal, &a.new_owner.to_account_info(), ctx.remaining_accounts)?;
    let keys = signer_keys(&a.principal, ctx.remaining_accounts, None, &a.policy.terms)?;
    let guardian_bitmap = a.policy.terms.quorum(&keys)?;
    let old_owner = a.principal.owner; let previous_generation = a.principal.authority_generation;
    let next_generation = plus(previous_generation, 1)?;
    a.principal.owner = args.new_owner;
    a.principal.authority_generation = next_generation;
    a.policy.recovery_sequence = next_sequence;
    a.receipt.set_inner(OwnerRecoveryReceipt { version: RECOVERY_VERSION, bump: ctx.bumps.receipt,
        principal, request_id: args.request_id, policy_revision: args.policy_revision,
        policy_digest: args.policy_digest, recovery_sequence: args.recovery_sequence,
        next_recovery_sequence: next_sequence, old_owner, new_owner: args.new_owner,
        previous_generation, next_generation, expires_at: args.expires_at, recovered_at: now,
        guardian_bitmap, request_digest: digest });
    emit!(PrincipalOwnerRecovered { principal, old_owner, new_owner: args.new_owner,
        authority_generation: next_generation, recovery_sequence: next_sequence, receipt: a.receipt.key() });
    Ok(())
}

#[event]
pub struct RecoveryPolicyConfigured {
    pub principal: Pubkey,
    pub revision: u64,
    pub policy_digest: [u8; 32],
    pub receipt: Pubkey,
}

#[event]
pub struct PrincipalOwnerRecovered {
    pub principal: Pubkey,
    pub old_owner: Pubkey,
    pub new_owner: Pubkey,
    pub authority_generation: u64,
    pub recovery_sequence: u64,
    pub receipt: Pubkey,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guardian(seed: u8) -> Pubkey {
        let mut bytes = [seed; 32];
        for tail in 0u8..=u8::MAX {
            bytes[31] = tail;
            let key = Pubkey::new_from_array(bytes);
            if key != Pubkey::default() && key.is_on_curve() { return key; }
        }
        panic!("deterministic test seed has no curve point");
    }

    fn terms() -> RecoveryPolicyTerms {
        RecoveryPolicyTerms { guardian_count: 2, threshold: 2,
            guardians: [guardian(11), guardian(12), Pubkey::default(), Pubkey::default(), Pubkey::default()] }
    }

    #[test]
    fn guardian_policy_has_one_bounded_canonical_shape() {
        let valid = terms();
        valid.validate().unwrap();
        for count in [0, 6, u8::MAX] {
            let mut invalid = valid.clone(); invalid.guardian_count = count;
            assert!(invalid.validate().is_err());
        }
        for threshold in [0, 3, u8::MAX] {
            let mut invalid = valid.clone(); invalid.threshold = threshold;
            assert!(invalid.validate().is_err());
        }
        let mut duplicate = valid.clone(); duplicate.guardians[1] = duplicate.guardians[0];
        assert!(duplicate.validate().is_err());
        let mut missing = valid.clone(); missing.guardians[0] = Pubkey::default();
        assert!(missing.validate().is_err());
        let mut hidden = valid.clone(); hidden.guardians[2] = guardian(13);
        assert!(hidden.validate().is_err());
        let mut passkey_alias = valid.clone();
        passkey_alias.guardians[0] = Pubkey::find_program_address(&[b"test-guardian"], &crate::ID).0;
        // Structural selection alone never authenticates a quorum member.
        assert!(passkey_alias.validate().is_ok());
        let mut maximum = valid;
        maximum.guardian_count = 5; maximum.threshold = 5;
        maximum.guardians = [guardian(1), guardian(2), guardian(3), guardian(4), guardian(5)];
        maximum.validate().unwrap();
        assert_eq!(maximum.quorum(&maximum.guardians).unwrap(), 31);
    }

    #[test]
    fn duplicate_or_foreign_keys_cannot_supply_missing_quorum() {
        let policy = terms();
        let [first, second, ..] = policy.guardians;
        assert_eq!(policy.quorum(&[first, second]).unwrap(), 3);
        assert!(policy.quorum(&[first, first]).is_err());
        assert!(policy.quorum(&[first, guardian(99)]).is_err());
        let mut first_lamports = 0; let mut first_data = [];
        let mut second_lamports = 0; let mut second_data = [];
        let mut foreign_lamports = 0; let mut foreign_data = [];
        let foreign = guardian(99); let system = System::id();
        let first_info = AccountInfo::new(&first, true, false, &mut first_lamports, &mut first_data, &system, false);
        let second_info = AccountInfo::new(&second, false, false, &mut second_lamports, &mut second_data, &system, false);
        let foreign_info = AccountInfo::new(&foreign, true, false, &mut foreign_lamports, &mut foreign_data, &system, false);
        let native_keys = |infos: &[AccountInfo]| guardian_keys(infos, None, &policy, |info| {
            require!(info.is_signer && info.key().is_on_curve(), RegistryError::RecoveryQuorum);
            Ok(())
        });
        assert!(native_keys(&[first_info.clone(), first_info.clone()]).is_err());
        assert!(native_keys(&[first_info.clone(), second_info]).is_err());
        assert!(native_keys(&[first_info.clone(), foreign_info]).is_err());
        assert_eq!(native_keys(&[first_info]).unwrap(), vec![first]);
    }

    #[test]
    fn only_one_instructions_sysvar_is_metadata_and_every_guardian_authenticates() {
        let policy = terms();
        let first = policy.guardians[0]; let second = policy.guardians[1];
        let system = System::id(); let unrelated = guardian(99);
        let mut first_lamports = 0; let mut first_data = [];
        let mut second_lamports = 0; let mut second_data = [];
        let mut sysvar_lamports = 0; let mut sysvar_data = [];
        let mut unrelated_lamports = 0; let mut unrelated_data = [];
        let first_info = AccountInfo::new(&first, true, false, &mut first_lamports, &mut first_data, &system, false);
        let second_info = AccountInfo::new(&second, true, false, &mut second_lamports, &mut second_data, &system, false);
        let sysvar_info = AccountInfo::new(&INSTRUCTIONS_ID, false, false,
            &mut sysvar_lamports, &mut sysvar_data, &system, false);
        let unrelated_info = AccountInfo::new(&unrelated, false, false,
            &mut unrelated_lamports, &mut unrelated_data, &system, false);
        let mut authenticated = Vec::new();
        let keys = guardian_keys(&[first_info.clone(), sysvar_info.clone(), second_info.clone()],
            None, &policy, |info| { authenticated.push(info.key()); Ok(()) }).unwrap();
        assert_eq!(keys, vec![first, second]);
        assert_eq!(authenticated, keys);
        assert!(guardian_keys(&[first_info.clone(), sysvar_info.clone(), sysvar_info], None,
            &policy, |_| Ok(())).is_err());
        assert!(guardian_keys(&[first_info.clone(), unrelated_info], None,
            &policy, |_| Ok(())).is_err());
        // Selection and a real native signature do not bypass the chosen
        // profile's authentication result (including canonical alias checks).
        assert!(guardian_keys(&[first_info, second_info], None, &policy,
            |_| err!(RegistryError::RecoveryQuorum)).is_err());
    }

    #[test]
    fn changing_guardians_requires_both_quorums_with_overlap_counted_once_per_policy() {
        let old = terms();
        let mut proposed = old.clone(); proposed.guardians[1] = guardian(13);
        let old_only = vec![old.guardians[0], old.guardians[1]];
        let new_only = vec![proposed.guardians[0], proposed.guardians[1]];
        assert!(old.quorum(&old_only).is_ok()); assert!(proposed.quorum(&old_only).is_err());
        assert!(old.quorum(&new_only).is_err()); assert!(proposed.quorum(&new_only).is_ok());
        let union = [old.guardians[0], old.guardians[1], proposed.guardians[1]];
        assert_eq!(old.quorum(&union).unwrap(), 3);
        assert_eq!(proposed.quorum(&union).unwrap(), 3);
    }

    #[test]
    fn policy_digest_binds_identity_revision_and_exact_guardian_order() {
        let policy = terms(); let principal = guardian(21); let domain = [3; 32];
        let digest = recovery_policy_digest(&domain, &principal, 7, &policy).unwrap();
        assert_ne!(digest, recovery_policy_digest(&[4; 32], &principal, 7, &policy).unwrap());
        assert_ne!(digest, recovery_policy_digest(&domain, &guardian(22), 7, &policy).unwrap());
        assert_ne!(digest, recovery_policy_digest(&domain, &principal, 8, &policy).unwrap());
        let mut swapped = policy.clone(); swapped.guardians.swap(0, 1);
        assert_ne!(digest, recovery_policy_digest(&domain, &principal, 7, &swapped).unwrap());
        let mut lowered = policy.clone(); lowered.threshold = 1;
        assert_ne!(digest, recovery_policy_digest(&domain, &principal, 7, &lowered).unwrap());
        let mut bytes = Vec::new(); policy.serialize(&mut bytes).unwrap();
        assert_eq!(bytes.len(), 162);
        assert_eq!(&bytes[..2], &[2, 2]);
        assert_eq!(&bytes[2..34], policy.guardians[0].as_ref());
    }

    #[test]
    fn recovery_request_digest_binds_every_mutable_authorization_field() {
        let principal = guardian(21); let domain = [3; 32];
        let original = RecoverPrincipalOwnerArgs { request_id: [4; 32], policy_revision: 7,
            recovery_sequence: 2, policy_digest: [5; 32], new_owner: guardian(22), expires_at: 123456 };
        let digest = owner_recovery_digest(&domain, &principal, &original).unwrap();
        let mut changed = original.clone(); changed.request_id[0] ^= 1;
        assert_ne!(digest, owner_recovery_digest(&domain, &principal, &changed).unwrap());
        changed = original.clone(); changed.policy_revision += 1;
        assert_ne!(digest, owner_recovery_digest(&domain, &principal, &changed).unwrap());
        changed = original.clone(); changed.recovery_sequence += 1;
        assert_ne!(digest, owner_recovery_digest(&domain, &principal, &changed).unwrap());
        changed = original.clone(); changed.policy_digest[0] ^= 1;
        assert_ne!(digest, owner_recovery_digest(&domain, &principal, &changed).unwrap());
        changed = original.clone(); changed.new_owner = guardian(23);
        assert_ne!(digest, owner_recovery_digest(&domain, &principal, &changed).unwrap());
        changed = original.clone(); changed.expires_at += 1;
        assert_ne!(digest, owner_recovery_digest(&domain, &principal, &changed).unwrap());
        assert_ne!(digest, owner_recovery_digest(&[4; 32], &principal, &original).unwrap());
        assert_ne!(digest, owner_recovery_digest(&domain, &guardian(24), &original).unwrap());
        let mut bytes = Vec::new(); original.serialize(&mut bytes).unwrap();
        assert_eq!(bytes.len(), 120);
        let configured = ConfigureRecoveryPolicyArgs { request_id: [4; 32], expected_revision: 7,
            authority_generation: 2, terms: terms() };
        let configuration_digest = recovery_configuration_digest(&domain, &principal, &guardian(22), &configured).unwrap();
        assert_ne!(digest, configuration_digest);
        assert_ne!(configuration_digest,
            recovery_configuration_digest(&domain, &principal, &guardian(23), &configured).unwrap());
        let mut another_generation = configured; another_generation.authority_generation += 1;
        assert_ne!(configuration_digest,
            recovery_configuration_digest(&domain, &principal, &guardian(22), &another_generation).unwrap());
    }
}
