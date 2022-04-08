# Introduction

parity-processbot is a
[GitHub App](https://docs.github.com/en/developers/apps/getting-started-with-apps/about-apps)
which drives the
[Companion Build System](https://github.com/paritytech/parity-processbot/issues/327)'s
merge process.

Note that parity-processbot works independently of the Companion Build System's
cross-repository integration check, which is done on CI (see
[check_dependent_project](https://github.com/paritytech/pipeline-scripts#check_dependent_project)
for that).

Before starting to work on this project we recommend reading the
[Implementation section](#implementation).

# TOC

- [How it works](#how-it-works)
- [Commands](#commands)
  - [Relation to CI](#commands-relation-to-ci)
- [Criteria for merge](#criteria-for-merge)
  - [Checks and statuses](#criteria-for-merge-checks-and-statuses)
- [GitHub App](#github-app)
  - [Configuration](#github-app-configuration)
  - [Installation](#github-app-installation)
- [Setup](#setup)
  - [Requirements](#setup-requirements)
  - [Environment variables](#setup-environment-variables)
- [Development](#development)
  - [Run the application](#development-run)
  - [Example workflows](#development-example-workflows)
  - [Test repositories](#development-test-repositories)
  - [Integration tests](#development-integration-tests)
- [Deployment](#deployment)
  - [Logs](#deployment-logs)
  - [Environments](#deployment-environments)

# How it works <a name="how-it-works"></a>

processbot receives [commands](#commands) from pull request comments.

The merge commands will either merge the pull request right away, if possible,
or merge it automatically once all of its requirements are passing; a guided
description of how that works internally is provided in the
[Implementation section](#implementation).

# Commands <a name="commands"></a>

The following commands should be posted as pull request comments. **Your whole
comment should only have the command**.

- `bot merge`: merge once checks pass
- `bot merge force`: merge immediately while disregarding checks
  ([not all of them can be disregarded](#criteria-for-merge-checks-and-statuses))
- `bot merge cancel`: cancel a pending `bot merge`; does not affect anything
  outside of processbot, only stops the bot from following through with the
  merge
- `bot rebase`: create a merge commit from origin/master into the PR

Note: The commands will only work if you are a member of the organization where
the GitHub App is installed. Organization membership is fetched from the GitHub
API at the time a comment arrives.

## Relation to CI <a name="commands-relation-to-ci"></a>

processbot categorizes CI statuses as following, ranked in descending order of
importance:

### 1. Required

Required through GitHub branch protection rules

They are meant to be blockers so can't be skipped anyhow.

### 2. Important

Derived from Gitlab Jobs which **do not** have `allow_failure: true`

They are relevant but not blockers, thus can be skipped with `bot merge force`
but will not pass `bot merge`. Note that the merge of companions follows the
logic of `bot merge`, thus a brittle job in this category might get in the way
of a companion merge.

### 3. Fallible

Derived from Gitlab Jobs which have `allow_failure: true`

Unstable statuses will have `allow_failure: true` encoded in their descriptions
([delivered from vanity-service](https://gitlab.parity.io/parity/websites/vanity-service/-/blob/ddc0af0ec8520a99a35b9e33de57d28d37678686/service.js#L77))
which will allow processbot to detect and disregard them.

# Criteria for merge <a name="criteria-for-merge"></a>

## Checks and statuses <a name="criteria-for-merge-checks-and-statuses"></a>

All [Important and above](#commands-relation-to-ci) checks should be green when
using `bot merge`.

Non-Required statuses can bypassed by using `bot merge force`.

# GitHub App <a name="github-app"></a>

The GitHub App is necessary for the application to receive
[webhook events](https://docs.github.com/en/developers/webhooks-and-events/webhooks/about-webhooks)
and access the GitHub API properly.

Follow the instructions of
<https://gitlab.parity.io/groups/parity/opstooling/-/wikis/Bots/Development/Create-a-new-GitHub-App>
for creating a new GitHub App.

After creating the app, you should [configure](#github-app-configuration) and
[install it](#github-app-installation) (make sure the
[environment](#setup-environment-variables) is properly set up before using it).

## Configuration <a name="github-app-configuration"></a>

### Repository permissions

- Contents: Read & write
  - Enables pushing commits for updating companions after their dependencies
    have been merged
- Issues: Read & write
  - Enables comment on pull requests
- Pull requests: Read & write
  - Enables merging pull requests
- Commit statuses: Read-only
  - Enables fetching the CI statuses before merge
- Checks: Read-only
  - Enables fetching the checks' statuses before merge

### Organization permissions

- Members: Read-only
  - Enables fetching the command requester's organization membership even if
    their membership is private

### Events

- Issue comment
  - Enables reacting to [commands](#commands) from GitHub comments
- Check run, Status, Workflow job
  - Used to trigger the processing of pending pull requests

## Installation <a name="github-app-installation"></a>

Having [created](#github-app) and [configured](#github-app-configuration) the
GitHub App, install it in a repository through
`https://github.com/settings/apps/${APP}/installations`.

If processbot has to merge PRs into protected branches which have the
"Restrict who can push to matching branches" rule enabled, it should
be added to the allowlist for that rule, otherwise merging will not work
([example](https://github.com/paritytech/polkadot/pull/4122#issuecomment-948680155)).
In such cases it's necessary to add the app to the allowlist, as
demonstrated below:

![image](https://user-images.githubusercontent.com/77391175/138313741-b33b86a5-ee58-4031-a7da-12703ea9958e.png)

# Setup <a name="setup"></a>

## Requirements <a name="setup-requirements"></a>

- Rust for running the application
  - [rustup](https://rustup.rs/) is the recommended way of setting up a Rust
    toolchain
- libssl for the HTTPS requests library
- libclang for building the database (RocksDB)
- git for cloning companions and updating them

## Environment variables <a name="setup-environment-variables"></a>

All relevant environment variables are documented in the
[.env.example](./.env.example) file. For development you're welcome to copy that
file to `.env` so that all values will be loaded automatically once the
application starts.

# Development <a name="development"></a>

## Run the application <a name="development-run"></a>

1. [Set up the GitHub App](#github-app)
2. [Set up the application](#setup)

    During development it's handy to use a [smee.io](https://smee.io/) proxy,
    through the `WEBHOOK_PROXY_URL` environment variable, for receiving GitHub
    Webhook Events in your local server instance.

3. Run the project with `cargo run`
4. Optionally [try out the example workflows](#development-example-workflows) in
   the repositories where you have installed the app or the
   [test repositories](#development-test-repositories) after a deployment

## Example workflows <a name="development-example-workflows"></a>

### Single merge use-case

Example: https://github.com/paritytech/main-for-processbot-staging/pull/55

Steps:

1. Create a pull request in the repositories where the app is installed
2. Comment `bot merge`

### Companion use-case

Example:
  - Repository A: https://github.com/paritytech/main-for-processbot-staging/pull/53
  - Repository B: https://github.com/paritytech/companion-for-processbot-staging/pull/31

Steps:

1. Install the app in Repository A
2. Install the app in Repository B
  - Repository B needs to be a dependency of Repository A
    ([example](https://github.com/paritytech/companion-for-processbot-staging/blob/8ff68ae8287342f2a4581b1950913b4e9e88a0e0/Cargo.toml#L8))
3. Create a pull request on Repository B and copy its link
4. Create a pull request on Repository A and put `companion: [link from step 3]`
  in its description
5. Comment `bot merge` on the pull request in Repository A
6. Observe that the the pull request in Repository A will be merged first and
   the pull request on Repository B will be merged after

## Test repositories <a name="development-test-repositories"></a>

The staging instance is installed in the following repositories:

- https://github.com/paritytech/main-for-processbot-staging
- https://github.com/paritytech/companion-for-processbot-staging

The GitHub App for staging is managed by
[paritytech](http://github.com/paritytech)'s Organizational GitHub Admins.

## Integration tests <a name="development-integration-tests"></a>

The integration tests are executed as follows:

```sh
./scripts/run_integration_tests.sh
```

We use [insta](https://github.com/mitsuhiko/insta#introduction) for integration
tests' snapshots. After creating or modifying a snapshot, use `cargo insta
review` to manage the results.

# Deployment <a name="deployment"></a>

All of the relevant configuration for deployment lives in the [./helm](./helm)
folder. The values for each specific environment are in
`helm/values-${ENVIRONMENT}.yml`. If you add a value, it needs to be used in
[helm/templates/processbot.yaml](helm/templates/processbot.yaml).

## Logs <a name="deployment-logs"></a>

See <https://gitlab.parity.io/groups/parity/opstooling/-/wikis>

## Environments <a name="deployment-environments"></a>

When you push a deployment tag to GitHub, it will be
[mirrored to GitLab](https://gitlab.parity.io/parity/parity-processbot) and then
its [CI pipeline](./.gitlab-ci.yml) will be run for deploying the app.

The application can be deployed to the following environments:

- Production: push a tag with the pattern `/^v[0-9]+\.[0-9]+.*$/`, e.g. `v1.1`

    The production instance is installed in
    [Substrate](https://github.com/paritytech/substrate),
    [Polkadot](https://github.com/paritytech/polkadot) and
    [Cumulus](https://github.com/paritytech/cumulus).

- Staging: push a tag with the pattern `/^pre-v[0-9]+\.[0-9]+.*$/`, e.g.
  `pre-v0.6`

    The staging instance is installed in the
    [test repositories](#development-test-repositories).

# Implementation <a name="implementation"></a>

Before reading any of this, we strongly recommend to have a good understanding
of the Companion Build System by
[consulting its explanation](https://github.com/paritytech/parity-processbot/issues/327).

A
[web server](https://github.com/paritytech/parity-processbot/blob/4b36d6dcb8dd6d2ba9063c28c1c61bff503c364d/src/server.rs#L88)
(set up from
[main](https://github.com/paritytech/parity-processbot/blob/4b36d6dcb8dd6d2ba9063c28c1c61bff503c364d/src/main.rs#L107))
receives
[GitHub Webhook events](https://docs.github.com/en/developers/webhooks-and-events/webhooks/webhook-events-and-payloads)
as HTTP `POST` requests.

When someone comments in a pull request, the
[issue comment event is parsed](https://github.com/paritytech/parity-processbot/blob/4b36d6dcb8dd6d2ba9063c28c1c61bff503c364d/src/webhook.rs#L220)
and from it a
[command is extracted](https://github.com/paritytech/parity-processbot/blob/4b36d6dcb8dd6d2ba9063c28c1c61bff503c364d/src/webhook.rs#L906)
and
[handled](https://github.com/paritytech/parity-processbot/blob/4b36d6dcb8dd6d2ba9063c28c1c61bff503c364d/src/webhook.rs#L752).

The merge chain is
[started](https://github.com/paritytech/parity-processbot/blob/4b36d6dcb8dd6d2ba9063c28c1c61bff503c364d/src/webhook.rs#L761)
from a
[merge command](https://github.com/paritytech/parity-processbot/blob/4b36d6dcb8dd6d2ba9063c28c1c61bff503c364d/src/webhook.rs#L785). If the pull request at the root of the chain is
[ready to be merged, it will be merged immediately](https://github.com/paritytech/parity-processbot/blob/4b36d6dcb8dd6d2ba9063c28c1c61bff503c364d/src/webhook.rs#L787),
otherwise it will
[be saved to the database](https://github.com/paritytech/parity-processbot/blob/4b36d6dcb8dd6d2ba9063c28c1c61bff503c364d/src/webhook.rs#L813)
and
[merged later once its requirements are ready](https://github.com/paritytech/parity-processbot/blob/4b36d6dcb8dd6d2ba9063c28c1c61bff503c364d/src/webhook.rs#L580);
by "requirements" we mean its statuses, checks and dependencies
([the root of the chain is started without dependencies](https://github.com/paritytech/parity-processbot/blob/4b36d6dcb8dd6d2ba9063c28c1c61bff503c364d/src/webhook.rs#L777-L778),
hence why it can be merged first).

After a pull request is merged,
[its dependents are checked](https://github.com/paritytech/parity-processbot/blob/4b36d6dcb8dd6d2ba9063c28c1c61bff503c364d/src/webhook.rs#L831)
and possibly merged if all of their requirements are ready (note that a pull
request my might depend on more than one pull request, as
[explained in the presentation at 4:48](https://drive.google.com/file/d/1E4Fd3aO2QRJuoUBI4j0Zp4027yGeHeer/view?t=4m48s)
or
[slide number 6](https://docs.google.com/presentation/d/12ksmejR_UXC1tIHD2f4pQQZ1uw5NK3n8enmwkTCPOpw/edit?usp=sharing)).
This process is repeated for each item that is merged throughout the merge
chain (referred as "Phase 1 and Phase 2"
[in the presentation at 25:48](https://drive.google.com/file/d/1E4Fd3aO2QRJuoUBI4j0Zp4027yGeHeer/view?t=25m48s)
or
[slide number 21](https://docs.google.com/presentation/d/12ksmejR_UXC1tIHD2f4pQQZ1uw5NK3n8enmwkTCPOpw/edit?usp=sharing)).
