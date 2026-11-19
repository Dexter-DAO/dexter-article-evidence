//! Delivery rules: a program that signs an evidence order's delivery report
//! under a fixed rule.
//!
//! A rule says who signs for delivery (up to five keys, `threshold` of whom
//! must agree), what a silent signer supports, who may object and until when,
//! who decides an objection and until when, and what happens if the decider
//! stays silent. A rule lives at `["rule", sha256("DEXTER:DELIVERY-RULE:1" ||
//! terms)]`; no instruction changes or closes it.
//!
//! An order names a rule through its billing policy: `source_identity` is the
//! rule's address and `source` is this program's address
//! `["delivery-source", rule]`. ClaimsCore accepts such an order only while this
//! program has no upgrade authority, and admits its report only when that
//! address signs, which only this program can do.
//!
//! Each order has one case at `["case", bill]`. Signers attest, objectors
//! object and the decider decides, each in its window; the windows follow the
//! order's work deadline. Once the rule's outcome is fixed, anyone settles: this
//! program reports the outcome into ClaimsCore by CPI. Nothing is reported while
//! the outcome can still change.
use anchor_lang::prelude::*;
use anchor_lang::solana_program::log::sol_log_data;
use claims_core::{program::ClaimsCore, EvidenceBill, DELIVERY_SOURCE_SEED};
use dexter_contracts::billing::{BillingPolicy, SourceReport, COMPLETED_WORK, FINAL_FOR_BILLING, MAX_BILLING_METRICS};
use solana_sha256_hasher::hashv;

declare_id!("EsR1knVgJG8NAYFjeGCYpTuvjf1DtwAU1ne5XBdcotq3");

pub const RULE_SEED: &[u8] = b"rule";
pub const CASE_SEED: &[u8] = b"case";
pub const RULE_DOMAIN: &[u8] = b"DEXTER:DELIVERY-RULE:1";
pub const OUTCOME_DOMAIN: &[u8] = b"DEXTER:DELIVERY-RULE-OUTCOME:1";
pub const MAX_SIGNERS: usize = 5;
pub const MAX_OBJECTORS: usize = 5;
/// The order's evidence deadline is at least this long after the rule's
/// decision deadline, so anyone has an hour to settle once the outcome is fixed.
pub const MIN_SETTLE_SECONDS: i64 = 3_600;
/// A rule with objectors gives its decider at least this long after objections
/// close, so an objection filed in the last second can still be decided.
pub const MIN_DECIDE_SECONDS: u32 = 3_600;
/// `signers_silent`: silence supports zero units. `decider_silent`: zero units.
pub const NOTHING: u8 = 0;
/// `signers_silent`: silence supports the policy's maximum units.
pub const PAID: u8 = 1;
/// `decider_silent`: the objection lapses; the case settles as if nobody objected.
pub const LAPSE: u8 = 1;
/// `Case::outcome` once settled: the signers' group, a decision, or an undecided objection.
pub const SIGNERS: u8 = 1;
pub const DECIDED: u8 = 2;
pub const DECIDER_SILENT: u8 = 3;

type Units = [u64; MAX_BILLING_METRICS];

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct RuleTerms {
    /// Delivery signers; slots past `signer_count` are zero. Signer i is bit i of a group.
    pub signers: [Pubkey; MAX_SIGNERS],
    pub signer_count: u8,
    /// The size of a group. A group supports, per metric, the smallest amount
    /// its members support; the highest-priced group is reported.
    pub threshold: u8,
    /// NOTHING or PAID: what a silent signer supports when fewer than `threshold` attested.
    pub signers_silent: u8,
    /// Signing closes this long after the order's work deadline.
    pub sign_seconds: u32,
    /// Keys that may object; slots past `objector_count` are zero.
    pub objectors: [Pubkey; MAX_OBJECTORS],
    pub objector_count: u8,
    /// Objections close this long after signing closes.
    pub object_seconds: u32,
    /// Decides an objection, after signing closes; required when there are objectors.
    pub decider: Pubkey,
    /// The decision closes this long after objections close; at least MIN_DECIDE_SECONDS with objectors.
    pub decide_seconds: u32,
    /// NOTHING or LAPSE: the outcome when an objection is not decided in time.
    pub decider_silent: u8,
}

