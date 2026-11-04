//! Finite financed commitments, delivered usage and independent supplier collection.
use anchor_lang::prelude::*;
use crate::CoreError;
use anchor_spl::{associated_token, token::{Mint, Token, TokenAccount}};
use dexter_contracts::{self as contracts, plus, minus, AcceptArgs, StageArgs, TabTerms,
    CORE_ID, POOL_ID, REGISTRY_CONSUMER_SEED};
use dexter_contracts::commercial::{InterestState, InterestTerms, SellerFeeState, SellerFeeTerms,
    SponsorQuote, within_domain_assignment_policy};
use dexter_contracts::financing::{Asset, EarnedCollection, FINANCING_VERSION, PrincipalAmounts};
use dexter_contracts::funded::*;
use principal_registry::{self, Principal, Actor, Mandate, Budget, Reservation, program::PrincipalRegistry};
use crate::funding_state::*;
use crate::funding_journal::{journal, inc, dec};
use crate::seller_fees::SellerFeeAgreement;

pub fn operation_id(tag:&[u8],scope:Pubkey,raw:&[u8;32])->[u8;32] {
    solana_sha256_hasher::hashv(&[b"DEXTER:FUNDING-OPERATION:1",CORE_ID.as_ref(),tag,scope.as_ref(),raw]).to_bytes()
}

#[derive(Accounts)]
#[instruction(principal:Pubkey)]
pub struct InitializeFinanceBook<'info> {
    #[account(mut)] pub payer:Signer<'info>,
    #[account(init_if_needed,payer=payer,space=8+FinanceBook::INIT_SPACE,
        seeds=[b"financial-book",principal.as_ref(),mint.key().as_ref()],bump)]
    pub book:Box<Account<'info,FinanceBook>>,
    pub mint:Box<Account<'info,Mint>>,
    pub system_program:Program<'info,System>,
}

#[derive(Accounts)]
#[instruction(request_id:[u8;32])]
pub struct CreateFinancierCustody<'info> {
    #[account(mut)] pub owner:Signer<'info>,
    pub financier:Box<Account<'info,Principal>>,
    /// CHECK: Registry binding and full graph for wallet profile; Core sentinel for escrow.
    pub wallet_binding:UncheckedAccount<'info>,
    /// CHECK: Retained Swig configuration authenticated by Registry's strict reader.
    pub wallet_config:UncheckedAccount<'info>,
    /// CHECK: Canonical Swig wallet or Core escrow authority; checked in handler.
    pub wallet_authority:UncheckedAccount<'info>,
    pub custody_token:Box<Account<'info,TokenAccount>>,
    #[account(init_if_needed,payer=owner,space=8+FinancierCustody::INIT_SPACE,
        seeds=[b"custody-reserve",custody_token.key().as_ref()],bump)]
    pub custody:Box<Account<'info,FinancierCustody>>,
    #[account(init_if_needed,payer=owner,space=8+FinanceReceipt::INIT_SPACE,
        seeds=[b"finance-operation",financier.key().as_ref(),&operation_id(b"custody",financier.key(),&request_id)],bump)]
    pub receipt:Box<Account<'info,FinanceReceipt>>,
    pub mint:Box<Account<'info,Mint>>,
    /// CHECK: Exact Pool id; new allocation requires executable current projection.
    #[account(address=POOL_ID)] pub pool_program:UncheckedAccount<'info>,
    pub system_program:Program<'info,System>,
}

#[derive(Accounts)]
#[instruction(facility_id:[u8;32])]
pub struct OpenFundingFacility<'info> {
    #[account(mut)] pub owner:Signer<'info>,
    pub financier:Box<Account<'info,Principal>>,
    pub issuer_owner:Signer<'info>,
    pub issuer:Box<Account<'info,Principal>>,
    pub custody:Box<Account<'info,FinancierCustody>>,
    #[account(init_if_needed,payer=owner,space=8+FundingFacility::INIT_SPACE,
        seeds=[b"funding-facility",financier.key().as_ref(),&facility_id],bump)]
    pub facility:Box<Account<'info,FundingFacility>>,
    pub protocol_custody:Option<Box<Account<'info,FinancierCustody>>>,
    pub system_program:Program<'info,System>,
}

#[derive(Accounts)]
#[instruction(line_id:[u8;32])]
pub struct OpenCustomerCreditLine<'info> {
    #[account(mut)] pub owner:Signer<'info>,
    pub company:Box<Account<'info,Principal>>,
    pub issuer_owner:Signer<'info>,
    pub issuer:Box<Account<'info,Principal>>,
    pub issuer_custody:Box<Account<'info,FinancierCustody>>,
    pub facility:Box<Account<'info,FundingFacility>>,
    #[account(init_if_needed,payer=owner,space=8+CustomerCreditLine::INIT_SPACE,
        seeds=[b"credit-line",company.key().as_ref(),&line_id],bump)]
    pub credit_line:Box<Account<'info,CustomerCreditLine>>,
    pub protocol_custody:Option<Box<Account<'info,FinancierCustody>>>,
    pub system_program:Program<'info,System>,
}

#[derive(Accounts)]
#[instruction(request_id:[u8;32])]
pub struct ManageFundingFacility<'info> {
    #[account(mut)] pub owner:Signer<'info>,
    pub financier:Box<Account<'info,Principal>>,
    #[account(mut)] pub facility:Box<Account<'info,FundingFacility>>,
    #[account(init_if_needed,payer=owner,space=8+FacilityPolicyReceipt::INIT_SPACE,
        seeds=[b"facility-policy",facility.key().as_ref(),&operation_id(b"facility-policy",financier.key(),&request_id)],bump)]
    pub receipt:Box<Account<'info,FacilityPolicyReceipt>>,
    pub system_program:Program<'info,System>,
}

