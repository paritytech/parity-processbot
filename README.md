# 👾 processbot

# Commands

To be posted as a comment in the Pull Request. **Your whole comment should only
have the command**.

- `bot merge`: [if approved](#criteria-for-merge), merge once checks pass.
- `bot merge force`: [if approved](#criteria-for-merge), merge immediately
  while disregarding checks.
- `bot merge cancel`: cancel a pending `bot merge`; does not affect anything
  outside of processbot, only stops the bot from following through with the
  merge.
- `bot rebase`: create a merge commit from origin/master into the PR.

Note: The commands will only work if you are a member of the organization where
this bot is installed. Organization membership is always gotten fresh from the
Github API at the time a comment arrives.

## Relation to CI

processbot categorizes CI statuses as following, ranked in descending order of
importance:

### 1. Required

Required through Github branch protection rules

They are meant to be blockers so can't be skipped anyhow.

### 2. Important

Derived from Gitlab Jobs which **do not** have `allow_failure: true`

They are relevant but not blockers, thus can be skipped with `bot merge force`
but will not pass `bot merge`. Note that the companion build system follows the
logic of `bot merge`, thus a brittle job in this category might get in the way
of a companion merge.

### 3. Fallible

Derived from Gitlab Jobs which have `allow_failure: true`

Unstable statuses will have `allow_failure: true` encoded in their descriptions
([delivered from vanity-service](https://gitlab.parity.io/parity/websites/vanity-service/-/blob/ddc0af0ec8520a99a35b9e33de57d28d37678686/service.js#L77))
which will allow processbot to detect and disregard them.

# Criteria for merge

## Approvals

A Pull Request needs either (meaning, only **one of** the following
requirements needs to be fulfilled)

- [core-dev](https://github.com/orgs/paritytech/teams/core-devs) member approvals (2 for Substrate, 1 otherwise), or
- One [substrateteamleads](https://github.com/orgs/paritytech/teams/substrateteamleads) member approval

This criterion strictly matters only for the bot's internal logic irrespective
of Github Repository Settings and will not trump the latter in any case. For
instance, the rule:

> One substrateteamleads member approval

does not imply that the pull request will be mergeable if the Github Settings
require more approvals than that. The bot's rules work *in addition* to the
repository's settings while still respecting them. Specifically when it comes
to the approvals' count, however, the bot might able to help if a
[team lead](https://github.com/orgs/paritytech/teams/substrateteamleads)
is requesting the merge.

When the bot is commanded to merge, if the PR is short of 1 approval and the
command's requester might not be able to fulfill the approval count on their
own, then the bot will try to pitch in the missing approval if the requester is
a [team lead](https://github.com/orgs/paritytech/teams/substrateteamleads).
The reasoning for this feature is as follows:

1. PR authors cannot approve their own merge requests, although
	[team leads](https://github.com/orgs/paritytech/teams/substrateteamleads)
	should have the means to bypass that requirement e.g. for trivial or urgent
	changes.

2. If the
	[team lead](https://github.com/orgs/paritytech/teams/substrateteamleads)
	has already approved and it's still
	short of one, they cannot "approve twice" in order to meet the quota. In that
	case, the bot should contribute one approval in order to help them meet that
	requirement.

## Checks and statuses

All [Important and above](#relation-to-ci) checks should be green when using
`bot merge` (can be bypassed by using `bot merge force`).

# Github App Configuration

Repository permissions

- Contents: Read & write
- Issues: Read & write
- Metadata: Read-only
- Pull requests: Read & write
- Projects: Read-only
- Commit statuses: Read-only
- Checks: Read-only

Organization permissions

- Members: Read-only

Events:

- Check run
- Issue comment
- Status
- Workflow job

---

If processbot has to merge PRs into protected branches which have the
"Restrict who can push to matching branches" rule enabled, it should
be added to the allowlist for that rule, otherwise merging will not work
(example: https://github.com/paritytech/polkadot/pull/4122#issuecomment-948680155).

In such cases it's necessary to add the app to the allowlist, as
demonstrated below:

![image](https://user-images.githubusercontent.com/77391175/138313741-b33b86a5-ee58-4031-a7da-12703ea9958e.png)

# Local development

This project is a standard Rust project with some notable requirements.

## Requirements

Before you can generate a debug or release binary, we have some library
requirements to install.

```sh
$ sudo apt install \
    libssl-dev \
    libclang-dev
```

## Environment variables

The bot requires some environment variables listed in
[config.rs](./src/config.rs).

They can, optionally, be set through an `.env`
file which should be placed at the root of this repository. We provide an
[example .env file](./.env.example) which can be used as a starting point.

```
$ cp .env.example .env
```

During **development**, it's handy to use a [smee.io](https://smee.io/) proxy,
through the `WEBHOOK_PROXY_URL` environment variable, for receiving Github
Webhook Events in your local instance of processbot.

## Running the bot

```sh
$ cargo run
```

## Integration tests

The integration tests are executed as follows:

```sh
./scripts/run_integration_tests.sh
```

We use [insta](https://github.com/mitsuhiko/insta#introduction) for integration
tests' snapshots. After creating or modifying a snapshot, use `cargo insta
review` to manage the results.

# Deployment

The bot is automatically deployed by pushing a tag with one of the following formats

- `/^v[0-9]+\.[0-9]+.*$/`, e.g. `v1.1`, will deploy it to production (cluster
  `parity-prod`).

- `/^pre-v[0-9]+\.[0-9]+.*$/`, e.g. `pre-v0.6` will deploy to staging (cluster
  `parity-stg`).
  - The staging package is deployed to a dedicated separate Github App. There's
    [a (private) repository for staging](https://github.com/paritytech/polkadot-for-processbot-staging)
    which has it installed, already set up with a mirrored
	[a (private) project on Gitlab](https://gitlab.parity.io/parity/polkadot-for-processbot-staging)
	for CI.

The deployment's status can be followed through
[Gitlab Pipelines on the parity-processbot mirror](https://gitlab.parity.io/parity/parity-processbot/-/pipelines)
([example](https://gitlab.parity.io/parity/parity-processbot/-/jobs/867102)).

All of the relevant configuration for deployment lives in the [./helm](./helm)
folder. The values for each specific environment are in `values-*.yml`. If you
add a value, it needs to be used in `templates/processbot.yaml`.
