# Contributing Guide

* [New Contributor Guide](#contributing-guide)
  * [Ways to Contribute](#ways-to-contribute)
  * [Ask for Help](#ask-for-help)
  * [Pull Request Lifecycle](#pull-request-lifecycle)
  * [Development Environment Setup](#development-environment-setup)
  * [Sign Your Commits](#sign-your-commits)
  * [Pull Request Checklist](#pull-request-checklist)

Welcome! We are glad that you want to contribute to our project! 💖

As you get started, you are in the best position to give us feedback on areas of
our project that we need help with including:

* Problems found during setting up a new developer environment
* Gaps in our Quickstart Guide or documentation
* Bugs in our automation scripts

If anything doesn't make sense, or doesn't work when you run it, please open a
bug report and let us know!

## Ways to Contribute

We welcome many different types of contributions including:

* New features
* Builds, CI/CD
* Bug fixes
* Documentation
* Issue Triage
* Answering questions on Github issues
* Communications / Social Media / Blog Posts
* Release management


## Ask for Help

The best way to reach us with a question when contributing is to ask on the original github issue.

## Pull Request Lifecycle

- Open a PR; if it's not finished yet, please make it a draft first
- Reviewers will be automatically assigned based on code owners
- Reviewers will get to the PR as soon as possible, but usually within 2 days

### AWS Shipper E2E

The optional **AWS Shipper E2E** status runs the private delivery suite before
merge. For enabled team PRs targeting `master`, every commit starts the existing
native build for both architectures. Trusted default-branch automation verifies
and transfers those exact ZIPs to the private runner; it never rebuilds them or
executes PR code alongside reporting credentials.

A new commit supersedes its previous test. Deployment waits for independent
cleanup of the old attempt, then tests the newest head. Success means all 19
Terraform/arm64 delivery cases passed with no skips and cleanup was verified.
Both architecture bundles are validated; x86-64 runtime and SAR/CloudFormation
installation are separate coverage. Skipping the build cannot pass E2E.
Fork and bot PRs require a reviewed maintainer-owned shipper branch for this path.

The relay is disabled by default. Operators configure these repository variables:
`SHIPPER_E2E_PR_ENABLED` (`true` for normal operation),
`SHIPPER_E2E_PR_ALLOWED_PRS` (JSON numeric PR IDs for a canary),
`SHIPPER_E2E_AWS_ACCOUNT_ID`, `SHIPPER_E2E_AWS_REGION`,
`SHIPPER_E2E_GATE_BUCKET`, and `SHIPPER_E2E_RC_WRITER_ROLE_ARN`.
It uses OIDC, plus the existing `AUTOMATION_APP_ID` / `AUTOMATION_PRIVATE_KEY`
secrets only in a separate private-dispatch job. The private controller has its
own matching rollout configuration and owns commit-status writes.

During bootstrap only, `SHIPPER_E2E_PR_IMPLEMENTATION_SHA` can pin a reviewed relay
commit while normal enablement is off and a canary PR is allowlisted. Dispatch
inputs never select executable code. Default-branch listeners must be reviewed
and installed before automatic event routing can be tested. Keep the status
non-required until the canary and cleanup checks pass; remove the temporary pin
before normal enablement.

Retry by rerunning all public build jobs, so both ZIPs come from one new attempt.
Never cancel the independent cleanup workflow. Private run reports provide
detailed failures; public status descriptions omit account and resource details.

## Development Environment Setup

You Must have Cargo and Cargo-lambda installed and configured. 

## Sign Your Commits

### CLA

We require that contributors have signed our Contributor License Agreement (CLA).

When a contributor submits their first Pull Request, the CLA Bot will step in with a friendly comment on the new pull request, kindly requesting them to sign the [Coralogix's CLA](https://cla-assistant.io/coralogix/coralogix-aws-shipper).

## Terraform dependencies
This application has strong dependecies with our Terraform module [Terraform-coralogix-aws](https://github.com/coralogix/terraform-coralogix-aws/tree/master/modules/coralogix-aws-shipper). Please review your PR does not modify anything that has impact on it. For example changing a Variable Name or adding a new one. 

## Pull Request Checklist

When you submit your pull request, or you push new commits to it, our automated
systems will run some checks on your new code. We require that your pull request
passes these checks, but we also have more criteria than just that before we can
accept and merge it. If the pull request modifies anything, that could impact 
Terraform module (https://github.com/coralogix/terraform-coralogix-aws), necessary
PR needs to be created in module repository. For example, a new environment variable
is added.
We recommend that you check the following things locally
before you submit your code:

- CLA,
- semantic version increse,
- passing CI,
- resolved discussions,

## PR E2E listener bootstrap

The PR lifecycle and trusted relay listeners are installed disabled. They make
no AWS or private dispatch requests until an operator sets the PR enablement or
canary allowlist. This bootstrap does not enable a merge requirement.

Before canary use, set `SHIPPER_E2E_PR_ENABLED=false`, allowlist one PR using
`SHIPPER_E2E_PR_ALLOWED_PRS` (JSON numeric IDs), and set
`SHIPPER_E2E_PR_IMPLEMENTATION_SHA` to the reviewed relay implementation commit.
The helpers are on the implementation branch until its automatic acceptance
passes; dispatch inputs cannot choose code. Configure account, region, bucket,
OIDC rc-writer role and the existing automation App separately. Never enable
normal triggering while a temporary implementation pin is set.
