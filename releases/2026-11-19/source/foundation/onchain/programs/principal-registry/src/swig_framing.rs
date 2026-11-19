//! Borrowed framing for the funding wallet's one-instruction SignV2 profile.
//!
//! Callers retain outer account checks, action-specific counts, the Core target,
//! typed arguments and authority. Body validation is separate so those callers
//! still check count bounds, then the Core target, then the exact body length.
//! Role lookup is deliberately lazy: framing does not take over suffix checks
//! that belong to Swig or Core. Coverage concerns outer positions, not keys.
use anchor_lang::prelude::*;
use anchor_lang::solana_program::instruction::AccountMeta;
use core::ops::RangeInclusive;

use crate::RegistryError;

pub(crate) struct CompactFrame<'a> {
    program_index: u8,
    indices: &'a [u8],
    length_and_body: &'a [u8],
}

impl<'a> CompactFrame<'a> {
    pub(crate) fn parse(data: &'a [u8], role_counts: RangeInclusive<usize>) -> Result<Self> {
        require!(data.len() >= 12 && data[..2] == 11u16.to_le_bytes() && data[4..8] == [0; 4],
            RegistryError::FundingAction);
        let len = u16::from_le_bytes(data[2..4].try_into().unwrap()) as usize;
        require!(data.len() == 8 + len + 1 && data[8 + len] == 2, RegistryError::FundingAction);
        let compact = &data[8..8 + len];
        require!(compact.len() >= 3 && compact[0] == 1, RegistryError::FundingAction);
        let count = compact[2] as usize;
        require!(role_counts.contains(&count) && compact.len() >= 5 + count, RegistryError::FundingAction);
        Ok(Self { program_index: compact[1], indices: &compact[3..3 + count],
            length_and_body: &compact[3 + count..] })
    }

    pub(crate) fn program_index(&self) -> usize { self.program_index as usize }

    pub(crate) fn role_count(&self) -> usize { self.indices.len() }

