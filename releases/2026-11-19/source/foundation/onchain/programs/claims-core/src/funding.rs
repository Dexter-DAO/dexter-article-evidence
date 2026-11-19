//! Finite financed commitments, delivered usage and independent supplier collection.
use anchor_lang::prelude::*;
use crate::CoreError;
use anchor_spl::{associated_token, token::{Mint, Token, TokenAccount}};
use dexter_contracts::{self as contracts, plus, AcceptArgs, TabTerms,
    CORE_ID, POOL_ID, REGISTRY_CONSUMER_SEED};
use dexter_contracts::commercial::{InterestState, InterestTerms, SellerFeeState, SellerFeeTerms,
    within_domain_assignment_policy};
use dexter_contracts::financing::Asset;
use dexter_contracts::billing::BillingPolicy;
use dexter_contracts::funded::*;
use principal_registry::{self, Principal, Actor, Mandate, Budget, Reservation, Term, program::PrincipalRegistry};
use crate::funding_state::*;
use crate::funding_journal::{journal, inc, exact_data};
use crate::seller_fees::SellerFeeAgreement;
use crate::tab_events::{self, TabStepHeader, EventBytes, RecordOperation, TabCommitted, RunEarned, EarnDetail,
    UnusedReleased, DETAIL_SPACE, books_space};

pub fn operation_id(tag:&[u8],scope:Pubkey,raw:&[u8;32])->[u8;32] {
    solana_sha256_hasher::hashv(&[b"DEXTER:FUNDING-OPERATION:1",CORE_ID.as_ref(),tag,scope.as_ref(),raw]).to_bytes()
}

/// A release's operation id: the Tab and its supplier principal, never the fee
/// payer, so a release retried by another payer is the same step.
pub fn release_operation_id(claim:Pubkey,supplier_principal:Pubkey,raw:&[u8;32])->[u8;32] {
    solana_sha256_hasher::hashv(&[b"DEXTER:FUNDING-OPERATION:1",CORE_ID.as_ref(),b"release",claim.as_ref(),
        supplier_principal.as_ref(),raw]).to_bytes()
}

/// The `RecordOperation` kind of a custody's creation.
pub const OPERATION_CUSTODY_CREATED: u8 = INITIAL_CAPITAL;

/// Emit a custody, facility or receiving-custody operation's event.
#[inline(never)]
pub(crate) fn emit_record_operation<'info>(authority:&AccountInfo<'info>,kind:u8,operation:&[u8;32],record:&Pubkey,
    number:u64,detail:&FinanceDetail,books:&[dexter_contracts::funded::BookStep],
    changes:&[dexter_contracts::funded::BookChange])->Result<()> {
    let event=EventBytes::new::<RecordOperation>(1+32+32+8+DETAIL_SPACE+books_space(books,changes))
        .put(&kind)?.put(operation)?.put(record)?.put(&number)?.put(detail)?.put(books)?.put(changes)?;
    tab_events::emit_event(authority,event)
}

#[derive(Accounts)]
#[instruction(principal:Pubkey)]
pub struct InitializeFinanceBook<'info> {
    #[account(mut)] pub payer:Signer<'info>,
    #[account(init_if_needed,payer=payer,space=8+FinanceBook::INIT_SPACE,
        seeds=[b"financial-book",principal.as_ref(),mint.key().as_ref()],bump)]
    pub book:Box<Account<'info,FinanceBook>>,
    pub mint:Box<InterfaceAccount<'info,anchor_spl::token_interface::Mint>>,
    pub system_program:Program<'info,System>,
}

#[derive(Accounts)]
#[instruction(request_id:[u8;32])]
pub struct CreateFinancierCustody<'info> {
    #[account(mut)] pub payer:Signer<'info>,
    /// CHECK: Native signer or exact passkey owner proof for fresh allocation;
    /// historical receipt recovery does not require the former credential.
    pub owner:UncheckedAccount<'info>,
    pub financier:Box<Account<'info,Principal>>,
    /// CHECK: Registry binding and full graph for wallet profile; Core sentinel for escrow.
    pub wallet_binding:UncheckedAccount<'info>,
    /// CHECK: Retained Swig configuration authenticated by Registry's strict reader.
    pub wallet_config:UncheckedAccount<'info>,
    /// CHECK: Canonical Swig wallet or Core escrow authority; checked in handler.
    pub wallet_authority:UncheckedAccount<'info>,
    pub custody_token:Box<Account<'info,TokenAccount>>,
    #[account(init_if_needed,payer=payer,space=8+FinancierCustody::INIT_SPACE,
        seeds=[b"custody-reserve",custody_token.key().as_ref()],bump)]
    pub custody:Box<Account<'info,FinancierCustody>>,
    pub mint:Box<Account<'info,Mint>>,
    /// CHECK: Exact Pool id; new allocation requires executable current projection.
    #[account(address=POOL_ID)] pub pool_program:UncheckedAccount<'info>,
    pub system_program:Program<'info,System>,
    /// CHECK: ClaimsCore's event authority signs its event CPI.
    #[account(address=crate::EVENT_AUTHORITY_AND_BUMP.0)] pub core_event_authority:UncheckedAccount<'info>,
    /// CHECK: ClaimsCore itself, the event CPI's program.
    #[account(address=crate::ID)] pub core_program:UncheckedAccount<'info>,
}

#[derive(Accounts)]
#[instruction(facility_id:[u8;32])]
pub struct OpenFundingFacility<'info> {
    #[account(mut)] pub payer:Signer<'info>,
    /// CHECK: Fresh financier consent is authenticated in the handler.
    pub owner:UncheckedAccount<'info>,
    pub financier:Box<Account<'info,Principal>>,
    /// CHECK: Fresh issuer consent is authenticated in the handler.
    pub issuer_owner:UncheckedAccount<'info>,
    pub issuer:Box<Account<'info,Principal>>,
    pub custody:Box<Account<'info,FinancierCustody>>,
    #[account(init_if_needed,payer=payer,space=8+FundingFacility::INIT_SPACE,
        seeds=[b"funding-facility",financier.key().as_ref(),&facility_id],bump)]
    pub facility:Box<Account<'info,FundingFacility>>,
    #[account(init_if_needed,payer=payer,space=8+FacilityAuthority::INIT_SPACE,
        seeds=[b"facility-authority",facility.key().as_ref()],bump)]
    pub facility_authority:Box<Account<'info,FacilityAuthority>>,
    pub protocol_custody:Option<Box<Account<'info,FinancierCustody>>>,
    pub system_program:Program<'info,System>,
}

