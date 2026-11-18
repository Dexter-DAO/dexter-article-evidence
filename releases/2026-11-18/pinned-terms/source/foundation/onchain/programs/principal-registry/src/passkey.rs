//! Native WebAuthn authorization for an exact top-level Account operation.
//! An immutable key alias is an identity, never a stored bearer permission.
use anchor_lang::prelude::*;
use anchor_lang::solana_program::instruction::{get_stack_height, Instruction, TRANSACTION_LEVEL_STACK_HEIGHT};
use dexter_contracts::{plus, CORE_ID, SCHEMA_VERSION};
use solana_instructions_sysvar::{load_current_index_checked, load_instruction_at_checked};
use solana_sha256_hasher::hashv;
use crate::{Principal, RegistryError};

pub const PASSKEY_VERSION:u16=1;
pub const AUTHORITY_DEPLOYMENT_SEED:&[u8]=b"passkey-deployment";
pub const PASSKEY_AUTHORITY_SEED:&[u8]=b"passkey-authority";
pub const PASSKEY_OWNER:u8=1;
pub const PASSKEY_POSSESSION:u8=2;
pub const PASSKEY_GUARDIAN:u8=3;
pub const MAX_PASSKEY_PROOFS:usize=12;
pub const INSTRUCTIONS_KEY:Pubkey=pubkey!("Sysvar1nstructions1111111111111111111111111");
const R1:Pubkey=pubkey!("Secp256r1SigVerify1111111111111111111111111");
const LOADER:Pubkey=pubkey!("BPFLoaderUpgradeab1e11111111111111111111111");

#[account]
#[derive(InitSpace)]
pub struct AuthorityDeployment {
    pub version:u16,pub bump:u8,pub genesis_hash:[u8;32],
    pub attesting_upgrade_authority:Pubkey,pub configured_at:i64,
}

#[account]
#[derive(InitSpace)]
pub struct PasskeyAuthority {
    pub version:u16,pub bump:u8,pub principal:Pubkey,pub domain:[u8;32],
    pub genesis_hash:[u8;32],pub public_key:[u8;33],
    /// Monotonic; neither credential reappointment nor recovery resets it.
    pub nonce:u64,pub last_authorization_index:u16,pub last_target_index:u16,
    pub last_target_digest:[u8;32],pub last_role:u8,pub last_generation:u64,
    pub last_expires_at:i64,pub last_ceremony_nonce:[u8;32],
    pub last_client_data_hash:[u8;32],pub last_sign_count:u32,pub last_flags:u8,
}

#[derive(AnchorSerialize,AnchorDeserialize,Clone,Debug,PartialEq,Eq,InitSpace)]
pub struct InitializePasskeyAuthorityArgs {
    pub domain:[u8;32],pub principal_id:[u8;32],pub public_key:[u8;33],
}

#[derive(AnchorSerialize,AnchorDeserialize,Clone,Debug,PartialEq,Eq)]
pub struct AuthorizePasskeyOperationArgs {
    pub nonce:u64,pub role:u8,pub authority_generation:u64,pub target_index:u16,
    pub expires_at:i64,pub ceremony_nonce:[u8;32],pub client_data_json:Vec<u8>,
}

#[derive(AnchorSerialize,AnchorDeserialize,Clone,Debug,PartialEq,Eq,InitSpace)]
pub struct CreatePasskeyPrincipalArgs {
    pub principal_id:[u8;32],pub domain:[u8;32],pub kind:u8,
    pub governing_policy:[u8;32],pub public_key:[u8;33],
}

pub fn passkey_key_hash(key:&[u8;33])->[u8;32] {hashv(&[key]).to_bytes()}
pub fn passkey_authority_address(principal:&Pubkey,key:&[u8;33])->(Pubkey,u8) {
    Pubkey::find_program_address(&[PASSKEY_AUTHORITY_SEED,principal.as_ref(),&passkey_key_hash(key)],&crate::ID)
}
pub fn passkey_seed_owner(domain:&[u8;32],key:&[u8;33])->Pubkey {
    Pubkey::new_from_array(hashv(&[b"DEXTER:PASSKEY-SEED:1",crate::ID.as_ref(),domain,key]).to_bytes())
}
pub fn passkey_principal_address(domain:&[u8;32],id:&[u8;32],key:&[u8;33])->(Pubkey,u8) {
    Pubkey::find_program_address(&[b"principal",passkey_seed_owner(domain,key).as_ref(),id],&crate::ID)
}

