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
