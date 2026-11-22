//! A provider's bills, as any billing site shows them, and their FHIR R4
//! shapes (docs/fhir.md), mapped by hand: no FHIR library.
//!
//! - [`Account`]: a billing account at a provider, for one patient, with the
//!   responsible party. FHIR `Account`. R4's Account has no balance element,
//!   so the balance a site shows is not mapped: it is the sum of the open
//!   (`issued`) bills' `Invoice.totalNet`.
//! - [`Bill`]: one service billed on an account (a visit, or a claim at the
//!   provider), with its charge lines and what was paid, adjusted and is
//!   owed. FHIR `Invoice`; each line's date of service and code is a
//!   contained `ChargeItem` the line points at, and a bill the site shows
//!   without lines gets one line of its own holding its date of service.
//! - [`Payment`]: one payment the patient or responsible party made. FHIR
//!   `PaymentReconciliation`, its `detail[]` naming the provider paid and,
//!   where the site says, the bills it paid. Insurance payments stay on the
//!   bill's lines.
//! - [`Statement`]: a statement PDF. FHIR `DocumentReference` typed `bill`
//!   in [`DOC_TYPE_SYSTEM`], with its `Binary`. A receipt is typed
//!   [`DocType::Receipt`] the same way.
//!
//! Money is whole cents here and a FHIR `Money` in the resources: a JSON
//! number in the decimal spelling a bill prints (`125.00`), in USD. Ids are
//! stable across runs, derived from the site's own account, bill, payment and
//! statement numbers, so a records store merges the same bill pulled twice.

use serde_json::{json, Map, Value};

/// The code system of a bill line's amounts (`Invoice.lineItem.priceComponent.code`).
pub const COMPONENT_SYSTEM: &str = "urn:pk-med:bill-component";
/// An `Invoice.identifier.type.text` naming the insurer's claim number.
pub const INSURER_CLAIM: &str = "insurer-claim";
/// The code system of a document's house type (`DocumentReference.type`),
/// which a records store files by.
pub const DOC_TYPE_SYSTEM: &str = "urn:pk-med:doc-type";

/// A billing document's house type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocType {
    /// A statement: what is owed.
    Bill,
    /// A payment receipt: what was paid, never an amount due.
    Receipt,
}

impl DocType {
    pub fn code(self) -> &'static str {
        match self {
            DocType::Bill => "bill",
            DocType::Receipt => "receipt",
        }
    }

    /// `DocumentReference.type`: the coding in [`DOC_TYPE_SYSTEM`], and the
    /// code as text for a reader that knows no system.
    pub fn concept(self) -> Value {
        json!({ "coding": [{ "system": DOC_TYPE_SYSTEM, "code": self.code() }], "text": self.code() })
    }
}

/// An amount in whole cents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Cents(pub i64);

impl Cents {
    /// A US dollar amount as a site prints it: `$1,234.56`, `1234.5`,
    /// `-$12.00`, `($12.00)`, `$12.00 CR` (a credit, negative). `None` for
    /// anything else, never a guess.
    pub fn parse(raw: &str) -> Option<Cents> {
        let mut s = raw.trim();
        let mut negative = false;
        if let Some(inner) = s.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
            negative = true;
            s = inner.trim();
        }
        if let Some(rest) = s.strip_suffix("CR").or_else(|| s.strip_suffix("cr")) {
            negative = !negative;
            s = rest.trim();
        }
        if let Some(rest) = s.strip_prefix('-') {
            negative = !negative;
            s = rest.trim();
        }
        s = s.strip_prefix('$').unwrap_or(s).trim();
        if let Some(rest) = s.strip_prefix('-') {
            negative = !negative;
            s = rest.trim();
        }
        let digits: String = s.chars().filter(|c| *c != ',').collect();
        let (whole, frac) = match digits.split_once('.') {
            Some((w, f)) => (w, f),
            None => (digits.as_str(), ""),
        };
        if whole.is_empty() && frac.is_empty() {
            return None;
        }
        if !whole.bytes().all(|b| b.is_ascii_digit()) || !frac.bytes().all(|b| b.is_ascii_digit()) || frac.len() > 2 {
            return None;
        }
        let whole: i64 = if whole.is_empty() { 0 } else { whole.parse().ok()? };
        let frac: i64 = match frac.len() {
            0 => 0,
            1 => frac.parse::<i64>().ok()? * 10,
            _ => frac.parse().ok()?,
        };
        let cents = whole.checked_mul(100)?.checked_add(frac)?;
        Some(Cents(if negative { -cents } else { cents }))
    }

    /// The decimal FHIR and the DTOs carry: `-12.30`.
    pub fn decimal(self) -> String {
        let sign = if self.0 < 0 { "-" } else { "" };
        format!("{sign}{}.{:02}", (self.0 / 100).abs(), (self.0 % 100).abs())
    }

    /// FHIR `Money`: the amount as a JSON number spelled as [`decimal`]
    /// writes it (`125.00`, `-12.30`), in USD. serde_json keeps the spelling
    /// only with its `arbitrary_precision` feature, which Cargo.toml turns
    /// on; a test pins it.
    ///
    /// [`decimal`]: Cents::decimal
    pub fn money(self) -> Value {
        let number: serde_json::Number = self
            .decimal()
            .parse()
            .expect("a decimal with two places is a JSON number");
        json!({ "value": number, "currency": "USD" })
    }
}

