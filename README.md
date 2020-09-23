# 👾 Processbot

### Available Commands (post as a comment in the relevant PR thread) 
- `bot merge` to automatically merge it once checks pass (if approvals have been
  given)
- `bot merge force` to attempt merge without waiting for checks (if approvals
  have been given)
- `bot merge cancel` to cancel a pending `bot merge`
- `bot compare substrate` to see a diff between current branch's Substrate
  version and the latest Polkadot release's Substrate version.
- `bot rebase` to merge origin/master.
- `bot burnin` to build and deploy the PR for a burn-in test.

### FAQ
- Who are `core-devs`? 
	- https://github.com/orgs/paritytech/teams/core-devs/members

- Who are `substrateteamleads`?
	- https://github.com/orgs/paritytech/teams/substrateteamleads/members

- What is a project column and how do I attach one?
	- A project column is necessary for Processbot to identify a project owner.
	- Approval from a relevant project owner removes the need for further approvals.
	- A pull request can be attached to a project column using the Github web UI (similar to attaching a label):

		- No project *(cannot be recognised)*

		![](https://github.com/paritytech/parity-processbot/blob/master/no-project.png)
	
		- A project but no column *(cannot be recognised)*
	
		![](https://github.com/paritytech/parity-processbot/blob/master/no-column.png)
	
		- The project `parity-processbot` and column `general` *(will be recognised)*
	
		![](https://github.com/paritytech/parity-processbot/blob/master/proj-column.png)

## Repository Configuration 

### `Process.json` file
In the repository's root directory. Eg:

```
[{
	"project_name": "Networking",
	"owner": "tomaka",
	"matrix_room_id": "!vUADSGcyXmxhKLeDsW:matrix.parity.io"
},
{	"project_name": "Client",
	"owner": "gnunicorn",
	"matrix_room_id": "!aenJixaHcSKbJOWxYk:matrix.parity.io"
},
{
	"project_name": "Runtime",
	"owner": "gavofyork",
	"matrix_room_id": "!yBKstWVBkwzUkPslsp:matrix.parity.io"
},
{
	"project_name": "Consensus",
	"owner": "andresilva",
	"matrix_room_id": "!XdNWDTfVNFVixljKZU:matrix.parity.io"
},
{
	"project_name": "Smart Contracts",
	"owner": "pepyakin",
	"matrix_room_id": "!yBKstWVBkwzUkPslsp:matrix.parity.io"
}]
```

## CI/CD

 - Deployment via gitlab is done by tagging any commit with `/^pre-v[0-9]+\.[0-9]+.*$/` for staging or `/^v[0-9]+\.[0-9]+.*$/` for production. The latter should only be done on `master`, but that is currently not enforced.
 - The environment variables for both staging and production live in the helm `kubernetes/processbot/values*.yml` files. If you add one, it also needs to be added in `templates/processbot.yaml`.
 - If any secrets need to be changed, contact the devops team.

## Staging Environment

The staging environment is deployed in the Kubernetes cluster `parity-stg` (GCP project
`parity-stg`). There is a separate Github App for it and
[a (private) repository that contains the Polkadot code](https://github.com/paritytech/polkadot-for-processbot-staging),
which has this app installed. This repo is connected to [a (private) project on Gitlab](https://gitlab.parity.io/parity/polkadot-for-processbot-staging).

