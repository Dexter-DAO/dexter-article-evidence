//! Owner-approved market and issuer constraints, separate from replaceable quotes.
//! The existing economic request binds this immutable policy through adapter_pool.
use anchor_lang::prelude::*;
use crate::{ContractError, CORE_ID};
use crate::funded::financial_digest_fixed;
use crate::wallet_operation::{WalletSpendRequest, JUPITER_ID, TOKEN_ID, TOKEN_2022_ID};
use crate::spending::supported_token_program;

pub const ROUTE_PUMP_CURVE: u8 = 3;
pub const ROUTE_PUMP_AMM: u8 = 4;
pub const ROUTE_JUPITER_CONTROLLED: u8 = 5;
pub const ROUTE_JUPITER_PUMP: u8 = 6;
pub const NATIVE_MINT: Pubkey = pubkey!("So11111111111111111111111111111111111111112");
pub const PUMP_ID: Pubkey = pubkey!("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P");
pub const PUMP_AMM_ID: Pubkey = pubkey!("pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA");
pub const TRADING_POLICY_VERSION: u16 = 1;
pub const TRADING_DETAILS_VERSION: u16 = 1;
pub const MAX_INTERMEDIATES: usize = 3;

pub fn policy_route(profile: u8) -> bool {
    matches!(profile, ROUTE_PUMP_CURVE | ROUTE_PUMP_AMM | ROUTE_JUPITER_CONTROLLED | ROUTE_JUPITER_PUMP)
}
pub fn route_program(profile: u8) -> Option<Pubkey> {
    match profile { ROUTE_PUMP_CURVE => Some(PUMP_ID), ROUTE_PUMP_AMM => Some(PUMP_AMM_ID),
        ROUTE_JUPITER_CONTROLLED => Some(JUPITER_ID), ROUTE_JUPITER_PUMP => Some(CORE_ID), _ => None }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct WalletTradingPolicyTerms {
    pub binding: Pubkey,
    pub authority_generation: u64,
    pub route_profile: u8,
    pub input_mint: Pubkey,
    pub input_token_program: Pubkey,
    pub output_mint: Pubkey,
    pub output_token_program: Pubkey,
    /// Zero selects the ordinary strict token profile. Nonzero is exact consent
    /// to the issuer controls reconstructed from current raw mint extensions.
    pub input_control_digest: [u8; 32],
    pub output_control_digest: [u8; 32],
    /// Conservative sum of admitted per-hop configured venue rates, in bps.
    /// This is separate from endpoint Token-2022 withholding ceilings.
    pub maximum_venue_fee_bps: u16,
    pub intermediate_count: u8,
    pub intermediate_mints: [Pubkey; MAX_INTERMEDIATES],
    pub intermediate_token_programs: [Pubkey; MAX_INTERMEDIATES],
}
impl WalletTradingPolicyTerms {
    pub fn validate(&self) -> Result<()> {
        let zero = Pubkey::default();
        require!(self.binding != zero && self.authority_generation > 0 && policy_route(self.route_profile)
            && self.input_mint != zero && self.output_mint != zero && self.input_mint != self.output_mint
            && supported_token_program(self.input_token_program) && supported_token_program(self.output_token_program)
            && self.maximum_venue_fee_bps <= 10_000 && self.intermediate_count as usize <= MAX_INTERMEDIATES,
            ContractError::WrongAccountType);
        if self.route_profile != ROUTE_JUPITER_CONTROLLED {
            require!(self.input_control_digest == [0; 32] && self.output_control_digest == [0; 32],
                ContractError::WrongAccountType);
        } else {
            require!(self.intermediate_count == 0
                && (self.input_control_digest == [0; 32] || self.input_token_program == TOKEN_2022_ID)
                && (self.output_control_digest == [0; 32] || self.output_token_program == TOKEN_2022_ID),
                ContractError::WrongAccountType);
        }
        if self.route_profile == ROUTE_PUMP_CURVE { require!(self.intermediate_count == 0, ContractError::WrongAccountType); }
        for i in 0..MAX_INTERMEDIATES {
            if i < self.intermediate_count as usize {
                require!(self.intermediate_mints[i] != zero && self.intermediate_mints[i] != self.input_mint
                    && self.intermediate_mints[i] != self.output_mint
                    && !self.intermediate_mints[..i].contains(&self.intermediate_mints[i])
                    && supported_token_program(self.intermediate_token_programs[i]), ContractError::WrongAccountType);
            } else { require!(self.intermediate_mints[i] == zero && self.intermediate_token_programs[i] == zero,
                ContractError::WrongAccountType); }
        }
        if self.route_profile == ROUTE_JUPITER_PUMP {
            require!((0..self.intermediate_count as usize).any(|i|
                self.intermediate_mints[i] == NATIVE_MINT && self.intermediate_token_programs[i] == TOKEN_ID),
                ContractError::WrongAccountType);
        }
        Ok(())
    }
    pub fn matches_request(&self, request: &WalletSpendRequest) -> Result<()> {
        self.validate()?;
        require!(self.binding == request.binding && self.authority_generation == request.authority_generation
            && self.route_profile == request.route_profile && self.input_mint == request.input_mint
            && self.input_token_program == request.input_token_program && self.output_mint == request.output_mint
            && self.output_token_program == request.output_token_program
            && Some(request.adapter_program) == route_program(self.route_profile), ContractError::WrongAccountType);
        Ok(())
    }
}
pub fn trading_policy_address(principal: &Pubkey, id: &[u8; 32]) -> Pubkey {
    Pubkey::find_program_address(&[b"wallet-trading-policy", principal.as_ref(), id], &CORE_ID).0
}
pub fn trading_policy_digest(principal: &Pubkey, id: &[u8; 32], owner: &Pubkey,
    terms: &WalletTradingPolicyTerms) -> Result<[u8; 32]> {
    financial_digest_fixed::<{ 96 + WalletTradingPolicyTerms::INIT_SPACE }, _>(
        b"DEXTER:WALLET-TRADING-POLICY:1", &(principal, id, owner, terms))
}

/// Actual retained and immediately paid venue fees, each in this hop's quote
/// mint. LP fees stay in pool liquidity and are bounded by the configured rate;
/// no unlike-mint atom values are summed or booked as new income.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default, Debug, PartialEq, Eq, InitSpace)]
pub struct WalletTradingHop {
    pub program: Pubkey,
    pub market: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub base_token_program: Pubkey,
    pub quote_token_program: Pubkey,
    pub base_amount: u64,
    pub quote_amount: u64,
    pub retained_protocol_fee: u64,
    pub retained_creator_fee: u64,
    pub buyback_fee: u64,
    pub lp_fee_bps_bound: u16,
    pub buy: bool,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default, Debug, PartialEq, Eq, InitSpace)]