#[derive(Accounts)]
#[instruction(line_id:[u8;32])]
pub struct OpenCustomerCreditLine<'info> {
    #[account(mut)] pub payer:Signer<'info>,
    /// CHECK: Fresh company-owner consent is authenticated in the handler.
    pub owner:UncheckedAccount<'info>,
    pub company:Box<Account<'info,Principal>>,
    /// CHECK: Fresh issuer consent is authenticated in the handler.
    pub issuer_owner:UncheckedAccount<'info>,
    pub issuer:Box<Account<'info,Principal>>,
    pub issuer_custody:Box<Account<'info,FinancierCustody>>,
    pub facility:Box<Account<'info,FundingFacility>>,
    #[account(init_if_needed,payer=payer,space=8+CustomerCreditLine::INIT_SPACE,
        seeds=[b"credit-line",company.key().as_ref(),&line_id],bump)]
    pub credit_line:Box<Account<'info,CustomerCreditLine>>,
    pub protocol_custody:Option<Box<Account<'info,FinancierCustody>>>,
    pub system_program:Program<'info,System>,
}

#[derive(Accounts)]
#[instruction(request_id:[u8;32])]
pub struct ManageFundingFacility<'info> {
    #[account(mut)] pub payer:Signer<'info>,
    /// CHECK: Fresh changes require native or passkey current-owner approval;
    /// an exact historical receipt never restamps facility approval.
    pub owner:UncheckedAccount<'info>,
    pub financier:Box<Account<'info,Principal>>,
    #[account(mut)] pub facility:Box<Account<'info,FundingFacility>>,
    #[account(init_if_needed,payer=payer,space=8+FacilityAuthority::INIT_SPACE,
        seeds=[b"facility-authority",facility.key().as_ref()],bump)]
    pub facility_authority:Box<Account<'info,FacilityAuthority>>,
    pub system_program:Program<'info,System>,
    /// CHECK: ClaimsCore's event authority.
    #[account(address=crate::EVENT_AUTHORITY_AND_BUMP.0)] pub core_event_authority:UncheckedAccount<'info>,
    /// CHECK: ClaimsCore itself.
    #[account(address=crate::ID)] pub core_program:UncheckedAccount<'info>,
}

#[derive(Accounts)]
#[instruction(request_id:[u8;32],terms:TabTerms)]
pub struct AcceptFinancedTab<'info> {
    /// Pays the rent of the Tab and its budget hold, and gets it back when they close.
    #[account(mut)] pub payer:Signer<'info>,
    pub principal:Box<Account<'info,Principal>>,
    pub actor:Box<Account<'info,Actor>>,
    pub credential:Signer<'info>,
    /// Writable: the Registry admits the order number through the mandate's window.
    #[account(mut)] pub mandate:Box<Account<'info,Mandate>>,
    #[account(mut)] pub budget:Box<Account<'info,Budget>>,
    /// CHECK: Registry creates the Tab's budget hold at ["hold", mandate, order].
    #[account(mut)] pub reservation:UncheckedAccount<'info>,
    /// CHECK: Core's canonical Registry consumer signer.
    #[account(seeds=[REGISTRY_CONSUMER_SEED],bump)] pub registry_authority:UncheckedAccount<'info>,
    pub registry_program:Program<'info,PrincipalRegistry>,
    /// CHECK: Fresh issuer consent uses the shared native/passkey owner proof.
    pub issuer_owner:UncheckedAccount<'info>,
    pub issuer:Box<Account<'info,Principal>>,
    /// CHECK: Fresh supplier consent uses the shared owner proof. Ordinary
    /// external-key admission additionally requires a native on-curve key;
    /// passkey suppliers explicitly select continuing receiving custody.
    pub supplier:UncheckedAccount<'info>,
    pub supplier_principal:Box<Account<'info,Principal>>,
    pub fee_custody:Option<Box<Account<'info,FinancierCustody>>>,
    /// Shared supplier-approved lifetime capacity; omitted only when this Tab cannot charge a fee.
    #[account(mut)] pub seller_fee_agreement:Option<Box<Account<'info,SellerFeeAgreement>>>,
    #[account(mut)] pub credit_line:Box<Account<'info,CustomerCreditLine>>,
    #[account(mut)] pub facility:Box<Account<'info,FundingFacility>>,
    pub financier:Box<Account<'info,Principal>>,
    /// CHECK: Authenticated only for fresh risk, after the accepted receipt
    /// recovery branch. A missing new-risk approval cannot erase old acceptance.
    pub facility_authority:UncheckedAccount<'info>,
    #[account(mut)] pub custody:Box<Account<'info,FinancierCustody>>,
    pub custody_token:Box<Account<'info,TokenAccount>>,
    /// CHECK: Actual wallet graph is authenticated for new cash risk.
    pub wallet_binding:UncheckedAccount<'info>,
    /// CHECK: Actual retained Swig configuration; escrow uses Core sentinel.
    pub wallet_config:UncheckedAccount<'info>,
    /// CHECK: Actual wallet or escrow authority, bound to custody.
    pub wallet_authority:UncheckedAccount<'info>,
    /// The Tab, at ["tab", mandate, order]. A retried commit finds it and is a no-op.
    #[account(init_if_needed,payer=payer,space=8+FundedClaim::INIT_SPACE,
        seeds=[b"tab",terms.mandate.as_ref(),&terms.order.to_le_bytes()],bump)]
    pub claim:Box<Account<'info,FundedClaim>>,
    pub mint:Box<Account<'info,Mint>>,
    pub system_program:Program<'info,System>,
    /// CHECK: New risk requires this program and every affected projection.
    #[account(address=POOL_ID)] pub pool_program:UncheckedAccount<'info>,
    /// CHECK: ClaimsCore's event authority.
    #[account(address=crate::EVENT_AUTHORITY_AND_BUMP.0)] pub core_event_authority:UncheckedAccount<'info>,
    /// CHECK: ClaimsCore itself.
    #[account(address=crate::ID)] pub core_program:UncheckedAccount<'info>,
}

