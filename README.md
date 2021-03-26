# 👾 Processbot

# Commands

To be posted as a comment in the Pull Request. **Your whole comment should only have the command**.

- `bot merge` to automatically merge it once checks pass (if approvals have been
  given).
- `bot merge force` to attempt merge without waiting for checks (if approvals
  have been given).
- `bot merge cancel` to cancel a pending `bot merge`.
- `bot compare substrate` to see a diff between current branch's Substrate.
  version and the latest Polkadot release's Substrate version.
- `bot rebase` to merge origin/master.
- `bot burnin` to build and deploy the PR for a burn-in test.

Commands should all be defined in [constants.rs](./src/constants.rs).

# Deployment

Note: As of March 26 of 2021, the bot has been running on production with [tag 0.5.0](https://github.com/paritytech/parity-processbot/releases/tag/v0.5.0) since October 27 of 2020. Although many bugs have been reported since then, that is the current "stable" version which developers are used to. `master` has since then received commits which have not landed in production; therefore, if a "doom scenario" comes up, it's advised that you always deploy that tag instead of retagging master for the time being.

The bot is automatically deployed by pushing a tag with one of the following formats

- `/^v[0-9]+\.[0-9]+.*$/`, e.g. `v1.1`, will deploy it to production (cluster `parity-prod`).

- `/^pre-v[0-9]+\.[0-9]+.*$/`, e.g. `pre-v0.6` will deploy to staging (cluster `parity-stg`).
  - The staging package is deployed to a dedicated separate Github App. There's a [a (private) repository for staging](https://github.com/paritytech/polkadot-for-processbot-staging)
which has it installed, already set up with a mirrored [a (private) project on Gitlab](https://gitlab.parity.io/parity/polkadot-for-processbot-staging) for CI.

The deployment's status can be followed through [Gitlab Pipelines on the parity-processbot mirror](https://gitlab.parity.io/parity/parity-processbot/-/pipelines) ([example](https://gitlab.parity.io/parity/parity-processbot/-/jobs/867102)).

## Notes

- All of the relevant configuration for deployment lives in the [./kubernetes/processbot](./kubernetes/processbot) folder. The environment variables for both staging and production are in `values*.yml`; if you add one, it also needs to be added to `templates/processbot.yaml`.
 - If any secrets need to be changed, contact the devops team.

# Configuration

## Project owners <a name="project-owners"></a>

Project owners can be configured by placing a `Process.json` file in the root of your repository. **The bot always considers only the `master` branch's version of the file**. See [./Process.json](./Process.json) or [Substrate's Process.json](https://github.com/paritytech/substrate/blob/master/Process.json) for examples.

The file should have a valid JSON array of `{ "project_name": string, "owner": string, "matrix_room_id": string }` where:

- `project_name` is the [Github Project](#github-project)'s name
- `owner` is the Github Username of the project's lead
- `matrix_room_id` is the project's room address on Matrix, like `"!yBKstWVBkwzUkPslsp:matrix.parity.io"`. *It's not currently used, but needs to be defined.*

## Runtime

Some of the bot's configuration (e.g. the number of required reviewers for a pull request) can be managed through environment variables defined in [./src/config.rs](./src/config.rs)

# Criteria for merge

**Approvals**: A Pull Request needs either

- `$MIN_REVIEWERS` (default: 2) [core-dev](#core-devs) member approvals, or
- One [substrateteamleads](#substrateteamleads) member approval, or
- One approval from the project owner **for the PR**. Projects are managed [through the Github UI](#github-project) and its [owners](#project-owners) are defined in `Process.json`. If the PR does not belong to any project or if it has been approved by a project owner which is not the PR's project owner, then this rule will not take effect.

**Checks and statuses**: all CI statuses and checks should be green when using `bot merge`; those can be bypassed by using `bot merge force`.

# FAQ

- Who are "core-devs"? <a name="core-devs"></a>
	- https://github.com/orgs/paritytech/teams/core-devs/members

- Who are "substrateteamleads"? <a name="substrateteamleads"></a>
	- https://github.com/orgs/paritytech/teams/substrateteamleads/members

- What is a project column and how do I attach one? <a name="github-project"></a>
	- A project column is necessary for Processbot to identify a [project owner](#project-owners).
	- A pull request can be attached to a project column using the Github web UI (similar to attaching a label):

		- No project *(cannot be recognised)*

		![](https://github.com/paritytech/parity-processbot/blob/master/no-project.png)
	
		- A project but no column *(cannot be recognised)*
	
		![](https://github.com/paritytech/parity-processbot/blob/master/no-column.png)
	
		- The project `parity-processbot` and column `general` *(will be recognised)*
	
		![](https://github.com/paritytech/parity-processbot/blob/master/proj-column.png)
