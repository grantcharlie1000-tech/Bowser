# Code signing (the SmartScreen warning)

An unsigned `Bowser.exe` opened from a download shows **"Windows protected your PC —
Microsoft Defender SmartScreen prevented an unrecognised app from starting"**, with
"Unknown publisher" under *More info*. Nothing inside the program can turn this off:
SmartScreen checks the file before any of Bowser's code runs. What it looks at is the
file's Authenticode signature and the reputation Microsoft has recorded for that signer.

So the fix is to sign every release with a code-signing certificate that chains to a root
Windows trusts. `release.yml` does this with **Azure Artifact Signing** (formerly *Trusted
Signing*) as soon as it is configured; until then releases are built unsigned, as before,
and the build job prints an "Unsigned release" warning.

## What the workflow does

In the `build` job of `.github/workflows/release.yml`, right after `cargo build`:

1. **Sign executable with Azure Artifact Signing** signs `target/release/bowser.exe`
   (SHA-256, RFC 3161 timestamp from `timestamp.acs.microsoft.com`). It runs only when the
   repository variable `AZURE_SIGNING_ACCOUNT` is set.
2. **Require a trusted signature when signing is configured** fails the release if the
   signature is not `Valid` or has no timestamp. Artifact Signing certificates last about
   three days, so without the timestamp the signature would stop validating almost at once.
3. The smoke test, `build-metadata.json` (new `authenticode` field) and the publisher all
   work on the signed bytes, so the published hash is the signed file's hash.

The updater already accepts this: `src/updater.rs` runs `WinVerifyTrust` on downloads and
takes a validly signed file (it refuses only signed-but-untrusted ones).

## One-time setup (about 30 minutes plus Microsoft's identity check)

1. **Azure subscription.** Any pay-as-you-go subscription. Register the resource provider
   `Microsoft.CodeSigning` (Subscription > Resource providers).
2. **Artifact Signing account.** Create one in a region you pick (for example *UK South*
   or *West Europe*); the Basic tier is a small monthly fee. Note its **name** and its
   **account URI**, e.g. `https://weu.codesigning.azure.net/` (the region prefix must match).
3. **Identity validation.** In the account, create an identity validation (Public Trust).
   Microsoft verifies the organisation or individual; this can take from hours to a few
   days. Eligibility (which countries, organisations vs. individuals) is set by Microsoft;
   check the current rules in the Artifact Signing docs before starting. The validated name
   is what Windows shows as the publisher, so use the name you want users to see.
4. **Certificate profile.** Once validation is approved, create a *Public Trust*
   certificate profile linked to it. Note its **name**.
5. **Identity for GitHub Actions.** Microsoft Entra ID > App registrations > New
   registration (e.g. `bowser-release-signing`). Create a client secret. On the Artifact
   Signing account, Access control (IAM) > Add role assignment >
   **Artifact Signing Certificate Profile Signer** (may still be called *Trusted Signing
   Certificate Profile Signer*) > that app.
6. **GitHub settings** (repository Settings > Secrets and variables > Actions):

   | Kind | Name | Value |
   | --- | --- | --- |
   | Secret | `AZURE_TENANT_ID` | Directory (tenant) ID of the app registration |
   | Secret | `AZURE_CLIENT_ID` | Application (client) ID |
   | Secret | `AZURE_CLIENT_SECRET` | The client secret value |
   | Variable | `AZURE_SIGNING_ENDPOINT` | The account URI from step 2 |
   | Variable | `AZURE_SIGNING_ACCOUNT` | The account name from step 2 |
   | Variable | `AZURE_SIGNING_PROFILE` | The certificate profile name from step 4 |

   Setting `AZURE_SIGNING_ACCOUNT` is what switches signing on.

7. **Check it** with a dry run:
   `gh workflow run release.yml --repo grantcharlie1000-tech/Bowser --ref main -f commit=<main sha> -f dry_run=true`.
   The build summary should end with `Authenticode Valid`. Locally,
   `Get-AuthenticodeSignature .\Bowser.exe` on a downloaded release shows `Valid` and your
   validated name.

## What to expect after signing

- *More info* on any remaining prompt shows your publisher name instead of "Unknown
  publisher", and the file properties have a *Digital Signatures* tab.
- SmartScreen is reputation-based. A brand-new signer can still be warned about for its
  first releases until enough people have run signed builds without trouble; the warning
  then stops for every release signed with the same identity. No certificate type
  (including EV) skips this any more. Keep signing with the same account and profile so the
  reputation carries over between releases.
- Already-published unsigned releases stay unsigned; only releases built after setup are
  signed.

## Alternatives

Any Authenticode certificate works if the signing step is swapped: an OV certificate from a
public CA on a cloud HSM (the CA's own signing tool replaces step 1 above), or, for an
open-source project with an OSI licence, a free certificate through the SignPath
Foundation. Artifact Signing is used here because it needs no hardware token, renews
itself, and has a maintained GitHub Action.