#[derive(Accounts)]
#[instruction(payment_id:[u8;32],namespace:[u8;32],run_id:[u8;32])]
pub struct EarnFinancedRun<'info> {
    /// Pays the run's rent and gets it back when the run closes.
    #[account(mut)] pub payer:Signer<'info>,
    pub supplier:Signer<'info>,
    pub supplier_principal:Box<Account<'info,Principal>>,
    pub principal:Box<Account<'info,Principal>>,
    #[account(mut)] pub budget:Box<Account<'info,Budget>>,
    #[account(mut)] pub reservation:Box<Account<'info,Reservation>>,
    /// CHECK: Canonical Core-to-Registry authority.
    #[account(seeds=[REGISTRY_CONSUMER_SEED],bump)] pub registry_authority:UncheckedAccount<'info>,
    pub registry_program:Program<'info,PrincipalRegistry>,
    #[account(mut)] pub line:Box<Account<'info,CustomerCreditLine>>,
    #[account(mut)] pub claim:Box<Account<'info,FundedClaim>>,
    #[account(mut)] pub custody:Box<Account<'info,FinancierCustody>>,
    pub custody_token:Box<Account<'info,TokenAccount>>,
    /// The run, under its Tab. A second worker billing the same run is a no-op.
    #[account(init_if_needed,payer=payer,space=8+FundedRun::INIT_SPACE,
        seeds=[b"run",claim.key().as_ref(),&run_id],bump)]
    pub run:Box<Account<'info,FundedRun>>,
    pub mint:Box<Account<'info,Mint>>,
    /// CHECK: Optional projection route; Core books remain mandatory.
    #[account(address=POOL_ID)] pub pool_program:UncheckedAccount<'info>,
    pub system_program:Program<'info,System>,
    /// CHECK: ClaimsCore's event authority.
    #[account(address=crate::EVENT_AUTHORITY_AND_BUMP.0)] pub core_event_authority:UncheckedAccount<'info>,
    /// CHECK: ClaimsCore itself.
    #[account(address=crate::ID)] pub core_program:UncheckedAccount<'info>,
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
    /// CHECK: ClaimsCore's event authority.
    #[account(address=crate::EVENT_AUTHORITY_AND_BUMP.0)] pub core_event_authority:UncheckedAccount<'info>,
    /// CHECK: ClaimsCore itself.
    #[account(address=crate::ID)] pub core_program:UncheckedAccount<'info>,
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
    /// CHECK: ClaimsCore's event authority; in the receipt's former position.
    #[account(address=crate::EVENT_AUTHORITY_AND_BUMP.0)] pub core_event_authority:UncheckedAccount<'info>,
    pub mint:Box<Account<'info,Mint>>,
    pub token_program:Program<'info,Token>,
    pub system_program:Program<'info,System>,
    /// CHECK: Accepted-cash servicing permits explicit no-Pool mode.
    #[account(address=POOL_ID)] pub pool_program:UncheckedAccount<'info>,
    /// CHECK: ClaimsCore itself.
    #[account(address=crate::ID)] pub core_program:UncheckedAccount<'info>,
}

/// Explicit owner-authorized withdrawal with an independent fee/rent payer.
/// The original twelve roles remain fixed for the retained Swig envelope;
/// authority and Instructions follow before the strict financial book suffix.
#[derive(Accounts)]
#[instruction(request_id:[u8;32])]
pub struct WithdrawFinancierFreeAuthorized<'info> {
    #[account(mut)] pub payer:Signer<'info>,
    pub financier_principal:Box<Account<'info,Principal>>,
    /// CHECK: Stable funding binding in wallet mode; Core sentinel for escrow.
    pub wallet_binding:UncheckedAccount<'info>,
    #[account(mut)] pub custody:Box<Account<'info,FinancierCustody>>,
    #[account(mut)] pub custody_token:Box<Account<'info,TokenAccount>>,
    /// CHECK: Strict Registry/Swig wrapper signs wallet custody; Core signs escrow.
    pub wallet_authority:UncheckedAccount<'info>,
    #[account(mut,token::mint=mint)] pub destination:Box<Account<'info,TokenAccount>>,
    /// CHECK: ClaimsCore's event authority; in the receipt's former position, so the
    /// Swig envelope's fixed role positions keep their meaning.
    #[account(address=crate::EVENT_AUTHORITY_AND_BUMP.0)] pub core_event_authority:UncheckedAccount<'info>,
    pub mint:Box<Account<'info,Mint>>,
    pub token_program:Program<'info,Token>,
    pub system_program:Program<'info,System>,
    /// CHECK: Explicit projection/service mode is authenticated by the journal.
    #[account(address=POOL_ID)] pub pool_program:UncheckedAccount<'info>,
    /// CHECK: Current native owner signer or exact passkey authority, after replay.
    pub authority:UncheckedAccount<'info>,
    /// CHECK: Canonical transaction introspection for the exact authorization.
    #[account(address=anchor_lang::compat::solana_instructions_sysvar::ID)]
    pub instructions:UncheckedAccount<'info>,
    /// CHECK: ClaimsCore itself.
    #[account(address=crate::ID)] pub core_program:UncheckedAccount<'info>,
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
    /// CHECK: Core consumer authority.
    #[account(seeds=[REGISTRY_CONSUMER_SEED],bump)] pub registry_authority:UncheckedAccount<'info>,
    pub registry_program:Program<'info,PrincipalRegistry>,
    #[account(mut)] pub line:Box<Account<'info,CustomerCreditLine>>,
    #[account(mut)] pub claim:Box<Account<'info,FundedClaim>>,
    #[account(mut)] pub facility:Box<Account<'info,FundingFacility>>,
    #[account(mut)] pub custody:Box<Account<'info,FinancierCustody>>,
    pub custody_token:Box<Account<'info,TokenAccount>>,
    pub mint:Box<Account<'info,Mint>>,
    /// CHECK: Optional projection route.
    #[account(address=POOL_ID)] pub pool_program:UncheckedAccount<'info>,
    pub system_program:Program<'info,System>,
    /// CHECK: ClaimsCore's event authority.
    #[account(address=crate::EVENT_AUTHORITY_AND_BUMP.0)] pub core_event_authority:UncheckedAccount<'info>,
    /// CHECK: ClaimsCore itself.
    #[account(address=crate::ID)] pub core_program:UncheckedAccount<'info>,
}

pub fn initialize_book<'info>(ctx:Context<'info, InitializeFinanceBook<'info>>,principal:Pubkey)->Result<()> {
    crate::wallet_tokens::read_mint(&ctx.accounts.mint.to_account_info(),
        *ctx.accounts.mint.to_account_info().owner,Clock::get()?.epoch)?;
    let b=&mut ctx.accounts.book;
    if b.data.version!=0 {
        require!(b.data.version==FUNDED_VERSION && b.data.principal==principal && b.data.mint==ctx.accounts.mint.key(),CoreError::FundingBookAccounts);
        return Ok(());
    }
    b.deposit_payer=ctx.accounts.payer.key();
    b.data=FinanceBookData{version:FUNDED_VERSION,bump:ctx.bumps.book,principal,mint:ctx.accounts.mint.key(),..FinanceBookData::default()};Ok(())
}