#[derive(Accounts)]
pub struct InitializeAuthorityDeployment<'info> {
    #[account(mut)] pub payer:Signer<'info>,
    pub upgrade_authority:Signer<'info>,
    /// CHECK: Canonical executable/loader/ProgramData relationship below.
    #[account(address=crate::ID)] pub program:UncheckedAccount<'info>,
    /// CHECK: Canonical loader-owned ProgramData and actual upgrade authority below.
    pub program_data:UncheckedAccount<'info>,
    #[account(init_if_needed,payer=payer,space=8+AuthorityDeployment::INIT_SPACE,
        seeds=[AUTHORITY_DEPLOYMENT_SEED],bump)]
    pub deployment:Box<Account<'info,AuthorityDeployment>>,
    pub system_program:Program<'info,System>,
}

pub fn initialize_deployment(ctx:Context<InitializeAuthorityDeployment>,genesis_hash:[u8;32])->Result<()> {
    // The actual loader authority may be a governance PDA signing through CPI.
    // This one-time attestation authenticates that exact authority below; it is
    // not an operating-key or passkey-execution permission.
    let a=ctx.accounts;
    if a.deployment.version!=0 {
        require!(a.deployment.version==PASSKEY_VERSION && a.deployment.genesis_hash==genesis_hash,
            RegistryError::PasskeyDeployment);
        return Ok(());
    }
    require!(genesis_hash!=[0;32] && a.program.executable && *a.program.owner==LOADER,
        RegistryError::PasskeyDeployment);
    let (expected,_)=Pubkey::find_program_address(&[crate::ID.as_ref()],&LOADER);
    require_keys_eq!(expected,a.program_data.key(),RegistryError::PasskeyDeployment);
    require!(*a.program_data.owner==LOADER && !a.program_data.executable,RegistryError::PasskeyDeployment);
    let program=a.program.try_borrow_data()?;let pd=a.program_data.try_borrow_data()?;
    require!(program.len()==36 && program[..4]==2u32.to_le_bytes()
        && program[4..36]==expected.to_bytes() && pd.len()>=45 && pd[..4]==3u32.to_le_bytes()
        && pd[12]==1 && pd[13..45]==a.upgrade_authority.key().to_bytes(),RegistryError::PasskeyDeployment);
    a.deployment.set_inner(AuthorityDeployment{version:PASSKEY_VERSION,bump:ctx.bumps.deployment,
        genesis_hash,attesting_upgrade_authority:a.upgrade_authority.key(),configured_at:Clock::get()?.unix_timestamp});
    Ok(())
}

#[derive(Accounts)]
#[instruction(args:InitializePasskeyAuthorityArgs)]
pub struct InitializePasskeyAuthority<'info> {
    #[account(mut)] pub payer:Signer<'info>,
    /// CHECK: An existing continuing Principal or the canonical prospective first Principal.
    pub principal:UncheckedAccount<'info>,
    #[account(init_if_needed,payer=payer,space=8+PasskeyAuthority::INIT_SPACE,
        seeds=[PASSKEY_AUTHORITY_SEED,principal.key().as_ref(),&passkey_key_hash(&args.public_key)],bump)]
    pub authority:Box<Account<'info,PasskeyAuthority>>,
    #[account(seeds=[AUTHORITY_DEPLOYMENT_SEED],bump=deployment.bump)]
    pub deployment:Box<Account<'info,AuthorityDeployment>>,
    pub system_program:Program<'info,System>,
}

fn principal_data(info:&AccountInfo)->Result<Option<Principal>> {
    if info.data_is_empty() {
        require!(*info.owner==System::id() && !info.executable,RegistryError::PasskeyPrincipal);
        return Ok(None);
    }
    require!(*info.owner==crate::ID && !info.executable && info.data_len()==8+Principal::INIT_SPACE,
        RegistryError::PasskeyPrincipal);
    let p=Principal::try_deserialize(&mut info.try_borrow_data()?.as_ref())
        .map_err(|_|error!(RegistryError::PasskeyPrincipal))?;
    let (key,bump)=Pubkey::find_program_address(&[b"principal",p.seed_owner.as_ref(),&p.principal_id],&crate::ID);
    require!(p.version==SCHEMA_VERSION && p.authority_generation>0 && key==info.key()
        && bump==p.bump && p.domain!=[0;32],RegistryError::PasskeyPrincipal);
    Ok(Some(p))
}

