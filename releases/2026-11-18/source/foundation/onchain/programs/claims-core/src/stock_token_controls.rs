//! Bounded opt-in parser for actual issuer-controlled transparent stock tokens.
//! Active hooks, confidential token accounts and transfer-fee combinations are
//! deliberately outside this qualified profile. The issuer retains its powers.
use anchor_lang::prelude::*;
use anchor_spl::token_2022::spl_token_2022::{self,
    extension::{BaseStateWithExtensions, ExtensionType, StateWithExtensions,
        confidential_transfer::ConfidentialTransferMint,
        default_account_state::DefaultAccountState,
        metadata_pointer::MetadataPointer,
        pausable::PausableConfig,
        permanent_delegate::PermanentDelegate,
        scaled_ui_amount::ScaledUiAmountConfig,
        transfer_hook::TransferHook},
    state::{AccountState, Mint},
};
use dexter_contracts::token_controls::{StockControlTerms, STOCK_CONTROL_VERSION,
    TRANSPARENT_ISSUER_STOCK, stock_control_digest, valid_multiplier_bits};
use crate::{CoreError, wallet_tokens::MintSnapshot};

/// Compute the complete current approved-control shape. This does not infer an
/// issuer or eligibility from metadata, and grants no transfer authority.
#[inline(never)]
pub fn read_control_terms(info: &AccountInfo, program: Pubkey) -> Result<StockControlTerms> {
    require_keys_eq!(program, spl_token_2022::ID, CoreError::FundingTokenRouting);
    require_keys_eq!(*info.owner, program, CoreError::FundingTokenRouting);
    require!(!info.executable, CoreError::FundingTokenRouting);
    let bytes = info.try_borrow_data()?;
    let state = StateWithExtensions::<Mint>::unpack(&bytes)
        .map_err(|_| error!(CoreError::FundingTokenRouting))?;
    let extensions = state.get_extension_types()
        .map_err(|_| error!(CoreError::FundingCashControl))?;
    let mut bitmap = 0_u64;
    for (index, extension) in extensions.iter().enumerate() {
        require!(!extensions[..index].contains(extension), CoreError::FundingCashControl);
        match extension {
            ExtensionType::MetadataPointer | ExtensionType::TokenMetadata
            | ExtensionType::PermanentDelegate | ExtensionType::DefaultAccountState
            | ExtensionType::ScaledUiAmount | ExtensionType::Pausable
            | ExtensionType::ConfidentialTransferMint | ExtensionType::TransferHook => {
                bitmap |= 1_u64 << (*extension as u16);
            }
            _ => return err!(CoreError::FundingCashControl),
        }
    }
    let mandatory = (1_u64 << (ExtensionType::MetadataPointer as u16))
        | (1_u64 << (ExtensionType::TokenMetadata as u16))
        | (1_u64 << (ExtensionType::DefaultAccountState as u16))
        | (1_u64 << (ExtensionType::ScaledUiAmount as u16))
        | (1_u64 << (ExtensionType::Pausable as u16))
        | (1_u64 << (ExtensionType::ConfidentialTransferMint as u16))
        | (1_u64 << (ExtensionType::TransferHook as u16));
    require!(bitmap == mandatory
        || bitmap == (mandatory | (1_u64 << (ExtensionType::PermanentDelegate as u16))),
        CoreError::FundingCashControl);
    let metadata = state.get_extension::<MetadataPointer>()
        .map_err(|_| error!(CoreError::FundingCashControl))?;
    require!(Option::<Pubkey>::from(metadata.metadata_address) == Some(info.key()),
        CoreError::FundingCashControl);
    let default = state.get_extension::<DefaultAccountState>()
        .map_err(|_| error!(CoreError::FundingCashControl))?;
    require!(default.state == AccountState::Initialized as u8, CoreError::FundingCashControl);
    let pausable = state.get_extension::<PausableConfig>()
        .map_err(|_| error!(CoreError::FundingCashControl))?;
    require!(!bool::from(pausable.paused), CoreError::FundingCashControl);
    let hook = state.get_extension::<TransferHook>()
        .map_err(|_| error!(CoreError::FundingCashControl))?;
    require!(Option::<Pubkey>::from(hook.program_id).is_none(), CoreError::FundingCashControl);
    let confidential = state.get_extension::<ConfidentialTransferMint>()
        .map_err(|_| error!(CoreError::FundingCashControl))?;
    let confidential_bytes = state.get_extension_bytes::<ConfidentialTransferMint>()
        .map_err(|_| error!(CoreError::FundingCashControl))?;
    require!(confidential_bytes.len() == 65 && !bool::from(confidential.auto_approve_new_accounts)
        && confidential_bytes[33..65] == [0; 32], CoreError::FundingCashControl);
    let scale = state.get_extension::<ScaledUiAmountConfig>()
        .map_err(|_| error!(CoreError::FundingCashControl))?;
    let multiplier_bits = u64::from_le_bytes(scale.multiplier.0);
    let new_multiplier_bits = u64::from_le_bytes(scale.new_multiplier.0);
    require!(valid_multiplier_bits(multiplier_bits) && valid_multiplier_bits(new_multiplier_bits),
        CoreError::FundingCashControl);
    let permanent_delegate = if extensions.contains(&ExtensionType::PermanentDelegate) {
        Option::<Pubkey>::from(state.get_extension::<PermanentDelegate>()
            .map_err(|_| error!(CoreError::FundingCashControl))?.delegate)
    } else { None };
    Ok(StockControlTerms {
        version: STOCK_CONTROL_VERSION, profile: TRANSPARENT_ISSUER_STOCK,
        mint: info.key(), token_program: program, decimals: state.base.decimals,
        extension_bitmap: bitmap, mint_authority: state.base.mint_authority.into(),
        freeze_authority: state.base.freeze_authority.into(), permanent_delegate,
        pause_authority: pausable.authority.into(), paused: false,
        default_account_state: default.state, hook_authority: hook.authority.into(),
        hook_program: None, confidential_authority: confidential.authority.into(),
        confidential_auto_approve: false, confidential_auditor: [0; 32],
        scale_authority: scale.authority.into(), multiplier_bits,
        new_multiplier_effective_timestamp: scale.new_multiplier_effective_timestamp.into(),
        new_multiplier_bits, metadata_pointer_authority: metadata.authority.into(),
        metadata_address: metadata.metadata_address.into(),
    })
}