#[derive(Accounts)]
#[instruction(request_id:[u8;32],terms:TabTerms)]
pub struct AcceptFinancedTab<'info> {
    #[account(mut)] pub payer:Signer<'info>,
    pub principal:Box<Account<'info,Principal>>,
    pub actor:Box<Account<'info,Actor>>,
    pub credential:Signer<'info>,
    pub mandate:Box<Account<'info,Mandate>>,
    #[account(mut)] pub budget:Box<Account<'info,Budget>>,
    /// CHECK: Registry creates and authenticates the canonical reservation atomically.
    #[account(mut)] pub reservation:UncheckedAccount<'info>,
    /// CHECK: Registry creates its domain-separated operation record atomically.
    #[account(mut)] pub registry_operation:UncheckedAccount<'info>,
    /// CHECK: Core's canonical Registry consumer signer.
    #[account(seeds=[REGISTRY_CONSUMER_SEED],bump)] pub registry_authority:UncheckedAccount<'info>,
    pub registry_program:Program<'info,PrincipalRegistry>,
    pub issuer_owner:Signer<'info>,
    pub issuer:Box<Account<'info,Principal>>,
    pub supplier:Signer<'info>,
    pub supplier_principal:Box<Account<'info,Principal>>,
    pub fee_custody:Option<Box<Account<'info,FinancierCustody>>>,
    /// Shared supplier-approved lifetime capacity; omitted only when this Tab cannot charge a fee.
    #[account(mut)] pub seller_fee_agreement:Option<Box<Account<'info,SellerFeeAgreement>>>,
    #[account(mut)] pub credit_line:Box<Account<'info,CustomerCreditLine>>,
    #[account(mut)] pub facility:Box<Account<'info,FundingFacility>>,
    #[account(mut)] pub custody:Box<Account<'info,FinancierCustody>>,
    pub custody_token:Box<Account<'info,TokenAccount>>,
    /// CHECK: Actual wallet graph is authenticated for new cash risk.
    pub wallet_binding:UncheckedAccount<'info>,
    /// CHECK: Actual retained Swig configuration; escrow uses Core sentinel.
    pub wallet_config:UncheckedAccount<'info>,
    /// CHECK: Actual wallet or escrow authority, bound to custody.
    pub wallet_authority:UncheckedAccount<'info>,
    #[account(init_if_needed,payer=payer,space=8+FundedClaim::INIT_SPACE,
        seeds=[b"claim",principal.key().as_ref(),&terms.tab_id],bump)]
    pub claim:Box<Account<'info,FundedClaim>>,
    #[account(init_if_needed,payer=payer,space=8+FinanceReceipt::INIT_SPACE,
        seeds=[b"finance-operation",principal.key().as_ref(),&operation_id(b"accept",credential.key(),&request_id)],bump)]
    pub receipt:Box<Account<'info,FinanceReceipt>>,
    pub mint:Box<Account<'info,Mint>>,
    pub system_program:Program<'info,System>,
    /// CHECK: New risk requires this program and every affected projection.
    #[account(address=POOL_ID)] pub pool_program:UncheckedAccount<'info>,
}

#[derive(Accounts)]
#[instruction(payment_id:[u8;32],namespace:[u8;32],run_id:[u8;32])]
pub struct EarnFinancedRun<'info> {
    #[account(mut)] pub payer:Signer<'info>,
    pub supplier:Signer<'info>,
    pub supplier_principal:Box<Account<'info,Principal>>,
    pub principal:Box<Account<'info,Principal>>,
    #[account(mut)] pub budget:Box<Account<'info,Budget>>,
    #[account(mut)] pub reservation:Box<Account<'info,Reservation>>,
    /// CHECK: Registry authenticates its completed-run receipt domain.
    #[account(mut)] pub registry_operation:UncheckedAccount<'info>,
    /// CHECK: Canonical Core-to-Registry authority.
    #[account(seeds=[REGISTRY_CONSUMER_SEED],bump)] pub registry_authority:UncheckedAccount<'info>,
    pub registry_program:Program<'info,PrincipalRegistry>,
    #[account(mut)] pub line:Box<Account<'info,CustomerCreditLine>>,
    #[account(mut)] pub claim:Box<Account<'info,FundedClaim>>,
    #[account(mut)] pub custody:Box<Account<'info,FinancierCustody>>,
    pub custody_token:Box<Account<'info,TokenAccount>>,
    #[account(init_if_needed,payer=payer,space=8+FundedRun::INIT_SPACE,
        seeds=[b"usage",claim.supplier_principal.as_ref(),&namespace,&run_id],bump)]
    pub run:Box<Account<'info,FundedRun>>,
    #[account(init_if_needed,payer=payer,space=8+FundedPayment::INIT_SPACE,
        seeds=[b"run-payment",principal.key().as_ref(),&payment_id],bump)]
    pub payment:Box<Account<'info,FundedPayment>>,
    #[account(init_if_needed,payer=payer,space=8+FinanceReceipt::INIT_SPACE,
        seeds=[b"finance-operation",principal.key().as_ref(),
            &funded_run_operation_id(&claim.supplier_principal,&namespace,&run_id)],bump)]
    pub receipt:Box<Account<'info,FinanceReceipt>>,
    pub mint:Box<Account<'info,Mint>>,
    /// CHECK: Optional projection route; Core books remain mandatory.
    #[account(address=POOL_ID)] pub pool_program:UncheckedAccount<'info>,
    pub system_program:Program<'info,System>,
}

#[derive(Accounts)]
#[instruction(operation_id:[u8;32])]
pub struct CollectFinancier<'info> {
    #[account(mut)] pub payer:Signer<'info>,
    pub financier_principal:Box<Account<'info,Principal>>,
    /// CHECK: Stable Registry funding-wallet binding for wallet mode.
    pub wallet_binding:UncheckedAccount<'info>,
    #[account(mut)] pub custody:Box<Account<'info,FinancierCustody>>,
    #[account(mut)] pub custody_token:Box<Account<'info,TokenAccount>>,
    /// CHECK: Swig signer in wallet mode; Core signs its escrow PDA internally.
    pub wallet_authority:UncheckedAccount<'info>,
    #[account(mut)] pub claim:Box<Account<'info,FundedClaim>>,
    #[account(mut)] pub run:Box<Account<'info,FundedRun>>,
    #[account(mut)] pub facility:Box<Account<'info,FundingFacility>>,
    #[account(mut,token::mint=mint)] pub destination:Box<Account<'info,TokenAccount>>,
    /// CHECK: Only the exact canonical fee route may be missing/frozen and waived.
    #[account(mut)] pub fee_destination:Option<UncheckedAccount<'info>>,
    #[account(mut)] pub fee_custody:Option<Box<Account<'info,FinancierCustody>>>,
    #[account(init_if_needed,payer=payer,space=8+FinanceReceipt::INIT_SPACE,
        seeds=[b"finance-operation",claim.terms.principal.as_ref(),&operation_id],bump)]
    pub receipt:Box<Account<'info,FinanceReceipt>>,
    pub mint:Box<Account<'info,Mint>>,
    pub token_program:Program<'info,Token>,
    pub system_program:Program<'info,System>,
    /// CHECK: Not invoked in the explicit accepted-servicing outage path.
    #[account(address=POOL_ID)] pub pool_program:UncheckedAccount<'info>,
    pub reimbursement_principal:Option<Box<Account<'info,Principal>>>,
    pub reimbursement_authority:Option<Signer<'info>>,
    /// CHECK: Exact quote-bound source read afresh after supplier collection.
    #[account(mut,dup)] pub reimbursement_from:Option<UncheckedAccount<'info>>,
    /// CHECK: Exact approved sponsor destination and custody binding.
    #[account(mut)] pub sponsor_destination:Option<UncheckedAccount<'info>>,
    #[account(mut)] pub sponsor_custody:Option<Box<Account<'info,FinancierCustody>>>,
}