pub fn initialize_authority(ctx:Context<InitializePasskeyAuthority>,args:InitializePasskeyAuthorityArgs)->Result<()> {
    let a=ctx.accounts;
    require!(a.deployment.version==PASSKEY_VERSION && a.deployment.genesis_hash!=[0;32]
        && args.domain!=[0;32] && matches!(args.public_key[0],2|3),RegistryError::PasskeyAuthority);
    if let Some(p)=principal_data(&a.principal)? {
        require!(p.domain==args.domain && p.principal_id==args.principal_id,RegistryError::PasskeyPrincipal);
    } else {
        require_keys_eq!(a.principal.key(),passkey_principal_address(&args.domain,&args.principal_id,&args.public_key).0,
            RegistryError::PasskeyPrincipal);
    }
    if a.authority.version!=0 {
        require!(a.authority.version==PASSKEY_VERSION && a.authority.principal==a.principal.key()
            && a.authority.public_key==args.public_key && a.authority.domain==args.domain
            && a.authority.genesis_hash==a.deployment.genesis_hash,RegistryError::PasskeyAuthority);
        return Ok(());
    }
    a.authority.set_inner(PasskeyAuthority{version:PASSKEY_VERSION,bump:ctx.bumps.authority,
        principal:a.principal.key(),domain:args.domain,genesis_hash:a.deployment.genesis_hash,
        public_key:args.public_key,nonce:0,last_authorization_index:0,last_target_index:0,
        last_target_digest:[0;32],last_role:0,last_generation:0,last_expires_at:0,last_ceremony_nonce:[0;32],
        last_client_data_hash:[0;32],last_sign_count:0,last_flags:0});
    Ok(())
}

pub fn read_passkey_authority(info:&AccountInfo,principal:&Account<Principal>)->Result<PasskeyAuthority> {
    let a=read_authority(info)?;
    require!(a.principal==principal.key() && a.domain==principal.domain,RegistryError::PasskeyPrincipal);
    Ok(a)
}

fn read_authority(info:&AccountInfo)->Result<PasskeyAuthority> {
    require!(*info.owner==crate::ID && !info.executable && info.data_len()==8+PasskeyAuthority::INIT_SPACE,
        RegistryError::PasskeyAuthority);
    let a=PasskeyAuthority::try_deserialize(&mut info.try_borrow_data()?.as_ref())
        .map_err(|_|error!(RegistryError::PasskeyAuthority))?;
    let (key,bump)=passkey_authority_address(&a.principal,&a.public_key);
    require!(a.version==PASSKEY_VERSION && key==info.key() && a.bump==bump && a.genesis_hash!=[0;32]
        && a.domain!=[0;32] && matches!(a.public_key[0],2|3),RegistryError::PasskeyAuthority);
    Ok(a)
}

pub fn passkey_target_digest(target:&Instruction)->Result<[u8;32]> {
    require!(target.accounts.len()<=128 && target.data.len()<=8192,RegistryError::PasskeyEnvelope);
    let mut bytes=Vec::with_capacity(64+target.accounts.len()*33+target.data.len());
    bytes.extend_from_slice(b"DEXTER:ACCOUNT-TARGET:1");bytes.extend_from_slice(target.program_id.as_ref());
    bytes.extend_from_slice(&(target.accounts.len() as u16).to_le_bytes());
    for meta in &target.accounts {
        bytes.extend_from_slice(meta.pubkey.as_ref());
        bytes.push(u8::from(meta.is_signer)|(u8::from(meta.is_writable)<<1));
    }
    bytes.extend_from_slice(&(target.data.len() as u32).to_le_bytes());bytes.extend_from_slice(&target.data);
    Ok(hashv(&[&bytes]).to_bytes())
}