/// A code as the site shows it: CPT or HCPCS when the site names one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Code {
    /// `http://www.ama-assn.org/go/cpt`, `https://www.cms.gov/Medicare/Coding/HCPCSReleaseCodeSets`,
    /// or `None` for a code whose system the site does not say.
    pub system: Option<String>,
    pub code: String,
    pub display: Option<String>,
}

/// The provider an account bills for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provider {
    /// The organization id the CLI gives it (`PatientContext.organization`),
    /// FHIR-id safe.
    pub id: String,
    pub name: String,
}

/// A billing account at a provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// The site's account number, as it prints it.
    pub number: String,
    pub provider: Provider,
    /// The patient's name as the site shows it.
    pub patient_name: Option<String>,
    /// The responsible party (guarantor) as the site shows it.
    pub guarantor_name: Option<String>,
    /// The account's balance as the site shows it.
    pub balance: Option<Cents>,
    /// The part of it the site says the patient owes now.
    pub patient_balance: Option<Cents>,
    /// The part the site says waits on insurance.
    pub insurance_pending: Option<Cents>,
    /// `false` when the site shows the account closed.
    pub active: bool,
}

/// What a bill or a line says was billed, paid, adjusted and is owed. Each
/// is as the site shows it, `None` when it shows none; payments and
/// adjustments are positive amounts taken off the charge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Amounts {
    pub billed: Option<Cents>,
    pub allowed: Option<Cents>,
    pub insurance_paid: Option<Cents>,
    pub adjustment: Option<Cents>,
    pub patient_paid: Option<Cents>,
    /// What the patient still owes on it.
    pub patient_owes: Option<Cents>,
}

/// One charge on a bill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Charge {
    /// The date of service, `YYYY-MM-DD`.
    pub date: String,
    pub code: Option<Code>,
    pub description: Option<String>,
    pub quantity: Option<u32>,
    pub amounts: Amounts,
}

/// One service billed on an account: a visit, or a claim at the provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bill {
    /// The site's own number for it (visit, encounter or claim number). A
    /// site that numbers none gets one derived from the account, the date
    /// and the order the site lists it in, by the CLI.
    pub number: String,
    /// The account's number.
    pub account: String,
    /// The first date of service, `YYYY-MM-DD`.
    pub service_start: String,
    /// The last, when the service spans days.
    pub service_end: Option<String>,
    /// The posting or statement date the site shows for it.
    pub posted: Option<String>,
    /// The rendering provider (a clinician) where the site names one.
    pub rendering: Option<String>,
    pub description: Option<String>,
    /// The insurer the site says it was billed to.
    pub insurer: Option<String>,
    /// The insurer's claim number, where the site shows it.
    pub insurer_claim: Option<String>,
    pub lines: Vec<Charge>,
    /// The bill's own totals as the site shows them; a total the site does
    /// not show is the sum of its lines'.
    pub totals: Amounts,
    /// `true` when the site shows the bill voided.
    pub voided: bool,
}

/// One payment made on an account by the patient or responsible party.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payment {
    /// The site's transaction or receipt number, where it shows one.
    pub number: Option<String>,
    pub account: String,
    /// `YYYY-MM-DD`.
    pub date: String,
    pub amount: Cents,
    /// How it was paid, as the site words it (`Visa`, `Check`).
    pub method: Option<String>,
    /// The bills it was applied to, by number, where the site says.
    pub bills: Vec<String>,
}

/// One statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    /// The site's statement number or document id.
    pub number: String,
    pub account: String,
    /// The statement date, `YYYY-MM-DD`.
    pub date: String,
    pub amount_due: Option<Cents>,
    pub due_date: Option<String>,
    /// The bills it lists, by number, where known.
    pub bills: Vec<String>,
}

