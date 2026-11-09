# iOS distribution

The iOS app (`mobile/ios`, bundle ID `app.zerus.mobile`) has three ways to reach
a phone. All three install the same app ID, so a phone keeps one Zerus and its
Keychain pairing across them.

| Channel | Source | Version shown in About |
| --- | --- | --- |
| Local Dev | `mobile/ios/scripts/dev-install.sh` on the Mac the phone is attached to | `X.Y.Z-local` and the Git revision |
| Dev | Successful full Nightly verification, uploaded to TestFlight, then added to Public beta and submitted for Beta App Review | `X.Y.Z-dev.N`, N = the CI run number |
| Stable | A maintainer-dispatched upload, submitted for App Store review | `X.Y.Z` |

`mobile/ios/DEV_VERSION` is the TestFlight marketing version. Keep it unchanged
for ordinary Dev uploads, patches and CI reruns; advance it only when a new
marketing/App Store version is actually needed. Do not bump it just to publish
another build or to work around review. `mobile/ios/VERSION` is the deliberate
Stable App Store version. App Store versions are three integers; the growing
Dev sequence travels as the build number (`CFBundleVersion`), never in the
marketing version. Apple may review later builds of the same version too;
keeping the version avoids an unnecessary reset, not all beta review.

## How uploads work

`.github/workflows/ios-testflight.yml` archives the exact main revision with the
newest Xcode 26 or later and uploads it with `xcodebuild -exportArchive`
(`method app-store-connect`, `destination upload`). Signing is automatic: the
archive is signed with an Apple Development identity, and the upload re-signs it
with a cloud-managed distribution certificate through an App Store Connect API
key. `mobile/ios/scripts/testflight-upload.sh` does the work and checks the
archived bundle ID, versions and channel before uploading. Before any signing
secret is in the environment, a step of its own builds the project task replica
(`mobile/ios/scripts/build-replica.sh`, Rust 1.99.1 with the Apple targets),
since cargo runs its dependencies' build scripts.

A runner starts without a development identity, and automatic signing would then
create a new development certificate on every run until the team reaches Apple's
certificate limit. CI therefore imports one dedicated development identity into a
temporary keychain. The script refuses to archive without it, requires exactly one
valid Apple Development identity of the team in that keychain, and stops before
uploading when the archive is signed by any other certificate. It removes the
keychain, the identity file and the API key and restores the keychain search list
on every exit. No certificate or profile is stored in the repository. On a
maintainer's Mac the script uses that Mac's own development identity and refuses
to import one.

If an archive reports that the account has reached the maximum number of
certificates, some run archived without the dedicated identity. Revoke only an
`Apple Development: Created via API` certificate after proving that it belongs
to an ephemeral CI archive, its private key was not retained or reused, and no
retained signing identity or profile depends on it. Matching its fingerprint to
the `codesign --sign` leaf in an archive log establishes attribution, not safe
revocation by itself. Never revoke the dedicated CI identity or a personal,
distribution or Developer ID certificate.

- **Dev:** `nightly.yml` calls the upload workflow after full Windows/iOS and
  shared checks succeed, when `IOS_TESTFLIGHT_ENABLED=true`. Build numbers come
  from successful upstream main CI for the exact source SHA; Nightly's own
  sequence never resets them. A read-only GET-only App Store Connect preflight
  checks the app, marketing version and build number before any archive or
  signing identity import. Known valid/processing builds are kept without
  uploading again; failed/expired/ambiguous records or API errors refuse.
  Internal testers with automatic distribution receive processed new builds.
- **Public beta:** after a successful Nightly Dev upload, a separate protected
  job waits up to ten minutes for that exact marketing version/build number to
  finish processing. It preserves existing English What to Test notes (or creates
  them), adds the exact build to the external **Public beta** group, and submits
  Beta App Review if that build is not already submitted or approved. There is
  no second archive/upload and no marketing-version change. A group assignment
  or submission is not instant delivery: `WAITING_FOR_REVIEW` and `IN_REVIEW`
  are normal pending states; Apple must approve before external testers receive
  a build. Dev uploads are not internal-testing-only.