pub fn manage_facility<'info>(ctx:Context<'info, ManageFundingFacility<'info>>,request_id:[u8;32],expected_revision:u64,
    commitment_limit:u64,enabled:bool,authority_generation:u64)->Result<()> {
    let a=ctx.accounts;let id=operation_id(b"facility-policy",a.financier.key(),&request_id);
    // The digest leaves out the revision guard, so a rebuilt request is the same operation.
    let digest=financial_digest(b"DEXTER:FACILITY-POLICY:2",&(a.facility.key(),a.financier.key(),a.owner.key(),
        commitment_limit,enabled,authority_generation))?;
    let rid=tab_events::record_operation_id(&id,&digest);
    a.facility.check(a.facility.key())?;
    // Not a money move: a retry after it landed is refused by the revision guard.
    owner_authority(&a.financier,&a.owner,authority_generation,ctx.remaining_accounts)?;
    require!(a.facility.financier==a.financier.key() && a.facility.revision==expected_revision,CoreError::FundingRevision);
    let next=plus(expected_revision,1)?;
    a.facility_authority.set_inner(FacilityAuthority{deposit_payer:if a.facility_authority.version!=0{a.facility_authority.deposit_payer}else{a.payer.key()},version:1,bump:ctx.bumps.facility_authority,
        facility:a.facility.key(),financier:a.financier.key(),authority_generation,approval_digest:digest});
    let event=tab_events::FacilityPolicyChanged{operation:rid,facility:a.facility.key(),
        request_id,request_digest:digest,owner:a.owner.key(),
        authority_generation,previous_revision:expected_revision,resulting_revision:next,
        previous_limit:a.facility.commitment_limit,resulting_limit:commitment_limit,
        previously_enabled:a.facility.accepts_new,resulting_enabled:enabled};
    // A lower limit may be below existing exposure. It fences new admission,
    // never erases debt, reserved cash, supplier rights or the original terms.
    a.facility.commitment_limit=commitment_limit;a.facility.accepts_new=enabled;a.facility.revision=next;
    tab_events::emit_event(&a.core_event_authority,EventBytes::new::<tab_events::FacilityPolicyChanged>(256).put(&event)?)
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
    let rid=tab_events::record_operation_id(&id,&digest);
    // A custody is created once: a retry after it landed finds it and is refused.
    require!(a.custody.version==0,CoreError::FundingCustody);
    owner_authority(&a.financier,&a.owner,authority_generation,ctx.remaining_accounts)?;
    let remaining=financial_remaining_accounts(ctx.remaining_accounts)?;
    check_wallet_graph(profile,&a.wallet_binding,&a.wallet_config,&a.wallet_authority,a.financier.key(),a.mint.key())?;
    require!(a.custody_token.owner==a.wallet_authority.key() && a.custody_token.mint==a.mint.key(),CoreError::FundingTokenRouting);
    require_keys_eq!(a.custody_token.key(),associated_token::get_associated_token_address(&a.wallet_authority.key(),&a.mint.key()),CoreError::FundingTokenRouting);
    admit_cash(&a.custody_token)?;
    require!(allocated_cash<=a.custody_token.amount && (allocated_cash==0 || provenance_hash!=[0;32]),CoreError::FundingCashShortfall);
    a.custody.set_inner(FinancierCustody{deposit_payer:a.payer.key(),version:FUNDED_VERSION,bump:ctx.bumps.custody,
        domain:a.financier.domain,
        principal:a.financier.key(),binding:a.wallet_binding.key(),wallet:a.wallet_authority.key(),token_account:a.custody_token.key(),
        mint:a.mint.key(),profile,accounted_cash:allocated_cash,unused:0,payable:0,contributed_capital:allocated_cash,
        withdrawn_cash:0,revision:1,token_deposit_payer:Pubkey::default(),
        window:dexter_contracts::window::OrderWindow::default()});
    let mut data=record_base(a.financier.key(),id,digest,INITIAL_CAPITAL,a.mint.key(),Pubkey::default(),Pubkey::default(),allocated_cash);
    data.capital_custody=a.custody.key();data.capital_allocated=allocated_cash;data.capital_provenance_hash=provenance_hash;
    data.capital_authority=a.owner.key();data.capital_owner_generation=authority_generation;
    let booked=journal(&rid,a.mint.key(),&[inc(a.financier.key(),CAPITAL_ALLOCATED,allocated_cash)],remaining,
        &a.pool_program,false,true)?;
    emit_record_operation(&a.core_event_authority,OPERATION_CUSTODY_CREATED,&rid,&a.custody.key(),
        0,&data,&booked.books,&booked.changes)
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
    let a=ctx.accounts;
    valid_principal(&a.financier)?;valid_principal(&a.issuer)?;
    a.custody.check_key(a.custody.key())?;
    // Wallet Token-2022 support is not an admission policy for supplier backing.
    require!(a.custody.profile==WALLET_CASH || a.custody.profile==ESCROW_CASH,
        CoreError::FundingAdmission);
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
    owner_authority(&a.financier,&a.owner,financier_generation,ctx.remaining_accounts)?;
    owner_authority(&a.issuer,&a.issuer_owner,issuer_generation,ctx.remaining_accounts)?;
    require!(accept_until>Clock::get()?.unix_timestamp,CoreError::FundingAdmission);
    a.facility_authority.set_inner(FacilityAuthority{deposit_payer:if a.facility_authority.version!=0{a.facility_authority.deposit_payer}else{a.payer.key()},version:1,bump:ctx.bumps.facility_authority,
        facility:a.facility.key(),financier:a.financier.key(),authority_generation:financier_generation,
        approval_digest:opening_hash});
    a.facility.set_inner(FundingFacility{deposit_payer:a.payer.key(),version:FUNDED_VERSION,bump:ctx.bumps.facility,facility_id,
        financier:a.financier.key(),issuer:a.issuer.key(),custody:a.custody.key(),mint:a.custody.mint,external,
        commitment_limit,reserved:0,drawn:0,repaid:0,accepts_new:true,accept_until,revision:1,terms_hash,opening_hash,
        interest:math(InterestState::new(Clock::get()?.unix_timestamp,&interest_terms))?,interest_terms,
        });Ok(())
}

