# The FHIR peryourhealth writes

`records export --ndjson` writes `records-stream/v1` (medical-common's
`health-records/v1` profile) with `method: portal`: what the site shows,
mapped by hand to FHIR R4 in `src/bill.rs`. Every resource carries
`meta.source` `peryourhealth:site` and a tag saying it was mapped. Ids are
stable across runs, derived from the site's own numbers, so a records store
merges the same bill pulled twice.

| What the site shows | FHIR | Notes |
|---|---|---|
| A billing account | `Account` | `identifier` the account number (system `urn:peryourhealth:<org>:account`); `subject` the Patient; `owner` the provider; `guarantor[].party.display` the responsible party; `status` `active` or `inactive`. R4's Account has no balance element: a balance is the sum of the open Invoices' `totalNet` |
| A service billed (a visit or claim at the provider) | `Invoice` | `identifier[]` the site's number for it (system `urn:peryourhealth:<org>:bill`), and the insurer's claim number with `type.text` `insurer-claim` and `assigner.display` the payer where the site shows one. `status` `issued` while the patient owes on it, `balanced` at 0, `cancelled` when voided. `subject` the Patient; `issuer` the provider; `account` the Account; `participant[]` the rendering provider; `date` the posting or statement date the site shows |
| A charge on it | `Invoice.contained[]` `ChargeItem`, pointed at by `lineItem[].chargeItemReference` (`#charge-<n>`) | `occurrenceDateTime` the date of service; `code` CPT or HCPCS where the site shows one, else the site's description as text; `performer[]` the rendering provider; `quantity` |
| A charge's amounts | `lineItem[].priceComponent[]`, each `amount` a `Money` in USD, code system `urn:pk-med:bill-component` | `base` `billed`; `informational` `allowed`; `deduction` `insurance-payment`; `discount` `insurance-adjustment`; `deduction` `patient-payment`; `informational` `patient-responsibility` (what the patient still owes) |
| The bill's totals | `totalGross`, `totalNet` | billed, and what the patient still owes: the site's totals, else the sum of the lines'. A bill the site shows without lines is one line of its own (`chargeItemCodeableConcept` text `Services`), with its date of service in `note` |
| A payment made | `PaymentReconciliation` | `paymentDate`, `paymentAmount`, `paymentIdentifier` the site's transaction number; `detail[].request` each Invoice it paid, where the site says; `disposition` how it was paid. Insurance payments stay on the Invoice's lines |
| A statement | `DocumentReference` (type text `bill`) and its `Binary` | `date` the statement date; `context.related` the Invoices it lists, where known |

A records store keys a bill as `bill|<first identifier>|<earliest date of
service>`, and matches it to the insurer's claim by the insurer's claim
number where present, else by person, date of service, provider and billed
amount.