#[account]
#[derive(InitSpace)]
pub struct Rule {
    pub terms: RuleTerms,
}

/// One order's facts under its rule. The deadlines are fixed when it opens.
#[account]
#[derive(InitSpace)]
pub struct Case {
    pub bill: Pubkey,
    pub rule: Pubkey,
    /// Paid the rent; receives it back when the case closes.
    pub opener: Pubkey,
    pub signing_closes: i64,
    pub objections_close: i64,
    pub decision_closes: i64,
    /// Bit i is set once signer i has attested `attestations[i]`.
    pub attested: u8,
    pub attestations: [[u64; MAX_BILLING_METRICS]; MAX_SIGNERS],
    pub objector: Option<Pubkey>,
    pub decision: Option<[u64; MAX_BILLING_METRICS]>,
    /// 0 until settled; then SIGNERS, DECIDED or DECIDER_SILENT.
    pub outcome: u8,
    /// The group whose units were reported (signer i = bit i); 0 when no group decided.
    pub group: u8,
    /// The units reported to ClaimsCore.
    pub units: [u64; MAX_BILLING_METRICS],
}

/// The rule's content hash; the rule lives at `["rule", rule_hash]`.
pub fn rule_hash(terms: &RuleTerms) -> [u8; 32] {
    let mut body = Vec::with_capacity(RuleTerms::INIT_SPACE);
    terms.serialize(&mut body).expect("writing to a Vec cannot fail");
    hashv(&[RULE_DOMAIN, &body]).to_bytes()
}

/// The first `count` keys are set and distinct; the rest are zero.
fn listed(keys: &[Pubkey], count: usize) -> bool {
    keys.iter().enumerate().all(|(i, k)|
        if i < count { *k != Pubkey::default() && !keys[..i].contains(k) } else { *k == Pubkey::default() })
}

impl RuleTerms {
    pub fn validate(&self) -> Result<()> {
        let (n, m) = (self.signer_count as usize, self.objector_count as usize);
        require!((1..=MAX_SIGNERS).contains(&n) && (1..=n).contains(&(self.threshold as usize))
            && m <= MAX_OBJECTORS && listed(&self.signers, n) && listed(&self.objectors, m)
            && (m == 0 || (self.decider != Pubkey::default() && self.decide_seconds >= MIN_DECIDE_SECONDS))
            && self.signers_silent <= PAID && self.decider_silent <= LAPSE, RuleError::InvalidRule);
        Ok(())
    }

    /// The signers' outcome. Each group of `threshold` signers supports, per metric,
    /// the smallest amount its members support. Once `threshold` signers attested,
    /// only attesting signers form groups. With fewer, every signer does, and a
    /// silent one supports the silence value. Groups are priced with the order's
    /// policy; the highest price wins and, among equal prices, the lowest mask
    /// (signer i = bit i).
    fn signers_outcome(&self, case: &Case, policy: &BillingPolicy) -> Result<(u8, Units)> {
        let silent = if self.signers_silent == PAID { policy.max_units } else { [0; MAX_BILLING_METRICS] };
        let n = self.signer_count as usize;
        let enough = case.attested.count_ones() >= self.threshold as u32;
        let mut best: Option<(u128, u8, Units)> = None;
        for mask in 1..(1u8 << n) {
            if mask.count_ones() != self.threshold as u32 || (enough && mask & !case.attested != 0) { continue; }
            let mut units = [u64::MAX; MAX_BILLING_METRICS];
            for i in (0..n).filter(|i| mask >> i & 1 == 1) {
                let member = if case.attested >> i & 1 == 1 { &case.attestations[i] } else { &silent };
                for (out, value) in units.iter_mut().zip(member) { *out = (*out).min(*value); }
            }
            let price = policy.supported_amount(&units).map_err(|_| error!(RuleError::UnitsOutsidePolicy))?;
            if best.map_or(true, |(top, _, _)| price > top) { best = Some((price, mask, units)); }
        }
        let (_, mask, units) = best.ok_or(error!(RuleError::InvalidRule))?;
        Ok((mask, units))
    }
}