pub fn open_credit_line<'info>(ctx:Context<'info, OpenCustomerCreditLine<'info>>,line_id:[u8;32],limit:u64,
    interest_terms:InterestTerms,assignment_policy_hash:[u8;32],owner_generation:u64,issuer_generation:u64)->Result<()> {
    let a=ctx.accounts;
    valid_principal(&a.company)?;valid_principal(&a.issuer)?;
    require!(a.company.domain==a.issuer.domain && a.company.key()!=a.issuer.key(),CoreError::FundingCreditLine);
    a.issuer_custody.check_key(a.issuer_custody.key())?;a.facility.check(a.facility.key())?;
    require!(a.issuer_custody.profile==WALLET_CASH || a.issuer_custody.profile==ESCROW_CASH,
        CoreError::FundingAdmission);
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
    owner_authority(&a.company,&a.owner,owner_generation,ctx.remaining_accounts)?;
    owner_authority(&a.issuer,&a.issuer_owner,issuer_generation,ctx.remaining_accounts)?;
    a.credit_line.set_inner(CustomerCreditLine{deposit_payer:a.payer.key(),version:FUNDED_VERSION,bump:ctx.bumps.credit_line,line_id,
        company:a.company.key(),issuer:a.issuer.key(),issuer_custody:a.issuer_custody.key(),facility:a.facility.key(),mint:a.facility.mint,
        limit,reserved:0,principal_outstanding:0,fee_exposure:0,allocated_fee_caps:0,interest_terms,enabled:true,revision:1,assignment_policy_hash});Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn accept<'info>(ctx:Context<'info, AcceptFinancedTab<'info>>,request_id:[u8;32],terms:TabTerms,maximum_interest:u64,
    seller_fee_terms:SellerFeeTerms,assignment_policy_hash:[u8;32],namespace:[u8;32],service_unit:[u8;32],
    unit_price:u64,external_contract_hash:[u8;32],order_terms:Option<Vec<Term>>)->Result<()> {
    require!(namespace != dexter_contracts::billing::BILLING_NAMESPACE, CoreError::BillingProfile);
    accept_bound(ctx.accounts,ctx.bumps,ctx.remaining_accounts,request_id,terms,maximum_interest,seller_fee_terms,assignment_policy_hash,
        namespace,service_unit,unit_price,external_contract_hash,order_terms)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn accept_bound<'info>(a:&mut AcceptFinancedTab<'info>,bumps:AcceptFinancedTabBumps,
    remaining_accounts:&[AccountInfo<'info>],request_id:[u8;32],terms:TabTerms,maximum_interest:u64,
    seller_fee_terms:SellerFeeTerms,assignment_policy_hash:[u8;32],namespace:[u8;32],service_unit:[u8;32],
    unit_price:u64,external_contract_hash:[u8;32],order_terms:Option<Vec<Term>>)->Result<()> {
    // The ordinary profile fixes an externally controlled payout key. An
    // immutable passkey alias is an authorization identity, not an SPL custody
    // signer; paying its ATA would silently create an unusable route.
    require!(a.supplier.key().is_on_curve(),CoreError::FundingOwner);
    let payee=a.supplier.key();
    accept_bound_payee(a,bumps,remaining_accounts,request_id,terms,maximum_interest,seller_fee_terms,
        assignment_policy_hash,namespace,service_unit,unit_price,external_contract_hash,payee,
        order_terms.map_or(OrderTerms::Hidden,OrderTerms::Run))
}

/// What an order shows the Registry. Without a list, the Registry refuses a
/// permission whose owner pinned terms. `Run`: the list hashes to the order's
/// `external_contract_hash`. `Billing`: the list hashes to the policy's
/// `request_hash`, and the policy's source is the delivery signer it names.
pub(crate) enum OrderTerms<'p> { Hidden, Run(Vec<Term>), Billing { bill:Pubkey, policy:&'p BillingPolicy, terms:Vec<Term> } }

