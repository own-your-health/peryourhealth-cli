# The FHIR peryourhealth writes

`records export --ndjson` writes `records-stream/v1` (medical-common's
`health-records/v1` profile) with `method: portal`: what the site shows,
mapped by hand to FHIR R4 in `src/bill.rs`. Every resource carries
`meta.source` `peryourhealth:site` and a tag saying it was mapped. Ids are
stable across runs, derived from the site's own numbers, so a records store
merges the same bill pulled twice.

| What the site shows | FHIR | Notes |
|---|---|---|
| A billing account | `Account` | `identifier` the account number (system `urn:peryourhealth:<org>:account`); `subject` the Patient; `owner` the provider; `guarantor[].party.display` the responsible party; `status` `active` or `inactive`. Written before the Invoices, so `Invoice.account` resolves in the stream. R4's Account has no balance element and peryourhealth writes no balance: a balance is the sum of the open (`issued`) Invoices' `totalNet` |
| A service billed (a visit or claim at the provider) | `Invoice` | `identifier[]` the site's number for it first (system `urn:peryourhealth:<org>:bill`), then the insurer's claim number with `type.text` `insurer-claim` and `assigner.display` the payer where the site shows one. `status` `issued` while the patient owes on it, `balanced` at 0, `cancelled` when voided. `subject` the Patient; `issuer` the provider (`reference` and `display`); `account` the Account; `participant[]` the rendering provider; `date` the posting or statement date the site shows, never the date of service |
| A charge on it | `Invoice.contained[]` `ChargeItem`, pointed at by `lineItem[].chargeItemReference` (`#charge-<n>`) | `occurrenceDateTime` the date of service (`YYYY-MM-DD`); `code` CPT or HCPCS with their system URIs where the site shows one, else the site's description as text; `performer[].actor.display` the rendering provider; `quantity`. A bill the site shows without lines gets one ChargeItem of its own, code text the bill's name (else `Services`), holding its date of service: `occurrenceDateTime`, or `occurrencePeriod` when the service spans days |
| A charge's amounts | `lineItem[].priceComponent[]`, code system `urn:pk-med:bill-component` | `base` `billed`; `informational` `allowed`; `deduction` `insurance-payment`; `discount` `insurance-adjustment`; `deduction` `patient-payment`; `informational` `patient-responsibility` (what the patient still owes). A bill without lines has its whole split on its one line |
| The bill's totals | `totalPriceComponent[]`, `totalNet`, `totalGross` | `totalPriceComponent` is the bill's own breakdown in the same codes, from the site's totals where it shows them, else summed from the lines: the billed amount is its `base` `billed`. `totalNet` is what the patient still owes. R4 puts both totals after the price components and `totalGross` adds tax, which a medical bill has none of, so `totalGross` equals `totalNet`; it is not the billed amount |
| A payment made | `PaymentReconciliation` | `status` `active`; `paymentDate`, `paymentAmount`, `paymentIdentifier` and `identifier` the site's transaction number; `disposition` how it was paid; `detail[]`, each `type` payment and `payee` the provider (`reference` and `display`): one per bill it paid where the site says, with `request` the Invoice and, when it paid that bill alone, `amount`; else one with the payee only. Insurance payments stay on the Invoice's lines |
| A statement | `DocumentReference` and its `Binary` | `type` `{"coding": [{"system": "urn:pk-med:doc-type", "code": "bill"}], "text": "bill"}`; `date` the statement date; `context.related` the Account, then the Invoices it lists where known. Amount due and due date have no R4 element, so none is written: a records store reads them from the file |
| A payment receipt the site serves as a file | `DocumentReference` and its `Binary` | built by `Mapper::document` with `DocType::Receipt`: typed as a statement is, code `receipt`, so a receipt's amount is never read as an amount due; `identifier` the receipt's number (system `urn:peryourhealth:<org>:receipt`), ids `receipt-<account>-<number>`; `context.related` the Account and the Invoices it paid |

Every `Money` is `{"value": <number>, "currency": "USD"}`, the value a
JSON number in the decimal spelling a bill prints (`125.00`, `-12.30`):
never a string, which R4 forbids, and never rounded to `125.0`.

A records store keys a bill as `bill|<first identifier>|<earliest date of
service>` and a payment as `payment|<first identifier>|<paymentDate>`
(`id|...` where a payment has no number), and matches a bill to the
insurer's claim by the insurer's claim number where present, else by
person, date of service, provider and billed amount.
