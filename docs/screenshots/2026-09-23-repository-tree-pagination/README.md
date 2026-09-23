# Repository tree pagination QA

Validated on 2026-09-23 against the production frontend build served by nginx at the normal CI/CD URL.

## Load reproduction

A temporary real Git repository was generated with 12,250 root entries: 12,000 files and 250 directories.

- Raw `git ls-tree -l` output: 929,499 bytes.
- Local command time: 73 ms.
- Estimated legacy DOM height at 44 px per row: 539,000 px.
- The updated API streams NUL-delimited Git output and stops after the requested bounded page instead of collecting the complete tree.

## Browser matrix

The repository tree was checked at 375x812, 768x900, 1280x900, 1920x1080, and 2560x1440 in light, gray, and dark themes.

The automated checks confirmed:

- no document-level horizontal overflow;
- no unexpected nested scrollers;
- no visible interactive target smaller than 40x40 px;
- no serious or critical Axe violations;
- no browser runtime errors or failed transport requests;
- exactly 100 rendered rows on a full page;
- 21 rows and a disabled next button on the final page;
- URL-backed page and directory-search state;
- one-result and no-result search states;
- an accessible error state on a failed second page with the previous-page action still available.

## Evidence

- `qa-report.json` contains the complete 15-case matrix and workflow assertions.
- `tree-375-light.png` records the narrow mobile layout.
- `tree-1920-gray.png` records the desktop gray theme.
- `tree-2560-dark.png` records the wide dark theme.