/// A ClaimsCore EvidenceBill: owned by ClaimsCore, with its account discriminator.
fn read_bill(info: &AccountInfo) -> Result<Box<EvidenceBill>> {
    require!(*info.owner == claims_core::ID, RuleError::NotABill);
    let data = info.try_borrow_data()?;
    EvidenceBill::try_deserialize(&mut &data[..]).map(Box::new).map_err(|_| error!(RuleError::NotABill))
}

/// Units count only if the order's policy would admit them in a report.
fn check_units(bill: &EvidenceBill, units: &Units) -> Result<()> {
    bill.policy.supported_amount(units).map_err(|_| error!(RuleError::UnitsOutsidePolicy))?;
    Ok(())
}

#[program]
pub mod delivery_rules {
    use super::*;

    /// Create a rule at the address of its contents. No instruction changes or closes it.
    pub fn create_rule(ctx: Context<CreateRule>, terms: RuleTerms) -> Result<()> {
        terms.validate()?;
        ctx.accounts.rule.terms = terms;
        Ok(())
    }

    /// Open the case of a bill that names this rule. Anyone may open it and pays its rent.
    /// The order's evidence deadline must fall at least MIN_SETTLE_SECONDS after the
    /// rule's decision deadline, so the settled report can still reach ClaimsCore.
    pub fn open_case(ctx: Context<OpenCase>) -> Result<()> {
        let a = ctx.accounts;
        let bill = read_bill(&a.bill)?;
        let rule = a.rule.key();
        let (source, _) = Pubkey::find_program_address(&[DELIVERY_SOURCE_SEED, rule.as_ref()], &crate::ID);
        require!(bill.policy.source_identity == rule.to_bytes() && bill.policy.source == source, RuleError::RuleNotNamed);
        require!(bill.source_report.is_none() && !bill.finalized, RuleError::BillClosed);
        let t = &a.rule.terms;
        let after = |start: i64, seconds: i64| start.checked_add(seconds).ok_or(error!(RuleError::RuleOutlastsOrder));
        let signing_closes = after(bill.policy.work_until, t.sign_seconds as i64)?;
        let objections_close = after(signing_closes, t.object_seconds as i64)?;
        let decision_closes = after(objections_close, t.decide_seconds as i64)?;
        require!(after(decision_closes, MIN_SETTLE_SECONDS)? <= bill.policy.evidence_deadline, RuleError::RuleOutlastsOrder);
        a.case.set_inner(Case { bill: a.bill.key(), rule, opener: a.payer.key(), signing_closes, objections_close,
            decision_closes, attested: 0, attestations: [[0; MAX_BILLING_METRICS]; MAX_SIGNERS], objector: None,
            decision: None, outcome: 0, group: 0, units: [0; MAX_BILLING_METRICS] });
        Ok(())
    }

    /// One of the rule's signers reports the units delivered, once, before signing closes.
    pub fn attest(ctx: Context<Attest>, units: [u64; MAX_BILLING_METRICS]) -> Result<()> {
        let a = ctx.accounts;
        require!(a.case.outcome == 0, RuleError::AlreadySettled);
        let t = &a.rule.terms;
        let index = t.signers[..t.signer_count as usize].iter().position(|k| *k == a.signer.key())
            .ok_or(error!(RuleError::NotASigner))?;
        require!(a.case.attested >> index & 1 == 0, RuleError::AlreadyAttested);
        require!(Clock::get()?.unix_timestamp <= a.case.signing_closes, RuleError::SigningClosed);
        check_units(&*read_bill(&a.bill)?, &units)?;
        a.case.attestations[index] = units;
        a.case.attested |= 1 << index;
        Ok(())
    }

    /// One of the rule's objectors objects, any time before objections close. The first objection opens a dispute.
    pub fn object(ctx: Context<Object>) -> Result<()> {
        let a = ctx.accounts;
        require!(a.case.outcome == 0, RuleError::AlreadySettled);
        let t = &a.rule.terms;
        require!(t.objectors[..t.objector_count as usize].contains(&a.objector.key()), RuleError::NotAnObjector);
        require!(Clock::get()?.unix_timestamp <= a.case.objections_close, RuleError::ObjectionsClosed);
        require!(a.case.objector.is_none(), RuleError::AlreadyObjected);
        a.case.objector = Some(a.objector.key());
        Ok(())
    }

