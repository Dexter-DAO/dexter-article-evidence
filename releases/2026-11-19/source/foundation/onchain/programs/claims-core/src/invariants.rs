//! Source checks for two invariants of the design note (sections 3.5 and 3.13)
//! that no single instruction test can cover: they read every program's source.
//!
//! I1. A Tab's `remaining` never increases: after the commit only
//! `FundedClaim::lower_remaining` writes it.
//!
//! The address chain. A window admits a number once only while its record
//! exists at its address: a closed record recreated at the same address would
//! start with an empty window and admit old numbers again. So every record type
//! with a window either never closes, or its address chains to a mandate order
//! number (repayment record -> run -> Tab -> the mandate's order window), which
//! the mandate admits once ever, so the address can never hold a record again.
//! The chain is also structural: a repayment record closes only after its run
//! has closed (no payment can recreate it, since a closed run never returns),
//! and a Tab only after every run and record.
//! Every close in every program is listed below with the type it closes; a new
//! close, or a new windowed type, fails here until it is classified.
use std::{fs, path::{Path, PathBuf}};

const PROGRAMS: [&str; 4] = ["claims-core", "principal-registry", "pool-issuer", "delivery-rules"];

/// Windowed record types that never close.
const NEVER_CLOSE: [&str; 6] = ["Mandate", "SpendingPermission", "RepaymentPermission",
    "FinancierCustody", "ReceivingCustody", "FinanceBook"];
/// Windowed record types that close, with the seed prefix chaining each to its
/// parent: the run it pays.
const CHAINED: [(&str, &str); 1] = [("RepaymentRecord", "[b\"repayment\", run.key().as_ref()")];
/// The rest of the chain: a run lives under its Tab, a Tab under its mandate's
/// order number.
const CHAIN: [(&str, &str); 2] = [("FundedRun", "[b\"run\",claim.key().as_ref()"),
    ("FundedClaim", "[b\"tab\",terms.mandate.as_ref(),&terms.order.to_le_bytes()]")];

/// Every close in every program: the source text of the close and the record
/// type it closes. None of them is a never-close windowed type.
const CLOSES: [(&str, &str, &str); 8] = [
    ("claims-core", "a.record.close(", "RepaymentRecord"),
    ("claims-core", "a.run.close(", "FundedRun"),
    ("claims-core", "bill.close(", "EvidenceBill"),
    ("claims-core", "a.claim.close(", "FundedClaim"),
    ("claims-core", "close_info(&binding", "BillAuthorityBinding"),
    ("principal-registry", "close = deposit_payer", "Reservation"),
    ("principal-registry", "close_exact_approval(", "ExactSpendingApproval/ExactRepaymentApproval"),
    ("delivery-rules", "close = opener", "Case"),
];

fn rust_files(dir: &Path, out: &mut Vec<(PathBuf, String)>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() { rust_files(&path, out); }
        else if path.extension().map_or(false, |e| e == "rs") {
            out.push((path.clone(), fs::read_to_string(&path).unwrap()));
        }
    }
}

/// A program's production sources: each file up to its `#[cfg(test)]` module,
/// without this file.
fn program(name: &str) -> Vec<(PathBuf, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join(name).join("src");
    let mut files = Vec::new();
    rust_files(&root, &mut files);
    assert!(!files.is_empty(), "no sources for {name}");
    files.into_iter().filter(|(path, _)| !path.ends_with("invariants.rs"))
        .map(|(path, text)| {
            let production = text.split("\n#[cfg(test)]").next().unwrap_or("").to_string();
            (path, production)
        }).collect()
}

/// Code only: drop `//` comments so prose never counts as a close or a write.
fn code(line: &str) -> &str { line.split("//").next().unwrap_or("") }

/// An Anchor `close = target` constraint: `close` right after `(` or `,`.
fn close_constraint(line: &str) -> bool {
    let mut rest = line;
    while let Some(at) = rest.find("close") {
        let before = rest[..at].trim_end().chars().last();
        let after = rest[at + 5..].trim_start();
        if matches!(before, Some('(') | Some(',')) && after.starts_with('=') && !after.starts_with("==") { return true; }
        rest = &rest[at + 5..];
    }
    false
}

#[test]
fn only_lower_remaining_writes_a_tabs_remaining() {
    let mut writes = Vec::new();
    for (path, text) in program("claims-core") {
        for (n, line) in text.lines().enumerate() {
            let line = code(line);
            let mut rest = line;
            while let Some(at) = rest.find(".remaining") {
                let after = rest[at + ".remaining".len()..].trim_start();
                let assigns = (after.starts_with('=') && !after.starts_with("=="))
                    || after.starts_with("+=") || after.starts_with("-=");
                if assigns { writes.push(format!("{}:{}: {}", path.display(), n + 1, line.trim())); }
                rest = &rest[at + 1..];
            }
        }
    }
    assert_eq!(writes.len(), 1, "a Tab's remaining is written outside lower_remaining: {writes:#?}");
    assert!(writes[0].contains("self.remaining = minus(self.remaining, amount)?"), "{writes:#?}");
}

