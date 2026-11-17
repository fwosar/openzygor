# Zygor Client protocol notes

These notes describe the behavior observed in the locally installed Windows Zygor Client 4.13.2.
They are an interoperability reference, not an official API specification.
Zygor can change the service without notice.

## Evidence

The installed application is Electron with an ASAR archive and unpacked JavaScript bundles.
Its package metadata identifies version `4.13.2` and Electron-era Node dependencies.
The bundles expose modules for login, API requests, package listing, checksum comparison, guide extraction, licence updates, and edition definitions.
Inspection used parsed JavaScript; the original application was not executed as part of extraction.

The inspected bundles had these SHA-256 hashes, including their packaging padding:

| File | SHA-256 |
| --- | --- |
| `renderer-bundle.js` | `34ea9b5127b6fd23aab46cb86be29631fa60231baf6d2297badcb100e6f6cb8f` |
| `background-bundle.js` | `8fe555b4a199147504214f291e21b19da65a69078bdd9df1bfdcdc5b2b9c2315` |
| `main-bundle.js` | `8f9b9385a5bea49096547c629ac280e704fd7fc8203b6e23dd510f402db248b2` |

Selected fields in existing client logs confirmed package identifiers, version strings, availability flags, and account response field types.
Installed addon metadata confirmed `Ver.lua` revision syntax, TOC version syntax, and `ZygorGuidesViewerForever/.zygor-meta` with a `DELETE` array.
Account credentials and personal field values were not copied into this repository.
Tests use invented credentials and guide files.

## Transport

The packager endpoint is `https://data.zygorguides.com/updater/packager.php`.
The observed client sends:

```text
X-Zygor-Api: 4
X-Zygor-UV: 4.13.2
X-Zygor-OS: <platform and OS release>
X-Zygor-ServerRestart: false
Accept: application/json
```

OpenZygor sends `linux` for the platform header and identifies itself through `User-Agent: OpenZygor/0.1.0` and `source=OpenZygor`.
The `uv` parameter and version header retain `4.13.2` as a protocol compatibility value, independent of the OpenZygor application version.
Requests use TLS certificate verification and reject HTTP redirects.

Common query parameters are `do`, `uv`, `source`, `full=1`, and `wwwdev=0`.
Credentials and tokens occur in query parameters in the observed protocol.
OpenZygor omits request URLs and raw response bodies from errors to avoid exposing them.

## Login and session renewal

Login is a GET request with these additional parameters:

```text
do=validate
user=<Zygor username>
pass=Qö<base64 of UTF-8 password>
auto=false
```

The `Qö` prefix consists of `Q` and U+00F6.
This encoding is not encryption and is not a password hash.
The parameter must be URL encoded after constructing it.
The official client rejects email addresses in the username field before submitting the request.

A successful response contains `user.login`, `user.token`, and `access`.
The client requires a nonempty token.
Observed access fields include `level`, `status`, `plan_name`, `elite_expiry_hr`, and `rebill_date_hr`.
Some responses include extensive additional account data; OpenZygor deserializes only the fields it needs.

Remembered sessions and token renewal use `do=validate`, `token=<token>`, and `auto=true`, without username or password.
The returned token may rotate and must replace the prior token before subsequent requests.
OpenZygor validates its session before package refreshes and installations.
Passwords are not persisted, and remembered tokens use the desktop Secret Service.

HTTP 401 indicates invalid or expired credentials, or an account lock.
The official client recognizes `userlocked` as a distinct server code.
OpenZygor reports a generic authentication error covering those cases without displaying the response body.
HTTP 503 indicates maintenance, and 429 is treated as rate limiting.

## Package discovery

The client issues `do=list_packages` with `token`, `region`, and `installed`.
`installed` is a JSON object mapping game IDs to installed version strings or null.

```json
{
  "wow": "9.6.100",
  "wow-classic": null,
  "wow-classic-tbc": null,
  "wow-classic-tbc-anniv": null,
  "wow-forever": null
}
```

The response contains `success`, `packages`, and optionally `access` and `user`.
Each package includes `game`, `name`, `ver`, and `status`.
The client uses `status.available` to enable downloads, `status.hidden` to control visibility, and `status.minimessage` to explain restrictions.
OpenZygor enforces those availability fields again in the Rust install command.
Unknown game IDs are accepted by the response parser but have no installation action.

The observed edition mapping is:

| Game ID | Display name | WoW directory | Request package |
| --- | --- | --- | --- |
| `wow` | Retail | `_retail_` | `ZygorGuidesViewer` |
| `wow-classic` | Classic Era | `_classic_era_` | `ZygorGuidesViewerClassic` |
| `wow-classic-tbc` | Mists of Pandaria Classic | `_classic_` | `ZygorGuidesViewerClassicTBC` |
| `wow-classic-tbc-anniv` | TBC Anniversary | `_anniversary_` | `ZygorGuidesViewerClassicTBCAnniv` |
| `wow-forever` | Forever | `_forever_` | `ZygorGuidesViewerForever` |

The progression edition retains a historical TBC identifier even though the client displays Mists of Pandaria.
The server's `name` controls the installed addon directory, while the table supplies the `package` request parameter.
OpenZygor validates the directory name before using it as a path.

## Installed version

Read `Interface/AddOns/<name>/<name>.toc` and `Ver.lua`.
The TOC supplies the major/minor component from `## Version:`, and `Ver.lua` supplies the decimal revision following `Revision:`.
For example, a TOC version of `9.6` and revision `100` produce `9.6.100`.
Lua is treated as text and is never executed.

## Guide downloads and repair

The official client recursively enumerates the selected addon and computes an MD5 for every local file.
It constructs a CRLF-delimited manifest:

```text
F ZygorGuidesViewer/Guide.lua##<lowercase MD5>\r\n
F ZygorGuidesViewer/Ver.lua##<lowercase MD5>\r\n
```

Windows uses backslashes in filesystem-relative paths; the Mac client uses forward slashes.
OpenZygor sends forward slashes.
The checksum is used for comparison, not as a cryptographic signature.

The comparison request is a POST to the packager endpoint:

```text
Query: do=comp&package=<package>&token=<token>&region=<region>&from=<installed version>
Body:  application/x-www-form-urlencoded, with list=<manifest>
```

Common parameters are included as described above.
For a fresh installation the manifest is empty and OpenZygor uses `from=0.0.000`.
The response is a ZIP archive, potentially containing only changed files, with an optional `X-Zygor-Guides-Version` header.
Repair repeats the checksum comparison even when the installed and advertised version strings match.
Each configured installation gets its own manifest and download, since two installations may have different missing or modified files.

Archive paths include the addon directory as their first component.
The official extraction code recognizes `.zygor-meta` files, parses their JSON, and removes files listed in `DELETE` after extraction.
The metadata may be located inside the addon directory.
An absent or null deletion list means no deletions.

OpenZygor copies existing files to staging, applies the archive there, processes the deletion metadata, and validates the resulting version before replacing the addon.
It confines archive writes and deletion paths to the selected addon and refuses links and special files.
Limits are 256 MiB for the compressed download, 1 GiB for extracted files, 128 MiB per file, and 100,000 entries.
The current addon is rehashed before replacement so edits made during a download are preserved by aborting the update.
A filesystem lock prevents concurrent OpenZygor processes from updating the same addon.

## Licence

`do=license` is a GET request with the game package, token, and common parameters.
The returned text is written to `<addon>/Licence.lua`.
OpenZygor fetches the licence before committing a guide install, so a failed licence request leaves the current addon intact.
The official client also refreshes licences during its wider periodic maintenance cycle; OpenZygor refreshes them during install, update, and repair.

## Other observed operations

The official client also contains `query`, `listfiles`, and `listfiles_user` packager operations.
It compares product ownership during its update-query flow and can remove files no longer offered to the account.
OpenZygor relies on the account-filtered catalogue, checksum response, server deletion metadata, and licence; it does not reproduce the separate product-ownership cleanup pass.
For a full local removal, use Uninstall.

Other subsystems use `data.zygorguides.com/ahscan/` for auction trends and `data.zygorguides.com/storage/uplo2.php` for settings synchronization.
The latter includes timestamp comparison, uploads, downloads, and conflict resolution.
These are separate from guide installation and are not implemented here.

## Verification limits

The source inspection and existing logs establish the protocol formats, but do not prove that the remote service will accept this client today.
The live service may enforce additional client checks or change its behavior.
HTTP and GUI tests use a synthetic service, and a real account session still needs to be tested outside the restricted workspace sandbox.
Packaging and desktop keyring integration also need validation on the target desktop.
The native WebDriver harness follows the same Linux capability mapping used by [Tauri's driver](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-driver/src/server.rs).