pub fn passkey_challenge(a:&PasskeyAuthority,alias:&Pubkey,args:&AuthorizePasskeyOperationArgs,
    target_digest:&[u8;32])->[u8;32] {
    hashv(&[b"DEXTER:ACCOUNT-AUTHORITY:1",&a.genesis_hash,crate::ID.as_ref(),a.principal.as_ref(),
        alias.as_ref(),&args.nonce.to_le_bytes(),&[args.role],&args.authority_generation.to_le_bytes(),
        &args.target_index.to_le_bytes(),target_digest,&args.expires_at.to_le_bytes(),&args.ceremony_nonce]).to_bytes()
}

fn is_instruction(ix:&Instruction,program:Pubkey,name:&str)->bool {
    ix.program_id==program && ix.data.len()>=8 && ix.data[..8]==hashv(&[b"global:",name.as_bytes()]).to_bytes()[..8]
}

fn allowed_target(ix:&Instruction)->bool {
    let names:&[&str]=if ix.program_id==crate::ID {&[
        "create_passkey_principal","create_actor","create_budget","create_mandate","create_pinned_mandate","rotate_actor","revoke_actor",
        "revoke_mandate","pin_mandate_terms","amend_allowance","rotate_principal_owner","configure_recovery_policy","recover_principal_owner",
        "bind_wallet","authorize_wallet","bind_funding_wallet","authorize_funding_wallet",
        "grant_spending_permission","revoke_spending_permission","grant_repayment_permission","revoke_repayment_permission",
    ]} else if ix.program_id==CORE_ID {&[
        "create_financier_custody","open_funding_facility","open_customer_credit_line","manage_funding_facility",
        "accept_financed_tab","accept_pinned_financed_tab","accept_evidence_financed_tab","accept_recoverable_financed_tab",
        "open_receiving_custody","withdraw_receiving_tokens",
        "submit_principal_bill_evidence","submit_principal_bill_claim",
        "withdraw_financier_free_authorized",
    ]} else {return false;};
    names.iter().any(|name|is_instruction(ix,ix.program_id,name))
}

fn authorization_args(ix:&Instruction)->Result<AuthorizePasskeyOperationArgs> {
    require!(is_instruction(ix,crate::ID,"authorize_passkey_operation") && ix.accounts.len()==4,
        RegistryError::PasskeyEnvelope);
    let mut data=&ix.data[8..];
    let args=AuthorizePasskeyOperationArgs::deserialize(&mut data).map_err(|_|error!(RegistryError::PasskeyEnvelope))?;
    require!(data.is_empty() && args.client_data_json.len()<=2048,RegistryError::PasskeyEnvelope);
    Ok(args)
}

fn role_allows_target(role:u8,target:&Instruction)->bool {
    match role {
        PASSKEY_OWNER=>allowed_target(target),
        PASSKEY_POSSESSION=>["rotate_principal_owner","recover_principal_owner","create_passkey_principal"]
            .iter().any(|name|is_instruction(target,crate::ID,name)),
        PASSKEY_GUARDIAN=>["configure_recovery_policy","recover_principal_owner"]
            .iter().any(|name|is_instruction(target,crate::ID,name)),
        _=>false,
    }
}

/// This profile is native-v1. Its complete prefix is only proof/authorization
/// pairs; resource settings belong to the message header, not arbitrary setup.
pub fn assert_passkey_prefix_envelope(instructions:&AccountInfo,target_index:u16)->Result<u16> {
    require_keys_eq!(instructions.key(),INSTRUCTIONS_KEY,RegistryError::PasskeyEnvelope);
    let end=usize::from(target_index);
    require!(end>=2 && end%2==0 && end/2<=MAX_PASSKEY_PROOFS,RegistryError::PasskeyEnvelope);
    let target=load_instruction_at_checked(end,instructions).map_err(|_|error!(RegistryError::PasskeyEnvelope))?;
    require!(allowed_target(&target),RegistryError::PasskeyEnvelope);
    let mut authorities=Vec::with_capacity(end/2);
    let deployment=Pubkey::find_program_address(&[AUTHORITY_DEPLOYMENT_SEED],&crate::ID).0;
    for index in (0..end).step_by(2) {
        let proof=load_instruction_at_checked(index,instructions).map_err(|_|error!(RegistryError::PasskeyEnvelope))?;
        let authorization=load_instruction_at_checked(index+1,instructions).map_err(|_|error!(RegistryError::PasskeyEnvelope))?;
        require!(proof.program_id==R1 && proof.accounts.is_empty(),RegistryError::PasskeyEnvelope);
        let args=authorization_args(&authorization)?;
        require!(args.target_index==target_index && authorization.accounts[0].is_writable
            && !authorization.accounts[0].is_signer && authorization.accounts[2].pubkey==deployment
            && authorization.accounts[3].pubkey==INSTRUCTIONS_KEY
            && !authorization.accounts[3].is_writable && !authorization.accounts[3].is_signer
            && !authorities.contains(&authorization.accounts[0].pubkey),RegistryError::PasskeyEnvelope);
        authorities.push(authorization.accounts[0].pubkey);
    }
    Ok(0)
}

