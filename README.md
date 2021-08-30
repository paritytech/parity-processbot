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
- `bot compare substrate`: see a diff between current branch's Substrate
  version and the latest Polkadot release's Substrate version.
- `bot rebase`: create a merge commit from origin/master into the PR.
- `bot burnin`: build and deploy the PR for a burn-in test.

Note: The commands will only work if you are a member of the organization where
this bot is installed. Organization membership is always gotten fresh from the
Github API at the time a comment arrives.

All [Important and above](#relation-to-ci) checks should be green when using
`bot merge` (can be bypassed by using `bot merge force`).

# Criteria for merge

A Pull Request needs either 2
[core-dev](https://github.com/orgs/paritytech/teams/core-devs/members)
approvals or one
[substrateteamlead](https://github.com/orgs/paritytech/teams/substrateteamleads/members)
approval. This criteria strictly matters only for the bot's internal
logic irrespective of Github Repository Settings and will not trump the latter
in any case. For instance, the rule:

> One substrateteamleads member approval

does not imply that the pull request will be mergeable if the Github Settings
require more approvals than that. The bot's rules work *in addition* to the
repository's settings while still respecting them. Specifically when it comes
to the approvals' count, however, the bot might able to help if a
[substrateteamlead](https://github.com/orgs/paritytech/teams/substrateteamleads/members)
is requesting the merge.

Additionally, when the bot is commanded to merge, if the PR is short of 1
approval and the command's requester might not be able to fulfill the approval
count on their own, then the bot will try to pitch in the missing approval if
the requester is a
[substrateteamlead](https://github.com/orgs/paritytech/teams/substrateteamleads/members).
The reasoning for this feature is as follows:

1. PR authors cannot approve their own merge requests, although they should
	 have the means to bypass that requirement e.g. for trivial or urgent
	 changes.

2. If the team lead has already approved and it's still short of one, they
	 cannot "approve twice" in order to meet the quota. In that case, the bot
	 should contribute one approval in order to help them meet that requirement.

# Relation to CI

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

# Deployment

The bot is automatically deployed by pushing a tag with one of the following formats

- `/^v-[0-9]+\.[0-9]+.*$/`, e.g. `v-1.1`, will deploy it to production (cluster
  `parity-prod`).

- `/^stg-[0-9]+\.[0-9]+.*$/`, e.g. `stg-0.6` will deploy to staging (cluster
  `parity-stg`).
  - The staging package is deployed to a dedicated separate Github App. There's
    [a (private) repository for staging](https://github.com/paritytech/polkadot-for-processbot-staging)
    which has it installed, already set up with a mirrored
	[a (private) project on Gitlab](https://gitlab.parity.io/parity/polkadot-for-processbot-staging)
	for CI.

The deployment's status can be followed through
[Gitlab Pipelines on the parity-processbot mirror](https://gitlab.parity.io/parity/parity-processbot/-/pipelines)
([example](https://gitlab.parity.io/parity/parity-processbot/-/jobs/867102)).

## Notes

- All of the relevant configuration for deployment lives in the
  [./kubernetes/processbot](./kubernetes/processbot) folder. The environment
  variables for both staging and production are in `values*.yml`; if you add
  one, it also needs to be added to `templates/processbot.yaml`.
 - Secrets are managed through Gitlab.