#[inline(never)]
pub fn read_mint(info: &AccountInfo, program: Pubkey, expected_digest: [u8; 32]) -> Result<MintSnapshot> {
    require!(expected_digest != [0; 32], CoreError::FundingCashControl);
    let terms = read_control_terms(info, program)?;
    let controls_digest = stock_control_digest(&terms)?;
    require!(controls_digest == expected_digest, CoreError::FundingCashControl);
    Ok(MintSnapshot { mint: info.key(), token_program: program, decimals: terms.decimals,
        epoch_fee: None, disabled_hook: true, controls_digest, pausable_account: true })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wallet_tokens;

    const AAPL: Pubkey = pubkey!("XsbEhLAtcf6HdfpFZ5xEMdqW8nfAvcsP5bdudRLJzJp");
    const APPLE_BYTES: &[u8] = include_bytes!("../tests/fixtures/stock-controls/aaplx.mint.bin");
    const APPLE_DIGEST: &[u8; 32] = include_bytes!("../tests/fixtures/stock-controls/aaplx.digest.bin");
    fn parse(key: Pubkey, raw: &mut [u8]) -> Result<StockControlTerms> {
        let mut lamports = 1;
        let info = AccountInfo::new(&key, false, false, &mut lamports, raw, &spl_token_2022::ID, false);
        read_control_terms(&info, spl_token_2022::ID)
    }
    fn tlv_offset(raw: &[u8], wanted: u16) -> usize {
        let mut cursor = 166;
        while cursor + 4 <= raw.len() {
            let kind = u16::from_le_bytes(raw[cursor..cursor+2].try_into().unwrap());
            let len = u16::from_le_bytes(raw[cursor+2..cursor+4].try_into().unwrap()) as usize;
            if kind == wanted { return cursor + 4; }
            cursor += 4 + len;
        }
        panic!("missing extension {wanted}")
    }

    #[test]
    fn captured_actual_issuer_mints_match_independent_encoding_and_digest() {
        let cases: &[(Pubkey, &[u8], &[u8], &[u8; 32])] = &[
            (AAPL, APPLE_BYTES, include_bytes!("../tests/fixtures/stock-controls/aaplx.control.bin"), APPLE_DIGEST),
            (pubkey!("Xsc9qvGR1efVDFGLrVsmkzv3qi45LTBjeUKSPmx9qEh"),
                include_bytes!("../tests/fixtures/stock-controls/nvdax.mint.bin"),
                include_bytes!("../tests/fixtures/stock-controls/nvdax.control.bin"),
                include_bytes!("../tests/fixtures/stock-controls/nvdax.digest.bin")),
            (pubkey!("123mYEnRLM2LLYsJW3K6oyYh8uP1fngj732iG638ondo"),
                include_bytes!("../tests/fixtures/stock-controls/aaplon.mint.bin"),
                include_bytes!("../tests/fixtures/stock-controls/aaplon.control.bin"),
                include_bytes!("../tests/fixtures/stock-controls/aaplon.digest.bin")),
            (pubkey!("gEGtLTPNQ7jcg25zTetkbmF7teoDLcrfTnQfmn2ondo"),
                include_bytes!("../tests/fixtures/stock-controls/nvdaon.mint.bin"),
                include_bytes!("../tests/fixtures/stock-controls/nvdaon.control.bin"),
                include_bytes!("../tests/fixtures/stock-controls/nvdaon.digest.bin")),
        ];
        for (key, bytes, expected, digest) in cases {
            let mut raw = bytes.to_vec();
            let terms = parse(*key, &mut raw).unwrap();
            let mut encoded = Vec::new(); terms.serialize(&mut encoded).unwrap();
            assert_eq!(encoded.as_slice(), *expected);
            assert_eq!(&stock_control_digest(&terms).unwrap(), *digest);
            let mut lamports = 1;
            let info = AccountInfo::new(key, false, false, &mut lamports, &mut raw, &spl_token_2022::ID, false);
            assert!(wallet_tokens::read_mint(&info, spl_token_2022::ID, 0).is_err());
            assert!(wallet_tokens::read_mint_with_controls(&info, spl_token_2022::ID, 0, **digest).is_ok());
            assert!(wallet_tokens::read_mint_with_controls(&info, spl_token_2022::ID, 0, [99;32]).is_err());
            assert!(wallet_tokens::read_mint_with_controls(&info, spl_token_2022::ID, 0, [0;32]).is_err());
        }
    }

    #[test]
    fn issuer_state_and_authority_drift_require_new_explicit_consent() {
        for (kind, offset, value) in [(26,32,1), (14,32,1), (6,0,2), (4,32,1), (4,33,1)] {
            let mut raw = APPLE_BYTES.to_vec();
            let position = tlv_offset(&raw, kind) + offset; raw[position] = value;
            assert!(parse(AAPL, &mut raw).is_err(), "unsupported control kind {kind} offset {offset}");
        }
        let mut bad_scale = APPLE_BYTES.to_vec(); let position = tlv_offset(&bad_scale,25) + 32;
        bad_scale[position..position+8].copy_from_slice(&f64::NAN.to_bits().to_le_bytes());
        assert!(parse(AAPL,&mut bad_scale).is_err());
        for (kind, offset) in [(26,0), (14,0), (12,0), (25,0), (25,40)] {
            let mut raw = APPLE_BYTES.to_vec(); let position = tlv_offset(&raw,kind) + offset;
            raw[position] ^= 1;
            let terms = parse(AAPL,&mut raw).unwrap();
            assert_ne!(stock_control_digest(&terms).unwrap(), *APPLE_DIGEST);
        }
        let mut supply = APPLE_BYTES.to_vec(); supply[36] ^= 1;
        assert_eq!(stock_control_digest(&parse(AAPL,&mut supply).unwrap()).unwrap(), *APPLE_DIGEST);
        let mut duplicate = APPLE_BYTES.to_vec();
        duplicate.extend_from_slice(&[12,0,32,0]); duplicate.extend_from_slice(&[1;32]);
        assert!(parse(AAPL,&mut duplicate).is_err());
    }

    #[test]
    fn actual_pausable_stock_accounts_require_transparent_initialized_matching_extensions() {
        let mut mint_bytes = APPLE_BYTES.to_vec(); let mut lamports = 1;
        let mint_info = AccountInfo::new(&AAPL,false,false,&mut lamports,&mut mint_bytes,&spl_token_2022::ID,false);
        let mint = read_mint(&mint_info,spl_token_2022::ID,*APPLE_DIGEST).unwrap();
        let key = pubkey!("HgEdPsXdbi15x94DUVsbrvdf6Fc8Wdp5PbaDT9JTo8bN");
        let vault = include_bytes!("../tests/fixtures/stock-controls/aaplx-vault.bin");
        for variant in 0..4 {
            let mut raw = vault.to_vec();
            if variant == 1 { raw[108] = AccountState::Frozen as u8; }
            if variant == 2 { raw.extend_from_slice(&[5,0,1,0,0]); }
            if variant == 3 { let pos=tlv_offset(&raw,15);raw[pos]=1; }
            let mut cash = 1;
            let info=AccountInfo::new(&key,false,false,&mut cash,&mut raw,&spl_token_2022::ID,false);
            assert_eq!(wallet_tokens::read_account(&info,&mint).is_ok(),variant==0);
        }
    }
}