/// The recovery profile authenticates its continuing custody before entering
/// here. Ordinary admission supplies the exact external supplier signer.
#[allow(clippy::too_many_arguments)]
pub(crate) fn accept_bound_payee<'info>(a:&mut AcceptFinancedTab<'info>,bumps:AcceptFinancedTabBumps,
    remaining_accounts:&[AccountInfo<'info>],request_id:[u8;32],terms:TabTerms,maximum_interest:u64,
    seller_fee_terms:SellerFeeTerms,assignment_policy_hash:[u8;32],namespace:[u8;32],service_unit:[u8;32],
    unit_price:u64,external_contract_hash:[u8;32],payee:Pubkey,order_terms:OrderTerms<'_>)->Result<()> {
    let id=operation_id(b"accept",a.credential.key(),&request_id);
    let digest=financial_digest(b"DEXTER:ACCEPT-FINANCED-TAB:3",&(&terms,maximum_interest,&seller_fee_terms,
        assignment_policy_hash,a.credit_line.key(),a.facility.key(),a.custody.key(),a.supplier_principal.key(),
        namespace,service_unit,unit_price,external_contract_hash,a.seller_fee_agreement.as_ref().map(|x|x.key())))?;
    let sid=tab_events::step_id(&id,&digest);
    // A shown list is the one the order committed to: its external contract
    // hash, or its billing policy's request hash.
    let order_terms_hash=match &order_terms {
        OrderTerms::Hidden=>None,
        OrderTerms::Run(terms)=>Some((principal_registry::order_terms_hash(terms),external_contract_hash)),
        OrderTerms::Billing{policy,terms,..}=>Some((principal_registry::order_terms_hash(terms),policy.request_hash)),
    };
    // The Tab exists: its order number is used (the mandate's window refuses
    // it too). A landed commit is never applied again; the SDK reads the Tab
    // or the mandate's window. The Registry compares a shown list with the
    // permission's pinned terms and the order's hash.
    require!(a.claim.version==0,CoreError::NumberUsed);
    valid_principal(&a.principal)?;valid_principal(&a.issuer)?;valid_principal(&a.supplier_principal)?;
    require!(a.issuer.owner==a.issuer_owner.key() && a.supplier_principal.owner==a.supplier.key(),CoreError::FundingOwner);
    principal_registry::require_owner_authority(&a.issuer,&a.issuer_owner,remaining_accounts)?;
    principal_registry::require_owner_authority(&a.supplier_principal,&a.supplier,remaining_accounts)?;
    let financial_accounts=financial_remaining_accounts(remaining_accounts)?;
    require!(a.principal.domain==a.issuer.domain && a.principal.domain==a.supplier_principal.domain,CoreError::FundingPrincipal);
    require!(terms.domain==a.principal.domain && terms.principal==a.principal.key() && terms.actor==a.actor.key()
        && terms.mandate==a.mandate.key() && terms.asset==a.mint.key() && terms.supplier==payee
        && terms.source==a.facility.key() && terms.cap>0 && terms.expires_at>Clock::get()?.unix_timestamp,CoreError::FundingTerms);
    require!(unit_price>0 && terms.external_terms_hash==contracts::completed_run_policy(&namespace,&service_unit,unit_price,&external_contract_hash),CoreError::FundingUsage);
    a.credit_line.check(a.credit_line.key())?;a.facility.check(a.facility.key())?;
    {
        require_keys_eq!(*a.facility_authority.owner,CORE_ID,CoreError::FundingAuthorityGeneration);
        let data=a.facility_authority.try_borrow_data()?;
        let authority=FacilityAuthority::try_deserialize(&mut data.as_ref())
            .map_err(|_|error!(CoreError::FundingAuthorityGeneration))?;
        authority.check(a.facility_authority.key(),a.facility.key(),&a.financier)?;
    }
    require!(a.facility.financier==a.financier.key(),CoreError::FundingAuthorityGeneration);
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
    let accepted=AcceptArgs{operation_id:id,terms:authorized};
    reserve(a,bumps.registry_authority,&accepted,&order_terms,namespace,service_unit,unit_price)?;
    a.credit_line.reserved=plus(a.credit_line.reserved,terms.cap)?;
    a.credit_line.fee_exposure=plus(a.credit_line.fee_exposure,maximum_interest)?;
    a.credit_line.allocated_fee_caps=plus(a.credit_line.allocated_fee_caps,maximum_interest)?;
    a.credit_line.revision=plus(a.credit_line.revision,1)?;
    a.facility.reserved=plus(a.facility.reserved,terms.cap)?;a.facility.revision=plus(a.facility.revision,1)?;
    a.custody.unused=plus(a.custody.unused,terms.cap)?;a.custody.revision=plus(a.custody.revision,1)?;
    let mut interest_terms=a.credit_line.interest_terms.clone();interest_terms.maximum_interest=maximum_interest;
    let claim_key=a.claim.key();let company=terms.principal;let issuer=a.issuer.key();let supplier=a.supplier_principal.key();
    let amount=terms.cap;let order=terms.order;
    let slot=Clock::get()?.slot;
    init_tab(&mut a.claim,TabInit{deposit_payer:a.payer.key(),bump:bumps.claim,terms,namespace,service_unit,unit_price,
        external_contract_hash,acceptance_hash:digest,credit_line:a.credit_line.key(),assignment_policy_hash,
        facility:a.facility.key(),custody:a.custody.key(),issuer,issuer_custody:a.credit_line.issuer_custody,
        supplier_principal:supplier,maximum_interest,interest_terms,seller_fee_agreement,seller_fee_allocation,
        seller_fee_terms,fingerprint:tab_events::tab_genesis(&claim_key,&id,slot)});
    a.claim.check(claim_key)?;
    let changes=[inc(company,CUSTOMER_RESERVED,amount),inc(company,CREDIT_CHARGE_EXPOSURE,maximum_interest),
        inc(issuer,UNUSED_SUPPLIER,amount),inc(supplier,SUPPLIER_COMMITTED,amount),inc(a.custody.principal,FUNDING_RESERVED,amount)];
    let data=record_base(company,id,digest,FINANCE_ACCEPT,a.mint.key(),claim_key,Pubkey::default(),amount);
    let booked=journal(&sid,a.mint.key(),&changes,financial_accounts,&a.pool_program,false,true)?;
    let header=TabStepHeader{tab:claim_key,step:a.claim.next_step()?,step_id:sid};
    let event=committed_event(&header,slot,order,&a.claim,order_terms_hash.map(|(shown,_)|shown),&data,&booked)?;
    tab_events::record_tab_step(&mut a.claim,&a.core_event_authority,event, true)
}

/// The Registry hold for a commit: `reserve`, `reserve_pinned` or
/// `reserve_pinned_billing`, the instruction Anchor's generated CPI sends (same
/// discriminator, Borsh arguments and account metas), with the data and account
/// list each allocated once at their final size. The generated CPI grows its
/// data from 256 bytes and both account lists from empty, and the bump heap
/// keeps every outgrown buffer: about 6.5 KB for a commit that shows its terms.
#[inline(never)]
fn reserve<'info>(a:&AcceptFinancedTab<'info>,bump:u8,accepted:&AcceptArgs,order_terms:&OrderTerms<'_>,
    namespace:[u8;32],service_unit:[u8;32],unit_price:u64)->Result<()> {
    use anchor_lang::{Discriminator,solana_program::{instruction::{AccountMeta,Instruction},program::invoke_signed}};
    use principal_registry::instruction as registry;
    let claim=a.claim.key();
    let data=match order_terms {
        OrderTerms::Hidden=>exact_data(registry::Reserve::DISCRIMINATOR,accepted)?,
        OrderTerms::Run(terms)=>exact_data(registry::ReservePinned::DISCRIMINATOR,
            &(accepted,&namespace,&service_unit,&unit_price,terms))?,
        OrderTerms::Billing{bill,policy,terms}=>exact_data(registry::ReservePinnedBilling::DISCRIMINATOR,
            &(accepted,bill,&claim,*policy,terms))?,
    };
    let accounts=vec![AccountMeta::new_readonly(a.registry_authority.key(),true),AccountMeta::new(a.payer.key(),true),
        AccountMeta::new_readonly(a.principal.key(),false),AccountMeta::new_readonly(a.actor.key(),false),
        AccountMeta::new_readonly(a.credential.key(),true),AccountMeta::new(a.mandate.key(),false),
        AccountMeta::new(a.budget.key(),false),AccountMeta::new(a.reservation.key(),false),
        AccountMeta::new_readonly(a.system_program.key(),false)];
    let infos=[a.registry_authority.to_account_info(),a.payer.to_account_info(),a.principal.to_account_info(),
        a.actor.to_account_info(),a.credential.to_account_info(),a.mandate.to_account_info(),a.budget.to_account_info(),
        a.reservation.to_account_info(),a.system_program.to_account_info()];
    invoke_signed(&Instruction{program_id:a.registry_program.key(),accounts,data},&infos,
        &[&[REGISTRY_CONSUMER_SEED,&[bump]]])?;
    Ok(())
}