#[derive(Accounts)]
pub struct AuthorizePasskeyOperation<'info> {
    #[account(mut)] pub authority:Box<Account<'info,PasskeyAuthority>>,
    /// CHECK: Continuing canonical Principal; creation possession admits only its empty prospective address.
    #[account(address=authority.principal)] pub principal:UncheckedAccount<'info>,
    #[account(seeds=[AUTHORITY_DEPLOYMENT_SEED],bump=deployment.bump)]
    pub deployment:Box<Account<'info,AuthorityDeployment>>,
    /// CHECK: The actual immutable instructions sysvar.
    #[account(address=INSTRUCTIONS_KEY)] pub instructions:UncheckedAccount<'info>,
}

pub fn authorize_operation(ctx:Context<AuthorizePasskeyOperation>,args:AuthorizePasskeyOperationArgs)->Result<()> {
    require!(get_stack_height()==TRANSACTION_LEVEL_STACK_HEIGHT,RegistryError::PasskeyEnvelope);
    let a=ctx.accounts;let alias=a.authority.key();
    let expected=passkey_authority_address(&a.principal.key(),&a.authority.public_key);
    require!(a.authority.version==PASSKEY_VERSION && alias==expected.0 && a.authority.bump==expected.1
        && a.deployment.version==PASSKEY_VERSION && a.authority.genesis_hash==a.deployment.genesis_hash,
        RegistryError::PasskeyAuthority);
    require!(args.nonce==a.authority.nonce,RegistryError::PasskeyNonce);
    require!((PASSKEY_OWNER..=PASSKEY_GUARDIAN).contains(&args.role),RegistryError::PasskeyRole);
    require!(args.expires_at>Clock::get()?.unix_timestamp && args.ceremony_nonce!=[0;32],RegistryError::PasskeyExpired);
    let current=load_current_index_checked(&a.instructions)?;
    require!(current%2==1 && current<args.target_index,RegistryError::PasskeyEnvelope);
    assert_passkey_prefix_envelope(&a.instructions,args.target_index)?;
    let target=load_instruction_at_checked(usize::from(args.target_index),&a.instructions)?;
    require!(role_allows_target(args.role,&target),RegistryError::PasskeyRole);
    require!(target.accounts.iter().any(|m|m.pubkey==alias && !m.is_signer)
        && target.accounts.iter().any(|m|m.pubkey==a.principal.key())
        && target.accounts.iter().any(|m|m.pubkey==INSTRUCTIONS_KEY && !m.is_signer && !m.is_writable),
        RegistryError::PasskeyEnvelope);
    let actual=load_instruction_at_checked(usize::from(current),&a.instructions)?;
    require!(actual.accounts[0].pubkey==alias && actual.accounts[1].pubkey==a.principal.key(),RegistryError::PasskeyEnvelope);
    if let Some(principal)=principal_data(&a.principal)? {
        require!(principal.domain==a.authority.domain,RegistryError::PasskeyPrincipal);
        if args.role==PASSKEY_OWNER {
            require!(principal.owner==alias && args.authority_generation==principal.authority_generation,
                RegistryError::PasskeyGeneration);
        } else {require!(args.authority_generation==0,RegistryError::PasskeyGeneration);}
    } else {
        require!(args.role==PASSKEY_POSSESSION && args.authority_generation==0
            && is_instruction(&target,crate::ID,"create_passkey_principal"),RegistryError::PasskeyPrincipal);
    }
    let digest=passkey_target_digest(&target)?;
    let challenge=passkey_challenge(&a.authority,&alias,&args,&digest);
    let proof=load_instruction_at_checked(usize::from(current-1),&a.instructions)?;
    let observation=dexter_contracts::webauthn::verify_webauthn_assertion(&proof,&a.authority.public_key,
        &args.client_data_json,&challenge)?;
    a.authority.nonce=plus(a.authority.nonce,1)?;
    a.authority.last_authorization_index=current;a.authority.last_target_index=args.target_index;
    a.authority.last_target_digest=digest;a.authority.last_role=args.role;
    a.authority.last_generation=args.authority_generation;a.authority.last_expires_at=args.expires_at;
    a.authority.last_ceremony_nonce=args.ceremony_nonce;
    a.authority.last_client_data_hash=hashv(&[&args.client_data_json]).to_bytes();
    a.authority.last_sign_count=observation.sign_count;a.authority.last_flags=observation.flags;
    emit!(PasskeyOperationAuthorized{principal:a.principal.key(),authority:alias,nonce:args.nonce,
        role:args.role,generation:args.authority_generation,target_index:args.target_index,target_digest:digest,
        client_data_hash:a.authority.last_client_data_hash,sign_count:observation.sign_count,flags:observation.flags});
    Ok(())
}