    /// The rule's decider decides the units of a disputed case, once, after signing
    /// closes (on the signers' complete reports) and before the decision closes.
    pub fn decide(ctx: Context<Decide>, units: [u64; MAX_BILLING_METRICS]) -> Result<()> {
        let a = ctx.accounts;
        require!(a.case.outcome == 0, RuleError::AlreadySettled);
        let decider = a.rule.terms.decider;
        require!(decider != Pubkey::default() && decider == a.decider.key(), RuleError::NotTheDecider);
        require!(a.case.objector.is_some(), RuleError::NoObjection);
        let now = Clock::get()?.unix_timestamp;
        require!(now > a.case.signing_closes, RuleError::SigningOpen);
        require!(now <= a.case.decision_closes, RuleError::DecisionClosed);
        require!(a.case.decision.is_none(), RuleError::AlreadyDecided);
        check_units(&*read_bill(&a.bill)?, &units)?;
        a.case.decision = Some(units);
        Ok(())
    }

    /// Anyone reports the rule's outcome into ClaimsCore, once it is fixed: after
    /// objections close with no objection, after a decision, or after the decision
    /// closes undecided. The case record is logged and hashed into the report.
    pub fn settle(ctx: Context<Settle>) -> Result<()> {
        let a = ctx.accounts;
        require!(a.case.outcome == 0, RuleError::AlreadySettled);
        let bill = read_bill(&a.bill)?;
        let now = Clock::get()?.unix_timestamp;
        let t = &a.rule.terms;
        let (outcome, group, units) = match (a.case.objector, a.case.decision) {
            (Some(_), Some(decided)) => (DECIDED, 0, decided),
            (Some(_), None) => {
                require!(now > a.case.decision_closes, RuleError::DecisionOpen);
                let (group, units) = if t.decider_silent == LAPSE { t.signers_outcome(&a.case, &bill.policy)? }
                    else { (0, [0; MAX_BILLING_METRICS]) };
                (DECIDER_SILENT, group, units)
            }
            (None, _) => {
                require!(now > a.case.objections_close, RuleError::ObjectionsOpen);
                let (group, units) = t.signers_outcome(&a.case, &bill.policy)?;
                (SIGNERS, group, units)
            }
        };
        a.case.outcome = outcome;
        a.case.group = group;
        a.case.units = units;
        // The case record at settlement: the same three segments are logged and hashed.
        let case = a.case.key();
        let state: &Case = &a.case;
        let mut record = Vec::with_capacity(Case::INIT_SPACE);
        state.serialize(&mut record).expect("writing to a Vec cannot fail");
        sol_log_data(&[OUTCOME_DOMAIN, case.as_ref(), &record]);
        let report = SourceReport { record_id: case.to_bytes(), request_hash: bill.policy.request_hash,
            started_at: bill.admitted_at, completed_at: now.min(bill.policy.work_until), units,
            result_hash: hashv(&[OUTCOME_DOMAIN, case.as_ref(), &record]).to_bytes(),
            finality: FINAL_FOR_BILLING, outcome: COMPLETED_WORK };
        let rule = a.rule.key();
        let seeds: &[&[u8]] = &[DELIVERY_SOURCE_SEED, rule.as_ref(), &[ctx.bumps.delivery_source]];
        claims_core::cpi::submit_program_bill_evidence(CpiContext::new_with_signer(a.claims_core.key(),
            claims_core::cpi::accounts::SubmitProgramBillRecord { bill: a.bill.to_account_info(),
                source: a.delivery_source.to_account_info(), rule_program: a.delivery_rules.to_account_info() },
            &[seeds]), report)
    }

    /// Anyone closes a case once its bill is finalized; the rent returns to whoever opened it.
    pub fn close_case(ctx: Context<CloseCase>) -> Result<()> {
        require!(read_bill(&ctx.accounts.bill)?.finalized, RuleError::BillNotFinal);
        Ok(())
    }
}