#[derive(Accounts)]
#[instruction(request_id:[u8;32])]
pub struct WithdrawFinancierFree<'info> {
    #[account(mut)] pub payer:Signer<'info>,
    pub financier_principal:Box<Account<'info,Principal>>,
    /// CHECK: Stable funding binding in wallet mode; Core sentinel for escrow.
    pub wallet_binding:UncheckedAccount<'info>,
    #[account(mut)] pub custody:Box<Account<'info,FinancierCustody>>,
    #[account(mut)] pub custody_token:Box<Account<'info,TokenAccount>>,
    /// CHECK: Current owner wrapper elevates Swig; Core signs escrow internally.
    pub wallet_authority:UncheckedAccount<'info>,
    #[account(mut,token::mint=mint)] pub destination:Box<Account<'info,TokenAccount>>,
    #[account(init_if_needed,payer=payer,space=8+FinanceReceipt::INIT_SPACE,
        seeds=[b"finance-operation",financier_principal.key().as_ref(),
            &operation_id(b"withdraw",financier_principal.key(),&request_id)],bump)]
    pub receipt:Box<Account<'info,FinanceReceipt>>,
    pub mint:Box<Account<'info,Mint>>,
    pub token_program:Program<'info,Token>,
    pub system_program:Program<'info,System>,
    /// CHECK: Accepted-cash servicing permits explicit no-Pool mode.
    #[account(address=POOL_ID)] pub pool_program:UncheckedAccount<'info>,
}

#[derive(Accounts)]
#[instruction(request_id:[u8;32])]
pub struct ReleaseFinancierUnused<'info> {
    #[account(mut)] pub payer:Signer<'info>,
    pub principal:Box<Account<'info,Principal>>,
    /// Current supplier principal authenticates surrender; payment route is unchanged.
    pub supplier_principal:Box<Account<'info,Principal>>,
    pub supplier:Option<Signer<'info>>,
    #[account(mut)] pub budget:Box<Account<'info,Budget>>,
    #[account(mut)] pub reservation:Box<Account<'info,Reservation>>,
    /// CHECK: Registry owns its release operation identity and account validation.
    #[account(mut)] pub registry_operation:UncheckedAccount<'info>,
    /// CHECK: Core consumer authority.
    #[account(seeds=[REGISTRY_CONSUMER_SEED],bump)] pub registry_authority:UncheckedAccount<'info>,
    pub registry_program:Program<'info,PrincipalRegistry>,
    #[account(mut)] pub line:Box<Account<'info,CustomerCreditLine>>,
    #[account(mut)] pub claim:Box<Account<'info,FundedClaim>>,
    #[account(mut)] pub facility:Box<Account<'info,FundingFacility>>,
    #[account(mut)] pub custody:Box<Account<'info,FinancierCustody>>,
    pub custody_token:Box<Account<'info,TokenAccount>>,
    #[account(init_if_needed,payer=payer,space=8+FinanceReceipt::INIT_SPACE,
        seeds=[b"finance-operation",principal.key().as_ref(),&operation_id(b"release",payer.key(),&request_id)],bump)]
    pub receipt:Box<Account<'info,FinanceReceipt>>,
    pub mint:Box<Account<'info,Mint>>,
    /// CHECK: Optional projection route.
    #[account(address=POOL_ID)] pub pool_program:UncheckedAccount<'info>,
    pub system_program:Program<'info,System>,
}

pub fn initialize_book<'info>(ctx:Context<'info, InitializeFinanceBook<'info>>,principal:Pubkey)->Result<()> {
    let b=&mut ctx.accounts.book;
    if b.data.version!=0 {
        require!(b.data.version==FUNDED_VERSION && b.data.principal==principal && b.data.mint==ctx.accounts.mint.key(),CoreError::FundingBookAccounts);
        return Ok(());
    }
    b.data=FinanceBookData{version:FUNDED_VERSION,bump:ctx.bumps.book,principal,mint:ctx.accounts.mint.key(),..FinanceBookData::default()};Ok(())
}

pub fn manage_facility<'info>(ctx:Context<'info, ManageFundingFacility<'info>>,request_id:[u8;32],expected_revision:u64,
    commitment_limit:u64,enabled:bool,authority_generation:u64)->Result<()> {
    let a=ctx.accounts;let id=operation_id(b"facility-policy",a.financier.key(),&request_id);
    let digest=financial_digest(b"DEXTER:FACILITY-POLICY:1",&(a.facility.key(),a.financier.key(),a.owner.key(),
        expected_revision,commitment_limit,enabled,authority_generation))?;
    if a.receipt.version!=0 {
        require!(a.receipt.version==FUNDED_VERSION && a.receipt.operation_id==id && a.receipt.request_digest==digest
            && a.receipt.facility==a.facility.key(),CoreError::FundingReplay);return Ok(());
    }
    owner(&a.financier,a.owner.key(),authority_generation)?;a.facility.check(a.facility.key())?;
    require!(a.facility.financier==a.financier.key() && a.facility.revision==expected_revision,CoreError::FundingRevision);
    let next=plus(expected_revision,1)?;
    a.receipt.set_inner(FacilityPolicyReceipt{version:FUNDED_VERSION,bump:ctx.bumps.receipt,facility:a.facility.key(),
        financier:a.financier.key(),request_id,operation_id:id,request_digest:digest,owner:a.owner.key(),authority_generation,
        previous_revision:expected_revision,resulting_revision:next,previous_limit:a.facility.commitment_limit,
        resulting_limit:commitment_limit,previously_enabled:a.facility.accepts_new,resulting_enabled:enabled});
    // A lower limit may be below existing exposure. It fences new admission,
    // never erases debt, reserved cash, supplier rights or the original terms.
    a.facility.commitment_limit=commitment_limit;a.facility.accepts_new=enabled;a.facility.revision=next;Ok(())
}