fn instructions_account<'a,'info>(remaining:&'a [AccountInfo<'info>])->Result<&'a AccountInfo<'info>> {
    let mut found=None;
    for info in remaining {if info.key()==INSTRUCTIONS_KEY {
        require!(found.is_none() && !info.is_writable && !info.is_signer && !info.executable,
            RegistryError::PasskeyEnvelope);found=Some(info);
    }}
    found.ok_or_else(||error!(RegistryError::PasskeyEnvelope))
}

/// Historical control receipts must be reconciled without a new passkey
/// prefix. Otherwise a relayer holding an unused old guardian/candidate proof
/// could burn its nonce on a no-op and invalidate a newer prepared recovery.
/// The entire transaction rolls back on refusal; native/proof-free historical
/// recognition remains available without the departed owner or guardians.
pub fn require_proof_free_control_replay(remaining:&[AccountInfo])->Result<()> {
    if !remaining.iter().any(|info|info.key()==INSTRUCTIONS_KEY) {return Ok(());}
    let instructions=instructions_account(remaining)?;
    let current=load_current_index_checked(instructions)?;
    if current==0 {return Ok(());}
    let previous=load_instruction_at_checked(usize::from(current-1),instructions)?;
    require!(!is_instruction(&previous,crate::ID,"authorize_passkey_operation"),
        RegistryError::PasskeyReplayPrefix);
    Ok(())
}

fn require_passkey_role(principal:&Account<Principal>,authority:&AccountInfo,remaining:&[AccountInfo],role:u8)->Result<()> {
    require!(get_stack_height()==TRANSACTION_LEVEL_STACK_HEIGHT,RegistryError::PasskeyEnvelope);
    let instructions=instructions_account(remaining)?;
    let current=load_current_index_checked(instructions)?;
    require_passkey_role_at(principal,authority,instructions,role,current)
}

fn require_passkey_role_at(principal:&Account<Principal>,authority:&AccountInfo,instructions:&AccountInfo,
    role:u8,current:u16)->Result<()> {
    require!(!authority.is_signer,RegistryError::PasskeyAuthority);
    let a=read_passkey_authority(authority,principal)?;
    // The actual earlier top-level authorization already validated the entire
    // immutable prefix AND hashed the actual target at this exact sysvar index.
    // Reparse this authority's executed authorization, not the large target or
    // all other guardians' proofs on every check: the SBF bump allocator cannot
    // reclaim those temporary vectors within one consumer invocation. The real
    // instructions sysvar cannot change between that earlier authorization and
    // this target, so its fresh executed provenance authenticates the same
    // program, data and effective account privileges without a second copy.
    // There is no authorization replay/early-return path: including an old
    // nonce would fail at that earlier instruction before this target executes.
    require!(current>=2 && current%2==0 && usize::from(current)/2<=MAX_PASSKEY_PROOFS
        && a.last_authorization_index%2==1,RegistryError::PasskeyEnvelope);
    require!(a.nonce>0 && a.last_target_index==current && a.last_authorization_index<current
        && a.last_role==role,RegistryError::PasskeyRole);
    let authorization=load_instruction_at_checked(usize::from(a.last_authorization_index),instructions)?;
    let args=authorization_args(&authorization)?;
    require!(authorization.accounts[0].pubkey==authority.key() && authorization.accounts[1].pubkey==principal.key()
        && args.nonce.checked_add(1)==Some(a.nonce) && args.role==role && args.target_index==current
        && args.authority_generation==a.last_generation && args.expires_at==a.last_expires_at
        && args.ceremony_nonce==a.last_ceremony_nonce && hashv(&[&args.client_data_json]).to_bytes()==a.last_client_data_hash,
        RegistryError::PasskeyEnvelope);
    require!(a.last_expires_at>Clock::get()?.unix_timestamp,RegistryError::PasskeyExpired);
    if role==PASSKEY_OWNER {
        require!(principal.owner==authority.key() && a.last_generation==principal.authority_generation,
            RegistryError::PasskeyGeneration);
    } else {require!(a.last_generation==0,RegistryError::PasskeyGeneration);}
    Ok(())
}