/// The struct names declared with `#[account]` whose fields include a window.
fn windowed_types() -> Vec<String> {
    let mut found = Vec::new();
    for name in PROGRAMS {
        for (_, text) in program(name) {
            let lines: Vec<&str> = text.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                let Some(rest) = line.trim_start().strip_prefix("pub struct ") else { continue };
                let account = lines[i.saturating_sub(3)..i].iter().any(|l| l.trim() == "#[account]");
                if !account { continue; }
                let type_name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                let body: Vec<&str> = if line.contains('}') { vec![*line] }
                    else { lines[i..].iter().take_while(|l| l.trim_end() != "}").copied().collect() };
                if body.iter().any(|l| code(l).contains("OrderWindow")) { found.push(type_name); }
            }
        }
    }
    found.sort(); found.dedup();
    found
}

#[test]
fn every_windowed_record_never_closes_or_chains_to_an_order_number() {
    // 1. Every windowed record type is classified.
    let mut classified: Vec<String> = NEVER_CLOSE.iter().map(|s| s.to_string())
        .chain(CHAINED.iter().map(|(t, _)| t.to_string())).collect();
    classified.sort();
    assert_eq!(windowed_types(), classified,
        "a record type with a window must be listed as never-close or as chained to an order number");

    // 2. Every close in every program is a listed close of a listed type, and no
    //    listed close is of a never-close windowed type.
    for (name, _, closed) in CLOSES {
        assert!(!NEVER_CLOSE.iter().any(|t| closed.split('/').any(|c| c == *t)),
            "{name} closes the never-close windowed type {closed}");
    }
    for name in PROGRAMS {
        for (path, text) in program(name) {
            for (n, line) in text.lines().enumerate() {
                let line = code(line);
                let closes = line.contains(".close(") || close_constraint(line)
                    || line.contains("close_info(&") || line.contains("close_exact_approval(");
                if !closes || line.contains("fn ") || line.contains("spl_token::instruction::close_account") { continue; }
                assert!(CLOSES.iter().any(|(program, site, _)| *program == name && line.contains(site)),
                    "unlisted close at {}:{}: {}; list it with the type it closes", path.display(), n + 1, line.trim());
            }
        }
    }

    // 3. A closeable windowed record's address chains to an order number: its
    //    seeds name its run, a run's seeds name its Tab, a Tab's name its
    //    mandate and order number.
    let core: String = program("claims-core").into_iter().map(|(_, t)| t).collect::<Vec<_>>().join("\n");
    let compact: String = core.chars().filter(|c| !c.is_whitespace()).collect();
    for (type_name, seeds) in CHAINED.iter().chain(CHAIN.iter()) {
        let wanted: String = seeds.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(compact.contains(&format!("seeds={wanted}")),
            "{type_name}'s address no longer chains to an order number: expected seeds {seeds}");
    }
    // Every account declared with one of these seeds is of that type.
    for (type_name, seeds) in CHAINED.iter().chain(CHAIN.iter()) {
        let wanted: String = seeds.chars().filter(|c| !c.is_whitespace()).collect();
        let mut rest = compact.as_str();
        while let Some(at) = rest.find(&format!("seeds={wanted}")) {
            let after = &rest[at..];
            let field = after.find("pub").map(|p| &after[p..]).unwrap_or("");
            let declared = field.split(">,").next().unwrap_or("");
            assert!(declared.contains(&format!("Account<'info,{type_name}")),
                "seeds {seeds} declare something other than {type_name}: {declared}");
            rest = &rest[at + 1..];
        }
    }
}

/// Find a function's body in a source file by its name.
fn body<'a>(text: &'a str, name: &str) -> &'a str {
    let start = text.find(&format!("fn {name}")).unwrap_or_else(|| panic!("no fn {name}"));
    let rest = &text[start..];
    let end = rest.find("\n}\n").unwrap_or(rest.len());
    &rest[..end]
}

#[test]
fn a_repayment_record_closes_only_after_its_run_and_a_tab_after_every_record() {
    let close = program("claims-core").into_iter().find(|(path, _)| path.ends_with("tab_close.rs"))
        .expect("tab_close.rs").1;
    let record = body(&close, "handle_close_repayment_record");
    assert!(record.contains("if !closed(&a.run)"),
        "close_repayment_record must stay a no-op until its run has closed");
    assert!(body(&close, "closed").contains("*info.owner != crate::ID && info.data_is_empty()"),
        "a run counts as closed only when ClaimsCore no longer owns it and it holds no data");
    assert!(body(&close, "handle_close_tab").contains("a.claim.open_records == 0"),
        "close_tab must wait for every repayment record");
    assert!(body(&close, "handle_close_tab").contains("a.claim.open_runs == 0"),
        "close_tab must wait for every run");
}
