# Delivery rules

This program signs an evidence order's delivery report under a fixed rule. An
order names a rule; ClaimsCore accepts the order only while this program has no
upgrade authority, and admits the report only when this program signs it.

A rule states who signs for delivery (up to five keys) and the size of a group,
`threshold`; what silence supports; who may object and until when; who decides
an objection and until when; and what happens if the decider stays silent. It
lives at `["rule", sha256("DEXTER:DELIVERY-RULE:1" || RuleTerms)]`.
`create_rule` writes it once; no instruction changes or closes it, so changing a
signer means a new rule and a new order. A rule with objectors names a decider
and gives it at least `MIN_DECIDE_SECONDS` (3,600) after objections close.

## How a case settles

**The signers' outcome.** Each group of `threshold` signers supports, per
metric, the smallest amount its members support. Once `threshold` signers have
attested, only attesting signers form groups. While fewer have attested, every
signer counts, and a silent signer supports the rule's silence value: zero when
silence pays nothing, the policy's maximum units when the seller is paid unless
someone objects. Each group's units are priced with the order's policy, with the
same arithmetic ClaimsCore uses for every report, and the highest-priced group
is reported. Among groups of equal price, the one with the lowest mask wins,
where signer i is bit i. With one metric and at least `threshold` reports, this
is the `threshold`-th highest report.

What this guarantees: once `threshold` signers have attested, the reported units
are units that `threshold` of them reported, each at least that much in every
metric. A single signer can raise the amount only when `threshold` is 1; while
fewer than `threshold` have attested under "paid unless someone objects", a
single report can lower the amount from the maximum. Under
"silence pays nothing" the amount is the same as if silent signers counted as
zero, since a group with a silent member is then worth 0.

Examples, with two of three carriers: reports of 900, 700 and 800 give 800.
Under "paid unless someone objects", reports of 2,000 and 500 with the third
carrier silent give 500, and reports of 2,000, 500 and 1,400 give 1,400. With a
single report of 500 and two carriers silent, the silent pair supports the
maximum. With two of two, one report of 500 and one silent carrier give 500.

**Objections and decisions.** An objector may object any time until objections
close; the first objection opens a dispute. The decider decides after signing
closes, on the signers' complete reports, and before the decision closes, at
least an hour after objections close. A decision replaces the signers' outcome.
An objection left undecided reports zero units, or lapses into the signers'
outcome when the rule says so.

**When.** Anyone settles once the outcome can no longer change: after objections
close when nobody objected, at once after a decision, or after the decision
closes undecided. The program then reports into ClaimsCore by CPI, signing as
the delivery-source address, and the bill is min(maximum, seller's claim,
reported amount).

**What the bill records.** The report's `result_hash` is
`sha256("DEXTER:DELIVERY-RULE-OUTCOME:1" || case address || Borsh(Case))`, the
case record at settlement: bill, rule, opener, the three deadlines, every
attestation, the objection, the decision, the outcome, the chosen group and the
reported units. `settle` logs the same three segments (`Program data:`), so the
record can be checked against the bill after the case closes.

## Instructions

| Instruction | Who | When |
|---|---|---|
| `open_case` | anyone; pays the rent | the bill names this rule, holds no report and is not finalized; the order's evidence deadline is at least `MIN_SETTLE_SECONDS` (3,600) after the rule's decision deadline |
| `attest(units)` | each signer, once | until the order's work deadline plus `sign_seconds` |
| `object` | an objector | until signing closes plus `object_seconds` |
| `decide(units)` | the decider, once, after an objection | after signing closes, until objections close plus `decide_seconds` |
| `settle` | anyone | once the outcome can no longer change |
| `close_case` | anyone; the rent returns to the opener | once the bill is finalized |

A settled case takes no attestation, objection or decision. Units are checked
against the order's policy when attested or decided, and ClaimsCore checks the
final report as it checks every report.

## The binary

[`artifacts/delivery-rules-retained.so`](artifacts/delivery-rules-retained.so)
is the binary the tests load by pinned hash and the file a deployment uses
(SHA-256 `704831da874812b856d47806083726bd4bfefe9b43fe526b3fba3c70029f9f0f`, 262,976 bytes). Once deployed it
cannot be upgraded, so it keeps reading the EvidenceBill layout and calling
`submit_program_bill_evidence` exactly as built. ClaimsCore's test
`evidence_billing::rule_program_layout` pins both.

Evidence: [delivery rules](../../../../../README.md).