/// Who wrote the resources: names, URIs and the patient they are about.
#[derive(Debug, Clone)]
pub struct Mapper {
    /// The producing CLI (`peryourhealth`).
    pub cli: String,
    /// `meta.source` on every resource (`peryourhealth:site`).
    pub meta_source: String,
    /// The tag every resource carries, in `tag_system`.
    pub tag_system: String,
    pub tag_code: String,
    pub tag_display: String,
    /// The `Patient` resource id the resources' `subject` names.
    pub patient_id: String,
    /// The `Organization` resource id of the provider.
    pub organization_id: String,
    pub organization_name: String,
}

/// FHIR's `id`: 1 to 64 of letters, digits, `-` and `.`.
pub fn is_id(s: &str) -> bool {
    (1..=64).contains(&s.len()) && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'.')
}

/// A resource id from a prefix and the site's own number: the number's
/// letters and digits kept, anything else a `-`, cut to fit. Two numbers
/// that differ only in punctuation get one id, which the site's numbers
/// (digits and letters, a separator at most) do not.
pub fn id_of(prefix: &str, number: &str) -> String {
    let mut body: String = number
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    while body.contains("--") {
        body = body.replace("--", "-");
    }
    let body = body.trim_matches('-');
    let mut id = format!("{prefix}-{body}");
    id.truncate(64);
    id.trim_end_matches('-').to_string()
}

impl Mapper {
    fn meta(&self, last_updated: Option<&str>) -> Value {
        let mut m = json!({
            "source": self.meta_source,
            "tag": [{ "system": self.tag_system, "code": self.tag_code, "display": self.tag_display }],
        });
        if let Some(at) = last_updated {
            m["lastUpdated"] = at.into();
        }
        m
    }

    fn system(&self, what: &str) -> String {
        format!("urn:{}:{}:{what}", self.cli, self.organization_id)
    }

    fn subject(&self) -> Value {
        json!({ "reference": format!("Patient/{}", self.patient_id) })
    }

    fn issuer(&self) -> Value {
        json!({ "reference": format!("Organization/{}", self.organization_id), "display": self.organization_name })
    }

    pub fn account_id(&self, number: &str) -> String {
        id_of("account", number)
    }

    pub fn bill_id(&self, account: &str, number: &str) -> String {
        id_of("bill", &format!("{account}-{number}"))
    }

    pub fn payment_id(&self, p: &Payment, index: usize) -> String {
        match &p.number {
            Some(n) => id_of("payment", n),
            None => id_of(
                "payment",
                &format!("{}-{}-{}-{index}", p.account, p.date, p.amount.decimal()),
            ),
        }
    }

    pub fn statement_id(&self, account: &str, number: &str) -> String {
        id_of("statement", &format!("{account}-{number}"))
    }

    /// The account, `last_updated` the time the site was read.
    pub fn account(&self, a: &Account, last_updated: &str) -> Value {
        let mut v = json!({
            "resourceType": "Account",
            "id": self.account_id(&a.number),
            "meta": self.meta(Some(last_updated)),
            "identifier": [{ "system": self.system("account"), "value": a.number }],
            "status": if a.active { "active" } else { "inactive" },
            "subject": [self.subject()],
            "owner": self.issuer(),
        });
        if let Some(name) = &a.patient_name {
            v["subject"][0]["display"] = name.clone().into();
        }
        if let Some(g) = &a.guarantor_name {
            v["guarantor"] = json!([{ "party": { "display": g } }]);
        }
        v
    }

