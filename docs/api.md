# PerYourHealth

PerYourHealth (www.peryourhealth.com) is the patient billing site of Optum's
revenue-cycle service. Physician groups and hospitals that bill through Optum
print a PerYourHealth ID on their statements, and the patient or responsible
party reads balances, services and statements there and pays. Receipts come
from `donotreply-rcmconnect@optum.com` ("PerYourHealth").

Optum publishes no API and no developer program for it. The site is
server-rendered pages (ASP.NET Core, with htmx and Alpine.js), so
peryourhealth reads the pages a signed-in patient's browser reads, with
`GET`s only.

Status: the signed-out pages and the hand-off to One Healthcare ID were
observed on 2026-11-20 without signing in. The signed-in pages are not yet
mapped; this file gets them when a sign-in is first made.

## Where the site is

- `https://www.peryourhealth.com/` answers with the sign-in page,
  `/Account/Login`.
- Every other path answers a visitor `302` to `/Account/Login`: `/Full`,
  `/Full/Services`, `/Full/Statements`, `/Account/Logout`, even
  `/robots.txt` and the footer's `/Privacy` link.
- The signed-in pages are under `/Full/` (the services page is
  `/Full/Services`).
- The site runs on Azure App Service: answers set `ARRAffinity` and
  `ARRAffinitySameSite` for a domain under
  `appserviceenvironment.net`, which a browser on www.peryourhealth.com does
  not keep. Its Content-Security-Policy allows scripts and styles from the
  site only.

## Sign-in

1. `GET /Account/Login`: one form, `POST /Account/Login`, fields
   `PatientIdEntry` (the PerYourHealth ID, 3 to 70 characters) and
   `__RequestVerificationToken` (ASP.NET's anti-forgery token, also sent as a
   `RequestVerificationToken` header on the page's htmx requests). The page
   says: "Please enter the PerYourHealth ID from your billing statement. When
   entering the ID, please include the entire number and special
   characters.", with an example of four digits, `*`, and seven digits. A
   payment receipt shows the same account as
   "Account ID", with a hyphen between the two parts, and the site accepts
   it in that form.
2. An ID the site does not accept re-renders the page with "Account access
   has been temporarily disabled. Please try again later." (seen for the
   page's own example ID).
3. An accepted ID goes to `/Account/SentinelInterstitial`, which shows the
   ID and says: "PerYourHealth partners with One Healthcare ID to secure
   access to your private patient billing account information. Please
   continue to One Healthcare ID to create a new secure ID or to sign in
   with a previously created ID." Its one form posts back to the same path
   ("Continue to One Healthcare ID").
4. That post redirects to One Healthcare ID,
   `https://identity.onehealthcareid.com/api/v1/auth/login`, Optum's sign-in
   service on CloudFront. It answers a headless Chromium `403` ("Request
   blocked"), so the sign-in happens in an ordinary Chrome window:
   `auth login --browser`. peryourhealth never drives or works around that
   page.
5. After One Healthcare ID, the browser comes back to the signed-in pages
   under `/Full/`. *Not yet observed:* the return path, the session cookies'
   names and lifetime, and whether one One Healthcare ID holds several
   PerYourHealth accounts.

There is no guest access: the account ID leads only to One Healthcare ID,
to sign in or to create an ID there.

## The session

`auth login --browser` keeps the whole www.peryourhealth.com cookie jar,
host-only cookies and domain cookies on its parents, `HttpOnly` included,
and none of One Healthcare ID's. It is confirmed by `GET /Full/Services`
with no redirect followed: a visitor gets `302` to `/Account/Login`, and a
signed-in patient the page.

## Terms

- www.peryourhealth.com links no terms of use. Its footer links "Privacy
  Policy" (`/Privacy`), which redirects to the sign-in page for a visitor. The
  sign-in page says: "This system is for the use of authorized users only.
  Individuals using this computer system are subject to having all of their
  activities on this system monitored and recorded by systems personnel."
- Optum's general Terms of Use (optum.com) forbid using "software or other
  means to access, 'scrape,' 'crawl,' or 'spider' any web pages or other
  services from the Online Services", and copying or harvesting data from
  them. Whether they govern www.peryourhealth.com, which does not link them,
  is not stated.
- One Healthcare ID's own terms are linked from its sign-in page, which only
  an ordinary browser can open; they are read and quoted here at the first
  sign-in.

peryourhealth reads only what the signed-in patient's own browser reads, one
request at a time, and never pays, changes a setting or sends a message.

## Traps

- The sign-in page's example ID answers "temporarily disabled", which reads
  like a lockout of the person trying; it is the site's answer to an ID it
  does not accept.
- `/Privacy` redirects a visitor to the sign-in page, so the privacy policy
  is readable only signed in.
- The receipt's "Account ID" separates its two parts with `-`; the sign-in
  page's example uses `*`.