pub struct WalletTradingDetailsData {
    pub version: u16,
    pub request_digest: [u8; 32],
    pub policy: Pubkey,
    pub policy_digest: [u8; 32],
    pub input_control_digest: [u8; 32],
    pub output_control_digest: [u8; 32],
    pub payload_digest: [u8; 32],
    pub venue_fee_bps_bound: u16,
    pub hop_count: u8,
    pub hops: [WalletTradingHop; 4],
    /// Exact executed payloads and gross inputs for the two mixed-route legs.
    /// Other route profiles leave both fields zero.
    pub leg_payload_digests: [[u8; 32]; 2],
    pub leg_amounts: [u64; 2],
}

#[cfg(test)]
mod tests {
    use super::*;
    fn terms() -> WalletTradingPolicyTerms {
        WalletTradingPolicyTerms { binding: Pubkey::new_from_array([1;32]),authority_generation:1,
            route_profile:ROUTE_JUPITER_PUMP,input_mint:Pubkey::new_from_array([2;32]),
            input_token_program:TOKEN_ID,output_mint:Pubkey::new_from_array([3;32]),
            output_token_program:TOKEN_2022_ID,input_control_digest:[0;32],output_control_digest:[0;32],
            maximum_venue_fee_bps:300,intermediate_count:1,
            intermediate_mints:[NATIVE_MINT,Pubkey::default(),Pubkey::default()],
            intermediate_token_programs:[TOKEN_ID,Pubkey::default(),Pubkey::default()] }
    }
    #[test]
    fn mixed_policy_requires_explicit_distinct_native_boundary() {
        let approved=terms();assert!(approved.validate().is_ok());
        let mut missing=approved.clone();missing.intermediate_count=0;
        missing.intermediate_mints=[Pubkey::default();3];missing.intermediate_token_programs=[Pubkey::default();3];
        assert!(missing.validate().is_err());
        let mut wrong_program=approved.clone();wrong_program.intermediate_token_programs[0]=TOKEN_2022_ID;
        assert!(wrong_program.validate().is_err());
        let mut endpoint=approved.clone();endpoint.input_mint=NATIVE_MINT;
        assert!(endpoint.validate().is_err());
        let mut duplicate=approved.clone();duplicate.intermediate_count=2;
        duplicate.intermediate_mints[1]=NATIVE_MINT;duplicate.intermediate_token_programs[1]=TOKEN_ID;
        assert!(duplicate.validate().is_err());
        let mut hidden=approved.clone();hidden.intermediate_mints[2]=Pubkey::new_from_array([4;32]);
        assert!(hidden.validate().is_err());
    }
    #[test]
    fn issuer_consent_is_confined_to_the_controlled_jupiter_profile() {
        let mut controlled=terms();controlled.route_profile=ROUTE_JUPITER_CONTROLLED;
        controlled.intermediate_count=0;controlled.intermediate_mints=[Pubkey::default();3];
        controlled.intermediate_token_programs=[Pubkey::default();3];controlled.output_control_digest=[9;32];
        assert!(controlled.validate().is_ok());
        for route in [ROUTE_PUMP_CURVE,ROUTE_PUMP_AMM,ROUTE_JUPITER_PUMP] {
            let mut wrong=controlled.clone();wrong.route_profile=route;assert!(wrong.validate().is_err());
        }
        controlled.output_token_program=TOKEN_ID;assert!(controlled.validate().is_err());
    }
    #[test]
    fn approval_commits_fee_generation_owner_and_route_allowlist() {
        let approved=terms();let principal=Pubkey::new_from_array([5;32]);let owner=Pubkey::new_from_array([6;32]);
        let id=[7;32];let digest=trading_policy_digest(&principal,&id,&owner,&approved).unwrap();
        let mut changed=approved.clone();changed.maximum_venue_fee_bps+=1;
        assert_ne!(digest,trading_policy_digest(&principal,&id,&owner,&changed).unwrap());
        changed=approved.clone();changed.authority_generation+=1;
        assert_ne!(digest,trading_policy_digest(&principal,&id,&owner,&changed).unwrap());
        changed=approved.clone();changed.intermediate_count=2;
        changed.intermediate_mints[1]=Pubkey::new_from_array([8;32]);changed.intermediate_token_programs[1]=TOKEN_ID;
        assert!(changed.validate().is_ok());
        assert_ne!(digest,trading_policy_digest(&principal,&id,&owner,&changed).unwrap());
        assert_ne!(digest,trading_policy_digest(&principal,&id,&Pubkey::new_from_array([9;32]),&approved).unwrap());
        assert_eq!(WalletTradingPolicyTerms::INIT_SPACE,428);
        // Change 5: the operation link and self-hash are gone; the details ride in the trade event.
        assert_eq!(WalletTradingDetailsData::INIT_SPACE,1217);
    }
}
