//! A basic electrical rule check on a schematic design (ADR 0047 phase 2a), kept simple:
//! unconnected pins, no-connect flags on connected nets, pin types that fight on one
//! net, power inputs nobody drives (the `PWR_FLAG` check), dangling labels, duplicate or
//! unannotated references, symbols whose library symbol is missing, and parts for the
//! board without a footprint. Authoritative ERC stays KiCad's (`kicad-cli sch erc`).

use crate::board::Pt;
use crate::netlist::{Design, Netlist};
use crate::sch::PinType;
use std::collections::HashMap;

/// How bad a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    pub fn name(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

/// One finding, with the sheet use and place to show it at.
#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub code: &'static str,
    pub severity: Severity,
    pub message: String,
    /// Index into [`Netlist::uses`].
    pub sheet_use: usize,
    pub at: Pt,
}

/// Whether two pin types on one net are an error (two drivers fighting, or a pin meant
/// to stay unconnected).
fn conflict(a: PinType, b: PinType) -> bool {
    use PinType::{NoConnect, Output, PowerOut};
    matches!((a, b), (Output | PowerOut, Output | PowerOut)) || a == NoConnect || b == NoConnect
}

/// Checks a design; at most `max` issues.
pub fn check(design: &Design, nl: &Netlist, max: usize) -> Vec<Issue> {
    let mut out = Vec::new();
    let push = |out: &mut Vec<Issue>, i: Issue| {
        if out.len() < max {
            out.push(i);
        }
    };
    for net in &nl.nets {
        let (nodes, flags) = (net.nodes.len(), net.no_connects);
        let wanted_alone = flags > 0;
        if nodes == 1
            && net.labels == 0
            && net.power_symbols == 0
            && !wanted_alone
            && let Some(n) = net.nodes.first().filter(|n| n.kind != PinType::NoConnect)
        {
            push(
                &mut out,
                Issue {
                    code: "pin_not_connected",
                    severity: Severity::Error,
                    message: format!("pin {} of {} is not connected", n.pin, n.reference),
                    sheet_use: n.sheet_use,
                    at: n.at,
                },
            );
        }
        if flags > 0
            && (nodes > 1 || net.labels > 0 || net.power_symbols > 0)
            && let Some((u, at, _)) = net.marks.iter().find(|m| m.2.is_empty())
        {
            push(
                &mut out,
                Issue {
                    code: "no_connect_connected",
                    severity: Severity::Warning,
                    message: format!("a no-connect flag is on net {}, which is connected", net.name),
                    sheet_use: *u,
                    at: *at,
                },
            );
        }
        // Pin type conflicts: the first pair found on the net.
        'pairs: for (i, a) in net.nodes.iter().enumerate() {
            for b in net.nodes.iter().skip(i + 1) {
                if conflict(a.kind, b.kind) {
                    push(
                        &mut out,
                        Issue {
                            code: "pin_conflict",
                            severity: Severity::Error,
                            message: format!(
                                "pin {} of {} ({}) and pin {} of {} ({}) are on one net",
                                a.pin,
                                a.reference,
                                a.kind.name(),
                                b.pin,
                                b.reference,
                                b.kind.name()
                            ),
                            sheet_use: b.sheet_use,
                            at: b.at,
                        },
                    );
                    break 'pairs;
                }
            }
        }
        if !net.power_out
            && !(wanted_alone && nodes == 1)
            && let Some(n) = net.nodes.iter().find(|n| n.kind == PinType::PowerIn)
        {
            push(
                &mut out,
                Issue {
                    code: "power_not_driven",
                    severity: Severity::Error,
                    message: format!(
                        "power input pin {} of {} (net {}) is not driven by a power output (add a PWR_FLAG)",
                        n.pin, n.reference, net.name
                    ),
                    sheet_use: n.sheet_use,
                    at: n.at,
                },
            );
        }
        if nodes == 0 && net.labels == 1 && net.power_symbols == 0 && flags == 0 {
            // Bus labels (`D[0..7]`, `{SDA SCL}`) sit on buses, which are not modelled.
            if let Some((u, at, t)) = net.marks.first().filter(|m| !m.2.contains(['[', '{'])) {
                push(
                    &mut out,
                    Issue {
                        code: "label_dangling",
                        severity: Severity::Warning,
                        message: format!("label {t} is not connected to anything"),
                        sheet_use: *u,
                        at: *at,
                    },
                );
            }
        }
    }
    // References: duplicates (same reference and unit on two symbols), unannotated.
    let mut seen: HashMap<(String, u32), usize> = HashMap::new();
    for (u, su) in nl.uses.iter().enumerate() {
        let Some((_, s)) = design.sheets.get(su.sheet) else { continue };
        for sym in &s.symbols {
            let r = sym.reference_in(&su.path);
            if s.is_power(sym) || r.starts_with('#') {
                continue;
            }
            if s.lib_symbol(sym).is_none() {
                push(
                    &mut out,
                    Issue {
                        code: "missing_symbol",
                        severity: Severity::Error,
                        message: format!("{r}: library symbol {} is not in the schematic", sym.lib_key()),
                        sheet_use: u,
                        at: sym.at,
                    },
                );
            }
            if r.is_empty() || r.ends_with('?') {
                push(
                    &mut out,
                    Issue {
                        code: "unannotated",
                        severity: Severity::Error,
                        message: format!("{} has no reference number", if r.is_empty() { "a symbol" } else { r }),
                        sheet_use: u,
                        at: sym.at,
                    },
                );
                continue;
            }
            let c = seen.entry((r.to_string(), sym.unit)).or_insert(0);
            *c += 1;
            if *c == 2 {
                push(
                    &mut out,
                    Issue {
                        code: "duplicate_reference",
                        severity: Severity::Error,
                        message: format!("reference {r} is used twice"),
                        sheet_use: u,
                        at: sym.at,
                    },
                );
            }
            if sym.on_board && sym.footprint().is_empty() && sym.unit == 1 {
                push(
                    &mut out,
                    Issue {
                        code: "no_footprint",
                        severity: Severity::Warning,
                        message: format!("{r} has no footprint"),
                        sheet_use: u,
                        at: sym.at,
                    },
                );
            }
        }
    }
    out
}