/// Only the strictly admitted Registry -> Swig -> Core free-cash withdrawal
/// may reuse the immediately preceding Registry target's verified owner proof.
/// Core additionally authenticates its funding binding, wallet signer, exact
/// destination and reserve floor; this is not a generic CPI authority adapter.
pub fn require_wallet_owner_authority(principal:&Account<Principal>,authority:&AccountInfo,instructions:&AccountInfo)->Result<()> {
    require!(get_stack_height()>TRANSACTION_LEVEL_STACK_HEIGHT,RegistryError::PasskeyEnvelope);
    require_keys_eq!(instructions.key(),INSTRUCTIONS_KEY,RegistryError::PasskeyEnvelope);
    require_keys_eq!(principal.owner,authority.key(),RegistryError::OwnerAuthority);
    let current=load_current_index_checked(instructions)?;
    require!(current>0,RegistryError::PasskeyEnvelope);
    let swig=load_instruction_at_checked(usize::from(current),instructions)?;
    require_keys_eq!(swig.program_id,crate::SWIG_ID,RegistryError::PasskeyEnvelope);
    let previous=load_instruction_at_checked(usize::from(current-1),instructions)?;
    require!(is_instruction(&previous,crate::ID,"authorize_funding_wallet")
        && previous.accounts.len()==7 && previous.accounts[3].pubkey==principal.key()
        && previous.accounts[6].pubkey==authority.key(),RegistryError::PasskeyEnvelope);
    let mut raw=&previous.data[8..];
    let args=crate::AuthorizeFundingWalletArgs::deserialize(&mut raw).map_err(|_|error!(RegistryError::PasskeyEnvelope))?;
    require!(raw.is_empty() && args.action==crate::FUNDING_WITHDRAW
        && args.authority_generation==principal.authority_generation,RegistryError::PasskeyEnvelope);
    if authority.key().is_on_curve() {
        require!(authority.is_signer,RegistryError::OwnerAuthority);return Ok(());
    }
    require_passkey_role_at(principal,authority,instructions,PASSKEY_OWNER,current-1)
}

pub fn require_owner_authority(principal:&Account<Principal>,authority:&AccountInfo,remaining:&[AccountInfo])->Result<()> {
    require_keys_eq!(principal.owner,authority.key(),RegistryError::OwnerAuthority);
    if authority.key().is_on_curve() {
        require!(authority.is_signer,RegistryError::OwnerAuthority);return Ok(());
    }
    require_passkey_role(principal,authority,remaining,PASSKEY_OWNER)
}
pub fn require_authority_possession(principal:&Account<Principal>,authority:&AccountInfo,remaining:&[AccountInfo])->Result<()> {
    if authority.key().is_on_curve() {
        require!(authority.is_signer && authority.key()!=Pubkey::default(),RegistryError::RecoveryOwner);return Ok(());
    }
    require_passkey_role(principal,authority,remaining,PASSKEY_POSSESSION)
}
pub fn require_guardian_authority(principal:&Account<Principal>,authority:&AccountInfo,remaining:&[AccountInfo])->Result<()> {
    if authority.key().is_on_curve() {
        require!(authority.is_signer && authority.key()!=Pubkey::default(),RegistryError::RecoveryQuorum);return Ok(());
    }
    require_passkey_role(principal,authority,remaining,PASSKEY_GUARDIAN)
}

