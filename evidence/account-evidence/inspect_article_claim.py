#!/usr/bin/env python3
"""Locate and decode the retained sequence used by the article."""
import argparse
import base64
import hashlib
import json
import lzma
from pathlib import Path
import sys
sys.dont_write_bytecode=True
ROOT=Path(__file__).resolve().parent
sys.path.insert(0,str(ROOT/'execution/foundation/onchain/tests'))
from verify_financing_evidence import Idl, Snapshot, accounts_of_type, token_amounts, VersionedTransaction, json_value

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path)
    args=parser.parse_args()
    evidence=ROOT/'account/docs/evidence/wallet-spending-20261101'
    trace_path=evidence/'wallet-spending-shared-trace.json.xz'
    trace=json.loads(lzma.decompress(trace_path.read_bytes()))
    core=Idl(evidence/'idl/claims_core.json')
    registry=Idl(evidence/'idl/principal_registry.json')
    rows=trace['transactions']
    names={row['name']:i for i,row in enumerate(rows)}
    wanted=['fresh authorization still refuses spending another facility\'s backing',
        'a separate exact owner approval cannot spend one committed token',
        'owner revokes the initiating agent after work was earned',
        'both owners authorize financier control rotation',
        'independent collector services accepted rights without initiating agent or financier backend']
    positions=[names[name] for name in wanted]
    assert positions==sorted(positions)
    selected=[]
    for i in positions:
        row=rows[i]
        tx=VersionedTransaction.from_bytes(base64.b64decode(row['transaction_base64']))
        tx.verify_and_hash_message()
        selected.append({'trace_index':i,'name':row['name'],'expected':row['expected'],
                         'recorded_result':row['result'].get('err'),'signature_count':len(tx.signatures),
                         'required_signers':[str(k) for k in tx.message.account_keys[:len(tx.signatures)]]})
    assert all(rows[i]['result']['err']['InstructionError'][1]['Custom']==6033 for i in positions[:2])
    assert all(rows[i]['result'].get('err') is None for i in positions[2:])
    collect=rows[positions[-1]]
    before,after=Snapshot(collect['observed_before']),Snapshot(collect['observed_after'])
    def changes(idl,kind,old=before,new=after):
        b=accounts_of_type(idl,old,kind);a=accounts_of_type(idl,new,kind)
        return {k:(b[k],v) for k,v in a.items() if k in b and b[k]!=v}
    custodies=changes(core,'FinancierCustody');assert len(custodies)==1
    custody,(cb,ca)=next(iter(custodies.items()))
    assert (cb['accounted_cash'],ca['accounted_cash'])==(300000,200000)
    assert (cb['payable'],ca['payable'])==(100000,0)
    assert cb['unused']==ca['unused']==200000
    token=str(cb['token_account'])
    assert token_amounts(before)[token]==300000 and token_amounts(after)[token]==200000
    claims=changes(core,'FundedClaim');assert len(claims)==1
    claim,(qb,qa)=next(iter(claims.items()))
    assert qb['debt']==qa['debt']==100000 and qb['payable']==100000 and qa['payable']==0
    runs=changes(core,'FundedRun');assert len(runs)==1
    run,(rb,ra)=next(iter(runs.items()))
    assert rb['principal']==ra['principal'] and rb['principal']['outstanding']==100000
    assert rb['gross_paid']==0 and ra['gross_paid']==100000
    facilities=accounts_of_type(core,before,'FundingFacility')
    shared=[k for k,v in facilities.items() if str(v['custody'])==custody]
    assert len(shared)>=2
    revoke=rows[positions[2]]
    revoked=changes(registry,'Actor',Snapshot(revoke['observed_before']),Snapshot(revoke['observed_after']))
    assert len(revoked)==1
    actor,(ab,aa)=next(iter(revoked.items()))
    assert ab['active'] and not aa['active']
    assert not accounts_of_type(registry,before,'Actor')[actor]['active']
    rotate=rows[positions[3]]
    rotated=changes(registry,'Principal',Snapshot(rotate['observed_before']),Snapshot(rotate['observed_after']))
    assert len(rotated)==1
    principal,(pb,pa)=next(iter(rotated.items()))
    assert pb['owner']!=pa['owner'] and pa['authority_generation']==pb['authority_generation']+1
    tx=VersionedTransaction.from_bytes(base64.b64decode(collect['transaction_base64']))
    assert len(tx.signatures)==1
    signer=tx.message.account_keys[0]
    assert signer not in [ab['current_key'],pb['owner'],pa['owner']]
    result={'schema':'dexter-article-claim-record/v1','status':'passed',
        'trace':str(trace_path.relative_to(ROOT)),'compressed_sha256':hashlib.sha256(trace_path.read_bytes()).hexdigest(),
        'sequence':selected,'shared_custody':custody,'facilities_sharing_custody':shared,
        'collection':{'signature_count':1,'signer':str(signer),'signer_is_initiating_agent_or_old_or_new_financier_owner':False,
            'actual_token_cash_before':300000,'actual_token_cash_after':200000,'unused_backing_before':200000,'unused_backing_after':200000,
            'supplier_payable_before':100000,'supplier_payable_after':0,'company_financing_debt_before':100000,'company_financing_debt_after':100000,
            'claim':claim,'earned_run':run},
        'scope':'Decoded retained local execution sequence and verified its signed packets. Complete snapshot/receipt validation is performed by verify_recorded_evidence.py. No new SVM or RPC execution.'}
    output=json.dumps(result,indent=2,default=json_value)+'\n'
    if args.output:
        args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(output)
    else:print(output,end='')

if __name__=='__main__':main()