pub fn check_wallet_graph(profile:u8,binding:&AccountInfo,config:&AccountInfo,wallet:&AccountInfo,
    principal:Pubkey,mint:Pubkey)->Result<()> {
    if profile==WALLET_CASH {
        let b=principal_registry::validate_funding_wallet(binding,config,wallet)?;
        require!(b.principal==principal && b.wallet==wallet.key(),CoreError::FundingWalletAuthority);
    } else {
        require!(profile==ESCROW_CASH && binding.key()==CORE_ID && config.key()==CORE_ID,CoreError::FundingWalletAuthority);
        let escrow=Pubkey::find_program_address(&[b"escrow-authority",principal.as_ref(),mint.as_ref()],&CORE_ID).0;
        require_keys_eq!(wallet.key(),escrow,CoreError::FundingWalletAuthority);
    }
    Ok(())
}

pub fn create_custody<'info>(ctx:Context<'info, CreateFinancierCustody<'info>>,request_id:[u8;32],profile:u8,
    allocated_cash:u64,authority_generation:u64,provenance_hash:[u8;32])->Result<()> {
    let a=ctx.accounts;let id=operation_id(b"custody",a.financier.key(),&request_id);
    let digest=financial_digest(b"DEXTER:CREATE-CUSTODY:1",&(a.financier.key(),a.owner.key(),a.custody_token.key(),
        a.wallet_binding.key(),a.wallet_authority.key(),profile,allocated_cash,authority_generation,provenance_hash))?;
    if replay(&a.receipt,&digest)? {return Ok(());}
    owner(&a.financier,a.owner.key(),authority_generation)?;
    require!(a.custody.version==0,CoreError::FundingCustody);
    check_wallet_graph(profile,&a.wallet_binding,&a.wallet_config,&a.wallet_authority,a.financier.key(),a.mint.key())?;
    require!(a.custody_token.owner==a.wallet_authority.key() && a.custody_token.mint==a.mint.key(),CoreError::FundingTokenRouting);
    require_keys_eq!(a.custody_token.key(),associated_token::get_associated_token_address(&a.wallet_authority.key(),&a.mint.key()),CoreError::FundingTokenRouting);
    admit_cash(&a.custody_token)?;
    require!(allocated_cash<=a.custody_token.amount && (allocated_cash==0 || provenance_hash!=[0;32]),CoreError::FundingCashShortfall);
    a.custody.set_inner(FinancierCustody{version:FUNDED_VERSION,bump:ctx.bumps.custody,domain:a.financier.domain,
        principal:a.financier.key(),binding:a.wallet_binding.key(),wallet:a.wallet_authority.key(),token_account:a.custody_token.key(),
        mint:a.mint.key(),profile,accounted_cash:allocated_cash,unused:0,payable:0,contributed_capital:allocated_cash,
        withdrawn_cash:0,revision:1});
    let mut data=record_base(a.financier.key(),id,digest,INITIAL_CAPITAL,a.mint.key(),Pubkey::default(),Pubkey::default(),allocated_cash);
    data.capital_custody=a.custody.key();data.capital_allocated=allocated_cash;data.capital_provenance_hash=provenance_hash;
    data.capital_authority=a.owner.key();data.capital_owner_generation=authority_generation;
    journal(&mut a.receipt,data,&[inc(a.financier.key(),CAPITAL_ALLOCATED,allocated_cash)],ctx.remaining_accounts,
        &a.pool_program,false,true)
}

pub fn validate_interest_route(terms:&InterestTerms,custody:Option<&Account<FinancierCustody>>,mint:Pubkey)->Result<()> {
    math(terms.validate())?;require!(!terms.stop_on_shortfall,CoreError::FundingTerms);
    if terms.protocol_share_bps==0 {require!(custody.is_none(),CoreError::FundingTokenRouting);return Ok(());}
    let custody=custody.ok_or_else(||error!(CoreError::FundingTokenRouting))?;
    custody.check_key(custody.key())?;
    require!(custody.version==FUNDED_VERSION && custody.mint==mint && custody.principal==terms.protocol_beneficiary
        && custody.token_account==terms.protocol_destination,CoreError::FundingTokenRouting);Ok(())
}

pub fn open_facility<'info>(ctx:Context<'info, OpenFundingFacility<'info>>,facility_id:[u8;32],commitment_limit:u64,
    accept_until:i64,external:bool,terms_hash:[u8;32],interest_terms:InterestTerms,
    financier_generation:u64,issuer_generation:u64)->Result<()> {
    let a=ctx.accounts;owner(&a.financier,a.owner.key(),financier_generation)?;
    owner(&a.issuer,a.issuer_owner.key(),issuer_generation)?;
    a.custody.check_key(a.custody.key())?;
    require!(a.financier.domain==a.issuer.domain && a.custody.domain==a.financier.domain
        && a.custody.principal==a.financier.key() && external==(a.financier.key()!=a.issuer.key()),CoreError::FundingFacility);
    validate_interest_route(&interest_terms,a.protocol_custody.as_deref(),a.custody.mint)?;
    require!(external || (interest_terms.annual_rate_bps==0 && interest_terms.maximum_interest==0),CoreError::FundingTerms);
    require!(commitment_limit>0 && terms_hash!=[0;32],CoreError::FundingTerms);
    let opening_hash=financial_digest(b"DEXTER:FACILITY-OPENING:1",&(facility_id,a.financier.key(),a.issuer.key(),
        a.custody.key(),commitment_limit,accept_until,external,terms_hash,&interest_terms))?;
    if a.facility.version!=0 {
        a.facility.check(a.facility.key())?;
        require!(a.facility.facility_id==facility_id && a.facility.financier==a.financier.key()
            && a.facility.issuer==a.issuer.key() && a.facility.custody==a.custody.key()
            && a.facility.terms_hash==terms_hash && a.facility.interest_terms==interest_terms
            && a.facility.external==external && a.facility.opening_hash==opening_hash,CoreError::FundingReplay);
        return Ok(()); // Current admission decisions and debt are never reset.
    }
    require!(accept_until>Clock::get()?.unix_timestamp,CoreError::FundingAdmission);
    a.facility.set_inner(FundingFacility{version:FUNDED_VERSION,bump:ctx.bumps.facility,facility_id,
        financier:a.financier.key(),issuer:a.issuer.key(),custody:a.custody.key(),mint:a.custody.mint,external,
        commitment_limit,reserved:0,drawn:0,repaid:0,accepts_new:true,accept_until,revision:1,terms_hash,opening_hash,
        interest:math(InterestState::new(Clock::get()?.unix_timestamp,&interest_terms))?,interest_terms});Ok(())
}