#[derive(Accounts)]
#[instruction(args:CreatePasskeyPrincipalArgs)]
pub struct CreatePasskeyPrincipal<'info> {
    #[account(mut)] pub payer:Signer<'info>,
    /// CHECK: Immutable alias authenticated by exact possession prefix.
    pub owner:UncheckedAccount<'info>,
    #[account(init_if_needed,payer=payer,space=8+Principal::INIT_SPACE,
        seeds=[b"principal",passkey_seed_owner(&args.domain,&args.public_key).as_ref(),&args.principal_id],bump)]
    pub principal:Box<Account<'info,Principal>>,
    pub system_program:Program<'info,System>,
}

pub fn create_with_passkey(ctx:Context<CreatePasskeyPrincipal>,args:CreatePasskeyPrincipalArgs)->Result<()> {
    let a=ctx.accounts;let seed_owner=passkey_seed_owner(&args.domain,&args.public_key);
    require!((1..=3).contains(&args.kind) && args.domain!=[0;32] && args.governing_policy!=[0;32],RegistryError::InvalidPolicy);
    require_keys_eq!(a.owner.key(),passkey_authority_address(&a.principal.key(),&args.public_key).0,RegistryError::PasskeyAuthority);
    if a.principal.version!=0 {
        require!(a.principal.version==SCHEMA_VERSION && a.principal.seed_owner==seed_owner
            && a.principal.principal_id==args.principal_id && a.principal.domain==args.domain
            && a.principal.kind==args.kind && a.principal.governing_policy==args.governing_policy,
            RegistryError::ConfigurationConflict);
        require_proof_free_control_replay(ctx.remaining_accounts)?;
        return Ok(());
    }
    a.principal.set_inner(Principal{version:SCHEMA_VERSION,bump:ctx.bumps.principal,principal_id:args.principal_id,
        domain:args.domain,kind:args.kind,governing_policy:args.governing_policy,seed_owner,
        owner:a.owner.key(),authority_generation:1});
    require_authority_possession(&a.principal,&a.owner,ctx.remaining_accounts)?;
    Ok(())
}

#[event]
pub struct PasskeyOperationAuthorized {
    pub principal:Pubkey,pub authority:Pubkey,pub nonce:u64,pub role:u8,pub generation:u64,
    pub target_index:u16,pub target_digest:[u8;32],pub client_data_hash:[u8;32],pub sign_count:u32,pub flags:u8,
}

#[cfg(test)]
mod tests {
    use super::*;
    use anchor_lang::solana_program::instruction::AccountMeta;
    #[test]
    fn target_hash_binds_order_roles_program_and_full_data() {
        let target=Instruction{program_id:crate::ID,accounts:vec![AccountMeta::new_readonly(Pubkey::new_unique(),false),
            AccountMeta::new(Pubkey::new_unique(),true)],data:vec![1,2,3]};
        let d=passkey_target_digest(&target).unwrap();
        let mut t=target.clone();t.accounts.swap(0,1);assert_ne!(d,passkey_target_digest(&t).unwrap());
        let mut t=target.clone();t.accounts[0].is_writable=true;assert_ne!(d,passkey_target_digest(&t).unwrap());
        let mut t=target.clone();t.accounts[0].is_signer=true;assert_ne!(d,passkey_target_digest(&t).unwrap());
        let mut t=target.clone();t.program_id=CORE_ID;assert_ne!(d,passkey_target_digest(&t).unwrap());
        let mut t=target;t.data.push(0);assert_ne!(d,passkey_target_digest(&t).unwrap());
    }
    #[test]
    fn immutable_alias_is_principal_and_key_specific_and_has_no_creation_cycle() {
        let key=[2;33];let domain=[3;32];let id=[4;32];
        let p=passkey_principal_address(&domain,&id,&key).0;
        let a=passkey_authority_address(&p,&key).0;
        assert_ne!(a,passkey_seed_owner(&domain,&key));
        assert_ne!(a,passkey_authority_address(&Pubkey::new_unique(),&key).0);
        let mut other=key;other[1]=5;assert_ne!(a,passkey_authority_address(&p,&other).0);
    }
}