- **Stable:** dispatch **iOS TestFlight** from `main` with channel `stable`
  after adding `mobile/ios/VERSION` and once that commit's main CI has succeeded.
  The manual workflow additionally requires full exact-source platform verification:
  the candidate's build number is that CI run number, so it is higher than every
  Dev build uploaded with the same version before the Dev base advanced. The uploaded build is then submitted for
  review in App Store Connect, together with the store listing.

The upload job runs in the `ios-testflight` environment and verifies that it runs
for `refs/heads/main` of `ufna/zerus`. Nightly forwards the five secret names;
manual Stable uses the same protected environment. PR code and other workflows
never receive them. The workflow has no cache access.

## Public beta automation and status

`scripts/ci/ios-public-beta.py` uses Apple's exact app, external group and
processed build identities. Every HTTP request gets a fresh two-minute JWT
scoped to its actual method and canonical path/query. A GET-scoped token must
never be reused for PATCH or POST. What to Test uses
[`POST /v1/betaBuildLocalizations`](https://developer.apple.com/documentation/appstoreconnectapi/post-v1-betabuildlocalizations)
or [`PATCH /v1/betaBuildLocalizations/{id}`](https://developer.apple.com/documentation/appstoreconnectapi/patch-v1-betabuildlocalizations-_id_);
the build's localization relationship route is read-only. Group membership uses
[`POST /v1/betaGroups/{id}/relationships/builds`](https://developer.apple.com/documentation/appstoreconnectapi/post-v1-betagroups-_id_-relationships-builds),
followed by [`POST /v1/betaAppReviewSubmissions`](https://developer.apple.com/documentation/appstoreconnectapi/post-v1-betaappreviewsubmissions).

Before each missing notes/group/review write, CI durably uploads a separate
90-day intent artifact keyed by marketing version, build number and action.
The apply step binds that artifact to the same original run, attempt, source,
app, group and build, then checks Apple's state again. Already observed actions
perform no write. A failed, timed-out or lost write response is resolved through
bounded GET readback, never another mutation. If an older intent is still not
observed at Apple, later runs refuse too. Inspect that original action manually;
do not delete the intent or rerun writes just to clear the error. The automation
never cancels reviews, removes group builds, changes a public link, or replaces
existing notes. Artifacts contain build metadata only, never API keys or JWTs.

The job summary lists the assigned group builds, exact target, notes presence,
external state and beta-review state. Rejection, missing group membership,
unsubmitted review, unknown/incomplete reads, processing timeout, or a review
pending for more than 48 hours produces an error and a failed job. The promotion
job is optional at the workflow-result level (`continue-on-error` on the job,
not its steps), so its failure remains visible without holding the independent
Nightly desktop publisher. Upload and verification failures remain blocking.

A separate schedule in `ios-testflight.yml` runs **read-only** every six hours,
without depending on Nightly verification or upload. It compares the latest iOS
build of the current marketing version with Public beta and fails its own run
on lag, rejection or stuck review. It uses only the three existing ASC API-key
secrets in `ios-testflight`, no signing identity, no writes and no automatic
recovery. This keeps the lag visible even when Nightly itself fails. Both paths
are restricted to upstream `main`; no PR code receives the secrets. Collection
reads refuse rather than silently truncate beyond 200 resources. API failures
log fixed categories/HTTP status only, never arbitrary bodies, headers or tokens.

## One-time setup (maintainer)

1. **App Store Connect app record.** In App Store Connect → Apps → New App:
   platform iOS, bundle ID `app.zerus.mobile` (register it under Certificates,
   Identifiers & Profiles first if it is not offered), a unique app name, primary
   language English, and a SKU such as `zerus-ios`. The record cannot be created
   through the API.
2. **App Store Connect API key.** Users and Access → Integrations → App Store
   Connect API → Team Keys: create a key with the **Admin** role for this
   runbook's cloud-managed distribution signing. The key needs
   [access to cloud-managed distribution certificates](https://developer.apple.com/help/account/certificates/cloud-managed-certificates).
   A Developer-role notarization key without that access may work for local
   development signing yet fail this export. Download the `.p8` once and note
   its key ID and the issuer ID.
3. **GitHub environment.** Create the `ios-testflight` environment with a
   deployment branch policy that admits only `main`. Add the secrets
   `APP_STORE_CONNECT_API_KEY_P8` (the `.p8` file contents),
   `APP_STORE_CONNECT_API_KEY_ID` and `APP_STORE_CONNECT_API_ISSUER_ID`, and the
   environment or repository variable `IOS_TEAM_ID` (the Apple Developer team ID).
   Create one Apple Development certificate for CI alone: generate its key and
   certificate signing request outside any login keychain, create the
   certificate from the request, and pack the key and certificate into a PKCS#12
   file with a random password and the legacy encryption that macOS
   `security import` accepts. Store the base64 file as `IOS_DEVELOPMENT_P12` and
   its password as `IOS_DEVELOPMENT_P12_PASSWORD`, note the certificate ID and
   fingerprint privately, and delete the local files. A personal identity must not
   be used.
4. **TestFlight.** Add yourself to an internal testing group with automatic
   distribution, and install TestFlight on the phone. For the public beta, create
   an external group with a public link and fill in Test Information (beta
   description, feedback email, the privacy policy URL) and the Beta App Review
   contact; the review notes point to **Try demo** as for App Review.
5. **Enable Dev uploads.** Set the repository variable `IOS_TESTFLIGHT_ENABLED`
   to `true`. The next successful full Nightly uploads a new Dev build.
6. **Push notifications.** In Certificates, Identifiers & Profiles enable Push
   Notifications for `app.zerus.mobile`; automatic signing then includes the
   app's `aps-environment` entitlement (development for Local Dev builds,
   production for TestFlight and App Store builds). Under Keys create an Apple
   Push Notifications service key with the environment **Sandbox & Production**
   and the team-scoped restriction: the relay signs both `apns` and
   `apns_sandbox` wakes with one key. Download its `.p8` once, and give it to the
   relay privately as `apns_key`, `apns_key_id` and `apns_team_id` (see
   [mobile services](../services/mobile/README.md#push-configuration)). Without
   it the app still works; alerts then arrive only while Zerus is open or when
   iOS runs its background refresh.

For Local Dev on a Mac, either sign in to Xcode with an Apple ID of the team
(Xcode → Settings → Accounts) or pass the same API key to `dev-install.sh`
through `ZERUS_IOS_ASC_KEY_PATH`, `ZERUS_IOS_ASC_KEY_ID` and
`ZERUS_IOS_ASC_ISSUER_ID`; see [the iOS README](../mobile/ios/README.md).

## App Store submission checklist

- **Privacy:** the app has no tracking, ads or analytics. Messages, voice
  recordings and photos or videos travel through the relay, which keeps them
  until delivery and in its seven-day backups, so the published App Privacy
  label is **User Content** (Other User Content, Audio Data, Photos or Videos):
  App Functionality only, linked to the paired device, not used for tracking.
  `PrivacyInfo.xcprivacy` declares the same types, and `scripts/ci/check-source.py`
  keeps them in step. The privacy policy at `https://zerus.dev/privacy` describes
  the relay trust model in [the mobile architecture](mobile-architecture.md#trust-and-authorization).
- **Export compliance:** `ITSAppUsesNonExemptEncryption` is `false`; the app uses
  only standard HTTPS and the operating system's encryption.
- **App Review access:** reviewers cannot pair a computer. The review notes point
  to **Try demo** in Machines, which opens the labelled read-only preview, and
  explain that real use needs the user's own Zerus relay.
- **Store listing:** screenshots for the required iPhone sizes, description,
  keywords, support URL (`https://zerus.dev`) and age rating.
- **Creative assets:** the optional product page header (21:9, 3840 × 1646 PNG)
  and search results image (3:2) shown on iOS 27 and later. Keep the logo, text
  and screens inside the safe area of Apple's templates. Place them before
  submitting: App Store Connect refuses new placements while the version waits
  for review, so adding one later means removing the version from review.
