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

- [core-dev](#core-devs) member approvals (2 for Substrate, 1 otherwise), or
- One [substrateteamleads](#substrateteamleads) member approval, or
- One approval from the project owner **for the PR**. Projects are managed
  [through the Github UI](#github-project) and its [owners](#project-owners)
  are defined in `Process.json`. If the PR does not belong to any project or if
  it has been approved by a project owner which is not the PR's project owner,
  then this rule will not take effect.

This criteria strictly matters only for the bot's internal logic irrespective
of Github Repository Settings and will not trump the latter in any case. For
instance, the rule:

> One substrateteamleads member approval

does not imply that the pull request will be mergeable if the Github Settings
require more approvals than that. The bot's rules work *in addition* to the
repository's settings while still respecting them. Specifically when it comes
to the approvals' count, however, the bot might able to help if an "Allowed
Developer" is requesting the merge.

Consider "Allowed Developers" to be either

- [Project Owners](#project-owners)
- [substrateteamleads](#substrateteamleads)

When the bot is commanded to merge, if the PR is short of 1 approval and the
command's requester might not be able to fulfill the approval count on their
own, then the bot will try to pitch in the missing approval if the requester is
an Allowed Developer. The reasoning for this feature is as follows:

1. PR authors cannot approve their own merge requests, although Allowed
   Developers should have the means to bypass that requirement e.g. for trivial
   or urgent changes.

2. If the Allowed Developer has already approved and it's still short of one,
   they cannot "approve twice" in order to meet the quota. In that case, the
   bot should contribute one approval in order to help them meet that
   requirement.

## Checks and statuses

All [Important and above](#relation-to-ci) checks should be green when using
`bot merge` (can be bypassed by using `bot merge force`).

# Project Owners <a name="project-owners"></a>

Project owners can be configured by placing a `Process.json` file in the root
of your repository. **The bot always considers only the `master` branch's
version of the file**. See [./Process.json](./Process.json) or
[Substrate's Process.json](https://github.com/paritytech/substrate/blob/master/Process.json)
for examples.

The file should have a valid JSON array of
`{ "project_name": string, "owner": string, "matrix_room_id": string }`
where:

- `project_name` is the [Github Project](#github-project)'s name
- `owner` is the Github Username of the project's lead
- `matrix_room_id` is the project's room address on Matrix, like
  `"!yBKstWVBkwzUkPslsp:matrix.parity.io"`. It's not currently used, but needs to
  be defined.

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

# Development

The bot requires some environment variables listed in
[config.rs](./src/config.rs). They can, optionally, be set through an `.env`
file which should be placed at the root of this repository. We provide an
[example .env file](./.env.example) which can be used as a starting point.

During **development**, it's handy to use a [smee.io](https://smee.io/) proxy,
through the `WEBHOOK_PROXY_URL` environment variable, for receiving Github
Webhook Events in your local instance of processbot.

After setting up the environment, run: `cargo run`

For executing the integration tests, run: `./scripts/run_integration_tests.sh`

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

## Notes

- All of the relevant configuration for deployment lives in the
  [./kubernetes/processbot](./kubernetes/processbot) folder. The environment
  variables for both staging and production are in `values*.yml`; if you add
  one, it also needs to be added to `templates/processbot.yaml`.
 - Secrets are managed through Gitlab.

# FAQ

- Who are "core-devs"? <a name="core-devs"></a>
	- https://github.com/orgs/paritytech/teams/core-devs/members

- Who are "substrateteamleads"? <a name="substrateteamleads"></a>
	- https://github.com/orgs/paritytech/teams/substrateteamleads/members

- What is a project column and how do I attach one? <a name="github-project"></a>
	- A project column is necessary for Processbot to identify a
	  [Project Owner](#project-owners).
	- A pull request can be attached to a project column using the Github UI:
		- Having no project, it *will not be recognized*
		![](https://github.com/paritytech/parity-processbot/blob/master/no-project.png)
		- Having a project, but no column, it *will not be recognized*
		![](https://github.com/paritytech/parity-processbot/blob/master/no-column.png)
		- Having both project a column, it *will be recognized*
		![](https://github.com/paritytech/parity-processbot/blob/master/proj-column.png)