#[derive(Accounts)]
#[instruction(terms: RuleTerms)]
pub struct CreateRule<'info> {
    #[account(mut)] pub payer: Signer<'info>,
    #[account(init, payer = payer, space = 8 + Rule::INIT_SPACE, seeds = [RULE_SEED, &rule_hash(&terms)], bump)]
    pub rule: Box<Account<'info, Rule>>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct OpenCase<'info> {
    #[account(mut)] pub payer: Signer<'info>,
    pub rule: Box<Account<'info, Rule>>,
    /// CHECK: A ClaimsCore EvidenceBill naming this rule, read in `open_case`.
    pub bill: UncheckedAccount<'info>,
    #[account(init, payer = payer, space = 8 + Case::INIT_SPACE, seeds = [CASE_SEED, bill.key().as_ref()], bump)]
    pub case: Box<Account<'info, Case>>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Attest<'info> {
    pub signer: Signer<'info>,
    #[account(mut, has_one = rule, has_one = bill)] pub case: Box<Account<'info, Case>>,
    pub rule: Box<Account<'info, Rule>>,
    /// CHECK: The case's bill; its policy bounds the units.
    pub bill: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct Object<'info> {
    pub objector: Signer<'info>,
    #[account(mut, has_one = rule)] pub case: Box<Account<'info, Case>>,
    pub rule: Box<Account<'info, Rule>>,
}

#[derive(Accounts)]
pub struct Decide<'info> {
    pub decider: Signer<'info>,
    #[account(mut, has_one = rule, has_one = bill)] pub case: Box<Account<'info, Case>>,
    pub rule: Box<Account<'info, Rule>>,
    /// CHECK: The case's bill; its policy bounds the units.
    pub bill: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct Settle<'info> {
    #[account(mut, has_one = rule, has_one = bill)] pub case: Box<Account<'info, Case>>,
    pub rule: Box<Account<'info, Rule>>,
    /// CHECK: The case's bill; ClaimsCore admits the report into it.
    #[account(mut)] pub bill: UncheckedAccount<'info>,
    /// CHECK: This program's delivery-source address for the rule; it signs the report.
    #[account(seeds = [DELIVERY_SOURCE_SEED, rule.key().as_ref()], bump)]
    pub delivery_source: UncheckedAccount<'info>,
    pub delivery_rules: Program<'info, program::DeliveryRules>,
    pub claims_core: Program<'info, ClaimsCore>,
}

#[derive(Accounts)]
pub struct CloseCase<'info> {
    #[account(mut, close = opener, has_one = opener, has_one = bill)] pub case: Box<Account<'info, Case>>,
    /// CHECK: Receives the case's rent; must be the case's opener.
    #[account(mut)] pub opener: UncheckedAccount<'info>,
    /// CHECK: The case's bill, which must be finalized.
    pub bill: UncheckedAccount<'info>,
}

#[error_code]
pub enum RuleError {
    #[msg("A rule names 1 to 5 distinct signers, a threshold from 1 to their number, up to 5 distinct objectors with a decider and at least MIN_DECIDE_SECONDS to decide, and known silence outcomes")] InvalidRule,
    #[msg("The account is not a ClaimsCore evidence bill")] NotABill,
    #[msg("The bill does not name this rule's delivery-source address as its delivery signer")] RuleNotNamed,
    #[msg("The bill already holds its delivery report or is finalized")] BillClosed,
    #[msg("The order's evidence deadline is less than MIN_SETTLE_SECONDS after the rule's decision deadline")] RuleOutlastsOrder,
    #[msg("The key is not one of the rule's delivery signers")] NotASigner,
    #[msg("This signer has already attested on this case")] AlreadyAttested,
    #[msg("The rule's signing window has closed")] SigningClosed,
    #[msg("The units are outside what the order's policy admits")] UnitsOutsidePolicy,
    #[msg("The key is not one of the rule's objectors")] NotAnObjector,
    #[msg("The rule's objection window has closed")] ObjectionsClosed,
    #[msg("This case already has an objection")] AlreadyObjected,
    #[msg("The key is not the rule's decider")] NotTheDecider,
    #[msg("Nobody has objected on this case")] NoObjection,
    #[msg("The rule's decision window has closed")] DecisionClosed,
    #[msg("The decider has already decided this case")] AlreadyDecided,
    #[msg("The objection window is still open")] ObjectionsOpen,
    #[msg("The decider can still decide")] DecisionOpen,
    #[msg("This case is settled")] AlreadySettled,
    #[msg("The bill is not finalized")] BillNotFinal,
    #[msg("The decider decides after signing closes")] SigningOpen,
}