pub fn open_credit_line<'info>(ctx:Context<'info, OpenCustomerCreditLine<'info>>,line_id:[u8;32],limit:u64,
    interest_terms:InterestTerms,assignment_policy_hash:[u8;32],owner_generation:u64,issuer_generation:u64)->Result<()> {
    let a=ctx.accounts;owner(&a.company,a.owner.key(),owner_generation)?;owner(&a.issuer,a.issuer_owner.key(),issuer_generation)?;
    require!(a.company.domain==a.issuer.domain && a.company.key()!=a.issuer.key(),CoreError::FundingCreditLine);
    a.issuer_custody.check_key(a.issuer_custody.key())?;a.facility.check(a.facility.key())?;
    require!(a.facility.issuer==a.issuer.key() && a.issuer_custody.principal==a.issuer.key()
        && a.issuer_custody.domain==a.company.domain && a.issuer_custody.mint==a.facility.mint && limit>0,CoreError::FundingCreditLine);
    validate_interest_route(&interest_terms,a.protocol_custody.as_deref(),a.facility.mint)?;
    require!(assignment_policy_hash==[0;32] || assignment_policy_hash==within_domain_assignment_policy(&a.company.domain),CoreError::FundingAssignment);
    if a.credit_line.version!=0 {
        a.credit_line.check(a.credit_line.key())?;
        require!(a.credit_line.company==a.company.key() && a.credit_line.issuer==a.issuer.key()
            && a.credit_line.issuer_custody==a.issuer_custody.key() && a.credit_line.facility==a.facility.key()
            && a.credit_line.interest_terms==interest_terms && a.credit_line.assignment_policy_hash==assignment_policy_hash
            && a.credit_line.limit==limit,
            CoreError::FundingReplay);return Ok(());
    }
    a.credit_line.set_inner(CustomerCreditLine{version:FUNDED_VERSION,bump:ctx.bumps.credit_line,line_id,
        company:a.company.key(),issuer:a.issuer.key(),issuer_custody:a.issuer_custody.key(),facility:a.facility.key(),mint:a.facility.mint,
        limit,reserved:0,principal_outstanding:0,fee_exposure:0,allocated_fee_caps:0,interest_terms,enabled:true,revision:1,assignment_policy_hash});Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn accept<'info>(ctx:Context<'info, AcceptFinancedTab<'info>>,request_id:[u8;32],terms:TabTerms,maximum_interest:u64,
    seller_fee_terms:SellerFeeTerms,assignment_policy_hash:[u8;32],namespace:[u8;32],service_unit:[u8;32],
    unit_price:u64,external_contract_hash:[u8;32])->Result<()> {
    let a=ctx.accounts;let id=operation_id(b"accept",a.credential.key(),&request_id);
    let digest=financial_digest(b"DEXTER:ACCEPT-FINANCED-TAB:3",&(&terms,maximum_interest,&seller_fee_terms,
        assignment_policy_hash,a.credit_line.key(),a.facility.key(),a.custody.key(),a.supplier_principal.key(),
        namespace,service_unit,unit_price,external_contract_hash,a.seller_fee_agreement.as_ref().map(|x|x.key())))?;
    if replay(&a.receipt,&digest)?{return Ok(());}
    require!(a.claim.version==0,CoreError::FundingClaim);
    valid_principal(&a.principal)?;valid_principal(&a.issuer)?;valid_principal(&a.supplier_principal)?;
    require!(a.issuer.owner==a.issuer_owner.key() && a.supplier_principal.owner==a.supplier.key(),CoreError::FundingOwner);
    require!(a.principal.domain==a.issuer.domain && a.principal.domain==a.supplier_principal.domain,CoreError::FundingPrincipal);
    require!(terms.domain==a.principal.domain && terms.principal==a.principal.key() && terms.actor==a.actor.key()
        && terms.mandate==a.mandate.key() && terms.asset==a.mint.key() && terms.supplier==a.supplier.key()
        && terms.source==a.facility.key() && terms.cap>0 && terms.expires_at>Clock::get()?.unix_timestamp,CoreError::FundingTerms);
    require!(unit_price>0 && terms.external_terms_hash==contracts::completed_run_policy(&namespace,&service_unit,unit_price,&external_contract_hash),CoreError::FundingUsage);
    a.credit_line.check(a.credit_line.key())?;a.facility.check(a.facility.key())?;
    custody_token(&a.custody,&a.custody_token,a.mint.key())?;admit_cash(&a.custody_token)?;
    check_wallet_graph(a.custody.profile,&a.wallet_binding,&a.wallet_config,&a.wallet_authority,a.custody.principal,a.mint.key())?;
    require!(a.wallet_binding.key()==a.custody.binding && a.wallet_authority.key()==a.custody.wallet,CoreError::FundingCustody);
    require!(a.credit_line.company==a.principal.key() && a.credit_line.issuer==a.issuer.key()
        && a.credit_line.facility==a.facility.key() && a.credit_line.mint==a.mint.key()
        && a.facility.issuer==a.issuer.key() && a.facility.custody==a.custody.key()
        && a.facility.financier==a.custody.principal && a.custody.domain==terms.domain,CoreError::FundingFacility);
    require!(a.credit_line.enabled && a.facility.accepts_new && Clock::get()?.unix_timestamp<a.facility.accept_until,CoreError::FundingAdmission);
    require!(assignment_policy_hash==a.credit_line.assignment_policy_hash,CoreError::FundingAssignment);
    let credit_total=plus(plus(a.credit_line.reserved,a.credit_line.principal_outstanding)?,a.credit_line.fee_exposure)?;
    require!(plus(credit_total,plus(terms.cap,maximum_interest)?)?<=a.credit_line.limit,CoreError::FundingCreditLine);
    require!(plus(a.credit_line.allocated_fee_caps,maximum_interest)?<=a.credit_line.interest_terms.maximum_interest,CoreError::FundingChargeCap);
    require!(plus(plus(a.facility.reserved,a.facility.drawn)?,terms.cap)?<=a.facility.commitment_limit,CoreError::FundingFacility);
    require!(plus(a.custody.protected()?,terms.cap)?<=a.custody.accounted_cash,CoreError::FundingCashShortfall);
    math(seller_fee_terms.validate())?;
    require!(seller_fee_terms.supplier==terms.supplier && seller_fee_terms.asset==(Asset{mint:a.mint.key(),token_program:anchor_spl::token::ID})
        && seller_fee_terms.gross_cap==terms.cap,CoreError::FundingTerms);
    // Separate cash routes make each accepted gross-to-net token leg exact.
    // One principal may own both wallets; journal roles coalesce by principal.
    let supplier_destination=associated_token::get_associated_token_address(&terms.supplier,&a.mint.key());
    require!(supplier_destination!=a.custody_token.key(),CoreError::FundingTokenRouting);
    let seller_fee_allocation=math(seller_fee_terms.cumulative_fee(terms.cap))?;
    let seller_fee_agreement=if seller_fee_allocation>0 {
        let fee=a.fee_custody.as_ref().ok_or_else(||error!(CoreError::FundingTerms))?;
        fee.check_key(fee.key())?;
        require!(seller_fee_terms.beneficiary==fee.principal && seller_fee_terms.beneficiary_wallet==fee.wallet
            && seller_fee_terms.destination==fee.token_account && fee.mint==a.mint.key() && fee.domain==terms.domain,
            CoreError::FundingTerms);
        require!(fee.key()!=a.custody.key() && seller_fee_terms.destination!=supplier_destination,CoreError::FundingTokenRouting);
        let parent=a.seller_fee_agreement.as_deref_mut().ok_or_else(||error!(CoreError::FundingSellerFeeAgreement))?;
        parent.check_identity(parent.key(),&terms.domain,a.supplier_principal.key(),&seller_fee_terms.asset,
            &seller_fee_terms.agreement_hash)?;
        require!(parent.to_account_info().data_len()==8+SellerFeeAgreement::INIT_SPACE,CoreError::FundingSellerFeeAgreement);
        parent.allocate(seller_fee_allocation)?;
        parent.key()
    } else {
        require!(a.fee_custody.is_none() && a.seller_fee_agreement.is_none(),CoreError::FundingTerms);
        Pubkey::default()
    };
    let mut authorized=terms.clone();authorized.cap=plus(terms.cap,maximum_interest)?;
    let bump=[ctx.bumps.registry_authority];let seeds:&[&[u8]]=&[REGISTRY_CONSUMER_SEED,&bump];
    principal_registry::cpi::reserve(CpiContext::new_with_signer(a.registry_program.key(),principal_registry::cpi::accounts::Reserve{
        core_authority:a.registry_authority.to_account_info(),payer:a.payer.to_account_info(),principal:a.principal.to_account_info(),
        actor:a.actor.to_account_info(),credential:a.credential.to_account_info(),mandate:a.mandate.to_account_info(),
        budget:a.budget.to_account_info(),reservation:a.reservation.to_account_info(),operation:a.registry_operation.to_account_info(),
        system_program:a.system_program.to_account_info(),
    },&[seeds]),AcceptArgs{operation_id:id,terms:authorized})?;
    a.credit_line.reserved=plus(a.credit_line.reserved,terms.cap)?;
    a.credit_line.fee_exposure=plus(a.credit_line.fee_exposure,maximum_interest)?;
    a.credit_line.allocated_fee_caps=plus(a.credit_line.allocated_fee_caps,maximum_interest)?;
    a.credit_line.revision=plus(a.credit_line.revision,1)?;
    a.facility.reserved=plus(a.facility.reserved,terms.cap)?;a.facility.revision=plus(a.facility.revision,1)?;
    a.custody.unused=plus(a.custody.unused,terms.cap)?;a.custody.revision=plus(a.custody.revision,1)?;
    let mut interest_terms=a.credit_line.interest_terms.clone();interest_terms.maximum_interest=maximum_interest;
    let claim_key=a.claim.key();let company=terms.principal;let issuer=a.issuer.key();let supplier=a.supplier_principal.key();
    let amount=terms.cap;
    a.claim.set_inner(FundedClaim{version:FUNDED_CLAIM_VERSION,bump:ctx.bumps.claim,terms,usage_namespace:namespace,
        service_unit,unit_price,external_contract_hash,acceptance_hash:digest,credit_line:a.credit_line.key(),assignment_policy_hash,
        facility:a.facility.key(),custody:a.custody.key(),issuer,issuer_custody:a.credit_line.issuer_custody,supplier_principal:supplier,
        remaining:amount,payable:0,debt:0,paid:0,repaid:0,released:0,revision:1,fee_cap:maximum_interest,
        fee_authority_remaining:maximum_interest,allocated_run_fee_caps:0,released_fee_caps:0,released_unallocated_fee_caps:0,
        interest_outstanding:0,interest_terms,seller_fee_agreement,seller_fee_allocation,
        seller_fee_terms,seller_fee_state:SellerFeeState::default(),principal_extinguished:0,corrections:Default::default()});
    a.claim.check(claim_key)?;
    let changes=[inc(company,CUSTOMER_RESERVED,amount),inc(company,CREDIT_CHARGE_EXPOSURE,maximum_interest),
        inc(issuer,UNUSED_SUPPLIER,amount),inc(supplier,SUPPLIER_COMMITTED,amount),inc(a.custody.principal,FUNDING_RESERVED,amount)];
    let data=record_base(company,id,digest,FINANCE_ACCEPT,a.mint.key(),claim_key,Pubkey::default(),amount);
    journal(&mut a.receipt,data,&changes,ctx.remaining_accounts,&a.pool_program,false,true)
}