    /// The bill as an Invoice, `last_updated` the time the site was read.
    pub fn bill(&self, b: &Bill, last_updated: &str) -> Value {
        let mut ident = vec![json!({ "system": self.system("bill"), "value": b.number })];
        if let Some(claim) = &b.insurer_claim {
            let mut i = json!({ "type": { "text": INSURER_CLAIM }, "value": claim });
            if let Some(payer) = &b.insurer {
                i["assigner"] = json!({ "display": payer });
            }
            ident.push(i);
        }
        let totals = self.totals(b);
        let owes = totals.patient_owes;
        let status = if b.voided {
            "cancelled"
        } else if owes.is_some_and(|c| c.0 > 0) {
            "issued"
        } else if owes.is_some() {
            "balanced"
        } else {
            "issued"
        };
        let mut v = Map::new();
        v.insert("resourceType".into(), "Invoice".into());
        v.insert("id".into(), self.bill_id(&b.account, &b.number).into());
        v.insert("meta".into(), self.meta(Some(last_updated)));
        // A bill the site shows without lines is one line of its own,
        // dated by its service, so its date of service is a ChargeItem's as
        // every other bill's is.
        let own_line;
        let lines: &[Charge] = if b.lines.is_empty() {
            own_line = [Charge {
                date: b.service_start.clone(),
                code: None,
                description: Some(b.description.clone().unwrap_or_else(|| "Services".into())),
                quantity: None,
                amounts: totals,
            }];
            &own_line
        } else {
            &b.lines
        };
        let contained: Vec<Value> = lines
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let mut item = self.charge_item(b, i, line);
                if b.lines.is_empty() {
                    if let Some(end) = b.service_end.as_ref().filter(|e| *e != &b.service_start) {
                        if let Some(o) = item.as_object_mut() {
                            o.remove("occurrenceDateTime");
                            o.insert(
                                "occurrencePeriod".into(),
                                json!({ "start": b.service_start, "end": end }),
                            );
                        }
                    }
                }
                item
            })
            .collect();
        v.insert("contained".into(), contained.into());
        v.insert("identifier".into(), ident.into());
        v.insert("status".into(), status.into());
        if let Some(d) = &b.description {
            v.insert("type".into(), json!({ "text": d }));
        }
        v.insert("subject".into(), self.subject());
        if let Some(d) = &b.posted {
            v.insert("date".into(), d.clone().into());
        }
        if let Some(r) = &b.rendering {
            v.insert(
                "participant".into(),
                json!([{ "role": { "text": "rendering provider" }, "actor": { "display": r } }]),
            );
        }
        v.insert("issuer".into(), self.issuer());
        v.insert(
            "account".into(),
            json!({ "reference": format!("Account/{}", self.account_id(&b.account)) }),
        );
        let items: Vec<Value> = lines
            .iter()
            .enumerate()
            .map(|(i, line)| self.line_item(i + 1, &format!("#charge-{}", i + 1), &line.amounts))
            .collect();
        v.insert("lineItem".into(), items.into());
        // The bill's own breakdown, in the lines' codes: the billed amount
        // is here (`base` `billed`), not in `totalGross`.
        let parts = price_components(&totals);
        if !parts.is_empty() {
            v.insert("totalPriceComponent".into(), parts.into());
        }
        // R4's totals come after the price components, and a medical bill
        // carries no tax: both are what the patient still owes.
        if let Some(c) = owes {
            v.insert("totalNet".into(), c.money());
            v.insert("totalGross".into(), c.money());
        }
        Value::Object(v)
    }

    /// The bill's totals: the site's, and the sum of its lines' where the
    /// site shows none.
    pub fn totals(&self, b: &Bill) -> Amounts {
        let sum = |f: fn(&Amounts) -> Option<Cents>| -> Option<Cents> {
            let vals: Vec<Cents> = b.lines.iter().filter_map(|l| f(&l.amounts)).collect();
            (!vals.is_empty()).then(|| Cents(vals.iter().map(|c| c.0).sum()))
        };
        let t = &b.totals;
        Amounts {
            billed: t.billed.or_else(|| sum(|a| a.billed)),
            allowed: t.allowed.or_else(|| sum(|a| a.allowed)),
            insurance_paid: t.insurance_paid.or_else(|| sum(|a| a.insurance_paid)),
            adjustment: t.adjustment.or_else(|| sum(|a| a.adjustment)),
            patient_paid: t.patient_paid.or_else(|| sum(|a| a.patient_paid)),
            patient_owes: t.patient_owes.or_else(|| sum(|a| a.patient_owes)),
        }
    }

    fn charge_item(&self, b: &Bill, i: usize, line: &Charge) -> Value {
        let mut v = Map::new();
        v.insert("resourceType".into(), "ChargeItem".into());
        v.insert("id".into(), format!("charge-{}", i + 1).into());
        v.insert("status".into(), "billable".into());
        let code = match &line.code {
            Some(c) => {
                let mut coding = json!({ "code": c.code });
                if let Some(s) = &c.system {
                    coding["system"] = s.clone().into();
                }
                if let Some(d) = &c.display {
                    coding["display"] = d.clone().into();
                }
                let mut cc = json!({ "coding": [coding] });
                if let Some(d) = line.description.as_ref().or(c.display.as_ref()) {
                    cc["text"] = d.clone().into();
                }
                cc
            }
            None => {
                json!({ "text": line.description.clone().or_else(|| b.description.clone()).unwrap_or_else(|| "Charge".into()) })
            }
        };
        v.insert("code".into(), code);
        v.insert("subject".into(), self.subject());
        v.insert("occurrenceDateTime".into(), line.date.clone().into());
        if let Some(r) = &b.rendering {
            v.insert(
                "performer".into(),
                json!([{ "function": { "text": "rendering provider" }, "actor": { "display": r } }]),
            );
        }
        if let Some(q) = line.quantity {
            v.insert("quantity".into(), json!({ "value": q }));
        }
        Value::Object(v)
    }

    fn line_item(&self, sequence: usize, charge_ref: &str, a: &Amounts) -> Value {
        let mut v = json!({ "sequence": sequence, "chargeItemReference": { "reference": charge_ref } });
        let parts = price_components(a);
        if !parts.is_empty() {
            v["priceComponent"] = parts.into();
        }
        v
    }

    /// A payment as a PaymentReconciliation, dated by the payment.
    /// `bill_known` says whether a bill number on the payment's account is
    /// an Invoice in the same stream.
    ///
    /// Every `detail` names the provider paid (`payee`). A detail per bill
    /// the payment was applied to carries its `request`, and its `amount`
    /// when the payment names that bill alone; a payment that names no bill
    /// in the stream has one detail, with the payee only.
    pub fn payment(&self, p: &Payment, index: usize, bill_known: impl Fn(&str) -> bool) -> Value {
        let mut v = json!({
            "resourceType": "PaymentReconciliation",
            "id": self.payment_id(p, index),
            "meta": self.meta(Some(&format!("{}T00:00:00Z", p.date))),
            "status": "active",
            "created": p.date,
            "requestor": self.issuer(),
            "outcome": "complete",
            "paymentDate": p.date,
            "paymentAmount": p.amount.money(),
        });
        if let Some(n) = &p.number {
            v["paymentIdentifier"] = json!({ "system": self.system("payment"), "value": n });
            v["identifier"] = json!([{ "system": self.system("payment"), "value": n }]);
        }
        if let Some(m) = &p.method {
            v["disposition"] = format!("Paid by {m}").into();
        }
        let detail = || {
            json!({
                "type": { "coding": [{ "system": "http://terminology.hl7.org/CodeSystem/payment-type", "code": "payment", "display": "Payment" }] },
                "payee": self.issuer(),
                "date": p.date,
            })
        };
        let only = p.bills.len() == 1;
        let mut details: Vec<Value> = p
            .bills
            .iter()
            .filter(|b| bill_known(b))
            .map(|b| {
                let mut d = detail();
                d["request"] = json!({ "reference": format!("Invoice/{}", self.bill_id(&p.account, b)) });
                if only {
                    d["amount"] = p.amount.money();
                }
                d
            })
            .collect();
        if details.is_empty() {
            details.push(detail());
        }
        v["detail"] = details.into();
        v
    }

    /// A statement's DocumentReference, naming its Binary (`binary_id`) of
    /// `content_type` and `size` bytes, its Account and the Invoices it
    /// lists. Its amount due and due date have no R4 element: a records
    /// store reads them from the file.
    pub fn statement(&self, s: &Statement, binary_id: &str, content_type: &str, size: usize, title: &str) -> Value {
        let mut v = json!({
            "resourceType": "DocumentReference",
            "id": self.statement_id(&s.account, &s.number),
            "meta": self.meta(Some(&format!("{}T00:00:00Z", s.date))),
            "identifier": [{ "system": self.system("statement"), "value": s.number }],
            "status": "current",
            "type": DocType::Bill.concept(),
            "category": [{ "text": "Billing statement" }],
            "subject": self.subject(),
            "date": format!("{}T00:00:00Z", s.date),
            "author": [self.issuer()],
            "description": title,
            "content": [{ "attachment": {
                "contentType": content_type,
                "url": format!("Binary/{binary_id}"),
                "size": size,
                "title": title,
                "creation": s.date,
            } }],
        });
        let mut related = vec![json!({ "reference": format!("Account/{}", self.account_id(&s.account)) })];
        related.extend(
            s.bills
                .iter()
                .map(|b| json!({ "reference": format!("Invoice/{}", self.bill_id(&s.account, b)) })),
        );
        v["context"] = json!({ "related": related });
        v
    }
}

