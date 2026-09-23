# Repository refs and tags pagination QA

Validated on 2026-09-23 against the production frontend build served by nginx at the normal CI/CD URL.

## Load reproduction

A temporary real Git repository was generated with 15,000 refs: 12,000 branches and 3,000 tags.

- Raw legacy `git for-each-ref` output: 1,293,000 bytes.
- Local legacy command time: 1,657 ms.
- The legacy branches tab would render 12,000 rows.
- Two unrestricted pull request datalists could contain up to 30,000 options.
- The updated API streams matching refs and stops after the requested bounded page instead of collecting every ref.
- The updated UI renders 100 rows per page and limits live suggestions to 50 while preserving manual revision entry.

## Browser matrix

The repository refs workflow was checked at 375x812, 768x900, 1280x900, 1920x1080, and 2560x1440 in light, gray, and dark themes.

The automated checks confirmed:

- no document-level horizontal overflow;
- no unexpected nested scrollers;
- no visible interactive target smaller than 40x40 px;
- no serious or critical Axe violations;
- no browser runtime errors or failed transport requests;
- exactly 100 branch rows on a full page and 21 rows on the final page;
- exactly 100 tag rows on a full page and 21 rows on the final page;
- URL-backed branch/tag page and search state;
- one-result and no-result branch search states;
- an accessible error state on a failed second page with the previous-page action still available;
- bounded 51-item look-ahead requests and at most 50 suggestions in the code, compare, pull request, and release workflows;
- selecting a tag opens the code browser at the corresponding `refs/tags/...` revision.

## Evidence

- `qa-report.json` contains the complete 15-case matrix and workflow assertions.
- `refs-375-light.png` records the narrow mobile layout.
- `refs-1920-gray.png` records the desktop gray theme.
- `refs-2560-dark.png` records the wide dark theme.