#[allow(clippy::too_many_arguments)]
pub fn earn<'info>(ctx:Context<'info, EarnFinancedRun<'info>>,payment_id:[u8;32],namespace:[u8;32],run_id:[u8;32],
    service_unit:[u8;32],external_contract_hash:[u8;32],units:u64,unit_price:u64,expected_revision:u64,
    service_without_pool:bool)->Result<()> {
    let a=ctx.accounts;let claim_key=a.claim.key();let run_key=a.run.key();
    a.claim.check(claim_key)?;valid_principal(&a.supplier_principal)?;
    require!(a.supplier_principal.key()==a.claim.supplier_principal && a.supplier_principal.owner==a.supplier.key(),CoreError::FundingOwner);
    require!(namespace==a.claim.usage_namespace && service_unit==a.claim.service_unit && unit_price==a.claim.unit_price
        && external_contract_hash==a.claim.external_contract_hash && units>0,CoreError::FundingUsage);
    let amount=units.checked_mul(unit_price).ok_or_else(||error!(CoreError::FundingAmount))?;
    let id=funded_run_operation_id(&a.claim.supplier_principal,&namespace,&run_id);
    let digest=financial_digest(b"DEXTER:FINANCED-COMPLETED-RUN:1",&(claim_key,namespace,run_id,service_unit,external_contract_hash,units,unit_price))?;
    let payment_digest=financial_digest(b"DEXTER:FINANCED-PAYMENT-ALIAS:1",&(a.principal.key(),payment_id,run_key,digest))?;
    if a.payment.version!=0 {
        require!(a.payment.version==FUNDED_VERSION && a.payment.company==a.principal.key() && a.payment.payment_id==payment_id
            && a.payment.run==run_key && a.payment.receipt==a.receipt.key() && a.payment.request_digest==payment_digest,CoreError::FundingReplay);
    }
    if a.run.version!=0 {
        a.run.check(run_key,claim_key,&a.claim)?;
        require!(a.run.capture_digest==digest && a.run.capture_receipt==a.receipt.key() && replay(&a.receipt,&digest)?,CoreError::FundingReplay);
        a.payment.set_inner(FundedPayment{version:FUNDED_VERSION,bump:ctx.bumps.payment,company:a.principal.key(),payment_id,
            run:run_key,receipt:a.receipt.key(),request_digest:payment_digest});return Ok(());
    }
    require!(a.receipt.data.version==0 && a.claim.revision==expected_revision,CoreError::FundingRevision);
    require!(Clock::get()?.unix_timestamp<=a.claim.terms.expires_at && amount<=a.claim.remaining,CoreError::FundingAmount);
    a.line.check(a.line.key())?;reservation_matches(&a.reservation,&a.claim)?;
    require!(a.line.key()==a.claim.credit_line && a.principal.key()==a.claim.terms.principal
        && a.custody.key()==a.claim.custody && a.mint.key()==a.claim.terms.asset,CoreError::FundingClaim);
    custody_token(&a.custody,&a.custody_token,a.mint.key())?;
    let prior_earned=a.claim.captured_total()?;
    let cumulative_cap=if amount==a.claim.remaining {
        minus(a.claim.fee_cap,a.claim.released_unallocated_fee_caps)?
    } else { ((plus(prior_earned,amount)? as u128)*(a.claim.fee_cap as u128)/(a.claim.terms.cap as u128)) as u64 };
    let run_fee_cap=minus(cumulative_cap,a.claim.allocated_run_fee_caps)?;
    a.claim.allocated_run_fee_caps=cumulative_cap;
    let mut interest_terms=a.claim.interest_terms.clone();interest_terms.maximum_interest=run_fee_cap;
    let bump=[ctx.bumps.registry_authority];let seeds:&[&[u8]]=&[REGISTRY_CONSUMER_SEED,&bump];
    principal_registry::cpi::consume_completed_run(CpiContext::new_with_signer(a.registry_program.key(),principal_registry::cpi::accounts::ConsumeCompletedRun{
        core_authority:a.registry_authority.to_account_info(),payer:a.payer.to_account_info(),principal:a.principal.to_account_info(),
        budget:a.budget.to_account_info(),reservation:a.reservation.to_account_info(),operation:a.registry_operation.to_account_info(),
        system_program:a.system_program.to_account_info(),
    },&[seeds]),StageArgs{operation_id:id,amount,expected_revision,service_without_pool})?;
    a.claim.remaining=minus(a.claim.remaining,amount)?;a.claim.payable=plus(a.claim.payable,amount)?;
    a.claim.debt=plus(a.claim.debt,amount)?;a.claim.revision=plus(a.claim.revision,1)?;
    a.line.reserved=minus(a.line.reserved,amount)?;a.line.principal_outstanding=plus(a.line.principal_outstanding,amount)?;
    a.line.revision=plus(a.line.revision,1)?;
    a.custody.unused=minus(a.custody.unused,amount)?;a.custody.payable=plus(a.custody.payable,amount)?;
    a.custody.revision=plus(a.custody.revision,1)?;
    a.run.set_inner(FundedRun{version:FUNDED_RUN_VERSION,bump:ctx.bumps.run,operation_id:id,claim:claim_key,namespace,run_id,
        service_unit,units,unit_price,supplier:a.claim.terms.supplier,supplier_principal:a.claim.supplier_principal,
        debtor:a.claim.terms.principal,mint:a.mint.key(),gross_earned:amount,gross_paid:0,current_creditor:a.claim.issuer,
        creditor_custody:a.claim.issuer_custody,ownership_revision:1,principal:PrincipalAmounts{originated:amount,outstanding:amount,repaid:0,extinguished:0},
        interest:math(InterestState::new(Clock::get()?.unix_timestamp,&interest_terms))?,interest_terms,
        acquisition_basis:amount,provenance_hash:digest,capture_receipt:a.receipt.key(),capture_digest:digest,fee_authority_released:0,corrections:Default::default()});
    a.payment.set_inner(FundedPayment{version:FUNDED_VERSION,bump:ctx.bumps.payment,company:a.principal.key(),payment_id,
        run:run_key,receipt:a.receipt.key(),request_digest:payment_digest});
    a.claim.check(claim_key)?;a.custody.check(a.custody_token.amount)?;
    let company=a.claim.terms.principal;let issuer=a.claim.issuer;let supplier=a.claim.supplier_principal;
    let changes=[dec(company,CUSTOMER_RESERVED,amount),inc(company,CUSTOMER_PRINCIPAL,amount),
        dec(issuer,UNUSED_SUPPLIER,amount),inc(issuer,UNPAID_SUPPLIER,amount),inc(issuer,OWNED_PRINCIPAL,amount),
        dec(supplier,SUPPLIER_COMMITTED,amount),inc(supplier,SUPPLIER_RECEIVABLE,amount),inc(a.custody.principal,FUNDING_RESERVED,0)];
    let data=record_base(company,id,digest,FINANCE_EARN,a.mint.key(),claim_key,run_key,amount);
    journal(&mut a.receipt,data,&changes,ctx.remaining_accounts,&a.pool_program,service_without_pool,false)
}