/// The `priceComponent`s of `a`, in `COMPONENT_SYSTEM`.
fn price_components(a: &Amounts) -> Vec<Value> {
    let mut parts = Vec::new();
    let mut part = |kind: &str, code: Option<(&str, &str)>, c: Option<Cents>| {
        if let Some(c) = c {
            let mut p = json!({ "type": kind, "amount": c.money() });
            if let Some((code, display)) = code {
                p["code"] = json!({ "coding": [{ "system": COMPONENT_SYSTEM, "code": code, "display": display }] });
            }
            parts.push(p);
        }
    };
    part("base", Some(("billed", "Billed charge")), a.billed);
    part("informational", Some(("allowed", "Allowed amount")), a.allowed);
    part(
        "deduction",
        Some(("insurance-payment", "Insurance payment")),
        a.insurance_paid,
    );
    part("discount", Some(("insurance-adjustment", "Adjustment")), a.adjustment);
    part(
        "deduction",
        Some(("patient-payment", "Patient payment")),
        a.patient_paid,
    );
    part(
        "informational",
        Some(("patient-responsibility", "Patient owes")),
        a.patient_owes,
    );
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapper() -> Mapper {
        Mapper {
            cli: "examplecli".into(),
            meta_source: "examplecli:site".into(),
            tag_system: "https://example.org/tag".into(),
            tag_code: "mapped".into(),
            tag_display: "Mapped".into(),
            patient_id: "pat-1".into(),
            organization_id: "org-1".into(),
            organization_name: "Example Physicians".into(),
        }
    }

    fn c(s: &str) -> Option<Cents> {
        Cents::parse(s)
    }

    #[test]
    fn cents_read_what_sites_print() {
        for (raw, want) in [
            ("$1,234.56", 123456),
            ("1234.5", 123450),
            ("0.00", 0),
            ("$0", 0),
            ("-$12.00", -1200),
            ("$-12.00", -1200),
            ("($12.00)", -1200),
            ("$12.00 CR", -1200),
            (".5", 50),
            (" $7.05 ", 705),
        ] {
            assert_eq!(Cents::parse(raw), Some(Cents(want)), "{raw}");
        }
        for raw in ["", "$", "abc", "1.234", "12,34.5x", "1.2.3", "--"] {
            assert_eq!(Cents::parse(raw), None, "{raw}");
        }
        assert_eq!(Cents(-1230).decimal(), "-12.30");
        assert_eq!(Cents(5).decimal(), "0.05");
        // The decimal spelling, kept as a JSON number (arbitrary_precision).
        assert_eq!(Cents(12500).money().to_string(), r#"{"value":125.00,"currency":"USD"}"#);
        assert_eq!(Cents(1230).money().to_string(), r#"{"value":12.30,"currency":"USD"}"#);
        assert_eq!(Cents(-1230).money()["value"].to_string(), "-12.30");
        assert_eq!(Cents(7713).money()["value"].to_string(), "77.13");
        assert!(Cents(12500).money()["value"].is_number());
        // And it reads back as written.
        let back: Value = serde_json::from_str(&Cents(12500).money().to_string()).unwrap();
        assert_eq!(back["value"].to_string(), "125.00");
        assert_eq!(back["value"].as_f64(), Some(125.0));
    }

    #[test]
    fn ids_are_fhir_ids_from_the_sites_numbers() {
        assert_eq!(id_of("bill", "A1*0099"), "bill-A1-0099");
        assert_eq!(id_of("bill", "--x--y--"), "bill-x-y");
        let long = id_of("bill", &"9".repeat(100));
        assert_eq!(long.len(), 64);
        assert!(is_id(&long));
        assert!(is_id(&mapper().bill_id("ACCT-1", "V 2")));
    }

    fn bill() -> Bill {
        Bill {
            number: "V100".into(),
            account: "ACCT-1".into(),
            service_start: "2020-01-02".into(),
            service_end: None,
            posted: Some("2020-01-10".into()),
            rendering: Some("Dr. Example".into()),
            description: Some("Office visit".into()),
            insurer: Some("Example Health Plan".into()),
            insurer_claim: Some("CLM-1".into()),
            lines: vec![
                Charge {
                    date: "2020-01-02".into(),
                    code: Some(Code {
                        system: Some("http://www.ama-assn.org/go/cpt".into()),
                        code: "99213".into(),
                        display: None,
                    }),
                    description: Some("Visit".into()),
                    quantity: Some(1),
                    amounts: Amounts {
                        billed: c("200.00"),
                        allowed: c("120.00"),
                        insurance_paid: c("90.00"),
                        adjustment: c("80.00"),
                        patient_paid: c("10.00"),
                        patient_owes: c("20.00"),
                    },
                },
                Charge {
                    date: "2020-01-02".into(),
                    code: None,
                    description: Some("Lab".into()),
                    quantity: None,
                    amounts: Amounts {
                        billed: c("50.00"),
                        patient_owes: c("0.00"),
                        ..Amounts::default()
                    },
                },
            ],
            totals: Amounts::default(),
            voided: false,
        }
    }

    #[test]
    fn a_bill_is_an_invoice_with_contained_charge_items() {
        let v = mapper().bill(&bill(), "2020-02-01T00:00:00Z");
        assert_eq!(v["resourceType"], "Invoice");
        assert_eq!(v["id"], "bill-ACCT-1-V100");
        assert_eq!(v["status"], "issued");
        assert_eq!(v["identifier"][0]["system"], "urn:examplecli:org-1:bill");
        assert_eq!(v["identifier"][1]["type"]["text"], INSURER_CLAIM);
        assert_eq!(v["identifier"][1]["assigner"]["display"], "Example Health Plan");
        assert_eq!(v["contained"][0]["resourceType"], "ChargeItem");
        assert_eq!(v["contained"][0]["occurrenceDateTime"], "2020-01-02");
        assert_eq!(v["contained"][0]["code"]["coding"][0]["code"], "99213");
        assert_eq!(v["contained"][1]["code"]["text"], "Lab");
        assert_eq!(v["lineItem"][0]["chargeItemReference"]["reference"], "#charge-1");
        let parts = v["lineItem"][0]["priceComponent"].as_array().unwrap();
        let codes: Vec<(&str, &str)> = parts
            .iter()
            .map(|p| {
                (
                    p["type"].as_str().unwrap(),
                    p["code"]["coding"][0]["code"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            codes,
            [
                ("base", "billed"),
                ("informational", "allowed"),
                ("deduction", "insurance-payment"),
                ("discount", "insurance-adjustment"),
                ("deduction", "patient-payment"),
                ("informational", "patient-responsibility"),
            ]
        );
        assert_eq!(v["contained"][0]["performer"][0]["actor"]["display"], "Dr. Example");
        // No tax on a medical bill: both totals are what the patient owes.
        assert_eq!(v["totalNet"]["value"].to_string(), "20.00");
        assert_eq!(v["totalGross"], v["totalNet"]);
        // The billed amount, and the rest of the bill's breakdown, are its
        // own price components, summed from the lines here.
        let totals = v["totalPriceComponent"].as_array().unwrap();
        assert_eq!(totals.len(), parts.len());
        assert_eq!(totals[0]["type"], "base");
        assert_eq!(totals[0]["code"]["coding"][0]["system"], COMPONENT_SYSTEM);
        assert_eq!(totals[0]["code"]["coding"][0]["code"], "billed");
        assert_eq!(totals[0]["amount"]["value"].to_string(), "250.00");
        assert_eq!(totals[2]["code"]["coding"][0]["code"], "insurance-payment");
        assert_eq!(totals[2]["amount"]["value"].to_string(), "90.00");
        assert_eq!(v["date"], "2020-01-10");
        assert_eq!(v["account"]["reference"], "Account/account-ACCT-1");
        assert_eq!(v["meta"]["lastUpdated"], "2020-02-01T00:00:00Z");
    }

    #[test]
    fn a_bill_without_lines_carries_its_date_and_amounts() {
        let mut b = bill();
        b.lines.clear();
        b.totals = Amounts {
            billed: c("10.00"),
            patient_owes: c("0"),
            ..Amounts::default()
        };
        b.service_end = Some("2020-01-04".into());
        let v = mapper().bill(&b, "2020-02-01T00:00:00Z");
        assert_eq!(v["status"], "balanced");
        // Its one line is a ChargeItem holding the date of service.
        let items = v["contained"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["occurrencePeriod"]["start"], "2020-01-02");
        assert_eq!(items[0]["occurrencePeriod"]["end"], "2020-01-04");
        assert!(items[0].get("occurrenceDateTime").is_none());
        assert_eq!(items[0]["code"]["text"], "Office visit");
        assert_eq!(v["lineItem"][0]["chargeItemReference"]["reference"], "#charge-1");
        assert_eq!(
            v["lineItem"][0]["priceComponent"][0]["amount"]["value"].to_string(),
            "10.00"
        );
        assert_eq!(v["totalPriceComponent"][0]["code"]["coding"][0]["code"], "billed");
        assert_eq!(v["totalPriceComponent"][0]["amount"]["value"].to_string(), "10.00");
        assert_eq!(v["totalNet"]["value"].to_string(), "0.00");
        assert_eq!(v["totalGross"], v["totalNet"]);
        assert_eq!(v["date"], "2020-01-10", "the posting date, not the service");
        assert!(v.get("note").is_none());
        // A one-day service is a dateTime, and no description is "Services".
        b.service_end = None;
        b.description = None;
        let one = mapper().bill(&b, "x");
        assert_eq!(one["contained"][0]["occurrenceDateTime"], "2020-01-02");
        assert_eq!(one["contained"][0]["code"]["text"], "Services");
        b.voided = true;
        assert_eq!(mapper().bill(&b, "x")["status"], "cancelled");
    }

    #[test]
    fn a_payment_names_the_bills_it_paid() {
        let p = Payment {
            number: Some("T-9".into()),
            account: "ACCT-1".into(),
            date: "2020-03-01".into(),
            amount: Cents(2000),
            method: Some("Visa".into()),
            bills: vec!["V100".into(), "V404".into()],
        };
        let v = mapper().payment(&p, 0, |b| b == "V100");
        assert_eq!(v["resourceType"], "PaymentReconciliation");
        assert_eq!(v["id"], "payment-T-9");
        assert_eq!(v["status"], "active");
        assert_eq!(v["paymentDate"], "2020-03-01");
        assert_eq!(v["paymentAmount"]["value"].to_string(), "20.00");
        assert!(v.get("paymentIssuer").is_none(), "R4 allows an Organization only");
        assert_eq!(v["detail"].as_array().unwrap().len(), 1);
        assert_eq!(v["detail"][0]["request"]["reference"], "Invoice/bill-ACCT-1-V100");
        assert_eq!(v["detail"][0]["payee"]["display"], "Example Physicians");
        assert!(
            v["detail"][0].get("amount").is_none(),
            "split over two bills, by amounts the site does not show"
        );
        // A payment that names one bill applied all of itself to it.
        let one = Payment {
            bills: vec!["V100".into()],
            ..p.clone()
        };
        let v = mapper().payment(&one, 0, |_| true);
        assert_eq!(v["detail"][0]["amount"]["value"].to_string(), "20.00");
        // One naming no bill in the stream still names the provider paid.
        let v = mapper().payment(&p, 0, |_| false);
        assert_eq!(v["detail"].as_array().unwrap().len(), 1);
        assert_eq!(v["detail"][0]["payee"]["display"], "Example Physicians");
        assert!(v["detail"][0].get("request").is_none());
        assert!(v["detail"][0].get("amount").is_none());
        let anonymous = Payment { number: None, ..p };
        assert_eq!(
            mapper().payment(&anonymous, 3, |_| false)["id"],
            "payment-ACCT-1-2020-03-01-20-00-3"
        );
    }

    #[test]
    fn an_account_names_its_patient_and_guarantor() {
        let a = Account {
            number: "ACCT-1".into(),
            provider: Provider {
                id: "org-1".into(),
                name: "Example Physicians".into(),
            },
            patient_name: Some("Pat Example".into()),
            guarantor_name: Some("Robin Example".into()),
            balance: c("20.00"),
            patient_balance: c("20.00"),
            insurance_pending: None,
            active: true,
        };
        let v = mapper().account(&a, "2020-02-01T00:00:00Z");
        assert_eq!(v["subject"][0]["display"], "Pat Example");
        assert_eq!(v["guarantor"][0]["party"]["display"], "Robin Example");
        assert!(v.get("balance").is_none(), "R4 Account has no balance");
    }

    #[test]
    fn a_statement_is_a_bill_document() {
        let s = Statement {
            number: "S1".into(),
            account: "ACCT-1".into(),
            date: "2020-01-15".into(),
            amount_due: c("20.00"),
            due_date: Some("2020-02-05".into()),
            bills: vec!["V100".into()],
        };
        let v = mapper().statement(
            &s,
            "statement-ACCT-1-S1-pdf",
            "application/pdf",
            1234,
            "Statement 2020-01-15",
        );
        assert_eq!(
            v["type"],
            json!({ "coding": [{ "system": "urn:pk-med:doc-type", "code": "bill" }], "text": "bill" })
        );
        assert_eq!(v["date"], "2020-01-15T00:00:00Z");
        assert_eq!(v["content"][0]["attachment"]["url"], "Binary/statement-ACCT-1-S1-pdf");
        assert_eq!(v["context"]["related"][0]["reference"], "Account/account-ACCT-1");
        assert_eq!(v["context"]["related"][1]["reference"], "Invoice/bill-ACCT-1-V100");
        assert_eq!(v["meta"]["lastUpdated"], "2020-01-15T00:00:00Z");
        let text = v.to_string();
        assert!(
            !text.contains("20.00") && !text.contains("2020-02-05"),
            "no amount due or due date: {text}"
        );
        // A statement listing no bills still names its Account.
        let alone = Statement { bills: Vec::new(), ..s };
        let v = mapper().statement(&alone, "b", "application/pdf", 1, "t");
        assert_eq!(v["context"]["related"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_receipt_is_typed_receipt() {
        assert_eq!(
            DocType::Receipt.concept(),
            json!({ "coding": [{ "system": "urn:pk-med:doc-type", "code": "receipt" }], "text": "receipt" })
        );
    }
}