    pub(crate) fn inner_data(&self) -> Result<&'a [u8]> {
        // parse has already established the complete two-byte length prefix.
        let len = u16::from_le_bytes(self.length_and_body[..2].try_into().unwrap()) as usize;
        require!(len >= 8 && self.length_and_body.len() == 2 + len, RegistryError::FundingAction);
        Ok(&self.length_and_body[2..])
    }

    pub(crate) fn role<'m>(&self, accounts: &'m [AccountMeta], role: usize) -> Result<&'m AccountMeta> {
        self.indices.get(role).and_then(|index| accounts.get(*index as usize))
            .ok_or_else(|| error!(RegistryError::FundingAction))
    }

    /// Call only at the caller's existing coverage stage, after its outer bound
    /// of at most 256 positions. Repeated role indices remain ordered and valid;
    /// an unused duplicate outer position still fails this check.
    pub(crate) fn require_covered_outer_indices(&self, accounts: &[AccountMeta]) -> Result<()> {
        for (index, _) in accounts.iter().enumerate() {
            require!(index < 3 || index == self.program_index() || self.indices.contains(&(index as u8)),
                RegistryError::FundingAction);
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::{AuthorizeFundingWallet, FundingWalletBinding, Principal};

    pub(crate) fn funding_action<T>(result: Result<T>) -> Box<anchor_lang::error::AnchorError> {
        match result {
            Err(anchor_lang::error::Error::AnchorError(error)) => {
                assert_eq!(error.error_name, "FundingAction");
                assert_eq!(error.error_code_number, u32::from(RegistryError::FundingAction));
                error
            }
            Err(error) => panic!("expected FundingAction, got {error:?}"),
            Ok(_) => panic!("expected FundingAction, got success"),
        }
    }

    /// Only parser input accounts are constructed here; this does not execute
    /// account constraints, ProgramExec, Swig, Core, or wallet authorization.
    pub(crate) fn with_authorization_accounts<T>(check: impl FnOnce(&AuthorizeFundingWallet<'_>) -> T) -> T {
        let config = Pubkey::new_unique(); let wallet = Pubkey::new_unique();
        let principal_key = Pubkey::new_unique(); let binding_key = Pubkey::new_unique();
        let caller = Pubkey::new_unique();
        let instructions = pubkey!("Sysvar1nstructions1111111111111111111111111");
        let system = System::id();
        let principal = Principal { deposit_payer: caller, version: dexter_contracts::SCHEMA_VERSION, bump: 0,
            principal_id: [1; 32], domain: [2; 32], kind: 1, governing_policy: [0; 32],
            seed_owner: caller, owner: caller, authority_generation: 1, approvals: 0 };
        let binding = FundingWalletBinding { deposit_payer: Pubkey::default(), version: dexter_contracts::SCHEMA_VERSION, bump: 0,
            principal: principal_key, config, wallet, wallet_id: [3; 32], graph_hash: [4; 32], nonce: 0 };
        let mut principal_data = Vec::new(); principal.try_serialize(&mut principal_data).unwrap();
        let mut binding_data = Vec::new(); binding.try_serialize(&mut binding_data).unwrap();
        let mut config_data = []; let mut wallet_data = []; let mut caller_data = []; let mut instructions_data = [];
        let mut config_lamports = 1; let mut wallet_lamports = 1; let mut caller_lamports = 1;
        let mut principal_lamports = 1; let mut binding_lamports = 1; let mut instructions_lamports = 1;
        let config_info = AccountInfo::new(&config, false, true, &mut config_lamports, &mut config_data, &system, false);
        let wallet_info = AccountInfo::new(&wallet, false, true, &mut wallet_lamports, &mut wallet_data, &system, false);
        let principal_info = AccountInfo::new(&principal_key, false, false, &mut principal_lamports,
            &mut principal_data, &crate::ID, false);
        let binding_info = AccountInfo::new(&binding_key, false, true, &mut binding_lamports,
            &mut binding_data, &crate::ID, false);
        let caller_info = AccountInfo::new(&caller, true, true, &mut caller_lamports, &mut caller_data, &system, false);
        let instructions_info = AccountInfo::new(&instructions, false, false, &mut instructions_lamports,
            &mut instructions_data, &system, false);
        let accounts = AuthorizeFundingWallet {
            config: UncheckedAccount::try_from(&config_info), wallet: UncheckedAccount::try_from(&wallet_info),
            binding: Box::new(Account::try_from(&binding_info).unwrap()),
            principal: Box::new(Account::try_from(&principal_info).unwrap()),
            caller: Signer::try_from(&caller_info).unwrap(), instructions: UncheckedAccount::try_from(&instructions_info),
        };
        check(&accounts)
    }

    pub(crate) fn framed_data(indices: &[u8], inner: &[u8]) -> Vec<u8> {
        let mut compact = vec![1, 3, indices.len() as u8];
        compact.extend_from_slice(indices);
        compact.extend_from_slice(&(inner.len() as u16).to_le_bytes());
        compact.extend_from_slice(inner);
        let mut data = Vec::from(11u16.to_le_bytes());
        data.extend_from_slice(&(compact.len() as u16).to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes()); data.extend_from_slice(&compact); data.push(2);
        data
    }

    pub(crate) fn control_metas(accounts: &AuthorizeFundingWallet) -> Vec<AccountMeta> {
        vec![AccountMeta::new(accounts.config.key(), false), AccountMeta::new(accounts.wallet.key(), false),
            AccountMeta::new_readonly(accounts.instructions.key(), false),
            AccountMeta::new_readonly(dexter_contracts::CORE_ID, false)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::test_support::funding_action;
    use serde_json::Value;

    fn bytes(hex: &str) -> Vec<u8> {
        assert_eq!(hex.len() % 2, 0);
        (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn shared_vectors_preserve_framing_stages_ordered_roles_and_exact_body_bytes() {
        let fixture: Value = serde_json::from_str(include_str!("../../../tests/swig_framing_vectors.json")).unwrap();
        assert_eq!(fixture["schema"], "dexter.swig-framing-v1");
        let vectors = fixture["vectors"].as_array().unwrap();
        assert_eq!(vectors.len(), 37);
        for vector in vectors {
            let name = vector["name"].as_str().unwrap();
            let data = bytes(vector["data_hex"].as_str().unwrap());
            let bounds = &vector["role_count_bounds"];
            let result = CompactFrame::parse(&data, bounds[0].as_u64().unwrap() as usize..=bounds[1].as_u64().unwrap() as usize);
            let expected = &vector["expected"];
            if expected["frame"] == "reject" {
                funding_action(result);
                continue;
            }
            let frame = result.unwrap_or_else(|error| panic!("{name}: unexpected frame refusal: {error:?}"));
            assert_eq!(frame.program_index(), expected["program_index"].as_u64().unwrap() as usize, "{name}");
            let indices: Vec<u8> = expected["role_indices"].as_array().unwrap().iter()
                .map(|value| value.as_u64().unwrap() as u8).collect();
            assert_eq!(frame.indices, indices, "{name}");
            assert_eq!(frame.role_count(), indices.len(), "{name}");
            assert_eq!(frame.indices.as_ptr(), data[11..].as_ptr(), "{name}: indices must be borrowed");
            if expected["body"] == "reject" {
                funding_action(frame.inner_data());
            } else {
                let inner = frame.inner_data().unwrap_or_else(|error| panic!("{name}: unexpected body refusal: {error:?}"));
                assert_eq!(inner, bytes(expected["inner_data_hex"].as_str().unwrap()), "{name}");
                assert_eq!(inner.as_ptr(), data[13 + indices.len()..].as_ptr(), "{name}: body must be borrowed");
            }
            if let Some(count) = vector["outer_account_count"].as_u64() {
                let mut accounts: Vec<_> = (0..count).map(|index| {
                    let mut key = [0; 32]; key[..8].copy_from_slice(&index.to_le_bytes());
                    AccountMeta::new_readonly(Pubkey::new_from_array(key), false)
                }).collect();
                if let Some(pairs) = vector["equal_outer_keys"].as_array() {
                    for pair in pairs {
                        let from = pair[0].as_u64().unwrap() as usize;
                        let to = pair[1].as_u64().unwrap() as usize;
                        accounts[to].pubkey = accounts[from].pubkey;
                    }
                }
                let coverage = frame.require_covered_outer_indices(&accounts);
                if vector["coverage"] == "accept" {
                    coverage.unwrap_or_else(|error| panic!("{name}: unexpected coverage refusal: {error:?}"));
                } else {
                    funding_action(coverage);
                }
                if let Some(probes) = vector["role_probes"].as_array() {
                    for probe in probes {
                        let result = frame.role(&accounts, probe["role"].as_u64().unwrap() as usize);
                        if let Some(index) = probe["outer_index"].as_u64() {
                            assert!(core::ptr::eq(result.unwrap(), &accounts[index as usize]), "{name}: exact outer position");
                        } else {
                            assert_eq!(probe["error"], "FundingAction", "{name}");
                            funding_action(result);
                        }
                    }
                }
            }
        }
    }
}
