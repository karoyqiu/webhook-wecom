# Gitea payload fixtures

Source: [`karoyqiu/gitea-ntfy`, `test/formatters.test.js`](https://github.com/karoyqiu/gitea-ntfy/blob/3b397e26f0e2e75e430f824995a63bef92f1464e/test/formatters.test.js), branch `develop`, commit `3b397e26f0e2e75e430f824995a63bef92f1464e`, retrieved 2026-11-25 via GitHub API.

Each JSON file contains the corresponding test case's `payload`, with `REPO` expanded to `karoyqiu/gitea-ntfy`. Fields and values are unchanged; only JSON serialization differs. These are reference project's representative native Gitea payload samples, not captured production deliveries.

| Fixture | Reference test case |
| --- | --- |
| `push.json` | `push` |
| `pull_request_opened.json` | `pull_request opened` |
| `pull_request_closed_not_merged.json` | `pull_request closed (not merged)` |
| `issues_opened.json` | `issues opened` |
| `issue_comment_created.json` | `issue_comment created` |
| `release_published.json` | `release published` |
| `workflow_run_success.json` | `workflow_run success` |
| `workflow_run_failure.json` | `workflow_run failure` |

Formatting follows [`server.js` at the same commit](https://github.com/karoyqiu/gitea-ntfy/blob/3b397e26f0e2e75e430f824995a63bef92f1464e/server.js). Service-authored labels are translated to Chinese. High-priority PR closure and release use their reference tags as title markers; workflow conclusions use ✅/❌/⚙️. Comment and release truncation counts Unicode scalar values rather than JavaScript UTF-16 code units so emoji remain intact.