#[inline(never)]
fn committed_event(header:&TabStepHeader,slot:u64,order:u64,tab:&FundedClaim,order_terms_hash:Option<[u8;32]>,
    detail:&FinanceDetail,booked:&crate::funding_journal::Booked)->Result<EventBytes> {
    EventBytes::new::<TabCommitted>(TabStepHeader::INIT_SPACE+16+FundedClaim::INIT_SPACE+1+32*order_terms_hash.is_some() as usize
        +DETAIL_SPACE+books_space(&booked.books,&booked.changes))
        .put(header)?.put(&slot)?.put(&order)?.put(tab)?.put(&order_terms_hash)?.put(detail)?
        .put(&booked.books)?.put(&booked.changes)
}

/// The Tab's committed fields. Built in its own frame: the Tab is large enough
/// that building it inline overflows the commit handler's 4 KB SBF frame.
pub(crate) struct TabInit {
    pub deposit_payer:Pubkey,pub bump:u8,pub terms:TabTerms,pub namespace:[u8;32],pub service_unit:[u8;32],
    pub unit_price:u64,pub external_contract_hash:[u8;32],pub acceptance_hash:[u8;32],pub credit_line:Pubkey,
    pub assignment_policy_hash:[u8;32],pub facility:Pubkey,pub custody:Pubkey,pub issuer:Pubkey,
    pub issuer_custody:Pubkey,pub supplier_principal:Pubkey,pub maximum_interest:u64,
    pub interest_terms:InterestTerms,pub seller_fee_agreement:Pubkey,pub seller_fee_allocation:u64,
    pub seller_fee_terms:SellerFeeTerms,pub fingerprint:[u8;32],
}

#[inline(never)]
pub(crate) fn init_tab(claim:&mut Account<FundedClaim>,t:TabInit) {
    let amount=t.terms.cap;
    claim.set_inner(FundedClaim{deposit_payer:t.deposit_payer,version:FUNDED_CLAIM_VERSION,bump:t.bump,terms:t.terms,
        usage_namespace:t.namespace,service_unit:t.service_unit,unit_price:t.unit_price,
        external_contract_hash:t.external_contract_hash,acceptance_hash:t.acceptance_hash,credit_line:t.credit_line,
        assignment_policy_hash:t.assignment_policy_hash,facility:t.facility,custody:t.custody,issuer:t.issuer,
        issuer_custody:t.issuer_custody,supplier_principal:t.supplier_principal,
        remaining:amount,payable:0,debt:0,paid:0,repaid:0,released:0,fee_cap:t.maximum_interest,
        fee_authority_remaining:t.maximum_interest,allocated_run_fee_caps:0,released_fee_caps:0,released_unallocated_fee_caps:0,
        interest_outstanding:0,interest_terms:t.interest_terms,seller_fee_agreement:t.seller_fee_agreement,
        seller_fee_allocation:t.seller_fee_allocation,seller_fee_terms:t.seller_fee_terms,
        seller_fee_state:SellerFeeState::default(),principal_extinguished:0,corrections:Default::default(),
        steps:0,money_steps:0,fingerprint:t.fingerprint,open_runs:0,open_records:0,
        hold:crate::settlement_hold::SettlementHold::default(),buyer_hold_used:false,seller_hold_used:false});
}

#[allow(clippy::too_many_arguments)]
pub fn earn<'info>(
    ctx: Context<'info, EarnFinancedRun<'info>>,
    payment_id: [u8; 32], namespace: [u8; 32], run_id: [u8; 32],
    service_unit: [u8; 32], external_contract_hash: [u8; 32],
    units: u64, unit_price: u64, expected_steps: u32, service_without_pool: bool,
) -> Result<()> {
    let a = ctx.accounts;
    a.claim.check(a.claim.key())?;
    require!(a.claim.usage_namespace != dexter_contracts::billing::BILLING_NAMESPACE,
        CoreError::BillingProfile);
    // Explicit trusted supplier reporting retains its current-owner authority.
    valid_principal(&a.supplier_principal)?;
    require!(a.supplier_principal.key() == a.claim.supplier_principal
        && a.supplier_principal.owner == a.supplier.key(), CoreError::FundingOwner);
    let capture_digest = financial_digest(b"DEXTER:FINANCED-COMPLETED-RUN:1",
        &(a.claim.key(), namespace, run_id, service_unit, external_contract_hash, units, unit_price))?;
    let operation_id = funded_run_operation_id(&a.claim.supplier_principal, &namespace, &run_id);
    let sid = tab_events::step_id(&operation_id, &capture_digest);
    // A run is billed once: billing an existing run is refused, whoever sends
    // it. The SDK reads the run (its capture digest) to tell its own landed
    // billing from a conflicting one.
    require!(a.run.version == 0, CoreError::FundingReplay);
    a.claim.guard(expected_steps)?;
    let earned = crate::funding_servicing::establish_run(crate::funding_servicing::EarnedRunAccounts {
        payer:&a.payer, principal:&a.principal, budget:&mut a.budget, reservation:&mut a.reservation,
        registry_authority:&a.registry_authority,
        registry_program:&a.registry_program, line:&mut a.line, claim:&mut a.claim,
        custody:&mut a.custody, custody_token:&a.custody_token, run:&mut a.run, mint:&a.mint,
        pool_program:&a.pool_program,
    }, crate::funding_servicing::EarnedRunTerms {payment_id,namespace,run_id,service_unit,
        external_contract_hash,units,unit_price,service_without_pool,capture_digest,
        enforce_deadline:true}, &sid, ctx.bumps.registry_authority,ctx.bumps.run,
        ctx.remaining_accounts)?;
    let header=TabStepHeader{tab:a.claim.key(),step:a.claim.next_step()?,step_id:sid};
    let event=tab_events::extra_event::<RunEarned,_>(&header,&earned.earn,EarnDetail::INIT_SPACE,&earned.detail,
        &earned.booked.books,&earned.booked.changes)?;
    tab_events::record_tab_step(&mut a.claim,&a.core_event_authority,event, true)
}