pub fn withdraw<'info>(ctx:Context<'info, WithdrawFinancierFree<'info>>,request_id:[u8;32],amount:u64,expected_revision:u64,
    authority_generation:u64,service_without_pool:bool)->Result<()> {
    let a=ctx.accounts;let id=operation_id(b"withdraw",a.financier_principal.key(),&request_id);
    let digest=financial_digest(b"DEXTER:FINANCIER-FREE-WITHDRAWAL:1",&(id,a.financier_principal.key(),a.payer.key(),
        a.custody.key(),a.destination.key(),amount,authority_generation))?;
    if replay(&a.receipt,&digest)?{return Ok(());}
    owner(&a.financier_principal,a.payer.key(),authority_generation)?;
    require!(amount>0 && a.custody.revision==expected_revision && a.custody.principal==a.financier_principal.key(),CoreError::FundingRevision);
    custody_token(&a.custody,&a.custody_token,a.mint.key())?;
    require_keys_eq!(a.destination.key(),associated_token::get_associated_token_address(&a.financier_principal.owner,&a.mint.key()),CoreError::FundingTokenRouting);
    require!(a.destination.owner==a.financier_principal.owner,CoreError::FundingTokenRouting);
    a.custody.debit_uncommitted(a.custody_token.amount,amount)?;
    crate::funding_collection::transfer_from_custody(&a.custody,&a.wallet_binding,&a.wallet_authority,
        &a.custody_token.to_account_info(),&a.destination.to_account_info(),&a.mint,a.token_program.key(),amount)?;
    a.custody_token.reload()?;a.destination.reload()?;
    a.custody.check(a.custody_token.amount)?;
    let mut data=record_base(a.financier_principal.key(),id,digest,FINANCE_WITHDRAW,a.mint.key(),Pubkey::default(),Pubkey::default(),amount);
    data.debit_principal=a.financier_principal.key();data.debit_authority=a.wallet_authority.key();data.debit_from=a.custody_token.key();
    let changes=[inc(a.financier_principal.key(),CASH_SENT,amount)];
    journal(&mut a.receipt,data,&changes,ctx.remaining_accounts,&a.pool_program,service_without_pool,false)
}

