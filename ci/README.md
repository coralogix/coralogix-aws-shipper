# AWS Shipper PR E2E

The tests run in [internal-aws-shipper-e2e](https://github.com/coralogix/internal-aws-shipper-e2e),
before merge. The shipper PR shows an **AWS Shipper E2E** status; once testing
starts, its Details link opens the actual run in that private repository.

## Flow

1. Open a same-repository, human-authored PR targeting `master`, or push a commit.
2. The trusted E2E controller checks the whole PR's changed files. Documentation-only
   PRs get **E2E not required**. Other changes wait for the exact PR build.
3. The relay verifies and stages that build's original arm64 and x86-64 ZIPs;
   E2E never rebuilds the shipper.
4. The private runner deploys to the configured AWS test account, triggers real
   sources, and uses CX CLI to verify delivery to the configured Coralogix team.
5. All **19 Terraform/arm64 cases**, teardown and independent cleanup must pass
   before the current commit becomes green. Both architecture ZIPs are verified;
   x86-64 runtime and traces are not covered.

A new commit cancels the old test. Its replacement waits for verified cleanup;
rapid pushes test only the newest waiting commit. Closing a PR also cancels and
cleans up. Never cancel the cleanup workflow.

## Which changes run E2E?

Code, dependencies, tests, templates, packaging, build scripts and workflows run
E2E. Unknown paths also run it. Only root documentation/license files, Markdown/
reStructuredText/AsciiDoc under `docs/`, example/CI READMEs, issue/PR templates and
CODEOWNERS are exempt. Renames check both the old and new paths.

The controller checks all changed files, not just the latest commit. Missing or
incomplete file information cannot produce an exemption. Add **`force-e2e`** to
run a documentation-only PR. Fork and bot PRs need a reviewed same-repo branch.

## Settings

Use **Settings → Secrets and variables → Actions**. No credentials go in Git.

| Repository variables | Where | Purpose |
|---|---|---|
| `SHIPPER_E2E_PR_ENABLED=true` | Both repos | Enable normal eligible PRs |
| `SHIPPER_E2E_PR_ALLOWED_PRS=[123]` (example) | Both repos | Optional test-PR allowlist when normal triggering is disabled; delete after validation |
| `SHIPPER_E2E_AWS_ACCOUNT_ID`, `SHIPPER_E2E_AWS_REGION`, `SHIPPER_E2E_GATE_BUCKET` | Both repos | AWS test account, isolated region and artifact/state bucket |
| `SHIPPER_E2E_RC_WRITER_ROLE_ARN` | Shipper | OIDC role for immutable artifact staging |
| `SHIPPER_E2E_EXECUTION_REF=master` | E2E | Trusted worker branch; changing it also requires matching AWS trust |
| `SHIPPER_E2E_PR_MAX_WORKERS` | E2E | Concurrent PR environments; default `1` |
| `SHIPPER_E2E_CX_REGION`, `SHIPPER_E2E_CORALOGIX_TIER` | E2E | Coralogix test-team region and query tier |

E2E repository secrets: **`CORALOGIX_API_KEY`** (SendData),
**`CORALOGIX_QUERY_API_KEY`** (DataQuerying), **`CORALOGIX_ENDPOINT`**
(ingestion base URL, such as `https://ingress.eu1.coralogix.com`). All three
must address the same team/region. The existing automation App credentials in
both repos handle dispatch/status; AWS access uses OIDC, not static AWS keys.

The private [setup guide](https://github.com/coralogix/internal-aws-shipper-e2e#setup)
covers Terraform provisioning, the remaining OIDC roles and shared fixtures.
Keep `SHIPPER_E2E_WATCHER_ENABLED=false` there to avoid duplicate post-merge runs.
Making **AWS Shipper E2E** required before merge is a separate branch-rule setting.

## Retry and failures

Rerun **all jobs** of the shipper PR's **build check** to test a new build attempt
of the same commit. Follow the E2E status link for case results and private reports.
Failed or cancelled attempts must finish independent cleanup before another
environment starts. The hourly reaper is the backup; do not edit state or delete
other runs' resources to bypass cleanup.