pub fn withdraw<'info>(ctx:Context<'info, WithdrawFinancierFree<'info>>,request_id:[u8;32],amount:u64,number:u64,
    authority_generation:u64,service_without_pool:bool)->Result<()> {
    let a=ctx.accounts;let id=operation_id(b"withdraw",a.financier_principal.key(),&request_id);
    let digest=financial_digest(b"DEXTER:FINANCIER-FREE-WITHDRAWAL:1",&(id,a.financier_principal.key(),a.payer.key(),
        a.custody.key(),a.destination.key(),amount,authority_generation))?;
    let rid=tab_events::record_operation_id(&id,&digest);
    a.custody.check_key(a.custody.key())?;
    // The owner's number, admitted once by the custody's window: a landed
    // withdrawal, or a rebuild of it, is refused.
    admit_number(&mut a.custody.window,number)?;
    owner(&a.financier_principal,a.payer.key(),authority_generation)?;
    require!(amount>0 && a.custody.principal==a.financier_principal.key(),CoreError::FundingRevision);
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
    let booked=journal(&rid,a.mint.key(),&changes,ctx.remaining_accounts,&a.pool_program,service_without_pool,false)?;
    emit_record_operation(&a.core_event_authority,FINANCE_WITHDRAW,&rid,&a.custody.key(),number,
        &data,&booked.books,&booked.changes)
}

#[allow(clippy::too_many_arguments)]
pub fn withdraw_authorized<'info>(ctx:Context<'info,WithdrawFinancierFreeAuthorized<'info>>,
    request_id:[u8;32],amount:u64,number:u64,authority_generation:u64,
    service_without_pool:bool,destination_authority:Pubkey)->Result<()> {
    let a=ctx.accounts;
    let id=operation_id(b"withdraw",a.financier_principal.key(),&request_id);
    let digest=financial_digest(b"DEXTER:FINANCIER-FREE-WITHDRAWAL:2",&(id,
        a.financier_principal.key(),a.authority.key(),a.custody.key(),a.destination.key(),
        destination_authority,amount,authority_generation))?;
    // A new payer or recovered current credential cannot repeat the effect:
    // the owner's number is admitted once by the custody's window.
    let rid=tab_events::record_operation_id(&id,&digest);
    a.custody.check_key(a.custody.key())?;
    admit_number(&mut a.custody.window,number)?;
    owner(&a.financier_principal,a.authority.key(),authority_generation)?;
    require!(!a.instructions.is_writable && !a.instructions.is_signer
        && !a.instructions.executable,CoreError::FundingWalletAuthority);
    if a.custody.profile==WALLET_CASH {
        // Core is the bounded inner call of the exact Swig operation already
        // approved by Registry; arbitrary CPIs cannot reuse a stored proof.
        principal_registry::require_wallet_owner_authority(&a.financier_principal,
            &a.authority,&a.instructions)?;
    } else {
        require!(a.custody.profile==ESCROW_CASH,CoreError::FundingWalletAuthority);
        principal_registry::require_owner_authority(&a.financier_principal,
            &a.authority,&[a.instructions.to_account_info()])?;
    }
    require!(amount>0
        && a.custody.principal==a.financier_principal.key()
        && a.custody.domain==a.financier_principal.domain,CoreError::FundingRevision);
    custody_token(&a.custody,&a.custody_token,a.mint.key())?;
    require!(destination_authority!=Pubkey::default() && destination_authority.is_on_curve()
        && a.destination.owner==destination_authority
        && a.destination.key()==associated_token::get_associated_token_address(&destination_authority,&a.mint.key())
        && a.destination.key()!=a.custody_token.key(),CoreError::FundingTokenRouting);
    a.custody.debit_uncommitted(a.custody_token.amount,amount)?;
    // The common transfer validates the canonical custody authority and exact
    // source debit/destination credit, including rejection of self-transfers.
    crate::funding_collection::transfer_from_custody(&a.custody,&a.wallet_binding,&a.wallet_authority,
        &a.custody_token.to_account_info(),&a.destination.to_account_info(),&a.mint,a.token_program.key(),amount)?;
    a.custody_token.reload()?;a.destination.reload()?;
    a.custody.check(a.custody_token.amount)?;
    let mut data=record_base(a.financier_principal.key(),id,digest,FINANCE_WITHDRAW,a.mint.key(),
        Pubkey::default(),Pubkey::default(),amount);
    data.debit_principal=a.financier_principal.key();data.debit_authority=a.wallet_authority.key();
    data.debit_from=a.custody_token.key();
    let changes=[inc(a.financier_principal.key(),CASH_SENT,amount)];
    let booked=journal(&rid,a.mint.key(),&changes,ctx.remaining_accounts,&a.pool_program,service_without_pool,false)?;
    emit_record_operation(&a.core_event_authority,FINANCE_WITHDRAW,&rid,&a.custody.key(),number,
        &data,&booked.books,&booked.changes)
}

pub fn release<'info>(ctx:Context<'info, ReleaseFinancierUnused<'info>>,request_id:[u8;32],amount:u64,expected_steps:u32,
    service_without_pool:bool,number:u64)->Result<()> {
    let a=ctx.accounts;
    // The evidence profile can release only inside its atomic finalizer.
    require!(a.claim.usage_namespace != dexter_contracts::billing::BILLING_NAMESPACE,
        CoreError::BillingProfile);
    let claim_key=a.claim.key();
    let id=release_operation_id(claim_key,a.claim.supplier_principal,&request_id);
    let digest=crate::funding_servicing::release_digest(claim_key,&id,amount,number)?;
    let sid=tab_events::step_id(&id,&digest);
    a.claim.guard(expected_steps)?;
    let surrendering=Clock::get()?.unix_timestamp <= a.claim.terms.expires_at;
    // The supplier chooses how much it surrenders before expiry, so the surrender
    // is numbered by its book window; after expiry anyone may release what the
    // Tab can no longer use, and there is nothing to number.
    let numbered=if surrendering { Some((a.claim.supplier_principal,number)) } else {
        require!(number==0,CoreError::FundingRevision); None };
    let released=crate::funding_servicing::release_unused(crate::funding_servicing::ReleaseAccounts {
        principal:&a.principal,budget:&mut a.budget,reservation:&mut a.reservation,
        registry_authority:&a.registry_authority,
        registry_program:&a.registry_program,line:&mut a.line,claim:&mut a.claim,
        facility:&mut a.facility,custody:&mut a.custody,custody_token:&a.custody_token,
        mint:&a.mint,pool_program:&a.pool_program,
    },id,&sid,amount,service_without_pool,ctx.bumps.registry_authority,
        ctx.remaining_accounts,Some((&a.supplier_principal,a.supplier.as_ref())),numbered)?;
    let header=TabStepHeader{tab:claim_key,step:a.claim.next_step()?,step_id:sid};
    let event=tab_events::extra_event::<UnusedReleased,_>(&header,&(surrendering,number),1+8,&released.0,
        &released.1.books,&released.1.changes)?;
    tab_events::record_tab_step(&mut a.claim,&a.core_event_authority,event, true)
}