pub fn release<'info>(ctx:Context<'info, ReleaseFinancierUnused<'info>>,request_id:[u8;32],amount:u64,expected_revision:u64,
    service_without_pool:bool)->Result<()> {
    let a=ctx.accounts;let claim_key=a.claim.key();let id=operation_id(b"release",a.payer.key(),&request_id);
    let digest=financial_digest(b"DEXTER:RELEASE-FINANCIER-UNUSED:1",&(claim_key,id,amount))?;
    if replay(&a.receipt,&digest)?{return Ok(());}
    a.claim.check(claim_key)?;a.line.check(a.line.key())?;a.facility.check(a.facility.key())?;
    require!(a.claim.revision==expected_revision && amount>0 && amount<=a.claim.remaining,CoreError::FundingRevision);
    require!(a.principal.key()==a.claim.terms.principal && a.supplier_principal.key()==a.claim.supplier_principal
        && a.line.key()==a.claim.credit_line && a.facility.key()==a.claim.facility && a.custody.key()==a.claim.custody
        && a.mint.key()==a.claim.terms.asset,CoreError::FundingClaim);
    reservation_matches(&a.reservation,&a.claim)?;custody_token(&a.custody,&a.custody_token,a.mint.key())?;
    valid_principal(&a.supplier_principal)?;
    if Clock::get()?.unix_timestamp<=a.claim.terms.expires_at {
        let supplier=a.supplier.as_ref().ok_or_else(||error!(CoreError::FundingReleaseAuthority))?;
        require_keys_eq!(supplier.key(),a.supplier_principal.owner,CoreError::FundingReleaseAuthority);
    }
    // Run closures consume their own allocated fee caps. This counter concerns
    // only never-earned work, so closing a run cannot subtract capacity twice.
    let total_released=plus(a.claim.released,amount)?;
    let target_unallocated_release=if amount==a.claim.remaining {
        minus(a.claim.fee_cap,a.claim.allocated_run_fee_caps)?
    } else { ((total_released as u128)*(a.claim.fee_cap as u128)/(a.claim.terms.cap as u128)) as u64 };
    let fee_release=minus(target_unallocated_release,a.claim.released_unallocated_fee_caps)?;
    let authority_release=plus(amount,fee_release)?;
    let bump=[ctx.bumps.registry_authority];let seeds:&[&[u8]]=&[REGISTRY_CONSUMER_SEED,&bump];
    principal_registry::cpi::release(CpiContext::new_with_signer(a.registry_program.key(),principal_registry::cpi::accounts::ServiceReservation{
        core_authority:a.registry_authority.to_account_info(),payer:a.payer.to_account_info(),principal:a.principal.to_account_info(),
        budget:a.budget.to_account_info(),reservation:a.reservation.to_account_info(),operation:a.registry_operation.to_account_info(),
        system_program:a.system_program.to_account_info(),
    },&[seeds]),StageArgs{operation_id:id,amount:authority_release,expected_revision,service_without_pool})?;
    a.claim.remaining=minus(a.claim.remaining,amount)?;a.claim.released=total_released;
    a.claim.released_unallocated_fee_caps=target_unallocated_release;
    a.claim.released_fee_caps=plus(a.claim.released_fee_caps,fee_release)?;
    a.claim.fee_authority_remaining=minus(a.claim.fee_authority_remaining,fee_release)?;
    a.claim.revision=plus(a.claim.revision,1)?;
    a.line.reserved=minus(a.line.reserved,amount)?;a.line.fee_exposure=minus(a.line.fee_exposure,fee_release)?;
    a.line.revision=plus(a.line.revision,1)?;
    a.facility.reserved=minus(a.facility.reserved,amount)?;a.facility.revision=plus(a.facility.revision,1)?;
    a.custody.unused=minus(a.custody.unused,amount)?;a.custody.revision=plus(a.custody.revision,1)?;
    a.claim.check(claim_key)?;a.custody.check(a.custody_token.amount)?;
    let company=a.claim.terms.principal;let issuer=a.claim.issuer;let supplier=a.claim.supplier_principal;
    let changes=[dec(company,CUSTOMER_RESERVED,amount),dec(company,CREDIT_CHARGE_EXPOSURE,fee_release),
        dec(issuer,UNUSED_SUPPLIER,amount),dec(supplier,SUPPLIER_COMMITTED,amount),dec(a.custody.principal,FUNDING_RESERVED,amount)];
    let data=record_base(company,id,digest,FINANCE_RELEASE,a.mint.key(),claim_key,Pubkey::default(),amount);
    journal(&mut a.receipt,data,&changes,ctx.remaining_accounts,&a.pool_program,service_without_pool,false)
}
